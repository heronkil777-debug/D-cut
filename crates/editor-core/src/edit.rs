// SPDX-License-Identifier: AGPL-3.0-or-later

//! Builders for user-level timeline edits. Every builder is pure: it inspects the project and
//! returns an [`EditCommand`] that the caller executes through [`crate::CommandHistory`].
//! Clips sharing a `link_group` (video + audio of the same file) are edited together.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::{Clip, ClipAudio, CoreError, EditCommand, Project, Track, TrackKind};

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
            effects: Vec::new(),
            speed: 1.0,
            audio: ClipAudio::default(),
            transition_in: None,
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
            left.audio.fade_out_frames = 0;
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
                source_start_frame: clip.source_start_frame + clip.source_frames_for(left_duration),
                duration_frames: clip.duration_frames - left_duration,
                link_group: right_group,
                effects: clip
                    .effects
                    .iter()
                    .map(|effect| effect.rebased(left_duration as i64))
                    .collect(),
                speed: clip.speed,
                audio: ClipAudio {
                    fade_in_frames: 0,
                    ..clip.audio
                },
                transition_in: None,
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
                let speed = f64::from(clip.speed);
                let source_room = (clip.source_start_frame as f64 / speed).floor() as i64;
                low = low.max(-source_room).max(to_i64(previous_end) - start);
                high = high.min(duration - 1);
            }
            TrimEdge::Tail => {
                let end = start + duration;
                let source_end = to_i64(clip.source_end_frame().unwrap_or(u64::MAX));
                let speed = f64::from(clip.speed);
                low = low.max(1 - duration);
                high = high.min(
                    ((to_i64(media.duration_frames) - source_end) as f64 / speed).floor() as i64,
                );
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
            let source_delta = (delta as f64 * f64::from(clip.speed)).round() as i64;
            clip.timeline_start_frame = clip.timeline_start_frame.saturating_add_signed(delta);
            clip.source_start_frame = clip.source_start_frame.saturating_add_signed(source_delta);
            clip.duration_frames = clip.duration_frames.saturating_add_signed(-delta);
            clip.effects = clip
                .effects
                .iter()
                .map(|effect| effect.rebased(delta))
                .collect();
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

/// Adds a track. Video tracks are inserted above the existing video tracks (earlier tracks are
/// drawn on top); other kinds are appended after the last track of the same kind.
pub fn add_track_command(project: &Project, kind: TrackKind) -> EditCommand {
    let count = project
        .tracks
        .iter()
        .filter(|track| track.kind == kind)
        .count()
        + 1;
    let prefix = match kind {
        TrackKind::Video => "V",
        TrackKind::Audio => "A",
        TrackKind::Subtitle => "T",
    };
    let track = Track {
        id: next_free_id(project),
        name: format!("{prefix}{count}"),
        kind,
        clips: Vec::new(),
    };
    let mut tracks = project.tracks.clone();
    let index = match kind {
        TrackKind::Video => tracks
            .iter()
            .position(|track| track.kind == TrackKind::Video)
            .unwrap_or(tracks.len()),
        _ => tracks
            .iter()
            .rposition(|track| track.kind == kind)
            .map_or(tracks.len(), |index| index + 1),
    };
    tracks.insert(index, track);
    EditCommand::ReplaceTracks {
        expected: project.tracks.clone(),
        replacement: tracks,
    }
}

/// Moves one clip (not its linked partners) to another track of the same kind, keeping its time.
pub fn move_clip_to_track_command(
    project: &Project,
    clip_id: u64,
    target_track_id: u64,
) -> Result<EditCommand, CoreError> {
    let (track_index, clip_index) =
        find_clip(project, clip_id).ok_or(CoreError::MissingClip(clip_id))?;
    let target_index = project
        .tracks
        .iter()
        .position(|track| track.id == target_track_id)
        .ok_or(CoreError::MissingTrack(target_track_id))?;
    if target_index == track_index {
        return Err(CoreError::InvalidEdit("clip is already on that track"));
    }
    if project.tracks[target_index].kind != project.tracks[track_index].kind {
        return Err(CoreError::InvalidEdit("tracks must be of the same kind"));
    }
    let mut tracks = project.tracks.clone();
    let clip = tracks[track_index].clips.remove(clip_index);
    let target = &mut tracks[target_index].clips;
    let position = target
        .iter()
        .position(|other| other.timeline_start_frame > clip.timeline_start_frame)
        .unwrap_or(target.len());
    target.insert(position, clip);
    Ok(EditCommand::ReplaceTracks {
        expected: project.tracks.clone(),
        replacement: tracks,
    })
}

/// Replaces one clip with a modified copy (speed, audio, effects, transition…) as a command.
pub fn update_clip_command(
    project: &Project,
    clip_id: u64,
    change: impl FnOnce(&mut Clip),
) -> Result<EditCommand, CoreError> {
    let (track_index, clip_index) =
        find_clip(project, clip_id).ok_or(CoreError::MissingClip(clip_id))?;
    let track = &project.tracks[track_index];
    let mut clips = track.clips.clone();
    change(&mut clips[clip_index]);
    Ok(EditCommand::ReplaceTrackClips {
        track_id: track.id,
        expected: track.clips.clone(),
        replacement: clips,
    })
}

/// Sets a clip's speed, keeping its source range: the timeline duration changes. Fails if the
/// longer clip would overlap the next clip.
pub fn set_speed_command(
    project: &Project,
    clip_id: u64,
    speed: f32,
) -> Result<EditCommand, CoreError> {
    if !speed.is_finite() || !(0.25..=4.0).contains(&speed) {
        return Err(CoreError::InvalidEdit("speed must be within 0.25..=4"));
    }
    let (track_index, clip_index) =
        find_clip(project, clip_id).ok_or(CoreError::MissingClip(clip_id))?;
    let clip = &project.tracks[track_index].clips[clip_index];
    let source = clip.source_frames_for(clip.duration_frames);
    let duration = ((source as f64 / f64::from(speed)).round() as u64).max(1);
    update_clip_command(project, clip_id, |clip| {
        clip.speed = speed;
        clip.duration_frames = duration;
    })
}
