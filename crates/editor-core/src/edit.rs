// SPDX-License-Identifier: AGPL-3.0-or-later

//! Builders for user-level timeline edits. Every builder is pure: it inspects the project and
//! returns an [`EditCommand`] that the caller executes through [`crate::CommandHistory`].
//! Clips sharing a `link_group` (video + audio of the same file) are edited together.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{Clip, CoreError, EditCommand, Project, TrackKind};

/// Which edge of a clip a trim operation moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimEdge {
    Head,
    Tail,
}

/// Returns an identifier larger than every identifier and link group used in the project.
pub fn next_free_id(project: &Project) -> u64 {
    project
        .media
        .iter()
        .map(|item| item.id)
        .chain(project.tracks.iter().flat_map(|track| {
            std::iter::once(track.id).chain(
                track
                    .clips
                    .iter()
                    .flat_map(|clip| std::iter::once(clip.id).chain(clip.link_group)),
            )
        }))
        .chain(project.text_overlays.iter().map(|item| item.id))
        .chain(project.transitions.iter().map(|item| item.id))
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

/// Locates a clip, returning `(track index, clip index)`.
pub fn find_clip(project: &Project, clip_id: u64) -> Option<(usize, usize)> {
    project
        .tracks
        .iter()
        .enumerate()
        .find_map(|(track_index, track)| {
            track
                .clips
                .iter()
                .position(|clip| clip.id == clip_id)
                .map(|clip_index| (track_index, clip_index))
        })
}

/// The clip itself plus every clip sharing its link group.
pub fn linked_clip_ids(project: &Project, clip_id: u64) -> Vec<u64> {
    let Some((track_index, clip_index)) = find_clip(project, clip_id) else {
        return Vec::new();
    };
    let Some(group) = project.tracks[track_index].clips[clip_index].link_group else {
        return vec![clip_id];
    };
    project
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .filter(|clip| clip.link_group == Some(group))
        .map(|clip| clip.id)
        .collect()
}

fn expand_links(project: &Project, clip_ids: &[u64]) -> HashSet<u64> {
    clip_ids
        .iter()
        .flat_map(|clip_id| linked_clip_ids(project, *clip_id))
        .collect()
}

/// Clips whose half-open timeline range contains `frame` (strictly inside or at the start).
pub fn clips_at_frame(project: &Project, frame: u64) -> Vec<u64> {
    project
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .filter(|clip| {
            clip.timeline_start_frame <= frame
                && clip.timeline_end_frame().is_some_and(|end| frame < end)
        })
        .map(|clip| clip.id)
        .collect()
}

/// Appends a media asset to the end of the first compatible video and/or audio track.
/// Video and audio clips created together are linked. Returns the command and new clip ids.
pub fn add_media_clips_command(
    project: &Project,
    media_id: u64,
) -> Result<(EditCommand, Vec<u64>), CoreError> {
    let media = project
        .media_asset(media_id)
        .ok_or(CoreError::MissingMedia(media_id))?;
    let mut targets = Vec::new();
    if media.has_video
        && let Some(track) = project
            .tracks
            .iter()
            .find(|track| track.kind == TrackKind::Video)
    {
        targets.push(track);
    }
    // Audio of a video file goes to the first audio track; audio-only files (music, voice-over)
    // go to the last audio track so they do not push the main edit around.
    let mut audio_tracks = project
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Audio);
    let audio_track = if media.has_video {
        audio_tracks.next()
    } else {
        audio_tracks.next_back()
    };
    if media.has_audio
        && let Some(track) = audio_track
    {
        targets.push(track);
    }
    if targets.is_empty() {
        return Err(CoreError::InvalidEdit(
            "no compatible video or audio track for this media",
        ));
    }
    let start = targets
        .iter()
        .flat_map(|track| track.clips.iter())
        .filter_map(Clip::timeline_end_frame)
        .max()
        .unwrap_or(0);
    let mut next_id = next_free_id(project);
    let link_group = if targets.len() > 1 {
        let group = next_id;
        next_id += 1;
        Some(group)
    } else {
        None
    };
    let mut commands = Vec::new();
    let mut created = Vec::new();
    for track in targets {
        let mut replacement = track.clips.clone();
        replacement.push(Clip {
            id: next_id,
            media_id,
            timeline_start_frame: start,
            source_start_frame: 0,
            duration_frames: media.duration_frames,
            link_group,
        });
        created.push(next_id);
        next_id += 1;
        commands.push(EditCommand::ReplaceTrackClips {
            track_id: track.id,
            expected: track.clips.clone(),
            replacement,
        });
    }
    Ok((EditCommand::Batch(commands), created))
}

/// Splits the given clips (and their linked partners) at an absolute timeline frame.
/// Clips that do not strictly contain the frame are left untouched.
pub fn split_at_command(
    project: &Project,
    clip_ids: &[u64],
    frame: u64,
) -> Result<EditCommand, CoreError> {
    let targets = expand_links(project, clip_ids);
    let mut next_id = next_free_id(project);
    let mut new_groups: HashMap<u64, u64> = HashMap::new();
    let mut commands = Vec::new();
    for track in &project.tracks {
        let mut changed = false;
        let mut replacement = Vec::with_capacity(track.clips.len() + 1);
        for clip in &track.clips {
            let end = clip.timeline_end_frame().ok_or(CoreError::InvalidClip(
                clip.id,
                "timeline frame range overflows",
            ))?;
            if !targets.contains(&clip.id) || frame <= clip.timeline_start_frame || frame >= end {
                replacement.push(clip.clone());
                continue;
            }
            let left_duration = frame - clip.timeline_start_frame;
            let mut left = clip.clone();
            left.duration_frames = left_duration;
            let right_group = clip.link_group.map(|group| {
                *new_groups.entry(group).or_insert_with(|| {
                    let id = next_id;
                    next_id += 1;
                    id
                })
            });
            let right = Clip {
                id: next_id,
                media_id: clip.media_id,
                timeline_start_frame: frame,
                source_start_frame: clip.source_start_frame + left_duration,
                duration_frames: clip.duration_frames - left_duration,
                link_group: right_group,
            };
            next_id += 1;
            replacement.push(left);
            replacement.push(right);
            changed = true;
        }
        if changed {
            commands.push(EditCommand::ReplaceTrackClips {
                track_id: track.id,
                expected: track.clips.clone(),
                replacement,
            });
        }
    }
    if commands.is_empty() {
        return Err(CoreError::InvalidEdit(
            "the playhead is not inside the selected clip",
        ));
    }
    Ok(EditCommand::Batch(commands))
}

/// Deletes clips (and linked partners). With `ripple`, later clips on each affected track move
/// left to close the gap.
pub fn delete_clips_command(
    project: &Project,
    clip_ids: &[u64],
    ripple: bool,
) -> Result<EditCommand, CoreError> {
    let targets = expand_links(project, clip_ids);
    if targets.is_empty() {
        return Err(CoreError::InvalidEdit("no clip selected"));
    }
    let mut commands = Vec::new();
    for track in &project.tracks {
        if !track.clips.iter().any(|clip| targets.contains(&clip.id)) {
            continue;
        }
        let removed: Vec<(u64, u64)> = track
            .clips
            .iter()
            .filter(|clip| targets.contains(&clip.id))
            .map(|clip| (clip.timeline_start_frame, clip.duration_frames))
            .collect();
        let replacement = track
            .clips
            .iter()
            .filter(|clip| !targets.contains(&clip.id))
            .map(|clip| {
                let mut clip = clip.clone();
                if ripple {
                    let shift: u64 = removed
                        .iter()
                        .filter(|(start, _)| *start < clip.timeline_start_frame)
                        .map(|(_, duration)| *duration)
                        .sum();
                    clip.timeline_start_frame -= shift.min(clip.timeline_start_frame);
                }
                clip
            })
            .collect();
        commands.push(EditCommand::ReplaceTrackClips {
            track_id: track.id,
            expected: track.clips.clone(),
            replacement,
        });
    }
    Ok(EditCommand::Batch(commands))
}

fn neighbors(project: &Project, track_index: usize, clip_index: usize) -> (u64, Option<u64>) {
    let clips = &project.tracks[track_index].clips;
    let previous_end = clip_index
        .checked_sub(1)
        .and_then(|index| clips[index].timeline_end_frame())
        .unwrap_or(0);
    let next_start = clips
        .get(clip_index + 1)
        .map(|clip| clip.timeline_start_frame);
    (previous_end, next_start)
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Inclusive range of frame deltas allowed when trimming `edge` of a clip and its linked partners.
pub fn trim_limits(
    project: &Project,
    clip_id: u64,
    edge: TrimEdge,
) -> Result<(i64, i64), CoreError> {
    let mut low = i64::MIN;
    let mut high = i64::MAX;
    for id in linked_clip_ids(project, clip_id) {
        let (track_index, clip_index) = find_clip(project, id).ok_or(CoreError::MissingClip(id))?;
        let clip = &project.tracks[track_index].clips[clip_index];
        let media = project
            .media_asset(clip.media_id)
            .ok_or(CoreError::MissingMedia(clip.media_id))?;
        let (previous_end, next_start) = neighbors(project, track_index, clip_index);
        let start = to_i64(clip.timeline_start_frame);
        let duration = to_i64(clip.duration_frames);
        match edge {
            TrimEdge::Head => {
                low = low
                    .max(-to_i64(clip.source_start_frame))
                    .max(to_i64(previous_end) - start);
                high = high.min(duration - 1);
            }
            TrimEdge::Tail => {
                let end = start + duration;
                let source_end = to_i64(clip.source_start_frame) + duration;
                low = low.max(1 - duration);
                high = high.min(to_i64(media.duration_frames) - source_end);
                if let Some(next_start) = next_start {
                    high = high.min(to_i64(next_start) - end);
                }
            }
        }
    }
    if low > high {
        return Err(CoreError::InvalidEdit("clip cannot be trimmed"));
    }
    Ok((low, high))
}

/// Trims one edge of a clip and its linked partners by `delta` frames.
/// For the head, positive deltas remove frames; for the tail, positive deltas extend the clip.
pub fn trim_command(
    project: &Project,
    clip_id: u64,
    edge: TrimEdge,
    delta: i64,
) -> Result<EditCommand, CoreError> {
    let (low, high) = trim_limits(project, clip_id, edge)?;
    if delta == 0 || delta < low || delta > high {
        return Err(CoreError::InvalidEdit("trim exceeds the available media"));
    }
    let ids: HashSet<u64> = linked_clip_ids(project, clip_id).into_iter().collect();
    adjust_clips(project, &ids, |clip| match edge {
        TrimEdge::Head => {
            clip.timeline_start_frame = clip.timeline_start_frame.saturating_add_signed(delta);
            clip.source_start_frame = clip.source_start_frame.saturating_add_signed(delta);
            clip.duration_frames = clip.duration_frames.saturating_add_signed(-delta);
        }
        TrimEdge::Tail => {
            clip.duration_frames = clip.duration_frames.saturating_add_signed(delta);
        }
    })
}

/// Inclusive range of frame deltas allowed when moving a clip and its linked partners.
pub fn move_limits(project: &Project, clip_id: u64) -> Result<(i64, i64), CoreError> {
    let mut low = i64::MIN;
    let mut high = i64::MAX;
    for id in linked_clip_ids(project, clip_id) {
        let (track_index, clip_index) = find_clip(project, id).ok_or(CoreError::MissingClip(id))?;
        let clip = &project.tracks[track_index].clips[clip_index];
        let (previous_end, next_start) = neighbors(project, track_index, clip_index);
        low = low.max(to_i64(previous_end) - to_i64(clip.timeline_start_frame));
        if let Some(next_start) = next_start {
            let end = to_i64(clip.timeline_start_frame) + to_i64(clip.duration_frames);
            high = high.min(to_i64(next_start) - end);
        }
    }
    Ok((low, high))
}

/// Moves a clip and its linked partners by `delta` frames without reordering clips.
pub fn move_command(project: &Project, clip_id: u64, delta: i64) -> Result<EditCommand, CoreError> {
    let (low, high) = move_limits(project, clip_id)?;
    if delta == 0 || delta < low || delta > high {
        return Err(CoreError::InvalidEdit(
            "clip would overlap a neighbor or start before zero",
        ));
    }
    let ids: HashSet<u64> = linked_clip_ids(project, clip_id).into_iter().collect();
    adjust_clips(project, &ids, |clip| {
        clip.timeline_start_frame = clip.timeline_start_frame.saturating_add_signed(delta);
    })
}

/// Removes the link between a clip and its partners so they can be edited independently.
pub fn unlink_command(project: &Project, clip_id: u64) -> Result<EditCommand, CoreError> {
    let ids: HashSet<u64> = linked_clip_ids(project, clip_id).into_iter().collect();
    if ids.len() < 2 {
        return Err(CoreError::InvalidEdit("clip is not linked"));
    }
    adjust_clips(project, &ids, |clip| clip.link_group = None)
}

fn adjust_clips(
    project: &Project,
    ids: &HashSet<u64>,
    mut change: impl FnMut(&mut Clip),
) -> Result<EditCommand, CoreError> {
    let mut per_track: BTreeMap<usize, Vec<Clip>> = BTreeMap::new();
    for (track_index, track) in project.tracks.iter().enumerate() {
        if track.clips.iter().any(|clip| ids.contains(&clip.id)) {
            let mut clips = track.clips.clone();
            clips
                .iter_mut()
                .filter(|clip| ids.contains(&clip.id))
                .for_each(&mut change);
            per_track.insert(track_index, clips);
        }
    }
    if per_track.is_empty() {
        return Err(CoreError::InvalidEdit("no clip selected"));
    }
    Ok(EditCommand::Batch(
        per_track
            .into_iter()
            .map(
                |(track_index, replacement)| EditCommand::ReplaceTrackClips {
                    track_id: project.tracks[track_index].id,
                    expected: project.tracks[track_index].clips.clone(),
                    replacement,
                },
            )
            .collect(),
    ))
}
