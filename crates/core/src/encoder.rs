use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::output;
use ffmpeg_next::software::scaling::{context::Context as Scaler, flag::Flags};
use ffmpeg_next::{codec, encoder, format, frame, Dictionary, Rational};

use crate::renderer::Renderer;
use crate::constant::downloads_dir;

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
        ffmpeg::init().context("Failed to initialize FFmpeg")?;

        let total = renderer.total_frames();
        let fps = self.fps;
        let width = self.width;
        let height = self.height;

        // Ensure output directory exists
        if let Some(parent) = self.output_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Cannot create output directory: {}", parent.display()))?;
        }

        // --- Streaming render + encode ---
        // Render and encode one frame at a time to avoid unbounded 4K RGBA memory usage.
        log::info!("Rendering + encoding {} frames at {}x{} @ {}fps", total, width, height, fps);

        let mut renderer = renderer;

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

        enc.set_width(width);
        enc.set_height(height);
        enc.set_format(format::Pixel::YUVA444P10LE);
        enc.set_frame_rate(Some(Rational::new(fps as i32, 1)));
        enc.set_time_base(Rational::new(1, fps as i32));

        if global_header {
            enc.set_flags(codec::Flags::GLOBAL_HEADER);
        }

        // ProRes 4444 profile (profile 4 = 4444, supports alpha)
        let mut opts = Dictionary::new();
        opts.set("profile:v", "4");

        let mut encoder = enc.open_with(opts)
            .context("Failed to open ProRes encoder")?;
        ost.set_parameters(&encoder);

        let mut scaler = Scaler::get(
            format::Pixel::RGBA,
            width,
            height,
            format::Pixel::YUVA444P10LE,
            width,
            height,
            Flags::BILINEAR,
        )
        .context("Failed to create RGBA -> YUVA444P10LE scaler")?;

        format::context::output::dump(&octx, 0, Some(&self.output_path.to_string_lossy()));
        octx.write_header().context("Failed to write video header")?;

        let time_base = Rational::new(1, fps as i32);

        for i in 0..total as usize {
            if let Some(p) = &progress {
                if p.is_cancelled() {
                    return Err(anyhow::anyhow!("Render cancelled"));
                }
            }

            let rgba_img = renderer
                .render_frame(i)
                .with_context(|| format!("Failed to render frame {}", i))?;
            if let Some(p) = &progress {
                p.current_frame.fetch_add(1, Ordering::Relaxed);
            }

            let mut src_frame = frame::Video::new(format::Pixel::RGBA, width, height);

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

            let mut dst_frame = frame::Video::new(format::Pixel::YUVA444P10LE, width, height);
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
        }

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

        // Copy to Downloads/Pedalmetrics/
        let downloads = downloads_dir();
        std::fs::create_dir_all(&downloads).ok();
        let dest = downloads.join(
            self.output_path.file_name().unwrap_or_default()
        );
        std::fs::copy(&self.output_path, &dest).ok();
        log::info!("Output saved to: {}", dest.display());

        Ok(self.output_path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        opts.set("profile:v", "4");

        let opened = enc.open_with(opts);
        assert!(
            opened.is_ok(),
            "Expected prores_ks encoder to open, got: {:?}",
            opened.err()
        );
    }
}
