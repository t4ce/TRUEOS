use std::fmt::Write as _;

use serde_json::json;
use transcribe_rs::{TranscriptionResult, TranscriptionSegment};

use crate::cli::OutputFormat;

pub fn render(result: &TranscriptionResult, format: OutputFormat) -> String {
    match format {
        OutputFormat::Text => format!("{}\n", result.text),
        OutputFormat::Json => render_json(result),
        OutputFormat::Jsonl => render_jsonl(result),
        OutputFormat::Srt => render_srt(result),
        OutputFormat::Vtt => render_vtt(result),
    }
}

fn render_json(result: &TranscriptionResult) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(&json_value(result))
            .expect("transcription output is JSON-serializable")
    )
}

fn render_jsonl(result: &TranscriptionResult) -> String {
    format!(
        "{}\n",
        serde_json::to_string(&json_value(result))
            .expect("transcription output is JSON-serializable")
    )
}

fn json_value(result: &TranscriptionResult) -> serde_json::Value {
    let segments = result.segments.as_ref().map(|segments| {
        segments
            .iter()
            .map(|segment| {
                let (start, end) = normalized_bounds(segment);
                json!({
                    "start": start,
                    "end": end,
                    "text": segment.text.trim(),
                })
            })
            .collect::<Vec<_>>()
    });

    json!({
        "text": result.text,
        "segments": segments,
    })
}

fn render_srt(result: &TranscriptionResult) -> String {
    let mut output = String::new();
    for (index, segment) in segments(result).enumerate() {
        let (start, end) = normalized_bounds(segment);
        let _ = writeln!(output, "{}", index + 1);
        let _ = writeln!(
            output,
            "{} --> {}",
            timestamp(start, ','),
            timestamp(end, ',')
        );
        let _ = writeln!(output, "{}\n", segment.text.trim());
    }
    output
}

fn render_vtt(result: &TranscriptionResult) -> String {
    let mut output = String::from("WEBVTT\n\n");
    for segment in segments(result) {
        let (start, end) = normalized_bounds(segment);
        let _ = writeln!(
            output,
            "{} --> {}",
            timestamp(start, '.'),
            timestamp(end, '.')
        );
        let _ = writeln!(output, "{}\n", segment.text.trim());
    }
    output
}

fn segments(result: &TranscriptionResult) -> impl Iterator<Item = &TranscriptionSegment> {
    result.segments.iter().flatten()
}

fn normalized_bounds(segment: &TranscriptionSegment) -> (f32, f32) {
    let start = finite_nonnegative(segment.start);
    let end = finite_nonnegative(segment.end).max(start);
    (start, end)
}

fn finite_nonnegative(seconds: f32) -> f32 {
    if seconds.is_finite() {
        seconds.max(0.0)
    } else {
        0.0
    }
}

fn timestamp(seconds: f32, millisecond_separator: char) -> String {
    let total_ms = (finite_nonnegative(seconds) as f64 * 1000.0).round() as u64;
    let milliseconds = total_ms % 1_000;
    let total_seconds = total_ms / 1_000;
    let seconds = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minutes = total_minutes % 60;
    let hours = total_minutes / 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}{millisecond_separator}{milliseconds:03}")
}

#[cfg(test)]
mod tests {
    use transcribe_rs::{TranscriptionResult, TranscriptionSegment};

    use crate::cli::OutputFormat;

    use super::{render, timestamp};

    fn result() -> TranscriptionResult {
        TranscriptionResult {
            text: "Hello world".to_string(),
            segments: Some(vec![TranscriptionSegment {
                start: 1.25,
                end: 62.5,
                text: " Hello world ".to_string(),
            }]),
        }
    }

    #[test]
    fn timestamp_rounds_across_second_boundary() {
        assert_eq!(timestamp(59.9996, ','), "00:01:00,000");
    }

    #[test]
    fn renders_srt() {
        assert_eq!(
            render(&result(), OutputFormat::Srt),
            "1\n00:00:01,250 --> 00:01:02,500\nHello world\n\n"
        );
    }

    #[test]
    fn renders_webvtt() {
        assert!(
            render(&result(), OutputFormat::Vtt)
                .starts_with("WEBVTT\n\n00:00:01.250 --> 00:01:02.500\nHello world\n")
        );
    }

    #[test]
    fn json_has_text_and_segments() {
        let value: serde_json::Value =
            serde_json::from_str(&render(&result(), OutputFormat::Json)).unwrap();
        assert_eq!(value["text"], "Hello world");
        assert_eq!(value["segments"][0]["start"], 1.25);
    }

    #[test]
    fn jsonl_is_one_compact_record() {
        let rendered = render(&result(), OutputFormat::Jsonl);
        assert_eq!(rendered.lines().count(), 1);
        assert!(!rendered.contains("  \"text\""));
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["text"], "Hello world");
    }
}
