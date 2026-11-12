// SPDX-License-Identifier: AGPL-3.0-or-later

//! Effect catalog. A package is `manifest.json` (id, name, category, typed parameters) plus
//! `effect.wgsl` defining `fn effect_main(uv: vec2<f32>) -> vec4<f32>`. The shader is composed
//! with a fixed prelude and validated with naga before any GPU work; invalid packages produce
//! an [`EffectError`], never a panic.

use std::sync::Arc;

use serde::Deserialize;
use thiserror::Error;

include!(concat!(env!("OUT_DIR"), "/builtin.rs"));

/// Maximum parameters per effect (four vec4 uniforms).
pub const MAX_PARAMS: usize = 16;
const PRELUDE: &str = include_str!("prelude.wgsl");

#[derive(Debug, Error, PartialEq)]
pub enum EffectError {
    #[error("invalid manifest JSON: {0}")]
    Json(String),
    #[error("effect '{id}': {reason}")]
    Manifest { id: String, reason: String },
    #[error("effect '{id}': shader does not compile:\n{message}")]
    Shader { id: String, message: String },
    #[error("duplicate effect id '{0}'")]
    Duplicate(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Color,
    Blur,
    Distortion,
    Stylize,
    Correction,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::Color => "Cor",
            Self::Blur => "Desfoque",
            Self::Distortion => "Distorção",
            Self::Stylize => "Estilização",
            Self::Correction => "Correção",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub id: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub category: Category,
    #[serde(default)]
    pub params: Vec<Param>,
    /// Parameter values used by the golden-image test (defaults when absent).
    #[serde(default)]
    pub golden_params: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
pub struct EffectPackage {
    pub manifest: Manifest,
    /// Prelude + package shader, already validated by naga.
    pub shader: String,
}

impl EffectPackage {
    pub fn defaults(&self) -> Vec<f32> {
        self.manifest
            .params
            .iter()
            .map(|param| param.default)
            .collect()
    }

    pub fn golden_params(&self) -> Vec<f32> {
        self.manifest
            .golden_params
            .clone()
            .unwrap_or_else(|| self.defaults())
    }

    /// Uniform block values: given values clamped to their ranges, defaults for missing ones.
    pub fn uniform_params(&self, values: &[f32]) -> [f32; MAX_PARAMS] {
        let mut out = [0.0; MAX_PARAMS];
        for (index, param) in self.manifest.params.iter().enumerate() {
            let value = values
                .get(index)
                .copied()
                .filter(|value| value.is_finite())
                .unwrap_or(param.default);
            out[index] = value.clamp(param.min, param.max);
        }
        out
    }
}

/// Parses and validates one package.
pub fn load_package(manifest_json: &str, shader: &str) -> Result<EffectPackage, EffectError> {
    let manifest: Manifest = serde_json::from_str(manifest_json)
        .map_err(|error| EffectError::Json(error.to_string()))?;
    let fail = |reason: String| EffectError::Manifest {
        id: manifest.id.clone(),
        reason,
    };
    if manifest.id.is_empty()
        || !manifest
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(fail(
            "id must be non-empty lowercase ascii, digits or '-'".into(),
        ));
    }
    if manifest.name.trim().is_empty() {
        return Err(fail("name is empty".into()));
    }
    if manifest.params.len() > MAX_PARAMS {
        return Err(fail(format!(
            "at most {MAX_PARAMS} parameters are supported"
        )));
    }
    for param in &manifest.params {
        let finite = [param.min, param.max, param.default]
            .iter()
            .all(|value| value.is_finite());
        if !finite || param.min >= param.max {
            return Err(fail(format!(
                "parameter '{}' needs finite min < max",
                param.id
            )));
        }
        if !(param.min..=param.max).contains(&param.default) {
            return Err(fail(format!(
                "parameter '{}' default is out of range",
                param.id
            )));
        }
    }
    if let Some(golden) = &manifest.golden_params {
        let fits = golden.len() == manifest.params.len()
            && golden
                .iter()
                .zip(&manifest.params)
                .all(|(value, param)| (param.min..=param.max).contains(value));
        if !fits {
            return Err(fail(
                "golden_params must match the parameters and their ranges".into(),
            ));
        }
    }
    let composed = format!("{PRELUDE}{shader}");
    validate_shader(&manifest.id, &composed)?;
    Ok(EffectPackage {
        manifest,
        shader: composed,
    })
}

/// Parses and validates WGSL with naga, requiring the `effect_main` entry function.
pub fn validate_shader(id: &str, source: &str) -> Result<(), EffectError> {
    if !source.contains("fn effect_main") {
        return Err(EffectError::Shader {
            id: id.to_owned(),
            message: "missing fn effect_main(uv: vec2<f32>) -> vec4<f32>".into(),
        });
    }
    let module = naga::front::wgsl::parse_str(source).map_err(|error| EffectError::Shader {
        id: id.to_owned(),
        message: error.emit_to_string(source),
    })?;
    let has_main = module
        .functions
        .iter()
        .any(|(_, function)| function.name.as_deref() == Some("effect_main"));
    if !has_main {
        return Err(EffectError::Shader {
            id: id.to_owned(),
            message: "missing fn effect_main(uv: vec2<f32>) -> vec4<f32>".into(),
        });
    }
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| EffectError::Shader {
        id: id.to_owned(),
        message: error.emit_to_string(source),
    })?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    packages: Vec<Arc<EffectPackage>>,
}

impl Catalog {
    /// The packages embedded from `crates/editor-effects/effects/` at build time.
    pub fn builtin() -> Result<Self, EffectError> {
        Self::from_sources(BUILTIN.iter().copied())
    }

    pub fn from_sources<'a>(
        sources: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, EffectError> {
        let mut packages: Vec<Arc<EffectPackage>> = Vec::new();
        for (manifest, shader) in sources {
            let package = load_package(manifest, shader)?;
            if packages
                .iter()
                .any(|other| other.manifest.id == package.manifest.id)
            {
                return Err(EffectError::Duplicate(package.manifest.id));
            }
            packages.push(Arc::new(package));
        }
        Ok(Self { packages })
    }

    pub fn packages(&self) -> &[Arc<EffectPackage>] {
        &self.packages
    }

    pub fn get(&self, id: &str) -> Option<&Arc<EffectPackage>> {
        self.packages
            .iter()
            .find(|package| package.manifest.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK_MANIFEST: &str = r#"{"id":"test","name":"Test","category":"color",
        "params":[{"id":"a","name":"A","min":0,"max":1,"default":0.5}]}"#;
    const OK_SHADER: &str =
        "fn effect_main(uv: vec2<f32>) -> vec4<f32> { return src(uv) * param(0u); }";

    #[test]
    fn builtin_catalog_has_two_packages_per_category() {
        let catalog = Catalog::builtin().expect("builtin catalog is valid");
        for category in [
            Category::Color,
            Category::Blur,
            Category::Distortion,
            Category::Stylize,
            Category::Correction,
        ] {
            let count = catalog
                .packages()
                .iter()
                .filter(|package| package.manifest.category == category)
                .count();
            assert!(count >= 2, "{category:?} has {count}");
        }
    }

    #[test]
    fn invalid_packages_fail_with_clear_errors() {
        assert!(load_package(OK_MANIFEST, OK_SHADER).is_ok());
        let cases = [
            ("{not json", OK_SHADER, "invalid manifest JSON"),
            (
                r#"{"id":"Bad Id","name":"x","category":"color"}"#,
                OK_SHADER,
                "lowercase",
            ),
            (
                r#"{"id":"x","name":"x","category":"sparkles"}"#,
                OK_SHADER,
                "invalid manifest JSON",
            ),
            (
                r#"{"id":"x","name":"x","category":"blur","params":[{"id":"a","name":"A","min":1,"max":0,"default":0}]}"#,
                OK_SHADER,
                "min < max",
            ),
            (
                r#"{"id":"x","name":"x","category":"blur","params":[{"id":"a","name":"A","min":0,"max":1,"default":3}]}"#,
                OK_SHADER,
                "out of range",
            ),
            (
                OK_MANIFEST,
                "fn effect_main(uv: vec2<f32>) -> vec4<f32> { return 1.0; }",
                "does not compile",
            ),
            (OK_MANIFEST, "fn other() {}", "missing fn effect_main"),
            (OK_MANIFEST, "this is not wgsl", "does not compile"),
        ];
        for (manifest, shader, expected) in cases {
            let error = load_package(manifest, shader)
                .expect_err("must fail")
                .to_string();
            assert!(error.contains(expected), "{error}");
        }
        let duplicate = Catalog::from_sources([(OK_MANIFEST, OK_SHADER), (OK_MANIFEST, OK_SHADER)]);
        assert_eq!(duplicate.err(), Some(EffectError::Duplicate("test".into())));
    }

    #[test]
    fn uniform_params_clamp_and_fill_defaults() {
        let package = load_package(OK_MANIFEST, OK_SHADER).expect("valid");
        assert_eq!(package.uniform_params(&[])[0], 0.5);
        assert_eq!(package.uniform_params(&[7.0])[0], 1.0);
        assert_eq!(package.uniform_params(&[f32::NAN])[0], 0.5);
    }
}
