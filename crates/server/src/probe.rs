use std::{process::Stdio, time::Duration};

use serde::Deserialize;
use tokio::{process::Command, time::timeout};

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);
const SHORT_MAX_DURATION_MS: i64 = 3 * 60 * 1000;

pub(crate) fn is_short_form_video(
    duration_ms: Option<i64>,
    width: Option<i64>,
    height: Option<i64>,
    rotation: i64,
) -> bool {
    let (Some(mut width), Some(mut height)) = (width, height) else {
        return false;
    };
    if rotation.rem_euclid(180) == 90 {
        std::mem::swap(&mut width, &mut height);
    }
    if width <= 0 || height <= 0 {
        return false;
    }

    // Vídeos verticais continuam sendo Shorts. Para vídeos quadrados ou quase
    // quadrados, a duração inferior a três minutos é obrigatória.
    if width < height {
        return true;
    }
    let is_squareish = i128::from(width) * 10 <= i128::from(height) * 11;
    let is_under_three_minutes =
        duration_ms.is_some_and(|duration| (0..SHORT_MAX_DURATION_MS).contains(&duration));
    is_squareish && is_under_three_minutes
}

#[derive(Debug)]
pub struct TechnicalMetadata {
    pub container: Option<String>,
    pub duration_ms: Option<i64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub rotation: i64,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub subtitle_codecs: Vec<String>,
    pub raw_json: String,
}

impl TechnicalMetadata {
    pub fn is_short(&self) -> bool {
        is_short_form_video(self.duration_ms, self.width, self.height, self.rotation)
    }
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    format_name: Option<String>,
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<i64>,
    height: Option<i64>,
    duration: Option<String>,
    tags: Option<ProbeTags>,
    #[serde(default)]
    side_data_list: Vec<SideData>,
}

#[derive(Debug, Deserialize)]
struct ProbeTags {
    rotate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SideData {
    rotation: Option<f64>,
}

pub async fn inspect(path: &str) -> Result<TechnicalMetadata, String> {
    let child = Command::new(crate::config::external_command(
        "MYMEDIA_FFPROBE_BIN",
        "ffprobe",
    ))
    .args([
        "-v",
        "error",
        "-show_format",
        "-show_streams",
        "-of",
        "json",
        "--",
        path,
    ])
    .stdin(Stdio::null())
    .stderr(Stdio::piped())
    .stdout(Stdio::piped())
    .kill_on_drop(true)
    .output();
    let output = timeout(PROBE_TIMEOUT, child)
        .await
        .map_err(|_| "ffprobe excedeu o limite de 20 segundos".to_owned())?
        .map_err(|error| format!("não foi possível executar ffprobe: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffprobe falhou: {}", stderr.trim()));
    }

    let raw_json = String::from_utf8(output.stdout)
        .map_err(|error| format!("ffprobe retornou JSON não-UTF-8: {error}"))?;
    parse(&raw_json)
}

fn parse(raw_json: &str) -> Result<TechnicalMetadata, String> {
    let output: ProbeOutput = serde_json::from_str(raw_json)
        .map_err(|error| format!("JSON inválido do ffprobe: {error}"))?;
    let video = output
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"));
    let audio = output
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"));
    let subtitle_codecs = output
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("subtitle"))
        .filter_map(|stream| stream.codec_name.clone())
        .collect();
    let rotation = video
        .and_then(|stream| {
            stream
                .side_data_list
                .iter()
                .find_map(|data| data.rotation)
                .map(|value| value.round() as i64)
                .or_else(|| {
                    stream
                        .tags
                        .as_ref()
                        .and_then(|tags| tags.rotate.as_deref())
                        .and_then(|value| value.parse().ok())
                })
        })
        .unwrap_or(0);
    let duration_ms = output
        .format
        .as_ref()
        .and_then(|format| format.duration.as_deref())
        .or_else(|| video.and_then(|stream| stream.duration.as_deref()))
        .and_then(parse_duration_ms);
    let container = output
        .format
        .as_ref()
        .and_then(|format| format.format_name.clone());

    Ok(TechnicalMetadata {
        container,
        duration_ms,
        width: video.and_then(|stream| stream.width),
        height: video.and_then(|stream| stream.height),
        rotation,
        video_codec: video.and_then(|stream| stream.codec_name.clone()),
        audio_codec: audio.and_then(|stream| stream.codec_name.clone()),
        subtitle_codecs,
        raw_json: raw_json.to_owned(),
    })
}

fn parse_duration_ms(value: &str) -> Option<i64> {
    let seconds = value.parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some((seconds * 1000.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rotation_and_short_classification() {
        let metadata = parse(
            r#"{
                "streams": [
                    {"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"side_data_list":[{"rotation":-90}]},
                    {"codec_type":"audio","codec_name":"aac"},
                    {"codec_type":"subtitle","codec_name":"subrip"}
                ],
                "format":{"format_name":"matroska,webm","duration":"30.125"}
            }"#,
        )
        .expect("valid ffprobe output");

        assert_eq!(metadata.duration_ms, Some(30_125));
        assert_eq!(metadata.rotation, -90);
        assert_eq!(metadata.video_codec.as_deref(), Some("h264"));
        assert_eq!(metadata.audio_codec.as_deref(), Some("aac"));
        assert_eq!(metadata.subtitle_codecs, ["subrip"]);
        assert!(metadata.is_short());
    }

    #[test]
    fn classifies_vertical_video_regardless_of_duration() {
        // 1080x1920 with 15 minutes (900 seconds) duration
        let metadata = parse(
            r#"{
                "streams": [
                    {"codec_type":"video","codec_name":"h264","width":1080,"height":1920}
                ],
                "format":{"format_name":"mp4","duration":"900.0"}
            }"#,
        )
        .expect("valid ffprobe output");

        assert_eq!(metadata.duration_ms, Some(900_000));
        assert!(metadata.is_short());

        // Horizontal video
        let horizontal = parse(
            r#"{
                "streams": [
                    {"codec_type":"video","codec_name":"h264","width":1920,"height":1080}
                ],
                "format":{"format_name":"mp4","duration":"30.0"}
            }"#,
        )
        .expect("valid ffprobe output");
        assert!(!horizontal.is_short());

        // Square video below three minutes
        let square = parse(
            r#"{
                "streams": [
                    {"codec_type":"video","codec_name":"h264","width":1080,"height":1080}
                ],
                "format":{"format_name":"mp4","duration":"30.0"}
            }"#,
        )
        .expect("valid ffprobe output");
        assert!(square.is_short());

        // Slightly landscape but still square-ish (1.1:1)
        let squareish = parse(
            r#"{
                "streams": [
                    {"codec_type":"video","codec_name":"h264","width":1188,"height":1080}
                ],
                "format":{"format_name":"mp4","duration":"179.999"}
            }"#,
        )
        .expect("valid ffprobe output");
        assert!(squareish.is_short());

        // Three minutes exactly is not less than three minutes.
        let square_three_minutes = parse(
            r#"{
                "streams": [
                    {"codec_type":"video","codec_name":"h264","width":1080,"height":1080}
                ],
                "format":{"format_name":"mp4","duration":"180.0"}
            }"#,
        )
        .expect("valid ffprobe output");
        assert!(!square_three_minutes.is_short());

        // A landscape video outside the square-ish tolerance remains normal.
        let landscape = parse(
            r#"{
                "streams": [
                    {"codec_type":"video","codec_name":"h264","width":1200,"height":1080}
                ],
                "format":{"format_name":"mp4","duration":"30.0"}
            }"#,
        )
        .expect("valid ffprobe output");
        assert!(!landscape.is_short());
    }
}
