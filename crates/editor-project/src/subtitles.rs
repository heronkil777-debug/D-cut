// SPDX-License-Identifier: AGPL-3.0-or-later

//! Minimal, strict SubRip import into project-frame subtitle overlays.

use editor_core::TextOverlay;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SubtitleImportError {
    #[error("project frame rate must be positive")]
    InvalidFrameRate,
    #[error("invalid SRT cue {cue}: {reason}")]
    InvalidCue { cue: usize, reason: &'static str },
    #[error("subtitle frame position overflowed")]
    FrameOverflow,
    #[error("subtitle identifier overflowed")]
    IdOverflow,
}

/// Parses UTF-8 SubRip text. Time is quantized to the project frame rate, with cue ends rounded up.
pub fn parse_srt(
    source: &str,
    first_id: u64,
    subtitle_track_id: Option<u64>,
    frame_rate_numerator: u32,
    frame_rate_denominator: u32,
) -> Result<Vec<TextOverlay>, SubtitleImportError> {
    if frame_rate_numerator == 0 || frame_rate_denominator == 0 {
        return Err(SubtitleImportError::InvalidFrameRate);
    }
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let blocks: Vec<&str> = normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty())
        .collect();
    let mut overlays = Vec::with_capacity(blocks.len());
    for (index, block) in blocks.iter().enumerate() {
        let lines: Vec<&str> = block.lines().map(str::trim).collect();
        let timing_index = if lines.first().is_some_and(|line| line.contains("-->")) {
            0
        } else {
            1
        };
        let timing = lines
            .get(timing_index)
            .ok_or(SubtitleImportError::InvalidCue {
                cue: index + 1,
                reason: "missing timing line",
            })?;
        let (start_text, end_text) =
            timing
                .split_once("-->")
                .ok_or(SubtitleImportError::InvalidCue {
                    cue: index + 1,
                    reason: "timing line must contain -->",
                })?;
        let start_ms =
            parse_timestamp(start_text.trim()).ok_or(SubtitleImportError::InvalidCue {
                cue: index + 1,
                reason: "invalid start timestamp",
            })?;
        let end_text = end_text.split_whitespace().next().unwrap_or("");
        let end_ms = parse_timestamp(end_text).ok_or(SubtitleImportError::InvalidCue {
            cue: index + 1,
            reason: "invalid end timestamp",
        })?;
        if end_ms <= start_ms {
            return Err(SubtitleImportError::InvalidCue {
                cue: index + 1,
                reason: "end timestamp must be after start timestamp",
            });
        }
        let text = lines
            .iter()
            .skip(timing_index + 1)
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        if text.trim().is_empty() {
            return Err(SubtitleImportError::InvalidCue {
                cue: index + 1,
                reason: "cue text is empty",
            });
        }
        let start_frame = milliseconds_to_frames(
            start_ms,
            frame_rate_numerator,
            frame_rate_denominator,
            false,
        )?;
        let end_frame =
            milliseconds_to_frames(end_ms, frame_rate_numerator, frame_rate_denominator, true)?;
        let id = first_id
            .checked_add(index as u64)
            .ok_or(SubtitleImportError::IdOverflow)?;
        overlays.push(TextOverlay {
            id,
            track_id: subtitle_track_id,
            start_frame,
            duration_frames: end_frame.saturating_sub(start_frame).max(1),
            text,
            x: 0.5,
            y: 0.86,
            font_size: 42.0,
            color: [1.0, 1.0, 1.0, 1.0],
        });
    }
    Ok(overlays)
}

fn parse_timestamp(value: &str) -> Option<u64> {
    let (clock, millis) = value.split_once(',').or_else(|| value.split_once('.'))?;
    let mut fields = clock.split(':');
    let hours = fields.next()?.parse::<u64>().ok()?;
    let minutes = fields.next()?.parse::<u64>().ok()?;
    let seconds = fields.next()?.parse::<u64>().ok()?;
    if fields.next().is_some() || minutes >= 60 || seconds >= 60 || millis.len() != 3 {
        return None;
    }
    let millis = millis.parse::<u64>().ok()?;
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes.checked_mul(60_000)?)?
        .checked_add(seconds.checked_mul(1_000)?)?
        .checked_add(millis)
}

fn milliseconds_to_frames(
    milliseconds: u64,
    numerator: u32,
    denominator: u32,
    round_up: bool,
) -> Result<u64, SubtitleImportError> {
    let scaled = milliseconds
        .checked_mul(u64::from(numerator))
        .ok_or(SubtitleImportError::FrameOverflow)?;
    let divisor = 1_000_u64
        .checked_mul(u64::from(denominator))
        .ok_or(SubtitleImportError::FrameOverflow)?;
    let quotient = scaled / divisor;
    if round_up && scaled % divisor != 0 {
        quotient
            .checked_add(1)
            .ok_or(SubtitleImportError::FrameOverflow)
    } else {
        Ok(quotient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiline_srt_and_quantizes_frames() {
        let source = "1\r\n00:00:00,010 --> 00:00:01,001\r\nHello\r\nworld\r\n\r\n2\r\n00:00:02,000 --> 00:00:02,500\r\nSecond cue\r\n";
        let cues = parse_srt(source, 20, Some(7), 30, 1).expect("valid SRT parses");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_frame, 0);
        assert_eq!(cues[0].duration_frames, 31);
        assert_eq!(cues[0].track_id, Some(7));
        assert_eq!(cues[0].text, "Hello\nworld");
        assert_eq!(cues[1].id, 21);
    }

    #[test]
    fn rejects_reversed_timestamps() {
        let source = "1\n00:00:02,000 --> 00:00:01,000\nNope";
        assert!(matches!(
            parse_srt(source, 1, None, 30, 1),
            Err(SubtitleImportError::InvalidCue { .. })
        ));
    }
}
