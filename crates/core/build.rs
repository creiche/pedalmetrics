fn main() {
    // On macOS with Homebrew, ffmpeg-next needs to find the FFmpeg libraries.
    // PKG_CONFIG_PATH is set here for arm64 and x86_64 Homebrew prefixes.
    #[cfg(target_os = "macos")]
    {
        // Apple Silicon
        println!("cargo:rustc-env=PKG_CONFIG_PATH=/opt/homebrew/lib/pkgconfig:/opt/homebrew/opt/ffmpeg/lib/pkgconfig");
        // x86_64 fallback
        println!("cargo:rustc-env=PKG_CONFIG_PATH=/usr/local/lib/pkgconfig:/usr/local/opt/ffmpeg/lib/pkgconfig:$PKG_CONFIG_PATH");
        println!("cargo:rustc-link-search=/opt/homebrew/lib");
        println!("cargo:rustc-link-search=/usr/local/lib");
    }
}
