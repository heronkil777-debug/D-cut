// SPDX-License-Identifier: AGPL-3.0-or-later
//! Embeds every package found in `effects/<id>/{manifest.json,effect.wgsl}`. Adding a package
//! only requires adding that folder (plus its golden image); no engine code changes.

use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("effects");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut entries: Vec<PathBuf> = fs::read_dir(&root)
        .map(|read| {
            read.flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    entries.sort();
    let mut code = String::from("pub(crate) const BUILTIN: &[(&str, &str)] = &[\n");
    for dir in entries {
        let manifest = dir.join("manifest.json");
        let shader = dir.join("effect.wgsl");
        if manifest.is_file() && shader.is_file() {
            println!("cargo:rerun-if-changed={}", manifest.display());
            println!("cargo:rerun-if-changed={}", shader.display());
            code.push_str(&format!(
                "    (include_str!({:?}), include_str!({:?})),\n",
                manifest.display().to_string(),
                shader.display().to_string()
            ));
        }
    }
    code.push_str("];\n");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap_or_default()).join("builtin.rs");
    if let Err(error) = fs::write(&out, code) {
        panic!("could not write {}: {error}", out.display());
    }
}
