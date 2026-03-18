# Pedalmetrics

A desktop app for creating telemetry overlay videos from GPX activity data.

> **Independent project notice:** Pedalmetrics is an independent Rust rewrite
> inspired by [Cyclemetry](https://github.com/brycedrennan/cyclemetry). It is
> not affiliated with, endorsed by, or maintained by the original Cyclemetry
> authors.

---

## Overview

Pedalmetrics renders transparent ProRes 4444 overlay videos showing live
telemetry (speed, power, heart rate, cadence, elevation, gradient, etc.) synced
to a GPX track. The overlay can then be composited over ride footage in any
video editor.

## Why a Rust rewrite?

The original Cyclemetry project uses a React + Tauri + Python Flask + ffmpeg
subprocess architecture. Pedalmetrics replaces all of that with a single
self-contained macOS application written entirely in Rust, providing:

- **No external runtime dependencies** — no Python, no Node, no separate server
- **Native performance** — parallel frame rendering via Rayon, direct FFmpeg
  bindings (no subprocess overhead)
- **Single binary** — one app bundle to install and ship
- **Fully typed template format** — template JSON is validated at load time

## Stack

| Component | Crate |
|-----------|-------|
| GUI | `eframe` / `egui` |
| 2D rendering | `tiny-skia` |
| Text rasterization | `fontdue` |
| Image compositing | `image` + `imageproc` |
| Video encoding | `ffmpeg-next` (ProRes 4444 with alpha) |
| Parallelism | `rayon` |
| GPX parsing | `gpx` + `quick-xml` |

## Requirements

- macOS (Apple Silicon or Intel)
- [Homebrew FFmpeg](https://formulae.brew.sh/formula/ffmpeg): `brew install ffmpeg`

For local development, FFmpeg must be installed so `ffmpeg-next` can link
against the shared libraries.

## Building

```sh
cargo build --release
```

The binary will be at `target/release/pedalmetrics`.

## Packaging For End Users (No System FFmpeg Required)

If you distribute a `.app`, bundle FFmpeg dylibs into the app so users do not
need Homebrew FFmpeg installed.

1. Build your app bundle (using your preferred macOS bundler/workflow).
2. Run:

```sh
scripts/macos_bundle_ffmpeg.sh /path/to/Pedalmetrics.app
```

Optional explicit FFmpeg prefix:

```sh
scripts/macos_bundle_ffmpeg.sh /path/to/Pedalmetrics.app /opt/homebrew/opt/ffmpeg
```

This copies `libav*` and `libsw*` dylibs into `Contents/Frameworks`, rewrites
library references to `@rpath`, and re-signs the bundle ad-hoc.

Quick verification:

```sh
otool -L /path/to/Pedalmetrics.app/Contents/MacOS/pedalmetrics
```

You should see FFmpeg references resolved via `@rpath/...` instead of Homebrew
Cellar paths.

## Usage

1. Open a `.gpx` file (or drag it onto the window)
2. Select or customise a template
3. Adjust scene settings (resolution, FPS, start/end time)
4. Click **Render Video**
5. The rendered `.mov` (ProRes 4444 with alpha) is saved to `~/Downloads/Pedalmetrics/`

## License

MIT
