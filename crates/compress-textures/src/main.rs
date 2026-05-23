// compress-textures CLI (Track 11d)
//
// Pre-compresses texture asset directories to GPU-native block-compressed formats
// (BC3/BC4/BC5 via texpresso) and caches the output next to the source files.
//
// Usage:
//   compress-textures <asset_dir> [asset_dir ...]
//   compress-textures --help
//
// Each eligible image file (PNG, JPEG, WebP, BMP, TGA) found recursively in the
// given directories is:
//   1. Decoded to RGBA8 pixels.
//   2. The target format is inferred from the filename (normal → BC5, roughness/ao → BC4,
//      everything else → BC3sRGB).
//   3. Compressed and cached to `.sce-cache/<stem>.<tag>.sceb`.  Files whose cache
//      is already up-to-date (matching mtime + dimensions) are skipped.
//
// This is the offline counterpart to the at-load compression in `asset_loader.rs`.
// Running it as part of a release build pipeline eliminates first-frame compression
// latency entirely.

use std::path::{Path, PathBuf};
use std::time::Instant;

use sturdy_engine::{TextureKind, compress_texture};

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_names(false)
        .try_init();
}

fn print_usage() {
    let usage = "Usage: compress-textures <asset_dir> [asset_dir ...]\n\n\
Recursively compresses eligible images to block-compressed GPU formats\n\
(BC3sRGB / BC5 / BC4) and caches the result as .sce-cache/<stem>.<tag>.sceb.\n\n\
Format selection heuristic (same as at-load compression):\n\
  *normal*, *_nrm*, *_nm*  → BC5  (XY normal map)\n\
  *rough*, *_ao*, *_metal* → BC4  (single-channel utility)\n\
  everything else          → BC3sRGB (colour albedo)\n";
    let _ = std::io::Write::write_all(&mut std::io::stderr(), usage.as_bytes());
}

fn main() {
    init_tracing();
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        std::process::exit(if args.is_empty() { 1 } else { 0 });
    }

    let extensions: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tga", "tiff"];
    let mut total = 0u64;
    let mut compressed = 0u64;
    let mut skipped = 0u64;
    let mut errors = 0u64;
    let t0 = Instant::now();

    for dir_arg in &args {
        let base = PathBuf::from(dir_arg);
        if !base.is_dir() {
            tracing::warn!(path = %base.display(), "asset directory does not exist; skipping");
            continue;
        }
        visit_dir(&base, extensions, &mut |path: &Path| {
            total += 1;
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            // Decode source image.
            let dyn_image = match image::open(path) {
                Ok(i) => i,
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "failed to decode source image");
                    errors += 1;
                    return;
                }
            };
            let channels = dyn_image.color().channel_count() as u32;
            let rgba = dyn_image.into_rgba8();
            let (w, h) = (rgba.width(), rgba.height());
            let pixels = rgba.into_raw();

            match compress_texture(&pixels, w, h, channels, name, Some(path), true) {
                Some(_) => {
                    tracing::info!(
                        path = %path.display(),
                        width = w,
                        height = h,
                        format = TextureKind::detect(name, channels).gpu_format_name(),
                        "compressed texture"
                    );
                    compressed += 1;
                }
                None => {
                    // Already cached / not compressible (HDR, too small, etc.)
                    skipped += 1;
                }
            }
        });
    }

    let elapsed = t0.elapsed().as_secs_f32();
    tracing::info!(
        elapsed_seconds = elapsed,
        total,
        compressed,
        skipped,
        errors,
        "texture compression complete"
    );
    if errors > 0 {
        std::process::exit(1);
    }
}

fn visit_dir(dir: &Path, extensions: &[&str], callback: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden dirs and the cache dir.
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') && name != ".sce-cache" {
                visit_dir(&path, extensions, callback);
            }
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if extensions.iter().any(|&e| e.eq_ignore_ascii_case(ext)) {
                callback(&path);
            }
        }
    }
}

// Extension method to get a human-readable format name for display.
trait FormatName {
    fn gpu_format_name(self) -> &'static str;
}
impl FormatName for TextureKind {
    fn gpu_format_name(self) -> &'static str {
        match self {
            TextureKind::Albedo => "BC3sRGB",
            TextureKind::NormalMap => "BC5",
            TextureKind::SingleChannel => "BC4",
        }
    }
}
