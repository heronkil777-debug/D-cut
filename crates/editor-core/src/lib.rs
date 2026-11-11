// SPDX-License-Identifier: AGPL-3.0-or-later

//! UI- and I/O-free project model and reversible timeline edits for D-cut.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

mod edit;
mod plan;

pub use edit::{
    TrimEdge, add_media_clips_command, clips_at_frame, delete_clips_command, find_clip,
    linked_clip_ids, move_command, move_limits, next_free_id, split_at_command, trim_command,
    trim_limits, unlink_command,
};
pub use edit::{add_track_command, move_clip_to_track_command};
pub use plan::{PlanSegment, SegmentSource, audio_plans, video_plan};

/// The product name shown in project metadata and command-line output.
pub const PRODUCT_NAME: &str = "D-cut";
/// Current on-disk project schema version.
pub const PROJECT_FORMAT_VERSION: u32 = 1;

/// Project canvas settings. Frame rate is expressed as an exact rational number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFormat {
    /// Canvas width in pixels.
    pub width: u32,
    /// Canvas height in pixels.
    pub height: u32,
    /// Frame-rate numerator (for example, 30000 for 29.97 fps).
    pub frame_rate_numerator: u32,
    /// Frame-rate denominator (for example, 1001 for 29.97 fps).
    pub frame_rate_denominator: u32,
}

impl Default for ProjectFormat {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            frame_rate_numerator: 30,
            frame_rate_denominator: 1,
        }
    }
}

impl ProjectFormat {
    /// Returns the frame rate as a floating-point value for display and playback scheduling.
    pub fn frames_per_second(self) -> f64 {
        f64::from(self.frame_rate_numerator) / f64::from(self.frame_rate_denominator.max(1))
    }

    /// Converts a project frame index to seconds using the exact rational frame rate.
    pub fn frames_to_seconds(self, frames: u64) -> f64 {
        frames as f64 * f64::from(self.frame_rate_denominator.max(1))
            / f64::from(self.frame_rate_numerator.max(1))
    }

    /// Converts seconds to the nearest project frame (negative and non-finite inputs map to 0).
    pub fn seconds_to_frame(self, seconds: f64) -> u64 {
        let frames = seconds * self.frames_per_second();
        if frames.is_finite() && frames > 0.0 {
            frames.round() as u64
        } else {
            0
        }
    }
}

/// Media metadata stored independently of the decoder implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaAsset {
    /// Stable project-local identifier.
    pub id: u64,
    /// Relative path from the project file, or an absolute path when the user opts to link external media.
    pub path: String,
    /// Optional content fingerprint for relinking and stale-media detection.
    pub content_hash: Option<String>,
    /// Duration measured in project frames after source timestamps are normalized.
    pub duration_frames: u64,
    /// Display dimensions after applying source rotation metadata.
    pub width: u32,
    /// Display dimensions after applying source rotation metadata.
    pub height: u32,
    /// Whether the source contains a video stream.
    pub has_video: bool,
    /// Whether the source contains an audio stream.
    pub has_audio: bool,
    /// Clockwise rotation metadata in degrees, normalized to a quarter turn.
    pub rotation_degrees: u16,
}

/// Timeline track type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
}

/// A timeline clip refers to a source asset and a half-open source/timeline frame range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub id: u64,
    pub media_id: u64,
    pub timeline_start_frame: u64,
    pub source_start_frame: u64,
    pub duration_frames: u64,
    /// Clips sharing a link group (for example a video clip and its audio) are edited together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link_group: Option<u64>,
}

impl Clip {
    /// Exclusive end frame on the timeline.
    pub fn timeline_end_frame(&self) -> Option<u64> {
        self.timeline_start_frame.checked_add(self.duration_frames)
    }

    /// Exclusive end frame in the source asset.
    pub fn source_end_frame(&self) -> Option<u64> {
        self.source_start_frame.checked_add(self.duration_frames)
    }
}

/// A single ordered timeline track.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub id: u64,
    pub name: String,
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Hold,
    Linear,
    Smooth,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub frame: u64,
    pub value: f32,
    pub interpolation_to_next: Interpolation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimationCurve {
    pub keyframes: Vec<Keyframe>,
}

impl AnimationCurve {
    /// Evaluates a sorted curve. Values outside its key range hold the nearest endpoint.
    pub fn evaluate(&self, frame: u64) -> Option<f32> {
        let first = *self.keyframes.first()?;
        if frame <= first.frame {
            return Some(first.value);
        }
        let last = *self.keyframes.last()?;
        if frame >= last.frame {
            return Some(last.value);
        }
        for pair in self.keyframes.windows(2) {
            let left = &pair[0];
            let right = &pair[1];
            if frame >= left.frame && frame <= right.frame {
                let span = (right.frame - left.frame) as f32;
                let mut t = (frame - left.frame) as f32 / span;
                match left.interpolation_to_next {
                    Interpolation::Hold => return Some(left.value),
                    Interpolation::Linear => {}
                    Interpolation::Smooth => t = t * t * (3.0 - 2.0 * t),
                }
                return Some(left.value + (right.value - left.value) * t);
            }
        }
        None
    }

    fn validate(&self) -> bool {
        self.keyframes.iter().all(|key| key.value.is_finite())
            && self
                .keyframes
                .windows(2)
                .all(|pair| pair[0].frame < pair[1].frame)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnimatedProperty {
    /// Clip or text overlay identifier.
    pub target_id: u64,
    /// Stable property key such as `opacity`, `position_x`, or `audio_gain_db`.
    pub property: String,
    pub curve: AnimationCurve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    CrossDissolve,
    WipeLeft,
    WipeRight,
    DipToBlack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub id: u64,
    pub track_id: u64,
    pub from_clip_id: u64,
    pub to_clip_id: u64,
    pub start_frame: u64,
    pub duration_frames: u64,
    pub kind: TransitionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaProxy {
    pub media_id: u64,
    pub path: String,
    pub content_hash: Option<String>,
    pub width: u32,
    pub height: u32,
}

/// A project-local text item; rendering is performed by the shared render graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextOverlay {
    pub id: u64,
    /// Subtitle track ID, or `None` for a title layer not attached to a subtitle track.
    pub track_id: Option<u64>,
    pub start_frame: u64,
    pub duration_frames: u64,
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub font_size: f32,
    /// Straight-alpha RGBA color with channels in the inclusive range 0.0..=1.0.
    pub color: [f32; 4],
}

/// Versioned, portable project document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub format_version: u32,
    pub format: ProjectFormat,
    pub media: Vec<MediaAsset>,
    pub tracks: Vec<Track>,
    pub text_overlays: Vec<TextOverlay>,
    pub animations: Vec<AnimatedProperty>,
    pub transitions: Vec<Transition>,
    pub proxies: Vec<MediaProxy>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            format_version: PROJECT_FORMAT_VERSION,
            format: ProjectFormat::default(),
            media: Vec::new(),
            tracks: Vec::new(),
            text_overlays: Vec::new(),
            animations: Vec::new(),
            transitions: Vec::new(),
            proxies: Vec::new(),
        }
    }
}

impl Project {
    /// Checks structural invariants before saving, rendering, or applying edits.
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.format_version != PROJECT_FORMAT_VERSION {
            return Err(CoreError::UnsupportedProjectVersion(self.format_version));
        }
        if self.format.width == 0 || self.format.height == 0 {
            return Err(CoreError::InvalidFormat(
                "canvas dimensions must be non-zero",
            ));
        }
        if self.format.frame_rate_numerator == 0 || self.format.frame_rate_denominator == 0 {
            return Err(CoreError::InvalidFormat("frame rate must be positive"));
        }

        let mut media_ids = HashSet::new();
        for media in &self.media {
            if !media_ids.insert(media.id) {
                return Err(CoreError::DuplicateId(media.id));
            }
            if media.path.trim().is_empty() {
                return Err(CoreError::InvalidMedia(media.id, "path is empty"));
            }
            if media.duration_frames == 0 || media.width == 0 || media.height == 0 {
                return Err(CoreError::InvalidMedia(
                    media.id,
                    "duration and dimensions must be non-zero",
                ));
            }
            if !matches!(media.rotation_degrees, 0 | 90 | 180 | 270) {
                return Err(CoreError::InvalidMedia(
                    media.id,
                    "rotation must be 0, 90, 180, or 270 degrees",
                ));
            }
            if !media.has_video && !media.has_audio {
                return Err(CoreError::InvalidMedia(
                    media.id,
                    "asset has no usable audio or video stream",
                ));
            }
        }

        let mut ids = media_ids;
        for track in &self.tracks {
            if !ids.insert(track.id) {
                return Err(CoreError::DuplicateId(track.id));
            }
            let mut previous_end = 0_u64;
            for clip in &track.clips {
                if !ids.insert(clip.id) {
                    return Err(CoreError::DuplicateId(clip.id));
                }
                if clip.timeline_start_frame < previous_end {
                    return Err(CoreError::InvalidClip(
                        clip.id,
                        "clips on a track must be sorted and must not overlap",
                    ));
                }
                previous_end = clip.timeline_end_frame().unwrap_or(u64::MAX);
                let media = self
                    .media
                    .iter()
                    .find(|asset| asset.id == clip.media_id)
                    .ok_or(CoreError::MissingMedia(clip.media_id))?;
                if clip.duration_frames == 0 {
                    return Err(CoreError::InvalidClip(clip.id, "duration must be non-zero"));
                }
                let source_end = clip.source_end_frame().ok_or(CoreError::InvalidClip(
                    clip.id,
                    "source frame range overflows",
                ))?;
                if source_end > media.duration_frames {
                    return Err(CoreError::InvalidClip(
                        clip.id,
                        "source frame range exceeds media duration",
                    ));
                }
                if clip.timeline_end_frame().is_none() {
                    return Err(CoreError::InvalidClip(
                        clip.id,
                        "timeline frame range overflows",
                    ));
                }
                match track.kind {
                    TrackKind::Video if !media.has_video => {
                        return Err(CoreError::MediaKindMismatch(clip.id));
                    }
                    TrackKind::Audio if !media.has_audio => {
                        return Err(CoreError::MediaKindMismatch(clip.id));
                    }
                    TrackKind::Subtitle => {
                        return Err(CoreError::InvalidClip(
                            clip.id,
                            "subtitle clips use text overlays, not media assets",
                        ));
                    }
                    _ => {}
                }
            }
        }

        for overlay in &self.text_overlays {
            if !ids.insert(overlay.id) {
                return Err(CoreError::DuplicateId(overlay.id));
            }
            if let Some(track_id) = overlay.track_id
                && self
                    .track(track_id)
                    .is_none_or(|track| track.kind != TrackKind::Subtitle)
            {
                return Err(CoreError::InvalidOverlay(
                    overlay.id,
                    "subtitle overlay track is missing or has the wrong kind",
                ));
            }
            if overlay.duration_frames == 0
                || overlay
                    .start_frame
                    .checked_add(overlay.duration_frames)
                    .is_none()
            {
                return Err(CoreError::InvalidOverlay(
                    overlay.id,
                    "time range must be positive and not overflow",
                ));
            }
            if !overlay.x.is_finite()
                || !overlay.y.is_finite()
                || !overlay.font_size.is_finite()
                || overlay.font_size <= 0.0
            {
                return Err(CoreError::InvalidOverlay(
                    overlay.id,
                    "position and font size must be finite, with positive font size",
                ));
            }
            if overlay
                .color
                .iter()
                .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(channel))
            {
                return Err(CoreError::InvalidOverlay(
                    overlay.id,
                    "color channels must be finite and within 0.0..=1.0",
                ));
            }
        }

        let mut proxy_media = HashSet::new();
        for proxy in &self.proxies {
            let media = self
                .media
                .iter()
                .find(|asset| asset.id == proxy.media_id)
                .ok_or(CoreError::MissingMedia(proxy.media_id))?;
            if !media.has_video
                || proxy.path.trim().is_empty()
                || proxy.width == 0
                || proxy.height == 0
                || !proxy_media.insert(proxy.media_id)
            {
                return Err(CoreError::InvalidProxy(
                    proxy.media_id,
                    "proxy must reference video media, have a path and non-zero dimensions, and be unique",
                ));
            }
        }

        for animation in &self.animations {
            let target_exists = self.tracks.iter().any(|track| {
                track
                    .clips
                    .iter()
                    .any(|clip| clip.id == animation.target_id)
            }) || self
                .text_overlays
                .iter()
                .any(|overlay| overlay.id == animation.target_id);
            if !target_exists
                || animation.property.trim().is_empty()
                || animation.curve.keyframes.is_empty()
                || !animation.curve.validate()
            {
                return Err(CoreError::InvalidAnimation(
                    animation.target_id,
                    "target/property/keyframes are invalid",
                ));
            }
        }

        for transition in &self.transitions {
            if !ids.insert(transition.id) {
                return Err(CoreError::DuplicateId(transition.id));
            }
            let track = self
                .track(transition.track_id)
                .ok_or(CoreError::MissingTrack(transition.track_id))?;
            let from_exists = track
                .clips
                .iter()
                .any(|clip| clip.id == transition.from_clip_id);
            let to_exists = track
                .clips
                .iter()
                .any(|clip| clip.id == transition.to_clip_id);
            if transition.from_clip_id == transition.to_clip_id
                || !from_exists
                || !to_exists
                || transition.duration_frames == 0
                || transition
                    .start_frame
                    .checked_add(transition.duration_frames)
                    .is_none()
                || track.kind != TrackKind::Video
            {
                return Err(CoreError::InvalidTransition(
                    transition.id,
                    "transition endpoints, time range, or track are invalid",
                ));
            }
        }
        Ok(())
    }

    pub fn track(&self, track_id: u64) -> Option<&Track> {
        self.tracks.iter().find(|track| track.id == track_id)
    }

    pub fn media_asset(&self, media_id: u64) -> Option<&MediaAsset> {
        self.media.iter().find(|asset| asset.id == media_id)
    }

    /// Exclusive end frame of the last clip on any track (0 for an empty timeline).
    pub fn duration_frames(&self) -> u64 {
        self.tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter_map(Clip::timeline_end_frame)
            .max()
            .unwrap_or(0)
    }
}

/// A reversible project edit. Every user edit is expressed as one of these commands.
/// The `expected` snapshots prevent applying stale edits to a project that changed meanwhile.
#[derive(Debug, Clone, PartialEq)]
pub enum EditCommand {
    ReplaceTrackClips {
        track_id: u64,
        expected: Vec<Clip>,
        replacement: Vec<Clip>,
    },
    ReplaceMedia {
        expected: Vec<MediaAsset>,
        replacement: Vec<MediaAsset>,
    },
    SetFormat {
        expected: ProjectFormat,
        replacement: ProjectFormat,
    },
    ReplaceTextOverlays {
        expected: Vec<TextOverlay>,
        replacement: Vec<TextOverlay>,
    },
    /// Replaces the whole track list (adding/removing/reordering tracks, moving clips across tracks).
    ReplaceTracks {
        expected: Vec<Track>,
        replacement: Vec<Track>,
    },
    /// Applies all commands in order; if one fails, the already applied ones are rolled back.
    Batch(Vec<EditCommand>),
}

impl EditCommand {
    /// Applies this command and returns its inverse for undo/redo.
    /// On error, the project is left unchanged.
    pub fn apply(self, project: &mut Project) -> Result<Self, CoreError> {
        match self {
            Self::ReplaceTrackClips {
                track_id,
                expected,
                replacement,
            } => {
                let track_index = project
                    .tracks
                    .iter()
                    .position(|track| track.id == track_id)
                    .ok_or(CoreError::MissingTrack(track_id))?;
                if project.tracks[track_index].clips != expected {
                    return Err(CoreError::StaleCommand(track_id));
                }
                let old_clips =
                    std::mem::replace(&mut project.tracks[track_index].clips, replacement.clone());
                if let Err(error) = project.validate() {
                    project.tracks[track_index].clips = old_clips;
                    return Err(error);
                }
                Ok(Self::ReplaceTrackClips {
                    track_id,
                    expected: replacement,
                    replacement: old_clips,
                })
            }
            Self::ReplaceMedia {
                expected,
                replacement,
            } => {
                if project.media != expected {
                    return Err(CoreError::StaleProject);
                }
                let old = std::mem::replace(&mut project.media, replacement.clone());
                if let Err(error) = project.validate() {
                    project.media = old;
                    return Err(error);
                }
                Ok(Self::ReplaceMedia {
                    expected: replacement,
                    replacement: old,
                })
            }
            Self::SetFormat {
                expected,
                replacement,
            } => {
                if project.format != expected {
                    return Err(CoreError::StaleProject);
                }
                project.format = replacement;
                if let Err(error) = project.validate() {
                    project.format = expected;
                    return Err(error);
                }
                Ok(Self::SetFormat {
                    expected: replacement,
                    replacement: expected,
                })
            }
            Self::ReplaceTextOverlays {
                expected,
                replacement,
            } => {
                if project.text_overlays != expected {
                    return Err(CoreError::StaleProject);
                }
                let old = std::mem::replace(&mut project.text_overlays, replacement.clone());
                if let Err(error) = project.validate() {
                    project.text_overlays = old;
                    return Err(error);
                }
                Ok(Self::ReplaceTextOverlays {
                    expected: replacement,
                    replacement: old,
                })
            }
            Self::ReplaceTracks {
                expected,
                replacement,
            } => {
                if project.tracks != expected {
                    return Err(CoreError::StaleProject);
                }
                let old = std::mem::replace(&mut project.tracks, replacement.clone());
                if let Err(error) = project.validate() {
                    project.tracks = old;
                    return Err(error);
                }
                Ok(Self::ReplaceTracks {
                    expected: replacement,
                    replacement: old,
                })
            }
            Self::Batch(commands) => {
                let mut inverses = Vec::with_capacity(commands.len());
                for command in commands {
                    match command.apply(project) {
                        Ok(inverse) => inverses.push(inverse),
                        Err(error) => {
                            for inverse in inverses.into_iter().rev() {
                                // Inverses of just-applied commands always match the current state.
                                let _ = inverse.apply(project);
                            }
                            return Err(error);
                        }
                    }
                }
                inverses.reverse();
                Ok(Self::Batch(inverses))
            }
        }
    }
}

/// Undo/redo stacks. A failed command leaves both the project and history unchanged.
#[derive(Debug, Default)]
pub struct CommandHistory {
    undo: Vec<EditCommand>,
    redo: Vec<EditCommand>,
}

impl CommandHistory {
    pub fn execute(
        &mut self,
        project: &mut Project,
        command: EditCommand,
    ) -> Result<(), CoreError> {
        let inverse = command.apply(project)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }

    pub fn undo(&mut self, project: &mut Project) -> Result<bool, CoreError> {
        let Some(command) = self.undo.pop() else {
            return Ok(false);
        };
        match command.clone().apply(project) {
            Ok(inverse) => {
                self.redo.push(inverse);
                Ok(true)
            }
            Err(error) => {
                self.undo.push(command);
                Err(error)
            }
        }
    }

    pub fn redo(&mut self, project: &mut Project) -> Result<bool, CoreError> {
        let Some(command) = self.redo.pop() else {
            return Ok(false);
        };
        match command.clone().apply(project) {
            Ok(inverse) => {
                self.undo.push(inverse);
                Ok(true)
            }
            Err(error) => {
                self.redo.push(command);
                Err(error)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

/// Constructs a split command at an absolute timeline frame.
pub fn split_clip_command(
    project: &Project,
    track_id: u64,
    clip_id: u64,
    split_frame: u64,
    right_clip_id: u64,
) -> Result<EditCommand, CoreError> {
    let track = project
        .track(track_id)
        .ok_or(CoreError::MissingTrack(track_id))?;
    let mut replacement = track.clips.clone();
    let index = replacement
        .iter()
        .position(|clip| clip.id == clip_id)
        .ok_or(CoreError::MissingClip(clip_id))?;
    let clip = &replacement[index];
    let end = clip.timeline_end_frame().ok_or(CoreError::InvalidClip(
        clip.id,
        "timeline frame range overflows",
    ))?;
    if split_frame <= clip.timeline_start_frame || split_frame >= end {
        return Err(CoreError::InvalidEdit(
            "split point must be inside the clip",
        ));
    }
    if project.media.iter().any(|media| media.id == right_clip_id)
        || project.tracks.iter().any(|item| {
            item.id == right_clip_id || item.clips.iter().any(|other| other.id == right_clip_id)
        })
        || project
            .text_overlays
            .iter()
            .any(|overlay| overlay.id == right_clip_id)
        || project
            .transitions
            .iter()
            .any(|transition| transition.id == right_clip_id)
    {
        return Err(CoreError::DuplicateId(right_clip_id));
    }
    let left_duration = split_frame - clip.timeline_start_frame;
    let right_source =
        clip.source_start_frame
            .checked_add(left_duration)
            .ok_or(CoreError::InvalidClip(
                clip.id,
                "source frame range overflows",
            ))?;
    let right = Clip {
        id: right_clip_id,
        media_id: clip.media_id,
        timeline_start_frame: split_frame,
        source_start_frame: right_source,
        duration_frames: clip.duration_frames - left_duration,
        link_group: clip.link_group,
    };
    replacement[index].duration_frames = left_duration;
    replacement.insert(index + 1, right);
    Ok(EditCommand::ReplaceTrackClips {
        track_id,
        expected: track.clips.clone(),
        replacement,
    })
}

/// Constructs a non-ripple trim command using a source-relative half-open range.
pub fn trim_clip_command(
    project: &Project,
    track_id: u64,
    clip_id: u64,
    new_source_start: u64,
    new_duration: u64,
) -> Result<EditCommand, CoreError> {
    let track = project
        .track(track_id)
        .ok_or(CoreError::MissingTrack(track_id))?;
    let mut replacement = track.clips.clone();
    let clip = replacement
        .iter_mut()
        .find(|clip| clip.id == clip_id)
        .ok_or(CoreError::MissingClip(clip_id))?;
    let old_source_end = clip.source_end_frame().ok_or(CoreError::InvalidClip(
        clip.id,
        "source frame range overflows",
    ))?;
    let new_source_end = new_source_start
        .checked_add(new_duration)
        .ok_or(CoreError::InvalidEdit("trim range overflows"))?;
    if new_duration == 0
        || new_source_start < clip.source_start_frame
        || new_source_end > old_source_end
    {
        return Err(CoreError::InvalidEdit(
            "trim must select a non-empty range inside the original source range",
        ));
    }
    let removed_head = new_source_start - clip.source_start_frame;
    clip.timeline_start_frame = clip
        .timeline_start_frame
        .checked_add(removed_head)
        .ok_or(CoreError::InvalidEdit("timeline range overflows"))?;
    clip.source_start_frame = new_source_start;
    clip.duration_frames = new_duration;
    Ok(EditCommand::ReplaceTrackClips {
        track_id,
        expected: track.clips.clone(),
        replacement,
    })
}

/// Constructs a ripple-delete command for a half-open timeline range on one track.
pub fn ripple_delete_command(
    project: &Project,
    track_id: u64,
    start_frame: u64,
    end_frame: u64,
) -> Result<EditCommand, CoreError> {
    if start_frame >= end_frame {
        return Err(CoreError::InvalidEdit(
            "ripple-delete range must be non-empty",
        ));
    }
    let track = project
        .track(track_id)
        .ok_or(CoreError::MissingTrack(track_id))?;
    let removed = end_frame - start_frame;
    let mut replacement = Vec::with_capacity(track.clips.len());
    let highest_id =
        project
            .media
            .iter()
            .map(|item| item.id)
            .chain(project.tracks.iter().flat_map(|item| {
                std::iter::once(item.id).chain(item.clips.iter().map(|clip| clip.id))
            }))
            .chain(project.text_overlays.iter().map(|item| item.id))
            .chain(project.transitions.iter().map(|item| item.id))
            .max()
            .unwrap_or(0);
    let mut next_id = highest_id.checked_add(1);
    for clip in &track.clips {
        let clip_end = clip.timeline_end_frame().ok_or(CoreError::InvalidClip(
            clip.id,
            "timeline frame range overflows",
        ))?;
        if clip_end <= start_frame {
            replacement.push(clip.clone());
        } else if clip.timeline_start_frame >= end_frame {
            let mut shifted = clip.clone();
            shifted.timeline_start_frame -= removed;
            replacement.push(shifted);
        } else {
            let left_len = start_frame.saturating_sub(clip.timeline_start_frame);
            let right_len = clip_end.saturating_sub(end_frame);
            if left_len > 0 {
                let mut left = clip.clone();
                left.duration_frames = left_len;
                replacement.push(left);
            }
            if right_len > 0 {
                let mut right = clip.clone();
                if left_len > 0 {
                    right.id = next_id.ok_or(CoreError::InvalidEdit(
                        "no unused clip identifier is available",
                    ))?;
                    next_id = right.id.checked_add(1);
                }
                let source_skip = clip.duration_frames - right_len;
                right.source_start_frame =
                    right.source_start_frame.checked_add(source_skip).ok_or(
                        CoreError::InvalidClip(clip.id, "source frame range overflows"),
                    )?;
                right.timeline_start_frame = start_frame;
                right.duration_frames = right_len;
                replacement.push(right);
            }
        }
    }
    Ok(EditCommand::ReplaceTrackClips {
        track_id,
        expected: track.clips.clone(),
        replacement,
    })
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CoreError {
    #[error("unsupported project format version {0}")]
    UnsupportedProjectVersion(u32),
    #[error("invalid project format: {0}")]
    InvalidFormat(&'static str),
    #[error("duplicate project identifier {0}")]
    DuplicateId(u64),
    #[error("invalid media asset {0}: {1}")]
    InvalidMedia(u64, &'static str),
    #[error("invalid clip {0}: {1}")]
    InvalidClip(u64, &'static str),
    #[error("invalid text overlay {0}: {1}")]
    InvalidOverlay(u64, &'static str),
    #[error("invalid animation for target {0}: {1}")]
    InvalidAnimation(u64, &'static str),
    #[error("invalid transition {0}: {1}")]
    InvalidTransition(u64, &'static str),
    #[error("invalid proxy for media {0}: {1}")]
    InvalidProxy(u64, &'static str),
    #[error("clip {0} is incompatible with its track type")]
    MediaKindMismatch(u64),
    #[error("missing media asset {0}")]
    MissingMedia(u64),
    #[error("missing track {0}")]
    MissingTrack(u64),
    #[error("missing clip {0}")]
    MissingClip(u64),
    #[error("edit conflicts with the current project state on track {0}")]
    StaleCommand(u64),
    #[error("edit conflicts with the current project state")]
    StaleProject,
    #[error("invalid edit: {0}")]
    InvalidEdit(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn project_with_clip() -> Project {
        Project {
            media: vec![MediaAsset {
                id: 1,
                path: "media/test.mp4".into(),
                content_hash: None,
                duration_frames: 100,
                width: 1920,
                height: 1080,
                has_video: true,
                has_audio: true,
                rotation_degrees: 0,
            }],
            tracks: vec![Track {
                id: 2,
                name: "Video 1".into(),
                kind: TrackKind::Video,
                clips: vec![Clip {
                    id: 3,
                    media_id: 1,
                    timeline_start_frame: 0,
                    source_start_frame: 0,
                    duration_frames: 100,
                    link_group: None,
                }],
            }],
            ..Project::default()
        }
    }

    #[test]
    fn animation_curves_evaluate_linear_and_hold_segments() {
        let curve = AnimationCurve {
            keyframes: vec![
                Keyframe {
                    frame: 10,
                    value: 2.0,
                    interpolation_to_next: Interpolation::Linear,
                },
                Keyframe {
                    frame: 20,
                    value: 12.0,
                    interpolation_to_next: Interpolation::Hold,
                },
                Keyframe {
                    frame: 30,
                    value: 30.0,
                    interpolation_to_next: Interpolation::Linear,
                },
            ],
        };
        assert_eq!(curve.evaluate(0), Some(2.0));
        assert_eq!(curve.evaluate(15), Some(7.0));
        assert_eq!(curve.evaluate(25), Some(12.0));
        assert_eq!(curve.evaluate(30), Some(30.0));
    }

    #[test]
    fn split_undo_redo_round_trip() {
        let mut project = project_with_clip();
        let original = project.clone();
        project.validate().expect("fixture should be valid");
        let mut history = CommandHistory::default();
        let split = split_clip_command(&project, 2, 3, 40, 4).expect("split point is valid");
        history.execute(&mut project, split).expect("split applies");
        assert_eq!(project.tracks[0].clips.len(), 2);
        assert!(history.undo(&mut project).expect("undo works"));
        assert_eq!(project, original);
        assert!(history.redo(&mut project).expect("redo works"));
        assert_eq!(project.tracks[0].clips[0].duration_frames, 40);
        assert_eq!(project.tracks[0].clips[1].source_start_frame, 40);
    }

    #[test]
    fn ripple_delete_trims_overlaps_and_closes_gap() {
        let mut project = project_with_clip();
        project.tracks[0].clips.push(Clip {
            id: 5,
            media_id: 1,
            timeline_start_frame: 120,
            source_start_frame: 0,
            duration_frames: 20,
            link_group: None,
        });
        let command = ripple_delete_command(&project, 2, 30, 50).expect("range is valid");
        command.apply(&mut project).expect("edit applies");
        assert_eq!(project.tracks[0].clips[0].duration_frames, 30);
        assert_eq!(project.tracks[0].clips[1].source_start_frame, 50);
        assert_ne!(project.tracks[0].clips[0].id, project.tracks[0].clips[1].id);
        assert_eq!(project.tracks[0].clips[2].timeline_start_frame, 100);
    }

    proptest! {
        #[test]
        fn split_then_undo_restores_valid_clip(duration in 2_u64..100_000, split_seed in 1_u64..100_000) {
            let split_offset = 1 + split_seed % (duration - 1);
            let mut project = project_with_clip();
            project.media[0].duration_frames = duration;
            project.tracks[0].clips[0].duration_frames = duration;
            let original = project.clone();
            let split_at = split_offset;
            let command = split_clip_command(&project, 2, 3, split_at, 4).map_err(|error| TestCaseError::fail(error.to_string()))?;
            let mut history = CommandHistory::default();
            history.execute(&mut project, command).map_err(|error| TestCaseError::fail(error.to_string()))?;
            prop_assert!(history.undo(&mut project).map_err(|error| TestCaseError::fail(error.to_string()))?);
            prop_assert_eq!(project, original);
        }
    }

    #[test]
    fn invalid_edit_is_rejected_without_mutation() {
        let project = project_with_clip();
        let original = project.clone();
        let history = CommandHistory::default();
        let command = split_clip_command(&project, 2, 3, 0, 4).expect_err("edge split is invalid");
        assert_eq!(
            command,
            CoreError::InvalidEdit("split point must be inside the clip")
        );
        assert_eq!(project, original);
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }

    fn editor_project() -> Project {
        Project {
            media: vec![
                MediaAsset {
                    id: 1,
                    path: "/media/a.mp4".into(),
                    content_hash: None,
                    duration_frames: 300,
                    width: 1280,
                    height: 720,
                    has_video: true,
                    has_audio: true,
                    rotation_degrees: 0,
                },
                MediaAsset {
                    id: 2,
                    path: "/media/b.wav".into(),
                    content_hash: None,
                    duration_frames: 90,
                    width: 1280,
                    height: 720,
                    has_video: false,
                    has_audio: true,
                    rotation_degrees: 0,
                },
            ],
            tracks: vec![
                Track {
                    id: 10,
                    name: "V1".into(),
                    kind: TrackKind::Video,
                    clips: Vec::new(),
                },
                Track {
                    id: 11,
                    name: "A1".into(),
                    kind: TrackKind::Audio,
                    clips: Vec::new(),
                },
            ],
            ..Project::default()
        }
    }

    #[test]
    fn adding_av_media_creates_linked_clips_and_split_keeps_them_linked() {
        let mut project = editor_project();
        let mut history = CommandHistory::default();
        let (command, created) = add_media_clips_command(&project, 1).expect("add works");
        history.execute(&mut project, command).expect("applies");
        assert_eq!(created.len(), 2);
        let split = split_at_command(&project, &[created[0]], 100).expect("split builds");
        history.execute(&mut project, split).expect("split applies");
        assert_eq!(project.tracks[0].clips.len(), 2);
        assert_eq!(project.tracks[1].clips.len(), 2);
        let right_video = &project.tracks[0].clips[1];
        let right_audio = &project.tracks[1].clips[1];
        assert_eq!(right_video.link_group, right_audio.link_group);
        assert_ne!(
            right_video.link_group,
            project.tracks[0].clips[0].link_group
        );
        assert_eq!(right_video.source_start_frame, 100);
    }

    #[test]
    fn ripple_delete_of_linked_clip_closes_gaps_on_both_tracks() {
        let mut project = editor_project();
        let (command, created) = add_media_clips_command(&project, 1).expect("add works");
        command.apply(&mut project).expect("applies");
        split_at_command(&project, &[created[0]], 100)
            .and_then(|command| command.apply(&mut project))
            .expect("split");
        let first = project.tracks[0].clips[0].id;
        delete_clips_command(&project, &[first], true)
            .and_then(|command| command.apply(&mut project))
            .expect("delete");
        assert_eq!(project.tracks[0].clips.len(), 1);
        assert_eq!(project.tracks[1].clips.len(), 1);
        assert_eq!(project.tracks[0].clips[0].timeline_start_frame, 0);
        assert_eq!(project.tracks[0].clips[0].source_start_frame, 100);
        assert_eq!(project.tracks[1].clips[0].timeline_start_frame, 0);
    }

    #[test]
    fn trim_limits_respect_media_bounds_and_neighbors() {
        let mut project = editor_project();
        let (command, created) = add_media_clips_command(&project, 1).expect("add works");
        command.apply(&mut project).expect("applies");
        let clip = created[0];
        assert_eq!(trim_limits(&project, clip, TrimEdge::Tail), Ok((-299, 0)));
        assert_eq!(trim_limits(&project, clip, TrimEdge::Head), Ok((0, 299)));
        trim_command(&project, clip, TrimEdge::Head, 30)
            .and_then(|command| command.apply(&mut project))
            .expect("head trim");
        assert_eq!(project.tracks[0].clips[0].timeline_start_frame, 30);
        assert_eq!(project.tracks[1].clips[0].source_start_frame, 30);
        assert!(trim_command(&project, clip, TrimEdge::Tail, 1).is_err());
        move_command(&project, clip, -30)
            .and_then(|command| command.apply(&mut project))
            .expect("move");
        assert_eq!(project.tracks[1].clips[0].timeline_start_frame, 0);
    }

    #[test]
    fn overlapping_clips_are_rejected_by_validation() {
        let mut project = project_with_clip();
        project.tracks[0].clips.push(Clip {
            id: 9,
            media_id: 1,
            timeline_start_frame: 50,
            source_start_frame: 0,
            duration_frames: 10,
            link_group: None,
        });
        assert!(project.validate().is_err());
    }

    #[test]
    fn video_plan_fills_gaps_and_merges_contiguous_ranges() {
        let mut project = editor_project();
        let (command, created) = add_media_clips_command(&project, 1).expect("add works");
        command.apply(&mut project).expect("applies");
        move_command(&project, created[0], 10)
            .and_then(|command| command.apply(&mut project))
            .expect("move");
        let plan = video_plan(&project, 0, project.duration_frames() + 5);
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].source, None);
        assert_eq!(plan[0].duration_frames, 10);
        assert_eq!(plan[1].duration_frames, 300);
        assert_eq!(plan[2].source, None);
        let partial = video_plan(&project, 50, 60);
        assert_eq!(partial.len(), 1);
        assert_eq!(
            partial[0].source.map(|source| source.source_start_frame),
            Some(40)
        );
        assert_eq!(audio_plans(&project, 0, 310).len(), 1);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Add(u64),
        Split(usize, u64),
        Delete(usize, bool),
        Trim(usize, bool, i64),
        Move(usize, i64),
    }

    fn op_strategy() -> impl Strategy<Value = Op> {
        prop_oneof![
            (1_u64..=2).prop_map(Op::Add),
            (0_usize..8, 0_u64..700).prop_map(|(index, frame)| Op::Split(index, frame)),
            (0_usize..8, any::<bool>()).prop_map(|(index, ripple)| Op::Delete(index, ripple)),
            (0_usize..8, any::<bool>(), -200_i64..200)
                .prop_map(|(index, head, delta)| Op::Trim(index, head, delta)),
            (0_usize..8, -200_i64..200).prop_map(|(index, delta)| Op::Move(index, delta)),
        ]
    }

    fn pick_clip(project: &Project, index: usize) -> Option<u64> {
        let all: Vec<u64> = project
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter().map(|clip| clip.id))
            .collect();
        (!all.is_empty()).then(|| all[index % all.len()])
    }

    fn build(project: &Project, op: &Op) -> Option<EditCommand> {
        match *op {
            Op::Add(media) => add_media_clips_command(project, media)
                .ok()
                .map(|(command, _)| command),
            Op::Split(index, frame) => {
                split_at_command(project, &[pick_clip(project, index)?], frame).ok()
            }
            Op::Delete(index, ripple) => {
                delete_clips_command(project, &[pick_clip(project, index)?], ripple).ok()
            }
            Op::Trim(index, head, delta) => {
                let clip = pick_clip(project, index)?;
                let edge = if head { TrimEdge::Head } else { TrimEdge::Tail };
                let (low, high) = trim_limits(project, clip, edge).ok()?;
                trim_command(project, clip, edge, delta.clamp(low, high)).ok()
            }
            Op::Move(index, delta) => {
                let clip = pick_clip(project, index)?;
                let (low, high) = move_limits(project, clip).ok()?;
                move_command(project, clip, delta.clamp(low, high)).ok()
            }
        }
    }

    proptest! {
        #[test]
        fn random_edit_sequences_undo_and_redo_exactly(ops in proptest::collection::vec(op_strategy(), 1..25)) {
            let mut project = editor_project();
            let original = project.clone();
            let mut history = CommandHistory::default();
            let mut snapshots = vec![project.clone()];
            for op in &ops {
                if let Some(command) = build(&project, op) {
                    history.execute(&mut project, command).map_err(|error| TestCaseError::fail(error.to_string()))?;
                    prop_assert!(project.validate().is_ok());
                    snapshots.push(project.clone());
                }
            }
            let final_state = project.clone();
            for expected in snapshots.iter().rev().skip(1) {
                prop_assert!(history.undo(&mut project).map_err(|error| TestCaseError::fail(error.to_string()))?);
                prop_assert_eq!(&project, expected);
            }
            prop_assert_eq!(&project, &original);
            while history.redo(&mut project).map_err(|error| TestCaseError::fail(error.to_string()))? {}
            prop_assert_eq!(&project, &final_state);
        }
    }

    #[test]
    fn tracks_and_text_commands_round_trip() {
        let mut project = editor_project();
        let original = project.clone();
        let mut history = CommandHistory::default();
        let (command, created) = add_media_clips_command(&project, 1).expect("add");
        history.execute(&mut project, command).expect("applies");
        let command = add_track_command(&project, TrackKind::Video);
        history.execute(&mut project, command).expect("track");
        assert_eq!(project.tracks[0].name, "V2");
        let v2 = project.tracks[0].id;
        let command = move_clip_to_track_command(&project, created[0], v2).expect("move");
        history.execute(&mut project, command).expect("applies");
        assert_eq!(project.tracks[0].clips.len(), 1);
        assert!(project.tracks[1].clips.is_empty());
        // The top video track wins in the render plan.
        let plan = video_plan(&project, 0, 10);
        assert_eq!(
            plan[0].source.map(|source| source.clip_id),
            Some(created[0])
        );
        let overlay = TextOverlay {
            id: next_free_id(&project),
            track_id: None,
            start_frame: 0,
            duration_frames: 30,
            text: "Título".into(),
            x: 0.5,
            y: 0.5,
            font_size: 80.0,
            color: [1.0, 1.0, 1.0, 1.0],
        };
        history
            .execute(
                &mut project,
                EditCommand::ReplaceTextOverlays {
                    expected: Vec::new(),
                    replacement: vec![overlay],
                },
            )
            .expect("text");
        assert_eq!(project.text_overlays.len(), 1);
        while history.undo(&mut project).expect("undo") {}
        assert_eq!(project, original);
    }
}
