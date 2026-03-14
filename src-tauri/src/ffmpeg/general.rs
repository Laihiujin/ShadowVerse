use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use async_ffmpeg_sidecar::{event::FfmpegEvent, log_parser::FfmpegLogParser};
use tokio::io::{AsyncWriteExt, BufReader};

use crate::{ffmpeg::hwaccel, progress::progress_reporter::ProgressReporterTrait};

use super::ffmpeg_path;

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(target_os = "windows")]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

/// Generate a random filename in hex
pub async fn random_filename() -> String {
    format!("{:x}", rand::random::<u64>())
}

fn escape_concat_path(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    #[cfg(target_os = "windows")]
    let path_str = {
        let s = path_str.as_ref();
        if s.starts_with(r"\\?\") {
            std::borrow::Cow::Borrowed(&s[4..])
        } else {
            path_str
        }
    };
    path_str.replace('\\', "\\\\").replace('\'', "'\\''")
}

pub async fn handle_ffmpeg_process(
    reporter: Option<&impl ProgressReporterTrait>,
    ffmpeg_process: &mut tokio::process::Command,
) -> Result<(), String> {
    log::info!("[FFmpeg] {:?}", ffmpeg_process);
    let child = ffmpeg_process
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn();
    if let Err(e) = child {
        return Err(e.to_string());
    }
    let mut child = child.unwrap();
    let stderr = child.stderr.take().unwrap();
    let reader = BufReader::new(stderr);
    let mut parser = FfmpegLogParser::new(reader);
    while let Ok(event) = parser.parse_next_event().await {
        match event {
            FfmpegEvent::Log(_level, content) => {
                // if contains "out_time_ms=66654667", by the way, it's actually in us
                if content.starts_with("out_time_ms") {
                    let time_str = content.strip_prefix("out_time_ms=").unwrap_or_default();
                    if let Some(reporter) = reporter {
                        reporter.update(time_str).await;
                    }
                }
            }
            FfmpegEvent::LogEOF => break,
            FfmpegEvent::Error(e) => {
                log::error!("[FFmpeg Error] {}", e);
                return Err(e);
            }
            _ => {}
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("ffmpeg exited with status: {}", status));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_concat_path_plain() {
        let path = Path::new("/tmp/video.mp4");
        assert_eq!(escape_concat_path(path), "/tmp/video.mp4");
    }

    #[test]
    fn test_escape_concat_path_single_quote() {
        let path = Path::new("/tmp/it's a video.mp4");
        assert_eq!(escape_concat_path(path), "/tmp/it'\\''s a video.mp4");
    }

    #[test]
    fn test_escape_concat_path_square_brackets() {
        let path = Path::new("/tmp/video [1].mp4");
        assert_eq!(escape_concat_path(path), "/tmp/video [1].mp4");
    }

    #[test]
    fn test_escape_concat_path_spaces() {
        let path = Path::new("/tmp/my video file.mp4");
        assert_eq!(escape_concat_path(path), "/tmp/my video file.mp4");
    }

    #[tokio::test]
    async fn test_random_filename() {
        let name1 = random_filename().await;
        let name2 = random_filename().await;
        assert!(!name1.is_empty());
        assert!(!name2.is_empty());
        assert!(name1.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(name2.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

pub async fn concat_videos(
    reporter: Option<&impl ProgressReporterTrait>,
    videos: &[PathBuf],
    output_path: &Path,
) -> Result<(), String> {
    concat_videos_with_transition(reporter, videos, output_path, None).await
}

pub async fn concat_videos_with_transition(
    reporter: Option<&impl ProgressReporterTrait>,
    videos: &[PathBuf],
    output_path: &Path,
    transition: Option<&str>,
) -> Result<(), String> {
    if videos.is_empty() {
        return Err("No videos to concat".to_string());
    }
    if videos.len() == 1 {
        let input = &videos[0];
        if input != output_path {
            if let Some(output_folder) = output_path.parent() {
                if !output_folder.exists() {
                    std::fs::create_dir_all(output_folder).unwrap();
                }
            }
            let _ = tokio::fs::remove_file(output_path).await;
            tokio::fs::rename(input, output_path)
                .await
                .map_err(|e| format!("Failed to rename output file: {}", e))?;
        }
        return Ok(());
    }

    let mut ffmpeg_process = tokio::process::Command::new(ffmpeg_path());
    #[cfg(target_os = "windows")]
    ffmpeg_process.creation_flags(CREATE_NO_WINDOW);

    let output_folder = output_path.parent().unwrap();
    if !output_folder.exists() {
        std::fs::create_dir_all(output_folder).unwrap();
    }

    if transition.is_none() || transition == Some("none") {
        let filelist_filename = format!("filelist_{}.txt", random_filename().await);
        let filelist_path = output_folder.join(&filelist_filename);

        let mut filelist = tokio::fs::File::create(&filelist_path).await.unwrap();
        for video in videos {
            let abs_path = tokio::fs::canonicalize(video).await.unwrap_or_else(|e| {
                log::warn!("Failed to canonicalize path {}: {e}", video.display());
                video.to_path_buf()
            });
            let escaped_path = escape_concat_path(&abs_path);
            filelist
                .write_all(format!("file '{}'\n", escaped_path).as_bytes())
                .await
                .unwrap();
        }
        filelist.flush().await.unwrap();

        let video_refs: Vec<&Path> = videos.iter().map(|p| p.as_path()).collect();
        let should_encode = !super::check_videos(&video_refs).await;

        ffmpeg_process.args([
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            filelist_path.to_str().unwrap(),
        ]);
        if should_encode {
            let video_encoder = hwaccel::get_x264_encoder().await;
            ffmpeg_process.args(["-vf", "scale=1920:1080:force_original_aspect_ratio=decrease,pad=1920:1080:(ow-iw)/2:(oh-ih)/2"]);
            ffmpeg_process.args(["-r", "60"]);
            ffmpeg_process.args(["-c:v", video_encoder]);
            ffmpeg_process.args(["-c:a", "aac"]);
            ffmpeg_process.args(["-b:v", "6000k"]);
            ffmpeg_process.args(["-b:a", "128k"]);
            ffmpeg_process.args(["-threads", "0"]);
        } else {
            ffmpeg_process.args(["-c", "copy"]);
        }
        ffmpeg_process.args([output_path.to_str().unwrap()]);
        ffmpeg_process.args(["-progress", "pipe:2"]);
        ffmpeg_process.args(["-y"]);

        let result = handle_ffmpeg_process(reporter, &mut ffmpeg_process).await;
        let _ = tokio::fs::remove_file(&filelist_path).await;
        result
    } else {
        let transition_duration = 1.0;
        let transition_type = transition.unwrap_or("fade");
        let mut durations = Vec::new();

        for video in videos {
            let metadata = super::extract_video_metadata(video).await?;
            durations.push(metadata.duration);
            ffmpeg_process.args(["-i", video.to_str().unwrap()]);
        }

        let mut filter_complex = String::new();
        for i in 0..(videos.len() - 1) {
            let left_input = if i == 0 {
                "[0:v]".to_string()
            } else {
                format!("[v{}]", i)
            };
            let output_label = if i == videos.len() - 2 {
                "outv".to_string()
            } else {
                format!("v{}", i + 1)
            };
            let offset =
                durations.iter().take(i + 1).sum::<f64>() - (i as f64 + 1.0) * transition_duration;
            filter_complex.push_str(&format!(
                "{}[{}:v]xfade=transition={}:duration={}:offset={}[{}];",
                left_input,
                i + 1,
                transition_type,
                transition_duration,
                offset,
                output_label
            ));
        }
        for i in 0..videos.len() {
            filter_complex.push_str(&format!("[{}:a]", i));
        }
        filter_complex.push_str(&format!("concat=n={}:v=0:a=1[outa]", videos.len()));

        ffmpeg_process.args(["-filter_complex", &filter_complex]);
        ffmpeg_process.args(["-map", "[outv]"]);
        ffmpeg_process.args(["-map", "[outa]"]);

        let video_encoder = hwaccel::get_x264_encoder().await;
        ffmpeg_process.args(["-c:v", video_encoder]);
        ffmpeg_process.args(["-preset", "medium"]);
        ffmpeg_process.args(["-crf", "23"]);
        ffmpeg_process.args(["-c:a", "aac"]);
        ffmpeg_process.args(["-progress", "pipe:2"]);
        ffmpeg_process.args(["-y"]);
        ffmpeg_process.args([output_path.to_str().unwrap()]);

        handle_ffmpeg_process(reporter, &mut ffmpeg_process).await
    }
}
