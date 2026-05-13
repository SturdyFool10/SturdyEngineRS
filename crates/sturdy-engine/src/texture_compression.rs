// Texture compression pipeline (Track 11d).
//
// Compresses raw decoded pixel data to GPU-native block-compressed formats
// (BC4/BC5/BC7) before upload. The first compression of any source image is
// cached on disk next to the source file; subsequent loads skip compression
// entirely.
//
// ## Format selection
//
// Heuristic — applied by name if the caller doesn't override:
//
//   | Pattern in stem      | Format   | Bytes/texel |
//   |----------------------|----------|-------------|
//   | "normal", "nrm", "nm", "nor" | BC5  | 1.0  |
//   | "rough", "ao", "metal", "mask", single-channel source | BC4 | 0.5 |
//   | everything else      | BC7Srgb  | 1.0         |
//
// Normal maps use BC5 (two-channel XY, Z reconstructed in shaders).
// Single-channel utility maps use BC4. All other colour textures use BC7
// sRGB — the GPU decodes RGB from sRGB to linear on sample, which is correct
// for game-art albedo textures.
//
// ## Disk cache
//
// Cache path: `<source_dir>/.sce-cache/<stem>.<format_tag>.sceb`
// Cache key:  source file mtime (u64 Unix seconds) + (width, height) + format.
// If the source mtime matches the cached mtime the compressed bytes are loaded
// directly, bypassing the compressor. The cache is never explicitly evicted —
// it accumulates until the user's CI or build script cleans it.
//
// ## BC6H
//
// `texpresso` does not support BC6H (HDR). HDR textures (load_hdr_texture)
// keep their existing `Rgba16Float` path; they are not routed through this
// module. A future crate addition or ISPC-based encoder can add BC6H.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::Format;

// ── TextureKind ───────────────────────────────────────────────────────────────

/// Which GPU-native block format to target for a given texture.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TextureKind {
    /// BC3 sRGB — RGBA colour map (albedo, diffuse). Default.
    Albedo = 2,
    /// BC5 — XY normal map; Z reconstructed in shader.
    NormalMap = 1,
    /// BC4 — single-channel utility map (roughness, AO, metallic).
    SingleChannel = 0,
}

impl TextureKind {
    /// Infer the texture kind from a file stem and the number of source channels.
    pub fn detect(stem: &str, source_channels: u32) -> Self {
        if source_channels == 1 {
            return TextureKind::SingleChannel;
        }
        let lower = stem.to_ascii_lowercase();
        if lower.contains("normal")
            || lower.contains("_nrm")
            || lower.ends_with("_nm")
            || lower.contains("_nor")
            || lower.contains("nmap")
        {
            TextureKind::NormalMap
        } else if lower.contains("rough")
            || lower.contains("_ao")
            || lower.contains("_metal")
            || lower.contains("_mask")
            || lower.contains("_height")
            || lower.contains("_disp")
            || lower.contains("_occ")
        {
            TextureKind::SingleChannel
        } else {
            TextureKind::Albedo
        }
    }

    pub fn gpu_format(self) -> Format {
        match self {
            // BC3 sRGB — same memory cost as BC7 (1 byte/texel), lower quality
            // but supported by texpresso without a separate encoder. BC7 can
            // replace this when a pure-Rust BC7 encoder is available.
            TextureKind::Albedo => Format::Bc3UnormSrgb,
            TextureKind::NormalMap => Format::Bc5Unorm,
            TextureKind::SingleChannel => Format::Bc4Unorm,
        }
    }

    fn cache_tag(self) -> &'static str {
        match self {
            TextureKind::Albedo => "bc3s",
            TextureKind::NormalMap => "bc5",
            TextureKind::SingleChannel => "bc4",
        }
    }
}

// ── CompressedTexture ─────────────────────────────────────────────────────────

/// The result of compressing a texture: ready-to-upload block data.
pub struct CompressedTexture {
    /// Block-compressed pixel data (tightly packed 4×4 blocks).
    pub data: Vec<u8>,
    /// GPU format corresponding to the compression.
    pub format: Format,
    /// Original image dimensions (pixels, not blocks). The GPU upload uses
    /// these to allocate the image — the block count is derived internally.
    pub width: u32,
    pub height: u32,
}

// ── Cache file format ─────────────────────────────────────────────────────────
//
// `.sceb` (Sturdy Compressed Engine Block):
//   [0..8]   magic   b"SCEBC001"
//   [8..16]  src_mtime_secs  u64 LE
//   [16..20] width           u32 LE
//   [20..24] height          u32 LE
//   [24]     format_tag      u8  (0=BC4, 1=BC5, 2=BC7sRGB)
//   [25..29] data_len        u32 LE
//   [29..]   compressed data
//

const MAGIC: &[u8; 8] = b"SCEBC001";
const HEADER_LEN: usize = 29;

fn write_cache(cache_path: &Path, src_mtime: u64, tex: &CompressedTexture, kind: TextureKind) {
    // Best-effort — failures are silently ignored; the next load will re-compress.
    let mut buf = Vec::with_capacity(HEADER_LEN + tex.data.len());
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&src_mtime.to_le_bytes());
    buf.extend_from_slice(&tex.width.to_le_bytes());
    buf.extend_from_slice(&tex.height.to_le_bytes());
    let tag: u8 = kind as u8;
    buf.push(tag);
    buf.extend_from_slice(&(tex.data.len() as u32).to_le_bytes());
    buf.extend_from_slice(&tex.data);

    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(cache_path, buf);
}

fn read_cache(
    cache_path: &Path,
    src_mtime: u64,
    expected_kind: TextureKind,
) -> Option<CompressedTexture> {
    let raw = std::fs::read(cache_path).ok()?;
    if raw.len() < HEADER_LEN {
        return None;
    }
    if &raw[0..8] != MAGIC {
        return None;
    }

    let cached_mtime = u64::from_le_bytes(raw[8..16].try_into().ok()?);
    if cached_mtime != src_mtime {
        return None;
    }

    let width = u32::from_le_bytes(raw[16..20].try_into().ok()?);
    let height = u32::from_le_bytes(raw[20..24].try_into().ok()?);
    let tag = raw[24];
    let data_len = u32::from_le_bytes(raw[25..29].try_into().ok()?) as usize;

    if raw.len() < HEADER_LEN + data_len {
        return None;
    }

    let kind = match tag {
        0 => TextureKind::SingleChannel,
        1 => TextureKind::NormalMap,
        2 => TextureKind::Albedo,
        _ => return None,
    };
    if kind != expected_kind {
        return None;
    }

    Some(CompressedTexture {
        data: raw[HEADER_LEN..HEADER_LEN + data_len].to_vec(),
        format: kind.gpu_format(),
        width,
        height,
    })
}

fn cache_path_for(source_path: &Path, kind: TextureKind) -> Option<PathBuf> {
    let parent = source_path.parent()?;
    let stem = source_path.file_stem()?.to_str()?;
    Some(
        parent
            .join(".sce-cache")
            .join(format!("{}.{}.sceb", stem, kind.cache_tag())),
    )
}

fn source_mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Compression ───────────────────────────────────────────────────────────────

/// Pad a dimension to the next multiple of 4 (BC block boundary).
fn pad4(v: u32) -> u32 {
    (v + 3) & !3
}

/// Allocate the output buffer for a given texpresso format and block count.
fn alloc_output(fmt: texpresso::Format, blocks_x: usize, blocks_y: usize) -> Vec<u8> {
    let block_bytes = match fmt {
        texpresso::Format::Bc1 | texpresso::Format::Bc4 => 8,
        _ => 16,
    };
    vec![0u8; blocks_x * blocks_y * block_bytes]
}

/// Compress `rgba8` data to BC3 sRGB (DXT5). Input must be RGBA8.
///
/// texpresso does not support BC7, so BC3 is the best available pure-Rust
/// colour compressor. BC3 = BC1 colour + BC4 alpha, 16 bytes per 4×4 block.
fn compress_bc3(rgba8: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let blocks_x = w.div_ceil(4);
    let blocks_y = h.div_ceil(4);
    let params = texpresso::Params {
        algorithm: texpresso::Algorithm::IterativeClusterFit,
        ..Default::default()
    };
    let mut output = alloc_output(texpresso::Format::Bc3, blocks_x, blocks_y);
    texpresso::Format::Bc3.compress(rgba8, w, h, params, &mut output);
    output
}

/// Compress the RG channels of `rgba8` to BC5.
///
/// texpresso uses all 4 RGBA input channels but only encodes R and G into BC5.
fn compress_bc5(rgba8: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let blocks_x = w.div_ceil(4);
    let blocks_y = h.div_ceil(4);
    let params = texpresso::Params::default();
    let mut output = alloc_output(texpresso::Format::Bc5, blocks_x, blocks_y);
    texpresso::Format::Bc5.compress(rgba8, w, h, params, &mut output);
    output
}

/// Compress the R channel of `rgba8` to BC4.
fn compress_bc4(rgba8: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let blocks_x = w.div_ceil(4);
    let blocks_y = h.div_ceil(4);
    let params = texpresso::Params::default();
    let mut output = alloc_output(texpresso::Format::Bc4, blocks_x, blocks_y);
    texpresso::Format::Bc4.compress(rgba8, w, h, params, &mut output);
    output
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Attempt to compress `rgba8` pixel data for GPU upload.
///
/// - If `source_path` is `Some`, the disk cache is checked first (and written
///   on a cache miss).
/// - If compression is disabled or unsupported, returns `None` and the caller
///   should fall back to the uncompressed `Rgba8Unorm` path.
///
/// # Arguments
///
/// * `rgba8` — raw RGBA8 pixel data (width × height × 4 bytes).
/// * `width`, `height` — image dimensions in pixels.
/// * `source_channels` — number of channels in the original decoded image.
///   Used for heuristic format selection (1 → BC4, etc.).
/// * `stem` — filename stem without extension; used for kind detection.
/// * `source_path` — original file path; used for disk-cache key.
/// * `prefer_compressed` — if `false`, returns `None` immediately.
pub fn compress_texture(
    rgba8: &[u8],
    width: u32,
    height: u32,
    source_channels: u32,
    stem: &str,
    source_path: Option<&Path>,
    prefer_compressed: bool,
) -> Option<CompressedTexture> {
    if !prefer_compressed {
        return None;
    }

    // BC formats require 4×4 alignment — skip very small or oddly-sized textures.
    if width < 4 || height < 4 {
        return None;
    }

    let kind = TextureKind::detect(stem, source_channels);

    // Check disk cache.
    if let Some(src_path) = source_path {
        let mtime = source_mtime(src_path);
        if let Some(cache) = cache_path_for(src_path, kind) {
            if let Some(cached) = read_cache(&cache, mtime, kind) {
                return Some(cached);
            }
            // Cache miss — compress and store.
            let tex = run_compression(rgba8, width, height, kind);
            write_cache(&cache, mtime, &tex, kind);
            return Some(tex);
        }
    }

    // No source path (in-memory texture) — compress without caching.
    Some(run_compression(rgba8, width, height, kind))
}

fn run_compression(rgba8: &[u8], width: u32, height: u32, kind: TextureKind) -> CompressedTexture {
    // Pad the pixel data to 4×4 block boundaries if needed.
    let pw = pad4(width);
    let ph = pad4(height);
    let padded = if pw != width || ph != height {
        pad_rgba8_to_block_boundary(rgba8, width, height, pw, ph)
    } else {
        rgba8.to_vec()
    };

    let data = match kind {
        TextureKind::Albedo => compress_bc3(&padded, pw, ph),
        TextureKind::NormalMap => compress_bc5(&padded, pw, ph),
        TextureKind::SingleChannel => compress_bc4(&padded, pw, ph),
    };

    CompressedTexture {
        data,
        format: kind.gpu_format(),
        width,
        height,
    }
}

/// Pad an RGBA8 image to the given (power-of-4) dimensions by replicating
/// border pixels. This prevents artefacts at block boundaries when the
/// source dimensions are not multiples of 4.
fn pad_rgba8_to_block_boundary(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<u8> {
    let mut out = vec![0u8; (dst_w * dst_h * 4) as usize];
    for y in 0..dst_h {
        let sy = y.min(src_h - 1) as usize;
        for x in 0..dst_w {
            let sx = x.min(src_w - 1) as usize;
            let src_off = (sy * src_w as usize + sx) * 4;
            let dst_off = (y as usize * dst_w as usize + x as usize) * 4;
            out[dst_off..dst_off + 4].copy_from_slice(&src[src_off..src_off + 4]);
        }
    }
    out
}
