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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("Usage: compress-textures <asset_dir> [asset_dir ...]");
        eprintln!();
        eprintln!("Recursively compresses eligible images to block-compressed GPU formats");
        eprintln!("(BC3sRGB / BC5 / BC4) and caches the result as .sce-cache/<stem>.<tag>.sceb.");
        eprintln!();
        eprintln!("Format selection heuristic (same as at-load compression):");
        eprintln!("  *normal*, *_nrm*, *_nm*  → BC5  (XY normal map)");
        eprintln!("  *rough*, *_ao*, *_metal* → BC4  (single-channel utility)");
        eprintln!("  everything else          → BC3sRGB (colour albedo)");
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
            eprintln!("warning: '{}' is not a directory, skipping", base.display());
            continue;
        }
        visit_dir(&base, extensions, &mut |path: &Path| {
            total += 1;
            let name = path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            // Decode source image.
            let dyn_image = match image::open(path) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("  error: {} — {e}", path.display());
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
                    println!("  compressed: {} ({}×{}, {})",
                        path.display(), w, h,
                        TextureKind::detect(name, channels).gpu_format_name());
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
    println!();
    println!("Done in {elapsed:.1}s — {total} files scanned, {compressed} compressed, {skipped} cached/skipped, {errors} errors");
    if errors > 0 {
        std::process::exit(1);
    }
}

fn visit_dir(dir: &Path, extensions: &[&str], callback: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
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
