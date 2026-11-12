// SPDX-License-Identifier: AGPL-3.0-or-later

//! Converts the timeline into a flat, gap-filled list of segments. The same plan drives the
//! preview and the export, so what is shown is what gets rendered.

use crate::{Project, Track, TrackKind};

/// Source material for one plan segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentSource {
    pub clip_id: u64,
    pub media_id: u64,
    pub source_start_frame: u64,
    pub speed: f32,
    /// Timeline frames between the clip start and this segment's start.
    pub clip_offset_frames: u64,
}

/// A contiguous timeline range; `source == None` means a gap (black video or silence).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanSegment {
    pub timeline_start_frame: u64,
    pub duration_frames: u64,
    pub source: Option<SegmentSource>,
}

fn source_at(track: &Track, frame: u64) -> Option<SegmentSource> {
    track
        .clips
        .iter()
        .find(|clip| {
            clip.timeline_start_frame <= frame
                && clip.timeline_end_frame().is_some_and(|end| frame < end)
        })
        .map(|clip| SegmentSource {
            clip_id: clip.id,
            media_id: clip.media_id,
            source_start_frame: clip.source_start_frame
                + clip.source_frames_for(frame - clip.timeline_start_frame),
            speed: clip.speed,
            clip_offset_frames: frame - clip.timeline_start_frame,
        })
}

fn build_plan(tracks: &[&Track], start: u64, end: u64) -> Vec<PlanSegment> {
    if start >= end {
        return Vec::new();
    }
    let mut boundaries = vec![start, end];
    for track in tracks {
        for clip in &track.clips {
            let Some(clip_end) = clip.timeline_end_frame() else {
                continue;
            };
            for point in [clip.timeline_start_frame, clip_end] {
                if point > start && point < end {
                    boundaries.push(point);
                }
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let mut segments: Vec<PlanSegment> = Vec::new();
    for pair in boundaries.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        // Earlier tracks in the list have priority (they are drawn on top).
        let source = tracks.iter().find_map(|track| source_at(track, a));
        if let Some(previous) = segments.last_mut() {
            let continues = match (previous.source, source) {
                (None, None) => true,
                // Adjacent intervals of the same clip are always continuous.
                (Some(left), Some(right)) => left.clip_id == right.clip_id,
                _ => false,
            };
            if continues {
                previous.duration_frames += b - a;
                continue;
            }
        }
        segments.push(PlanSegment {
            timeline_start_frame: a,
            duration_frames: b - a,
            source,
        });
    }
    segments
}

/// Gap-filled video plan for `[start, end)`; the first video track with a clip wins.
pub fn video_plan(project: &Project, start: u64, end: u64) -> Vec<PlanSegment> {
    let tracks: Vec<&Track> = project
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Video)
        .collect();
    build_plan(&tracks, start, end)
}

/// One gap-filled plan per audio track that contains at least one clip. Tracks are mixed.
pub fn audio_plans(project: &Project, start: u64, end: u64) -> Vec<Vec<PlanSegment>> {
    project
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Audio && !track.clips.is_empty())
        .map(|track| build_plan(&[track], start, end))
        .collect()
}

/// The clip shown at `frame` (the first video track with a clip there), if any.
pub fn top_video_clip_at(project: &Project, frame: u64) -> Option<&crate::Clip> {
    project
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Video)
        .find_map(|track| {
            track.clips.iter().find(|clip| {
                clip.timeline_start_frame <= frame
                    && clip.timeline_end_frame().is_some_and(|end| frame < end)
            })
        })
}

/// An active transition between two adjacent clips on a video track.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionWindow {
    pub track_id: u64,
    pub from_clip: u64,
    pub to_clip: u64,
    pub package: String,
    /// The cut between the clips; the window is centered on it.
    pub cut_frame: u64,
    pub start_frame: u64,
    pub duration_frames: u64,
}

impl TransitionWindow {
    pub fn contains(&self, frame: u64) -> bool {
        frame >= self.start_frame && frame < self.start_frame + self.duration_frames
    }

    /// 0..1 across the window (frame centers).
    pub fn progress(&self, frame: u64) -> f32 {
        ((frame.saturating_sub(self.start_frame)) as f32 + 0.5) / self.duration_frames.max(1) as f32
    }
}

/// Transitions whose clips are adjacent; durations are clamped to fit both clips.
pub fn transition_windows(project: &Project) -> Vec<TransitionWindow> {
    let mut windows = Vec::new();
    for track in project
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Video)
    {
        for pair in track.clips.windows(2) {
            let (from, to) = (&pair[0], &pair[1]);
            let Some(transition) = &to.transition_in else {
                continue;
            };
            if from.timeline_end_frame() != Some(to.timeline_start_frame) {
                continue;
            }
            let duration = transition
                .duration_frames
                .min(from.duration_frames)
                .min(to.duration_frames)
                & !1;
            if duration < 2 {
                continue;
            }
            windows.push(TransitionWindow {
                track_id: track.id,
                from_clip: from.id,
                to_clip: to.id,
                package: transition.package.clone(),
                cut_frame: to.timeline_start_frame,
                start_frame: to.timeline_start_frame - duration / 2,
                duration_frames: duration,
            });
        }
    }
    windows
}
