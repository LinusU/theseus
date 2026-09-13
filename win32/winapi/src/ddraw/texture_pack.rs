//! Dumping the textures a game draws with, and drawing with replacements.
//!
//! A texture is named by a hash of its decoded pixels (what it looks like,
//! palette and color key applied), so the name doesn't depend on the order
//! things were loaded in or on which surface holds them:
//!
//! - `THESEUS_DUMP_TEXTURES=dir` writes each texture the first time it is
//!   drawn with as `dir/<hash>.png` (RGBA; transparent pixels have alpha 0).
//! - `THESEUS_TEXTURE_PACK=dir` draws with `dir/<hash>.png` instead, whenever
//!   one exists. A replacement can be any size (texture coordinates are
//!   fractions of the texture) and gets mipmaps and smooth filtering, since
//!   a detailed texture shimmers when sampled the way the game asks for its
//!   own low-resolution ones.

use super::gpu::TextureImage;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// FNV-1a over the size and pixels.
pub fn hash(width: u32, height: u32, rgba: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in width
        .to_le_bytes()
        .iter()
        .chain(&height.to_le_bytes())
        .chain(rgba)
    {
        h ^= *byte as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn file_name(hash: u64) -> String {
    format!("{hash:016x}.png")
}

#[derive(Default)]
struct State {
    dump_dir: Option<String>,
    dumped: HashSet<u64>,
    /// Hash -> path of every replacement in the pack.
    pack: HashMap<u64, std::path::PathBuf>,
    /// Replacements decoded so far (None: failed to decode).
    loaded: HashMap<u64, Option<Rc<TextureImage>>>,
}

thread_local! {
    static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State::init());
}

impl State {
    fn init() -> State {
        let mut state = State {
            dump_dir: std::env::var("THESEUS_DUMP_TEXTURES").ok().filter(|d| !d.is_empty()),
            ..Default::default()
        };
        if let Some(dir) = &state.dump_dir {
            if let Err(err) = std::fs::create_dir_all(dir) {
                log::warn!("textures: can't create {dir}: {err}");
            }
            // Don't rewrite what an earlier run already dumped.
            state.dumped = list_pngs(dir).into_keys().collect();
        }
        if let Ok(dir) = std::env::var("THESEUS_TEXTURE_PACK") {
            state.pack = list_pngs(&dir);
            log::info!("textures: {} replacements in {dir}", state.pack.len());
        }
        state
    }
}

/// `<16 hex digits>.png` files in a directory, by hash.
fn list_pngs(dir: &str) -> HashMap<u64, std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        log::warn!("textures: can't read {dir}");
        return HashMap::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let stem = name.strip_suffix(".png").or_else(|| name.strip_suffix(".PNG"))?;
            let hash = u64::from_str_radix(stem.get(..16)?, 16).ok()?;
            (stem.len() == 16).then_some((hash, path))
        })
        .collect()
}

/// The image to upload for a texture with these pixels: its replacement if
/// the pack has one, else the pixels themselves. Dumps it if asked to.
pub fn image(width: u32, height: u32, rgba: Vec<u8>) -> TextureImage {
    STATE.with_borrow_mut(|state| {
        if state.dump_dir.is_none() && state.pack.is_empty() {
            return TextureImage::single(width, height, rgba);
        }
        let hash = hash(width, height, &rgba);
        if let Some(dir) = &state.dump_dir {
            if state.dumped.insert(hash) {
                let path = std::path::Path::new(dir).join(file_name(hash));
                if let Err(err) = write_png(&path, width, height, &rgba) {
                    log::warn!("textures: {}: {err}", path.display());
                }
            }
        }
        let Some(path) = state.pack.get(&hash) else {
            return TextureImage::single(width, height, rgba);
        };
        let replacement = state
            .loaded
            .entry(hash)
            .or_insert_with(|| match read_png(path) {
                Ok((w, h, pixels)) => {
                    log::info!("textures: replacing {hash:016x} ({width}x{height}) with {w}x{h}");
                    Some(Rc::new(with_mipmaps(w, h, pixels)))
                }
                Err(err) => {
                    log::warn!("textures: {}: {err}", path.display());
                    None
                }
            })
            .clone();
        match replacement {
            Some(image) => TextureImage {
                width: image.width,
                height: image.height,
                levels: image.levels.clone(),
                smooth: true,
            },
            None => TextureImage::single(width, height, rgba),
        }
    })
}

fn write_png(path: &std::path::Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())
}

fn read_png(path: &std::path::Path) -> Result<(u32, u32, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(info.buffer_size());
    let pixels = (info.width * info.height) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => {
            buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("unexpanded palette".into()),
    };
    if rgba.len() != pixels * 4 {
        return Err("unexpected pixel data size".into());
    }
    Ok((info.width, info.height, rgba))
}

/// Box-filter mip levels down to 1x1. Colors are weighted by alpha, so
/// transparent pixels (whose color is arbitrary) don't darken edges.
fn with_mipmaps(width: u32, height: u32, rgba: Vec<u8>) -> TextureImage {
    let mut levels = vec![rgba];
    let (mut w, mut h) = (width as usize, height as usize);
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let src = levels.last().unwrap();
        let mut out = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                let mut sum = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let sx = (x * 2 + dx).min(w - 1);
                    let sy = (y * 2 + dy).min(h - 1);
                    let p = &src[(sy * w + sx) * 4..][..4];
                    let a = p[3] as u32;
                    for c in 0..3 {
                        sum[c] += p[c] as u32 * a;
                    }
                    sum[3] += a;
                }
                let o = &mut out[(y * nw + x) * 4..][..4];
                for c in 0..3 {
                    o[c] = if sum[3] == 0 { 0 } else { (sum[c] / sum[3]) as u8 };
                }
                o[3] = (sum[3] / 4) as u8;
            }
        }
        levels.push(out);
        (w, h) = (nw, nh);
    }
    TextureImage {
        width,
        height,
        levels,
        smooth: true,
    }
}
