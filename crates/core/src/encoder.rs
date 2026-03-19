use anyhow::{Context, Result};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::output;
use ffmpeg_next::software::scaling::{context::Context as Scaler, flag::Flags};
use ffmpeg_next::{codec, encoder, format, frame, Dictionary, Rational};

use crate::renderer::Renderer;

// ---------------------------------------------------------------------------
// Progress tracking
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct RenderProgress {
    pub current_frame: Arc<AtomicU32>,
    pub total_frames: u32,
    pub cancelled: Arc<AtomicBool>,
}

impl RenderProgress {
    pub fn new(total_frames: u32) -> Self {
        Self {
            current_frame: Arc::new(AtomicU32::new(0)),
            total_frames,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn current(&self) -> u32 {
        self.current_frame.load(Ordering::Relaxed)
    }

    pub fn percent(&self) -> f32 {
        if self.total_frames == 0 { return 0.0; }
        self.current() as f32 / self.total_frames as f32 * 100.0
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// VideoEncoder
// ---------------------------------------------------------------------------

pub struct VideoEncoder {
    output_path: PathBuf,
    width: u32,
    height: u32,
    fps: u32,
}

#[cfg(target_os = "macos")]
fn promote_current_thread_qos() {
    // Hint the scheduler that rendering/encoding work should run at an interactive throughput level.
    use libc::qos_class_t::QOS_CLASS_USER_INITIATED;

    unsafe {
        let rc = libc::pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED, 0);
        if rc != 0 {
            log::debug!("Failed to set thread QoS class, rc={}", rc);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn promote_current_thread_qos() {}

impl VideoEncoder {
    pub fn new(output_path: impl AsRef<Path>, width: u32, height: u32, fps: u32) -> Self {
        Self {
            output_path: output_path.as_ref().to_owned(),
            // ProRes 4444 requires even dimensions
            width: width + (width % 2),
            height: height + (height % 2),
            fps,
        }
    }

    /// Render all frames in parallel and encode to ProRes 4444 with alpha.
    /// `progress`: optional progress tracker.
    /// Returns the path of the output file.
    pub fn encode(
        &self,
        renderer: Renderer,
        progress: Option<&RenderProgress>,
    ) -> Result<PathBuf> {
        promote_current_thread_qos();

        ffmpeg::init().context("Failed to initialize FFmpeg")?;

        let total = renderer.total_frames();
        let fps = self.fps;
        let width = self.width;
        let height = self.height;
        let start_timecode = renderer.start_timecode_string();

        // Ensure output directory exists
        if let Some(parent) = self.output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Cannot create output directory: {}", parent.display()))?;
        }

        // --- Streaming render + encode ---
        // Render and encode one frame at a time to avoid unbounded 4K RGBA memory usage.
        log::info!("Rendering + encoding {} frames at {}x{} @ {}fps", total, width, height, fps);

        let start_total = Instant::now();
        let mut encode_stage = Duration::default();
        let render_stage_ns = Arc::new(AtomicU64::new(0));

        let render_queue_len = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(2, 8);

        let (tx, rx) = mpsc::sync_channel::<Result<(usize, RgbaImage)>>(render_queue_len);
        let progress_for_render = progress.cloned();
        let render_stage_ns_for_thread = Arc::clone(&render_stage_ns);
        let render_handle = std::thread::spawn(move || {
            promote_current_thread_qos();
            let mut renderer = renderer;

            for i in 0..total as usize {
                if let Some(p) = &progress_for_render {
                    if p.is_cancelled() {
                        let _ = tx.send(Err(anyhow::anyhow!("Render cancelled")));
                        return;
                    }
                }

                let start_frame = Instant::now();
                let rendered = renderer
                    .render_frame(i)
                    .with_context(|| format!("Failed to render frame {}", i));
                let elapsed_ns = start_frame.elapsed().as_nanos() as u64;
                render_stage_ns_for_thread.fetch_add(elapsed_ns, Ordering::Relaxed);

                let is_ok = rendered.is_ok();
                if tx.send(rendered.map(|img| (i, img))).is_err() {
                    return;
                }

                if is_ok {
                    if let Some(p) = &progress_for_render {
                        p.current_frame.fetch_add(1, Ordering::Relaxed);
                    }
                } else {
                    return;
                }
            }
        });

        let mut octx = output(&self.output_path)
            .with_context(|| format!("Cannot open output file: {}", self.output_path.display()))?;

        let prores_codec = encoder::find_by_name("prores_ks")
            .context("prores_ks encoder not found — FFmpeg may not have been built with ProRes support")?;

        let global_header = octx.format().flags().contains(format::Flags::GLOBAL_HEADER);

        let mut ost = octx.add_stream(prores_codec)
            .context("Failed to add video stream")?;

        let mut enc = codec::context::Context::new_with_codec(prores_codec)
            .encoder()
            .video()
            .context("Not a video encoder")?;

        ost.set_parameters(&enc);
        ost.set_time_base(Rational::new(1, fps as i32));
        ost.set_rate(Rational::new(fps as i32, 1));
        ost.set_avg_frame_rate(Rational::new(fps as i32, 1));

        enc.set_width(width);
        enc.set_height(height);
        enc.set_format(format::Pixel::YUVA444P10LE);
        enc.set_frame_rate(Some(Rational::new(fps as i32, 1)));
        enc.set_time_base(Rational::new(1, fps as i32));
        let thread_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(16);
        enc.set_threading(codec::threading::Config {
            kind: codec::threading::Type::Frame,
            count: thread_count,
        });

        if global_header {
            enc.set_flags(codec::Flags::GLOBAL_HEADER);
        }

        // ProRes 4444 profile (profile 4 = 4444, supports alpha)
        let mut opts = Dictionary::new();
        // libavcodec private options use `profile`; keep `profile:v` as a compatibility alias.
        opts.set("profile", "4");
        opts.set("profile:v", "4");
        opts.set("alpha_bits", "8");
        opts.set("mbs_per_slice", "8");

        let mut encoder = enc.open_with(opts)
            .context("Failed to open ProRes encoder")?;
        ost.set_parameters(&encoder);
        ost.set_time_base(Rational::new(1, fps as i32));
        ost.set_rate(Rational::new(fps as i32, 1));
        ost.set_avg_frame_rate(Rational::new(fps as i32, 1));

        if let Some(tc) = &start_timecode {
            let mut stream_meta = Dictionary::new();
            stream_meta.set("timecode", tc);
            ost.set_metadata(stream_meta);
        }

        let mut scaler = Scaler::get(
            format::Pixel::RGBA,
            width,
            height,
            format::Pixel::YUVA444P10LE,
            width,
            height,
            Flags::FAST_BILINEAR,
        )
        .context("Failed to create RGBA -> YUVA444P10LE scaler")?;

        format::context::output::dump(&octx, 0, Some(&self.output_path.to_string_lossy()));
        let mut muxer_opts = Dictionary::new();
        muxer_opts.set("write_tmcd", "on");
        if let Some(tc) = &start_timecode {
            muxer_opts.set("timecode", tc);
        }
        octx
            .write_header_with(muxer_opts)
            .context("Failed to write video header")?;

        let time_base = Rational::new(1, fps as i32);
        let mut src_frame = frame::Video::new(format::Pixel::RGBA, width, height);
        let mut dst_frame = frame::Video::new(format::Pixel::YUVA444P10LE, width, height);

        while let Ok(rendered) = rx.recv() {
            if let Some(p) = &progress {
                if p.is_cancelled() {
                    return Err(anyhow::anyhow!("Render cancelled"));
                }
            }

            let (i, rgba_img) = rendered?;
            let start_encode_frame = Instant::now();

            // Copy RGBA pixel data into the ffmpeg frame
            let src = rgba_img.as_raw();
            // Get stride before taking a mutable borrow of data
            let row_size = width as usize * 4;
            let stride = src_frame.stride(0);
            {
                let dst = src_frame.data_mut(0);
                for row in 0..height as usize {
                    let src_off = row * row_size;
                    let dst_off = row * stride;
                    let len = row_size.min(stride);
                    dst[dst_off..dst_off + len].copy_from_slice(&src[src_off..src_off + len]);
                }
            } // drop mutable borrow of dst

            scaler
                .run(&src_frame, &mut dst_frame)
                .with_context(|| format!("Failed to scale frame {}", i))?;
            dst_frame.set_pts(Some(i as i64));

            // Send frame to encoder
            encoder.send_frame(&dst_frame)
                .with_context(|| format!("Failed to send frame {} to encoder", i))?;

            // Drain packets
            let mut packet = ffmpeg_next::Packet::empty();
            while encoder.receive_packet(&mut packet).is_ok() {
                packet.set_stream(0);
                packet.rescale_ts(time_base, octx.stream(0).unwrap().time_base());
                packet.write_interleaved(&mut octx)
                    .context("Failed to write packet")?;
            }

            encode_stage += start_encode_frame.elapsed();
        }

        render_handle
            .join()
            .map_err(|_| anyhow::anyhow!("Render worker thread panicked"))?;

        // Flush encoder
        encoder.send_eof().context("Failed to flush encoder")?;
        let mut packet = ffmpeg_next::Packet::empty();
        while encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(0);
            packet.rescale_ts(time_base, octx.stream(0).unwrap().time_base());
            packet.write_interleaved(&mut octx)
                .context("Failed to write final packets")?;
        }

        octx.write_trailer().context("Failed to write video trailer")?;

        let total_elapsed = start_total.elapsed();
        let render_stage = Duration::from_nanos(render_stage_ns.load(Ordering::Relaxed));
        let frame_count = total.max(1);
        let effective_fps = total as f64 / total_elapsed.as_secs_f64();
        log::info!(
            "Encode timing: total={:.2}s ({:.2} fps), render_stage={:.2}s ({:.2} ms/frame), encode_stage={:.2}s ({:.2} ms/frame)",
            total_elapsed.as_secs_f64(),
            effective_fps,
            render_stage.as_secs_f64(),
            render_stage.as_secs_f64() * 1000.0 / frame_count as f64,
            encode_stage.as_secs_f64(),
            encode_stage.as_secs_f64() * 1000.0 / frame_count as f64,
        );

        log::info!("Output saved to: {}", self.output_path.display());

        Ok(self.output_path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::RenderState;
    use crate::{Activity, Renderer, Template};
    use std::time::{Duration, Instant};
    use tempfile::{tempdir, TempDir};

    fn codec_tag_to_string(tag: u32) -> String {
        let bytes = [
            (tag & 0xff) as u8,
            ((tag >> 8) & 0xff) as u8,
            ((tag >> 16) & 0xff) as u8,
            ((tag >> 24) & 0xff) as u8,
        ];

        bytes
            .iter()
            .map(|b| {
                if b.is_ascii_graphic() {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect()
    }

    fn tiny_gpx() -> &'static str {
        r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="pedalmetrics-test" xmlns="http://www.topografix.com/GPX/1/1">
    <trk>
        <name>tiny</name>
        <trkseg>
            <trkpt lat="37.7749" lon="-122.4194"><ele>10.0</ele><time>2026-01-01T00:00:00Z</time></trkpt>
            <trkpt lat="37.7750" lon="-122.4193"><ele>10.5</ele><time>2026-01-01T00:00:01Z</time></trkpt>
            <trkpt lat="37.7751" lon="-122.4192"><ele>11.0</ele><time>2026-01-01T00:00:02Z</time></trkpt>
        </trkseg>
    </trk>
</gpx>"#
    }

    fn sample_30s_gpx() -> &'static str {
        include_str!("../tests/fixtures/sample_30s.gpx")
    }

    fn can_use_prores() -> bool {
        if ffmpeg::init().is_err() {
            eprintln!("Skipping tiny encode test: ffmpeg init failed");
            return false;
        }
        if encoder::find_by_name("prores_ks").is_none() {
            eprintln!("Skipping tiny encode test: prores_ks not available");
            return false;
        }
        true
    }

    fn tiny_encode_fixture() -> (Renderer, VideoEncoder, TempDir, std::path::PathBuf) {
        let mut activity = Activity::from_str(tiny_gpx()).expect("tiny GPX should parse");

        // 1 second clip at 12 fps keeps this test fast while still exercising frame loops.
        activity.trim(0, 2).expect("trim should succeed");
        activity.interpolate(12);

        let mut template = Template::default_4k();
        template.scene.width = 320;
        template.scene.height = 180;
        template.scene.fps = 12;
        template.scene.start = 0;
        template.scene.end = 1;
        template.scene.overlay_filename = "tiny_test.mov".to_string();
        template.labels.clear();
        template.values.clear();
        template.plots.clear();

        let render_state = RenderState::build(activity, template.clone(), ".")
            .expect("render state should build");
        let renderer = Renderer::new(render_state);

        let dir = tempdir().expect("temp dir should be created");
        let out = dir.path().join("tiny_output.mov");

        let encoder = VideoEncoder::new(
            &out,
            template.scene.width,
            template.scene.height,
            template.scene.fps,
        );

        (renderer, encoder, dir, out)
    }

    #[test]
    fn test_can_open_prores_ks_encoder_context() {
        // If FFmpeg isn't available in the test environment, avoid a hard failure.
        if ffmpeg::init().is_err() {
            eprintln!("Skipping test_can_open_prores_ks_encoder_context: ffmpeg init failed");
            return;
        }

        let Some(prores_codec) = encoder::find_by_name("prores_ks") else {
            eprintln!("Skipping test_can_open_prores_ks_encoder_context: prores_ks not available");
            return;
        };

        let mut enc = codec::context::Context::new_with_codec(prores_codec)
            .encoder()
            .video()
            .expect("Expected video encoder context for prores_ks");

        enc.set_width(1920);
        enc.set_height(1080);
        enc.set_format(format::Pixel::YUVA444P10LE);
        enc.set_frame_rate(Some(Rational::new(30, 1)));
        enc.set_time_base(Rational::new(1, 30));

        let mut opts = Dictionary::new();
        opts.set("profile", "4");
        opts.set("profile:v", "4");

        let opened = enc.open_with(opts);
        assert!(
            opened.is_ok(),
            "Expected prores_ks encoder to open, got: {:?}",
            opened.err()
        );
    }

    #[test]
    fn test_headless_tiny_encode_writes_output_file() {
        if !can_use_prores() {
            return;
        }

        let (renderer, encoder, _dir, out) = tiny_encode_fixture();

        let result = encoder.encode(renderer, None);
        assert!(result.is_ok(), "tiny encode should succeed: {:?}", result.err());
        assert!(out.exists(), "encoded output should exist at {}", out.display());

        let meta = std::fs::metadata(&out).expect("encoded output metadata should be readable");
        assert!(meta.len() > 0, "encoded output should be non-empty");
    }

    #[test]
    fn test_headless_tiny_encode_writes_mov_timecode_track_and_tag() {
        if !can_use_prores() {
            return;
        }

        let (renderer, encoder, _dir, out) = tiny_encode_fixture();
        let result = encoder.encode(renderer, None);
        assert!(result.is_ok(), "tiny encode should succeed: {:?}", result.err());

        let ictx = ffmpeg::format::input(&out)
            .expect("ffmpeg input stream inspection should succeed");

        let mut stream_debug = Vec::new();
        let mut has_tmcd_stream = false;
        let mut has_timecode_tag = false;

        for stream in ictx.streams() {
            let params = stream.parameters();
            let medium = params.medium();
            let tag = unsafe { (*params.as_ptr()).codec_tag };
            let tag_str = codec_tag_to_string(tag);
            let stream_metadata = stream.metadata();
            let stream_timecode = stream_metadata.get("timecode");

            stream_debug.push(format!(
                "stream={} medium={medium:?} codec_tag={tag_str} timecode={stream_timecode:?}",
                stream.index(),
            ));

            if medium == ffmpeg::media::Type::Data && tag_str == "tmcd" {
                has_tmcd_stream = true;
            }

            if stream_timecode == Some("00:00:00:00") {
                has_timecode_tag = true;
            }
        }

        let stream_debug = stream_debug.join("\n");
        assert!(
            has_tmcd_stream,
            "expected a tmcd data stream in output, stream debug:\n{}",
            stream_debug
        );
        assert!(
            has_timecode_tag,
            "expected stream timecode tag 00:00:00:00, stream debug:\n{}",
            stream_debug
        );
    }

    #[test]
    fn test_headless_tiny_encode_regression_guard() {
        if !can_use_prores() {
            return;
        }

        let (renderer, encoder, _dir, _out) = tiny_encode_fixture();
        let started = Instant::now();
        let result = encoder.encode(renderer, None);
        let elapsed = started.elapsed();

        assert!(result.is_ok(), "tiny encode should succeed: {:?}", result.err());

        // Keep this guard intentionally loose across local machines and CI variance.
        let max_elapsed = std::env::var("PEDALMETRICS_TINY_ENCODE_MAX_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(5000));

        assert!(
            elapsed <= max_elapsed,
            "Tiny encode regression: elapsed {:?} exceeded {:?}. Override with PEDALMETRICS_TINY_ENCODE_MAX_MS if needed.",
            elapsed,
            max_elapsed
        );
    }

    #[test]
    #[ignore = "Long-running benchmark-style test for local profiling"]
    fn test_headless_30s_encode_speed_profile() {
        if !can_use_prores() {
            return;
        }

        let mut activity = Activity::from_str(sample_30s_gpx())
            .expect("30s sample GPX should parse");

        // Keep source at 30 seconds and upsample for a realistic encode workload.
        activity.trim(0, 31).expect("trim should succeed");
        activity.interpolate(24);

        let mut template = Template::default_4k();
        template.scene.width = 1920;
        template.scene.height = 1080;
        template.scene.fps = 24;
        template.scene.start = 0;
        template.scene.end = 30;
        template.scene.overlay_filename = "sample_30s_speed.mov".to_string();
        template.labels.clear();
        template.values.clear();
        template.plots.clear();

        let render_state = RenderState::build(activity, template.clone(), ".")
            .expect("render state should build");
        let renderer = Renderer::new(render_state);

        let dir = tempdir().expect("temp dir should be created");
        let out = dir.path().join("sample_30s_speed.mov");

        let encoder = VideoEncoder::new(
            &out,
            template.scene.width,
            template.scene.height,
            template.scene.fps,
        );

        let started = Instant::now();
        let result = encoder.encode(renderer, None);
        let elapsed = started.elapsed();

        assert!(result.is_ok(), "30s encode should succeed: {:?}", result.err());
        assert!(out.exists(), "encoded output should exist at {}", out.display());

        eprintln!(
            "30s speed profile: elapsed={:.2}s, fps={:.2}",
            elapsed.as_secs_f64(),
            720.0 / elapsed.as_secs_f64()
        );
    }

    #[test]
    fn test_render_progress_new_and_methods() {
        let rp = RenderProgress::new(10);
        assert_eq!(rp.current(), 0);
        assert_eq!(rp.percent(), 0.0);
        assert!(!rp.is_cancelled());
        rp.current_frame.store(5, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(rp.current(), 5);
        assert_eq!(rp.percent(), 50.0);
        rp.cancel();
        assert!(rp.is_cancelled());
    }

    #[test]
    fn test_video_encoder_new_even_dimensions() {
        let enc = VideoEncoder::new("/tmp/foo.mov", 101, 202, 30);
        // Should round up to even
        assert_eq!(enc.width, 102);
        assert_eq!(enc.height, 202);
        assert_eq!(enc.fps, 30);
        assert_eq!(enc.output_path, std::path::PathBuf::from("/tmp/foo.mov"));
    }
}
