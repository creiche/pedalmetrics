#!/usr/bin/env bash
set -euo pipefail

# Bundle FFmpeg dynamic libraries into a macOS .app so end users do not
# need Homebrew FFmpeg installed.
#
# Usage:
#   scripts/macos_bundle_ffmpeg.sh /path/to/Pedalmetrics.app [ffmpeg_prefix]
#
# Example:
#   scripts/macos_bundle_ffmpeg.sh target/release/bundle/osx/Pedalmetrics.app

APP_PATH="${1:-}"
FFMPEG_PREFIX="${2:-}"

if [[ -z "$APP_PATH" ]]; then
  echo "Usage: $0 /path/to/Pedalmetrics.app [ffmpeg_prefix]" >&2
  exit 1
fi

if [[ ! -d "$APP_PATH" ]]; then
  echo "App bundle not found: $APP_PATH" >&2
  exit 1
fi

if [[ -z "$FFMPEG_PREFIX" ]]; then
  if command -v brew >/dev/null 2>&1; then
    FFMPEG_PREFIX="$(brew --prefix ffmpeg 2>/dev/null || true)"
  fi
fi

if [[ -z "$FFMPEG_PREFIX" ]]; then
  if [[ -d "/opt/homebrew/opt/ffmpeg" ]]; then
    FFMPEG_PREFIX="/opt/homebrew/opt/ffmpeg"
  elif [[ -d "/usr/local/opt/ffmpeg" ]]; then
    FFMPEG_PREFIX="/usr/local/opt/ffmpeg"
  fi
fi

if [[ -z "$FFMPEG_PREFIX" || ! -d "$FFMPEG_PREFIX/lib" ]]; then
  echo "Could not find FFmpeg prefix. Pass it explicitly as arg #2." >&2
  exit 1
fi

APP_BIN="$APP_PATH/Contents/MacOS/pedalmetrics"
FRAMEWORKS_DIR="$APP_PATH/Contents/Frameworks"

if [[ ! -f "$APP_BIN" ]]; then
  echo "Expected app binary at: $APP_BIN" >&2
  exit 1
fi

mkdir -p "$FRAMEWORKS_DIR"

copy_latest_lib() {
  local pattern="$1"
  local pick
  pick="$(ls -1 "$FFMPEG_PREFIX/lib"/$pattern 2>/dev/null | head -n 1 || true)"
  if [[ -z "$pick" ]]; then
    echo "Missing required FFmpeg lib pattern: $pattern" >&2
    exit 1
  fi
  cp -f "$pick" "$FRAMEWORKS_DIR/"
}

# Core FFmpeg libs + common transitive libs used by libav* on macOS
copy_latest_lib "libavcodec*.dylib"
copy_latest_lib "libavformat*.dylib"
copy_latest_lib "libavutil*.dylib"
copy_latest_lib "libavfilter*.dylib"
copy_latest_lib "libswresample*.dylib"
copy_latest_lib "libswscale*.dylib"

# Optional but often present/needed depending on FFmpeg build flags
for opt in "libavdevice*.dylib" "libpostproc*.dylib"; do
  pick="$(ls -1 "$FFMPEG_PREFIX/lib"/$opt 2>/dev/null | head -n 1 || true)"
  if [[ -n "$pick" ]]; then
    cp -f "$pick" "$FRAMEWORKS_DIR/"
  fi
done

# Normalize IDs for copied libs
for lib in "$FRAMEWORKS_DIR"/*.dylib; do
  chmod u+w "$lib"
  base="$(basename "$lib")"
  install_name_tool -id "@rpath/$base" "$lib"
done

# Ensure app binary can resolve Frameworks dir
install_name_tool -add_rpath "@executable_path/../Frameworks" "$APP_BIN" 2>/dev/null || true

# Rewrite the app binary's FFmpeg links to @rpath
while IFS= read -r dep; do
  base="$(basename "$dep")"
  if [[ -f "$FRAMEWORKS_DIR/$base" ]]; then
    install_name_tool -change "$dep" "@rpath/$base" "$APP_BIN"
  fi
done < <(otool -L "$APP_BIN" | awk '{print $1}' | grep -E '^/(opt/homebrew|usr/local|System|Library|@loader_path|@rpath|@executable_path)' | grep 'libav\|libsw\|libpostproc' || true)

# Rewrite internal lib->lib links among bundled dylibs
for lib in "$FRAMEWORKS_DIR"/*.dylib; do
  while IFS= read -r dep; do
    base="$(basename "$dep")"
    if [[ -f "$FRAMEWORKS_DIR/$base" ]]; then
      install_name_tool -change "$dep" "@rpath/$base" "$lib"
    fi
  done < <(otool -L "$lib" | awk 'NR>1 {print $1}' | grep -E 'libav|libsw|libpostproc' || true)
done

# Re-sign bundle after install_name_tool changes
if command -v codesign >/dev/null 2>&1; then
  codesign --force --deep --sign - "$APP_PATH"
fi

echo "Bundled FFmpeg libs into: $FRAMEWORKS_DIR"
echo "Verification hint: otool -L \"$APP_BIN\""
