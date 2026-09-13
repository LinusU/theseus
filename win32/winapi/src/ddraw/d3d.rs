//! Direct3D Immediate Mode as of DirectX 3/5: IDirect3D and the objects hung
//! off it, drawn through execute buffers.
//!
//! Execute buffers are interpreted here and their triangles drawn with wgpu
//! (see gpu.rs) into a render target standing in for the DirectDraw surface
//! the device renders to. Games still read and write that surface's memory
//! for their 2D drawing, so the two are kept in sync: GPU output is read back
//! before DirectDraw hands the surface to the game or shows it, and whatever
//! the game drew is uploaded before the next triangles.
//!
//! Only what a game has been seen to use is implemented; anything else is
//! logged the first time it shows up.

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    rc::Rc,
};

use runtime::{Context, Memory};

use super::gpu::{self, Batch, Gpu, PipelineKey, SamplerKey};
use crate::{
    ddraw::{DD, GUID, Surface, state},
    heap::Heap,
    kernel32, stub,
};

pub const IID_IDirect3D: GUID = GUID((
    0x3bba0080,
    0x2421,
    0x11cf,
    [0xa3, 0x1a, 0x00, 0xaa, 0x00, 0xb9, 0x33, 0x56],
));
pub const IID_IDirect3DHALDevice: GUID = GUID((
    0x84e63de0,
    0x46aa,
    0x11cf,
    [0x81, 0x6f, 0x00, 0x00, 0xc0, 0x20, 0x15, 0x6e],
));
pub const IID_IDirect3DTexture: GUID = GUID((
    0x2cdcd9e0,
    0x25a0,
    0x11cf,
    [0xa3, 0x1a, 0x00, 0xaa, 0x00, 0xb9, 0x33, 0x56],
));

/// sizeof(D3DDEVICEDESC) in the DirectX 5 headers, which the game was built
/// with.
const DEVICEDESC_SIZE: u32 = 0xcc;

/// sizeof(D3DTLVERTEX), the only vertex layout implemented.
const TLVERTEX_SIZE: usize = 32;

// D3DRENDERSTATETYPE values this file reads.
const RS_TEXTUREHANDLE: usize = 1;
const RS_TEXTUREADDRESS: usize = 3;
const RS_ZENABLE: usize = 7;
const RS_FILLMODE: usize = 8;
const RS_SHADEMODE: usize = 9;
const RS_ZWRITEENABLE: usize = 14;
const RS_ALPHATESTENABLE: usize = 15;
const RS_TEXTUREMAG: usize = 17;
const RS_TEXTUREMIN: usize = 18;
const RS_SRCBLEND: usize = 19;
const RS_DESTBLEND: usize = 20;
const RS_TEXTUREMAPBLEND: usize = 21;
const RS_CULLMODE: usize = 22;
const RS_ZFUNC: usize = 23;
const RS_ALPHAREF: usize = 24;
const RS_ALPHAFUNC: usize = 25;
const RS_BLENDENABLE: usize = 27;
const RS_FOGENABLE: usize = 28;
const RS_SPECULARENABLE: usize = 29;
const RS_STIPPLEDALPHA: usize = 33;
const RS_FOGCOLOR: usize = 34;
const RS_COLORKEYENABLE: usize = 41;
const RENDER_STATES: usize = 64;

/// The render states a fresh device starts with.
fn default_render_states() -> [u32; RENDER_STATES] {
    let mut rs = [0; RENDER_STATES];
    rs[RS_TEXTUREADDRESS] = 1; // WRAP
    rs[4] = 1; // TEXTUREPERSPECTIVE
    rs[RS_FILLMODE] = 3; // SOLID
    rs[RS_SHADEMODE] = 2; // GOURAUD
    rs[RS_ZWRITEENABLE] = 1;
    rs[16] = 1; // LASTPIXEL
    rs[RS_TEXTUREMAG] = 1; // NEAREST
    rs[RS_TEXTUREMIN] = 1; // NEAREST
    rs[RS_SRCBLEND] = 2; // ONE
    rs[RS_DESTBLEND] = 1; // ZERO
    rs[RS_TEXTUREMAPBLEND] = 2; // MODULATE
    rs[RS_CULLMODE] = 3; // CCW
    rs[RS_ZFUNC] = 4; // LESSEQUAL
    rs[RS_ALPHAFUNC] = 8; // ALWAYS
    rs[RS_SPECULARENABLE] = 1;
    // Version 1 devices had no switch for it: textures with a color key were
    // always keyed.
    rs[RS_COLORKEYENABLE] = 1;
    rs
}

struct ExecuteBuffer {
    size: u32,
    data: Option<u32>,
    /// D3DEXECUTEDATA: dwVertexOffset, dwVertexCount, dwInstructionOffset,
    /// dwInstructionLength, dwHVertexOffset.
    execute_data: [u32; 5],
}

struct Device {
    /// The surface rendered to.
    target: Rc<RefCell<Surface>>,
    render_states: [u32; RENDER_STATES],
    /// Vertices placed by PROCESSVERTICES, as raw D3DTLVERTEX bytes.
    vertices: Vec<[u8; TLVERTEX_SIZE]>,
}

#[derive(Default)]
pub struct D3D {
    execute_buffers: HashMap<u32, ExecuteBuffer>,
    /// IDirect3DTexture pointer (which is also its handle) -> its surface.
    textures: HashMap<u32, u32>,
    /// IDirect3DMaterial pointer (its handle) -> diffuse color.
    materials: HashMap<u32, [f32; 4]>,
    /// IDirect3DViewport pointer -> background material handle.
    backgrounds: HashMap<u32, u32>,
    next_matrix: u32,
    device: Option<Device>,
    gpu: Option<Gpu>,
    gpu_failed: bool,
    /// The GPU target holds drawing the surface's memory lacks.
    gpu_dirty: bool,
    /// The surface's memory may hold drawing the GPU target lacks.
    cpu_dirty: bool,
    /// The surface's memory as of when it and the GPU target last agreed.
    snapshot: Option<Vec<u8>>,
    /// The GPU drew since the last flip, so the flip shows its image.
    gpu_frame: bool,
    /// Host texture the full-resolution frame is shown through.
    present_texture: Option<(u32, u32, host::Surface)>,
    seen: BTreeSet<String>,
    scenes: u32,
    /// Host time (ms) of the last scene count logged.
    last_report: Option<u32>,
    triangles: u64,
}

impl D3D {
    /// Log `what` the first time it happens.
    fn note(&mut self, what: String) {
        if !self.seen.contains(&what) {
            log::info!("d3d: {what}");
            self.seen.insert(what);
        }
    }

    fn gpu(&mut self) -> Option<&mut Gpu> {
        if self.gpu.is_none() && !self.gpu_failed {
            self.gpu = Gpu::new();
            self.gpu_failed = self.gpu.is_none();
        }
        self.gpu.as_mut()
    }
}

fn note(what: String) {
    state().d3d.borrow_mut().note(what);
}

fn alloc(ctx: &mut Context, size: u32) -> u32 {
    let addr = kernel32::lock().process_heap.alloc(&mut ctx.memory, size);
    ctx.memory[addr..][..size as usize].fill(0);
    addr
}

fn write_u32s(ctx: &mut Context, addr: u32, values: &[u32]) {
    for (i, &value) in values.iter().enumerate() {
        ctx.memory.write::<u32>(addr + i as u32 * 4, value);
    }
}

/// A D3DDEVICEDESC for a hardware device with the capabilities of a good 1998
/// accelerator.
fn write_hal_desc(ctx: &mut Context, addr: u32) {
    const D3DPRIMCAPS: [u32; 14] = [
        56,      // dwSize
        0x72,    // dwMiscCaps: MASKZ | CULLNONE | CULLCW | CULLCCW
        0x1b1,   // dwRasterCaps: DITHER | ZTEST | SUBPIXEL | FOGVERTEX | FOGTABLE
        0xff,    // dwZCmpCaps: all
        0x1fff,  // dwSrcBlendCaps: all
        0x1fff,  // dwDestBlendCaps: all
        0xff,    // dwAlphaCmpCaps: all
        0x8520a, // dwShadeCaps: flat/gouraud color, specular, alpha, fog
        0xd,     // dwTextureCaps: PERSPECTIVE | ALPHA | TRANSPARENCY
        0x3f,    // dwTextureFilterCaps: nearest, linear, mipmapped
        0xcf,    // dwTextureBlendCaps: DECAL | MODULATE | DECALALPHA | MODULATEALPHA | COPY | ADD
        0x7,     // dwTextureAddressCaps: WRAP | MIRROR | CLAMP
        0,
        0, // dwStippleWidth, dwStippleHeight
    ];
    let mut desc = vec![
        DEVICEDESC_SIZE,
        0x7ff, // dwFlags: every field below is valid
        2,     // dcmColorModel: D3DCOLOR_RGB
        0x1 | 0x10 | 0x40 | 0x100 | 0x200 | 0x400, // dwDevCaps
        8,
        1, // dtcTransformCaps: D3DTRANSFORMCAPS_CLIP
        1, // bClipping
        16,
        0x7,
        1,
        8, // dlcLightingCaps: point/spot/directional, RGB model, 8 lights
    ];
    desc.extend_from_slice(&D3DPRIMCAPS); // dpcLineCaps
    desc.extend_from_slice(&D3DPRIMCAPS); // dpcTriCaps
    desc.extend_from_slice(&[
        0x400,  // dwDeviceRenderBitDepth: DDBD_16
        0x400,  // dwDeviceZBufferBitDepth: DDBD_16
        0,      // dwMaxBufferSize: no limit
        0xffff, // dwMaxVertexCount
        1, 1, 256, 256, // texture size limits
        0, 0, 0, 0, // stipple sizes
    ]);
    assert_eq!(desc.len() as u32 * 4, DEVICEDESC_SIZE);
    write_u32s(ctx, addr, &desc);
}

fn render_state_name(state: u32) -> &'static str {
    match state {
        1 => "TEXTUREHANDLE",
        2 => "ANTIALIAS",
        3 => "TEXTUREADDRESS",
        4 => "TEXTUREPERSPECTIVE",
        5 => "WRAPU",
        6 => "WRAPV",
        7 => "ZENABLE",
        8 => "FILLMODE",
        9 => "SHADEMODE",
        10 => "LINEPATTERN",
        11 => "MONOENABLE",
        12 => "ROP2",
        13 => "PLANEMASK",
        14 => "ZWRITEENABLE",
        15 => "ALPHATESTENABLE",
        16 => "LASTPIXEL",
        17 => "TEXTUREMAG",
        18 => "TEXTUREMIN",
        19 => "SRCBLEND",
        20 => "DESTBLEND",
        21 => "TEXTUREMAPBLEND",
        22 => "CULLMODE",
        23 => "ZFUNC",
        24 => "ALPHAREF",
        25 => "ALPHAFUNC",
        26 => "DITHERENABLE",
        27 => "BLENDENABLE",
        28 => "FOGENABLE",
        29 => "SPECULARENABLE",
        30 => "ZVISIBLE",
        31 => "SUBPIXEL",
        32 => "SUBPIXELX",
        33 => "STIPPLEDALPHA",
        34 => "FOGCOLOR",
        35 => "FOGTABLEMODE",
        36 => "FOGTABLESTART",
        37 => "FOGTABLEEND",
        38 => "FOGTABLEDENSITY",
        39 => "STIPPLEENABLE",
        40 => "EDGEANTIALIAS",
        41 => "COLORKEYENABLE",
        43 => "BORDERCOLOR",
        44 => "TEXTUREADDRESSU",
        45 => "TEXTUREADDRESSV",
        46 => "MIPMAPLODBIAS",
        47 => "ZBIAS",
        48 => "RANGEFOGENABLE",
        49 => "ANISOTROPY",
        50 => "FLUSHBATCH",
        _ => "?",
    }
}

/// A D3DCOLOR (0xAARRGGBB) as RGBA floats.
fn color(argb: u32) -> [f32; 4] {
    let c = |shift: u32| ((argb >> shift) & 0xff) as f32 / 255.0;
    [c(16), c(8), c(0), c(24)]
}

/// A channel mask's shift and width.
fn mask_bits(mask: u32) -> (u32, u32) {
    if mask == 0 {
        (0, 0)
    } else {
        (mask.trailing_zeros(), mask.count_ones())
    }
}

/// Decode a masked channel to 8 bits; `absent` when the mask is empty.
fn decode_channel(pixel: u32, mask: u32, absent: u8) -> u8 {
    let (shift, bits) = mask_bits(mask);
    if bits == 0 {
        return absent;
    }
    let max = (1u64 << bits) - 1;
    let value = ((pixel & mask) >> shift) as u64;
    ((value * 255 + max / 2) / max) as u8
}

fn encode_channel(value: u8, mask: u32) -> u32 {
    let (shift, bits) = mask_bits(mask);
    if bits == 0 {
        return 0;
    }
    let max = (1u32 << bits) - 1;
    ((value as u32 * max + 127) / 255) << shift
}

/// A texture surface's pixels as RGBA, color-keyed pixels made transparent.
fn texture_rgba(mem: &Memory, surface: &Surface) -> Vec<u8> {
    let (w, h) = (surface.width as usize, surface.height as usize);
    let mut out = vec![0u8; w * h * 4];
    let Some(pixels) = surface.pixels else {
        return out;
    };
    let bpp = surface.bytes_per_pixel as usize;
    let pitch = w * bpp;
    let data = &mem[pixels..][..pitch * h];
    let format = &surface.pixel_format;
    let key = surface.src_color_key;
    let palette = surface.palette.as_ref().map(|p| p.borrow());
    let lookup = |index: usize| -> [u8; 3] {
        match &palette {
            Some(p) => p
                .entries
                .get(index)
                .map_or([0; 3], |e| [e.peRed, e.peGreen, e.peBlue]),
            None => [index as u8; 3],
        }
    };
    const PALETTEINDEXED4: u32 = 0x8;
    const PALETTEINDEXED8: u32 = 0x20;
    const ALPHAPIXELS: u32 = 0x1;
    for y in 0..h {
        let row = &data[y * pitch..][..pitch];
        for x in 0..w {
            let (raw, rgba) = if format.dwFlags & PALETTEINDEXED8 != 0 {
                let index = row[x] as u32;
                let [r, g, b] = lookup(index as usize);
                (index, [r, g, b, 255])
            } else if format.dwFlags & PALETTEINDEXED4 != 0 {
                // Two to a byte, the first pixel in the high nibble.
                let byte = row[x / 2];
                let index = if x % 2 == 0 { byte >> 4 } else { byte & 0xf } as u32;
                let [r, g, b] = lookup(index as usize);
                (index, [r, g, b, 255])
            } else {
                let mut raw = 0u32;
                for i in 0..bpp.min(4) {
                    raw |= (row[x * bpp + i] as u32) << (8 * i);
                }
                let alpha = if format.dwFlags & ALPHAPIXELS != 0 {
                    decode_channel(raw, format.dwRGBAlphaBitMask, 255)
                } else {
                    255
                };
                (
                    raw,
                    [
                        decode_channel(raw, format.dwRBitMask, 0),
                        decode_channel(raw, format.dwGBitMask, 0),
                        decode_channel(raw, format.dwBBitMask, 0),
                        alpha,
                    ],
                )
            };
            let o = (y * w + x) * 4;
            out[o..o + 4].copy_from_slice(&rgba);
            if key.is_some_and(|k| k.matches(raw)) {
                out[o + 3] = 0;
            }
        }
    }
    out
}

/// DirectDraw is about to hand `surface`'s memory to the game (`writing`) or
/// show it. If it is the render target, bring the memory up to date with
/// what the GPU drew.
pub fn cpu_access(mem: &mut Memory, surface: &Surface, writing: bool) {
    let Ok(mut d3d) = state().d3d.try_borrow_mut() else {
        return;
    };
    let d3d = &mut *d3d;
    let Some(device) = &d3d.device else {
        return;
    };
    if !std::ptr::eq(device.target.as_ptr(), surface) {
        return;
    }
    if d3d.gpu_dirty {
        d3d.gpu_dirty = false;
        if let (Some(gpu), Some(pixels)) = (d3d.gpu.as_mut(), surface.pixels) {
            if let Some(rgba) = gpu.read() {
                write_rgba(mem, surface, pixels, &rgba);
                d3d.snapshot = Some(surface_bytes(mem, surface, pixels).to_vec());
            }
        }
    }
    if writing {
        d3d.cpu_dirty = true;
    }
}

/// Store RGBA pixels into a surface's memory in its own format.
fn write_rgba(mem: &mut Memory, surface: &Surface, pixels: u32, rgba: &[u8]) {
    let bpp = surface.bytes_per_pixel;
    let count = (surface.width * surface.height) as usize;
    let format = &surface.pixel_format;
    match bpp {
        2 => {
            let out = &mut mem[pixels..][..count * 2];
            for (o, p) in out.chunks_exact_mut(2).zip(rgba.chunks_exact(4)) {
                let value = encode_channel(p[0], format.dwRBitMask)
                    | encode_channel(p[1], format.dwGBitMask)
                    | encode_channel(p[2], format.dwBBitMask);
                o.copy_from_slice(&(value as u16).to_le_bytes());
            }
        }
        4 => mem[pixels..][..count * 4].copy_from_slice(&rgba[..count * 4]),
        _ => log::warn!("d3d: can't read back into a {bpp} byte per pixel surface"),
    }
}

/// Before drawing, upload whatever the game drew into the target's memory.
///
/// Only pixels that changed since the GPU and the memory last agreed (the
/// snapshot) are uploaded, so the game's 2D lands on top of the 3D without
/// replacing it with the game-resolution copy the memory holds.
fn sync_to_gpu(mem: &Memory, d3d: &mut D3D, target: &Surface) {
    if !d3d.cpu_dirty {
        return;
    }
    d3d.cpu_dirty = false;
    let Some(pixels) = target.pixels else {
        return;
    };
    let Some(rgba) = target.to_rgba(mem, &None) else {
        return;
    };
    let mut rgba = rgba.into_owned();
    let raw = surface_bytes(mem, target, pixels);
    let bpp = target.bytes_per_pixel as usize;
    let upload = match &d3d.snapshot {
        Some(snapshot) if snapshot.len() == raw.len() => {
            let mut changed = false;
            for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
                let range = i * bpp..(i + 1) * bpp;
                let same = raw[range.clone()] == snapshot[range];
                changed |= !same;
                px[3] = if same { 0 } else { 255 };
            }
            changed.then_some(true)
        }
        _ => {
            for px in rgba.chunks_exact_mut(4) {
                px[3] = 255;
            }
            Some(false)
        }
    };
    d3d.snapshot = Some(raw.to_vec());
    if let (Some(only_opaque), Some(gpu)) = (upload, d3d.gpu.as_mut()) {
        gpu.upload(&rgba, only_opaque);
    }
}

fn surface_bytes<'a>(mem: &'a Memory, surface: &Surface, pixels: u32) -> &'a [u8] {
    let size = surface.width * surface.height * surface.bytes_per_pixel;
    &mem[pixels..][..size as usize]
}

/// DirectDraw is flipping `surface` to the screen. If it is the render
/// target and the GPU drew this frame, show the GPU's full-resolution image
/// (with the game's 2D laid on top) instead of the surface's memory, and
/// report that it was shown.
///
/// Once there is a GPU, it presents to the window itself where it can (see
/// `host::Window::metal_layer`): every frame from then on, with or without
/// 3D, and nothing is read back to be shown.
pub fn present(
    mem: &mut Memory,
    surface: &Surface,
    palette: &Option<Rc<RefCell<super::Palette>>>,
    host: &mut host::Window,
) -> bool {
    let Ok(mut d3d) = state().d3d.try_borrow_mut() else {
        return false;
    };
    let d3d = &mut *d3d;
    let is_target = d3d
        .device
        .as_ref()
        .is_some_and(|device| std::ptr::eq(device.target.as_ptr(), surface));

    #[cfg(not(target_family = "wasm"))]
    if d3d.gpu.is_some() {
        if let Some(layer) = host.metal_layer() {
            let gpu_frame = is_target && std::mem::take(&mut d3d.gpu_frame);
            if gpu_frame {
                sync_to_gpu(mem, d3d, surface);
            } else if is_target {
                cpu_access(mem, surface, false);
            }
            let pixels = host.pixel_size();
            let gpu = d3d.gpu.as_mut().unwrap();
            if gpu_frame {
                gpu.present(layer, pixels, gpu::Frame::Target);
                if super::ddraw::dumping_frames() {
                    if let Some((_, Some((full, width, height)))) = gpu.read_frames(true) {
                        super::ddraw::dump_frame(&full, width, height);
                    }
                }
            } else if let Some(rgba) = surface.to_rgba(mem, palette) {
                super::ddraw::dump_frame(&rgba, surface.width, surface.height);
                gpu.present(
                    layer,
                    pixels,
                    gpu::Frame::Pixels {
                        rgba: &rgba,
                        width: surface.width,
                        height: surface.height,
                    },
                );
            }
            host.frame_presented();
            return true;
        }
    }

    if !is_target || !d3d.gpu_frame {
        return false;
    }
    d3d.gpu_frame = false;
    sync_to_gpu(mem, d3d, surface);
    let Some((small, Some((full, width, height)))) =
        d3d.gpu.as_mut().and_then(|gpu| gpu.read_frames(true))
    else {
        return false;
    };
    // The back buffer still holds what the game would have seen.
    if let Some(pixels) = surface.pixels {
        write_rgba(mem, surface, pixels, &small);
        d3d.snapshot = Some(surface_bytes(mem, surface, pixels).to_vec());
    }
    d3d.gpu_dirty = false;
    if !matches!(&d3d.present_texture, Some((w, h, _)) if (*w, *h) == (width, height)) {
        d3d.present_texture = Some((width, height, host.create_surface(width, height)));
    }
    let (_, _, texture) = d3d.present_texture.as_mut().unwrap();
    texture.set_pixels(&full, width * 4);
    super::ddraw::dump_frame(&full, width, height);
    host.render(texture);
    true
}

/// Where a texture handle's pixels are and how to draw them.
#[derive(Clone, Copy)]
struct TextureInfo {
    key: u64,
    alpha: bool,
    color_key: bool,
}

/// Upload the texture behind a handle if it changed, and describe it.
fn resolve_texture(ctx: &Context, d3d: &mut D3D, handle: u32) -> Option<TextureInfo> {
    let surface_ptr = *d3d.textures.get(&handle)?;
    let surface = state().surf.borrow().get(&surface_ptr)?.clone();
    let surface = surface.borrow();
    let key = &*surface as *const Surface as u64;
    let palette_generation = surface.palette.as_ref().map_or(0, |p| p.borrow().generation);
    let generation = surface.generation
        ^ surface.shared.generation.get().rotate_left(16)
        ^ palette_generation.rotate_left(32);
    let info = TextureInfo {
        key,
        alpha: surface.pixel_format.dwFlags & 0x1 != 0,
        color_key: surface.src_color_key.is_some(),
    };
    let format = &surface.pixel_format;
    d3d.note(format!(
        "texture format flags {:#x} {} bits masks {:#x}/{:#x}/{:#x}/{:#x}, color key {:?}, palette {}",
        format.dwFlags,
        format.dwRGBBitCount,
        format.dwRBitMask,
        format.dwGBitMask,
        format.dwBBitMask,
        format.dwRGBAlphaBitMask,
        surface.src_color_key.map(|k| (k.low, k.high)),
        surface.palette.is_some()
    ));
    let gpu = d3d.gpu.as_mut()?;
    gpu.texture(key, generation, || {
        let rgba = texture_rgba(&ctx.memory, &surface);
        // Dumped and replaced by what it looks like (see texture_pack).
        #[cfg(not(target_family = "wasm"))]
        return super::texture_pack::image(surface.width, surface.height, rgba);
        #[cfg(target_family = "wasm")]
        gpu::TextureImage::single(surface.width, surface.height, rgba)
    });
    Some(info)
}

fn filter_is_linear(filter: u32) -> bool {
    // D3DFILTER_LINEAR, _MIPLINEAR, _LINEARMIPLINEAR
    matches!(filter, 2 | 4 | 6) || linear_filter_forced()
}

/// THESEUS_D3D_FILTER=linear smooths textures even when the game asks for
/// nearest sampling.
fn linear_filter_forced() -> bool {
    #[cfg(not(target_family = "wasm"))]
    {
        static FORCED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *FORCED.get_or_init(|| std::env::var("THESEUS_D3D_FILTER").is_ok_and(|v| v == "linear"))
    }
    #[cfg(target_family = "wasm")]
    false
}

/// Run an execute buffer's instructions, drawing its triangles.
fn execute(ctx: &mut Context, data: u32, exec: &[u32; 5]) {
    let [vertex_offset, _vertex_count, instr_offset, instr_len, _hvertex_offset] = *exec;
    let mut d3d = state().d3d.borrow_mut();
    let d3d = &mut *d3d;
    let Some((width, height)) = d3d.device.as_ref().map(|d| {
        let t = d.target.borrow();
        (t.width, t.height)
    }) else {
        return;
    };
    let Some(gpu) = d3d.gpu() else {
        return;
    };
    gpu.set_target_size(width, height);
    let target = d3d.device.as_ref().unwrap().target.clone();
    sync_to_gpu(&ctx.memory, d3d, &target.borrow());

    let mut vertices: Vec<gpu::Vertex> = Vec::new();
    let mut batches: Vec<Batch> = Vec::new();
    let mut textures: HashMap<u32, Option<TextureInfo>> = HashMap::new();

    let mut at = data + instr_offset;
    let end = at + instr_len;
    'instructions: while at + 4 <= end {
        let op = ctx.memory.read::<u8>(at);
        let size = ctx.memory.read::<u8>(at + 1) as u32;
        let count = ctx.memory.read::<u16>(at + 2) as u32;
        let payload = at + 4;
        let mut next = payload + count * size;
        match op {
            3 => {
                // TRIANGLE: D3DTRIANGLE { v1, v2, v3, wFlags } per record.
                let rs = d3d.device.as_ref().unwrap().render_states;
                let handle = rs[RS_TEXTUREHANDLE];
                let texture = if handle == 0 {
                    None
                } else {
                    *textures
                        .entry(handle)
                        .or_insert_with(|| resolve_texture(ctx, d3d, handle))
                };
                let (key, sampler, flags) = triangle_state(&rs, texture);
                let alpha_ref = rs[RS_ALPHAREF] as f32 / 255.0;
                let fog = color(rs[RS_FOGCOLOR]);
                let flat = rs[RS_SHADEMODE] == 1;

                let texture_key = texture.map(|t| t.key);
                let start = vertices.len() as u32;
                let device = d3d.device.as_ref().unwrap();
                for i in 0..count {
                    let rec = payload + i * size;
                    let indices = [0, 2, 4].map(|o| ctx.memory.read::<u16>(rec + o) as usize);
                    let Some(raws) = indices
                        .iter()
                        .map(|&index| device.vertices.get(index))
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue; // an index past the vertices placed so far
                    };
                    let mut first: Option<([f32; 4], [f32; 4])> = None;
                    for raw in raws {
                        let mut v = tl_vertex(raw, width as f32, height as f32);
                        if flat {
                            let (c, s) = *first.get_or_insert((v.color, v.specular));
                            v.color = c;
                            v.specular = s;
                        }
                        v.flags = flags;
                        v.alpha_ref = alpha_ref;
                        v.fog_color = [fog[0], fog[1], fog[2]];
                        vertices.push(v);
                    }
                }
                let range = start..vertices.len() as u32;
                d3d.triangles += (range.len() / 3) as u64;
                match batches.last_mut() {
                    Some(last)
                        if last.pipeline == key
                            && last.texture == texture_key
                            && last.sampler == sampler
                            && last.range.end == range.start =>
                    {
                        last.range.end = range.end;
                    }
                    _ => batches.push(Batch {
                        pipeline: key,
                        texture: texture_key,
                        sampler,
                        range,
                    }),
                }
            }
            8 => {
                // STATERENDER: D3DSTATE { type, value } per record.
                for i in 0..count {
                    let ty = ctx.memory.read::<u32>(payload + i * size);
                    let value = ctx.memory.read::<u32>(payload + i * size + 4);
                    if ty as usize == RS_TEXTUREHANDLE {
                        d3d.note("STATERENDER TEXTUREHANDLE".into());
                    } else {
                        d3d.note(format!(
                            "STATERENDER {}={value:#x}",
                            render_state_name(ty)
                        ));
                    }
                    if let Some(slot) = d3d
                        .device
                        .as_mut()
                        .unwrap()
                        .render_states
                        .get_mut(ty as usize)
                    {
                        *slot = value;
                    }
                }
            }
            9 => {
                // PROCESSVERTICES: D3DPROCESSVERTICES { dwFlags, wStart,
                // wDest, dwCount, dwReserved } per record.
                for i in 0..count {
                    let rec = payload + i * size;
                    let flags = ctx.memory.read::<u32>(rec);
                    let start = ctx.memory.read::<u16>(rec + 4) as u32;
                    let dest = ctx.memory.read::<u16>(rec + 6) as usize;
                    let n = ctx.memory.read::<u32>(rec + 8) as usize;
                    if flags & 7 != 2 {
                        // Transforming or lighting would need D3DVERTEX input
                        // and the matrices; copying is all that's been seen.
                        d3d.note(format!("PROCESSVERTICES flags {flags:#x} treated as a copy"));
                    }
                    let device = d3d.device.as_mut().unwrap();
                    if device.vertices.len() < dest + n {
                        device.vertices.resize(dest + n, [0; TLVERTEX_SIZE]);
                    }
                    for j in 0..n {
                        let src = data + vertex_offset + (start + j as u32) * TLVERTEX_SIZE as u32;
                        device.vertices[dest + j]
                            .copy_from_slice(&ctx.memory[src..][..TLVERTEX_SIZE]);
                    }
                }
            }
            11 => break, // EXIT
            12 => {
                // BRANCHFORWARD: D3DBRANCH { dwMask, dwValue, bNegate,
                // dwOffset }. The status tested is the buffer's clip status,
                // which nothing computes, so it is always zero. The offset
                // counts from this instruction's start; zero ends the buffer.
                for i in 0..count {
                    let rec = payload + i * size;
                    let mask = ctx.memory.read::<u32>(rec);
                    let value = ctx.memory.read::<u32>(rec + 4);
                    let negate = ctx.memory.read::<u32>(rec + 8) != 0;
                    let offset = ctx.memory.read::<u32>(rec + 12);
                    let status = 0u32;
                    if ((status & mask) == value) != negate {
                        if offset == 0 {
                            break 'instructions;
                        }
                        next = at + offset;
                        break;
                    }
                }
            }
            14 => {} // SETSTATUS
            _ => {
                d3d.note(format!("execute buffer opcode {op} not implemented"));
                if !(1..=14).contains(&op) {
                    break;
                }
            }
        }
        at = next;
    }

    if !batches.is_empty() {
        d3d.gpu.as_mut().unwrap().draw(&vertices, &batches);
        d3d.gpu_dirty = true;
        d3d.gpu_frame = true;
    }
}

/// The pipeline, sampler and per-vertex flags for triangles drawn with these
/// render states.
fn triangle_state(
    rs: &[u32; RENDER_STATES],
    texture: Option<TextureInfo>,
) -> (PipelineKey, SamplerKey, u32) {
    // Stippled alpha was the fallback for cards that couldn't blend; blending
    // looks like what it was approximating.
    let blend = if rs[RS_BLENDENABLE] != 0 || rs[RS_STIPPLEDALPHA] != 0 {
        Some((rs[RS_SRCBLEND] as u8, rs[RS_DESTBLEND] as u8))
    } else {
        None
    };
    // No z-buffer is ever attached (AddAttachedSurface is unimplemented), so
    // depth testing has nothing to test against.
    let zbuffer = false;
    let key = PipelineKey {
        blend,
        depth_test: (zbuffer && rs[RS_ZENABLE] != 0).then_some(rs[RS_ZFUNC] as u8),
        depth_write: zbuffer && rs[RS_ZENABLE] != 0 && rs[RS_ZWRITEENABLE] != 0,
        color_write: true,
        cull: rs[RS_CULLMODE] as u8,
    };
    let sampler = SamplerKey {
        linear_mag: filter_is_linear(rs[RS_TEXTUREMAG]),
        linear_min: filter_is_linear(rs[RS_TEXTUREMIN]),
        smooth: false,
        address: rs[RS_TEXTUREADDRESS] as u8,
    };
    let mut flags = match texture {
        // Without a texture the diffuse color is drawn whatever the blend mode.
        None => 2,
        Some(_) => rs[RS_TEXTUREMAPBLEND] & gpu::flags::BLEND_MASK,
    };
    if let Some(texture) = texture {
        flags |= gpu::flags::TEXTURED;
        if texture.alpha {
            flags |= gpu::flags::TEXTURE_ALPHA;
        }
        if texture.color_key && rs[RS_COLORKEYENABLE] != 0 {
            flags |= gpu::flags::COLOR_KEY;
        }
    }
    if rs[RS_SPECULARENABLE] != 0 {
        flags |= gpu::flags::SPECULAR;
    }
    if rs[RS_FOGENABLE] != 0 {
        flags |= gpu::flags::FOG;
    }
    if rs[RS_ALPHATESTENABLE] != 0 {
        flags |= gpu::flags::ALPHA_TEST | ((rs[RS_ALPHAFUNC] & 0xf) << gpu::flags::ALPHA_FUNC_SHIFT);
    }
    (key, sampler, flags)
}

/// A D3DTLVERTEX (screen x/y, z, 1/w, diffuse, specular, u, v) in clip space
/// for a target of the given size.
fn tl_vertex(raw: &[u8; TLVERTEX_SIZE], width: f32, height: f32) -> gpu::Vertex {
    let f = |o: usize| f32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let d = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let (sx, sy, sz, rhw) = (f(0), f(4), f(8), f(12));
    let w = if rhw > 0.0 && rhw.is_finite() { 1.0 / rhw } else { 1.0 };
    // Direct3D puts pixel centers at integer coordinates and wgpu half a
    // pixel further on, but matching that would leave a half-pixel strip
    // along the top and left that full-screen geometry never covers, which
    // shows once the target is supersampled. Half a pixel of shift doesn't.
    let x = sx / width * 2.0 - 1.0;
    let y = 1.0 - sy / height * 2.0;
    // Games put far vertices at exactly 1; keep them inside the clip volume.
    let z = sz.clamp(0.0, 0.999_999);
    gpu::Vertex {
        pos: [x * w, y * w, z * w, w],
        color: color(d(16)),
        specular: color(d(20)),
        uv: [f(24), f(28)],
        ..Default::default()
    }
}

/// QueryInterface for objects that only answer to their own interface.
fn query_self(ctx: &mut Context, name: &str, this: u32, riid: u32, ppv: u32) -> DD {
    let iid = crate::Ptr::<GUID>::new(riid).read(&ctx.memory).unwrap();
    if iid == crate::ddraw::IID_IUnknown {
        ctx.memory.write::<u32>(ppv, this);
        return DD::OK;
    }
    log::warn!("{name}::QueryInterface({iid:?}): not supported");
    DD::E_NOINTERFACE
}

macro_rules! object_new {
    () => {
        pub static mut VTABLE: u32 = 0;

        pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
            let addr = heap.alloc(&mut ctx.memory, 4);
            ctx.memory.write(addr, unsafe { VTABLE });
            addr
        }
    };
}

fn new_object(ctx: &mut Context, new: fn(&mut Context, &mut Heap) -> u32) -> u32 {
    let mut kernel32 = kernel32::lock();
    new(ctx, &mut kernel32.process_heap)
}

pub mod IDirect3D {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 9] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "EnumDevices",
        "CreateLight",
        "CreateMaterial",
        "CreateViewport",
        "FindDevice",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3D", this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        0
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _riid: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EnumDevices(
        ctx: &mut Context,
        _this: u32,
        lpEnumDevicesCallback: u32,
        lpUserArg: u32,
    ) -> DD {
        // One hardware device. The software rasterizers real DirectX also lists
        // would only give the game something worse to pick.
        let guid = alloc(ctx, 16);
        let GUID((d1, d2, d3, d4)) = IID_IDirect3DHALDevice;
        ctx.memory.write::<u32>(guid, d1);
        ctx.memory.write::<u16>(guid + 4, d2);
        ctx.memory.write::<u16>(guid + 6, d3);
        ctx.memory[guid + 8..][..8].copy_from_slice(&d4);
        let name = b"Direct3D HAL\0";
        let name_addr = alloc(ctx, name.len() as u32);
        ctx.memory[name_addr..][..name.len()].copy_from_slice(name);
        let hw = alloc(ctx, DEVICEDESC_SIZE);
        write_hal_desc(ctx, hw);
        let hel = alloc(ctx, DEVICEDESC_SIZE);
        ctx.memory.write::<u32>(hel, DEVICEDESC_SIZE);

        // LPD3DENUMDEVICESCALLBACK(lpGuid, lpDeviceDescription, lpDeviceName,
        //                          lpD3DHWDeviceDesc, lpD3DHELDeviceDesc, lpContext)
        let callback = ctx.indirect(lpEnumDevicesCallback);
        ctx.call32_x86(callback, vec![guid, name_addr, name_addr, hw, hel, lpUserArg]);

        let heap = &kernel32::lock().process_heap;
        for addr in [guid, name_addr, hw, hel] {
            heap.free(&mut ctx.memory, addr);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateLight(
        ctx: &mut Context,
        _this: u32,
        lplpDirect3DLight: u32,
        _pUnkOuter: u32,
    ) -> DD {
        let addr = new_object(ctx, IDirect3DLight::new);
        ctx.memory.write::<u32>(lplpDirect3DLight, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateMaterial(
        ctx: &mut Context,
        _this: u32,
        lplpDirect3DMaterial: u32,
        _pUnkOuter: u32,
    ) -> DD {
        let addr = new_object(ctx, IDirect3DMaterial::new);
        ctx.memory.write::<u32>(lplpDirect3DMaterial, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateViewport(
        ctx: &mut Context,
        _this: u32,
        lplpD3DViewport: u32,
        _pUnkOuter: u32,
    ) -> DD {
        let addr = new_object(ctx, IDirect3DViewport::new);
        ctx.memory.write::<u32>(lplpD3DViewport, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn FindDevice(_ctx: &mut Context, _this: u32, _lpD3DFDS: u32, _lpD3DFDR: u32) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    object_new!();
}

pub mod IDirect3DDevice {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 22] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "GetCaps",
        "SwapTextureHandles",
        "CreateExecuteBuffer",
        "GetStats",
        "Execute",
        "AddViewport",
        "DeleteViewport",
        "NextViewport",
        "Pick",
        "GetPickRecords",
        "EnumTextureFormats",
        "CreateMatrix",
        "SetMatrix",
        "GetMatrix",
        "DeleteMatrix",
        "BeginScene",
        "EndScene",
        "GetDirect3D",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DDevice", this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        let mut d3d = state().d3d.borrow_mut();
        d3d.device = None;
        d3d.gpu_dirty = false;
        d3d.cpu_dirty = false;
        d3d.gpu_frame = false;
        d3d.snapshot = None;
        0
    }

    #[win32_derive::dllexport]
    pub fn Initialize(
        _ctx: &mut Context,
        _this: u32,
        _lpd3d: u32,
        _lpGUID: u32,
        _lpd3ddvdesc: u32,
    ) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(
        ctx: &mut Context,
        _this: u32,
        lpD3DHWDevDesc: u32,
        lpD3DHELDevDesc: u32,
    ) -> DD {
        if lpD3DHWDevDesc != 0 {
            write_hal_desc(ctx, lpD3DHWDevDesc);
        }
        if lpD3DHELDevDesc != 0 {
            ctx.memory[lpD3DHELDevDesc + 4..][..DEVICEDESC_SIZE as usize - 4].fill(0);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SwapTextureHandles(
        _ctx: &mut Context,
        _this: u32,
        _lpD3DTex1: u32,
        _lpD3DTex2: u32,
    ) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn EnumTextureFormats(
        ctx: &mut Context,
        _this: u32,
        lpd3dEnumTextureProc: u32,
        lpArg: u32,
    ) -> DD {
        // DDPF_* flags and masks: RGB565, ARGB1555, ARGB4444, 8- and 4-bit palettized.
        const FORMATS: &[(u32, u32, [u32; 4])] = &[
            (0x40, 16, [0xf800, 0x07e0, 0x001f, 0]),
            (0x41, 16, [0x7c00, 0x03e0, 0x001f, 0x8000]),
            (0x41, 16, [0x0f00, 0x00f0, 0x000f, 0xf000]),
            (0x60, 8, [0; 4]),
            (0x48, 4, [0; 4]),
        ];
        let size = std::mem::size_of::<crate::ddraw::types::DDSURFACEDESC>() as u32;
        let desc = alloc(ctx, size);
        for &(flags, bits, masks) in FORMATS {
            ctx.memory[desc..][..size as usize].fill(0);
            // DDSURFACEDESC: dwSize, dwFlags = DDSD_PIXELFORMAT | DDSD_CAPS, then
            // ddpfPixelFormat at 0x48 and ddsCaps (DDSCAPS_TEXTURE) at 0x68.
            write_u32s(ctx, desc, &[size, 0x1001]);
            write_u32s(ctx, desc + 0x48, &[32, flags, 0, bits]);
            write_u32s(ctx, desc + 0x58, &masks);
            ctx.memory.write::<u32>(desc + 0x68, 0x1000);
            let callback = ctx.indirect(lpd3dEnumTextureProc);
            ctx.call32_x86(callback, vec![desc, lpArg]);
            if ctx.cpu.regs.eax == 0 {
                break; // D3DENUMRET_CANCEL
            }
        }
        kernel32::lock().process_heap.free(&mut ctx.memory, desc);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateExecuteBuffer(
        ctx: &mut Context,
        _this: u32,
        lpDesc: u32,
        lplpDirect3DExecuteBuffer: u32,
        _pUnkOuter: u32,
    ) -> DD {
        // D3DEXECUTEBUFFERDESC: dwSize, dwFlags, dwCaps, dwBufferSize, lpData.
        let flags = ctx.memory.read::<u32>(lpDesc + 4);
        let size = ctx.memory.read::<u32>(lpDesc + 12);
        let data = if flags & 4 != 0 {
            Some(ctx.memory.read::<u32>(lpDesc + 16))
        } else {
            None
        };
        let addr = new_object(ctx, IDirect3DExecuteBuffer::new);
        state().d3d.borrow_mut().execute_buffers.insert(
            addr,
            ExecuteBuffer {
                size,
                data,
                execute_data: [0; 5],
            },
        );
        ctx.memory.write::<u32>(lplpDirect3DExecuteBuffer, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetStats(_ctx: &mut Context, _this: u32, _lpD3DStats: u32) -> DD {
        stub!(DD::OK)
    }

    #[win32_derive::dllexport]
    pub fn Execute(
        ctx: &mut Context,
        _this: u32,
        lpDirect3DExecuteBuffer: u32,
        _lpDirect3DViewport: u32,
        _dwFlags: u32,
    ) -> DD {
        let (data, exec) = {
            let d3d = state().d3d.borrow();
            let Some(buffer) = d3d.execute_buffers.get(&lpDirect3DExecuteBuffer) else {
                return DD::ERR_GENERIC;
            };
            (buffer.data, buffer.execute_data)
        };
        if let Some(data) = data {
            execute(ctx, data, &exec);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn AddViewport(_ctx: &mut Context, _this: u32, _lpDirect3DViewport: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DeleteViewport(_ctx: &mut Context, _this: u32, _lpDirect3DViewport: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn NextViewport(
        _ctx: &mut Context,
        _this: u32,
        _lpDirect3DViewport: u32,
        _lplpAnotherViewport: u32,
        _dwFlags: u32,
    ) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    #[win32_derive::dllexport]
    pub fn Pick(
        _ctx: &mut Context,
        _this: u32,
        _lpDirect3DExecuteBuffer: u32,
        _lpDirect3DViewport: u32,
        _dwFlags: u32,
        _lpRect: u32,
    ) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    #[win32_derive::dllexport]
    pub fn GetPickRecords(
        _ctx: &mut Context,
        _this: u32,
        _lpCount: u32,
        _lpD3DPickRec: u32,
    ) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    #[win32_derive::dllexport]
    pub fn CreateMatrix(ctx: &mut Context, _this: u32, lpD3DMatHandle: u32) -> DD {
        let handle = {
            let mut d3d = state().d3d.borrow_mut();
            d3d.next_matrix += 1;
            d3d.next_matrix
        };
        ctx.memory.write::<u32>(lpD3DMatHandle, handle);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetMatrix(
        _ctx: &mut Context,
        _this: u32,
        _d3dMatHandle: u32,
        _lpD3DMatrix: u32,
    ) -> DD {
        // Matrices only matter to PROCESSVERTICES transforms, which aren't
        // implemented.
        note("SetMatrix ignored".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetMatrix(
        _ctx: &mut Context,
        _this: u32,
        _D3DMatHandle: u32,
        _lpD3DMatrix: u32,
    ) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    #[win32_derive::dllexport]
    pub fn DeleteMatrix(_ctx: &mut Context, _this: u32, _d3dMatHandle: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn BeginScene(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EndScene(ctx: &mut Context, _this: u32) -> DD {
        let mut d3d = state().d3d.borrow_mut();
        d3d.scenes += 1;
        if d3d.scenes % 600 == 0 {
            let now = host::host().time();
            let fps = match d3d.last_report {
                Some(then) if now > then => format!(", {:.1} per second", 600_000.0 / (now - then) as f64),
                _ => String::new(),
            };
            d3d.last_report = Some(now);
            log::info!("d3d: {} scenes{fps}, {} triangles", d3d.scenes, d3d.triangles);
            runtime::profile::snapshot(ctx, &format!("{} scenes", d3d.scenes));
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetDirect3D(_ctx: &mut Context, _this: u32, _lplpD3D: u32) -> DD {
        todo!()
    }

    object_new!();
}

pub mod IDirect3DExecuteBuffer {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 10] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "Lock",
        "Unlock",
        "SetExecuteData",
        "GetExecuteData",
        "Validate",
        "Optimize",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DExecuteBuffer", this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(ctx: &mut Context, this: u32) -> u32 {
        let buffer = state().d3d.borrow_mut().execute_buffers.remove(&this);
        if let Some(ExecuteBuffer {
            data: Some(data), ..
        }) = buffer
        {
            kernel32::lock().process_heap.free(&mut ctx.memory, data);
        }
        0
    }

    #[win32_derive::dllexport]
    pub fn Initialize(
        _ctx: &mut Context,
        _this: u32,
        _lpDirect3DDevice: u32,
        _lpDesc: u32,
    ) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Lock(ctx: &mut Context, this: u32, lpDesc: u32) -> DD {
        let (size, data) = {
            let d3d = state().d3d.borrow();
            let buffer = &d3d.execute_buffers[&this];
            (buffer.size, buffer.data)
        };
        let data = match data {
            Some(data) => data,
            None => {
                let data = alloc(ctx, size);
                state()
                    .d3d
                    .borrow_mut()
                    .execute_buffers
                    .get_mut(&this)
                    .unwrap()
                    .data = Some(data);
                data
            }
        };
        // D3DEXECUTEBUFFERDESC: dwFlags = D3DDEB_BUFSIZE | D3DDEB_LPDATA.
        write_u32s(ctx, lpDesc + 4, &[1 | 4, 0, size, data]);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unlock(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetExecuteData(ctx: &mut Context, this: u32, lpData: u32) -> DD {
        let mut exec = [0; 5];
        for (i, value) in exec.iter_mut().enumerate() {
            *value = ctx.memory.read::<u32>(lpData + 4 + i as u32 * 4);
        }
        state()
            .d3d
            .borrow_mut()
            .execute_buffers
            .get_mut(&this)
            .unwrap()
            .execute_data = exec;
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetExecuteData(_ctx: &mut Context, _this: u32, _lpData: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Validate(
        _ctx: &mut Context,
        _this: u32,
        _lpdwOffset: u32,
        _lpFunc: u32,
        _lpUserArg: u32,
        _dwReserved: u32,
    ) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Optimize(_ctx: &mut Context, _this: u32, _dwDummy: u32) -> DD {
        DD::OK
    }

    object_new!();
}

pub mod IDirect3DViewport {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 16] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "GetViewport",
        "SetViewport",
        "TransformVertices",
        "LightElements",
        "SetBackground",
        "GetBackground",
        "SetBackgroundDepth",
        "GetBackgroundDepth",
        "Clear",
        "AddLight",
        "DeleteLight",
        "NextLight",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DViewport", this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, this: u32) -> u32 {
        state().d3d.borrow_mut().backgrounds.remove(&this);
        0
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpDirect3D: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetViewport(_ctx: &mut Context, _this: u32, _lpData: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetViewport(ctx: &mut Context, _this: u32, lpData: u32) -> DD {
        // D3DVIEWPORT: dwSize, dwX, dwY, dwWidth, dwHeight, then floats dvScaleX,
        // dvScaleY, dvMaxX, dvMaxY, dvMinZ, dvMaxZ. TL vertices are already in
        // screen space, so none of it affects drawing.
        let d = |i: u32| ctx.memory.read::<u32>(lpData + i * 4);
        note(format!(
            "viewport {}x{} at {},{}",
            d(3),
            d(4),
            d(1),
            d(2)
        ));
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn TransformVertices(
        _ctx: &mut Context,
        _this: u32,
        _dwVertexCount: u32,
        _lpData: u32,
        _dwFlags: u32,
        _lpOffscreen: u32,
    ) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn LightElements(
        _ctx: &mut Context,
        _this: u32,
        _dwElementCount: u32,
        _lpData: u32,
    ) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetBackground(_ctx: &mut Context, this: u32, hMat: u32) -> DD {
        state().d3d.borrow_mut().backgrounds.insert(this, hMat);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetBackground(_ctx: &mut Context, _this: u32, _lphMat: u32, _lpValid: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetBackgroundDepth(_ctx: &mut Context, _this: u32, _lpDDSurface: u32) -> DD {
        note("SetBackgroundDepth ignored".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetBackgroundDepth(
        _ctx: &mut Context,
        _this: u32,
        _lplpDDSurface: u32,
        _lpValid: u32,
    ) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Clear(ctx: &mut Context, this: u32, dwCount: u32, lpRects: u32, dwFlags: u32) -> DD {
        const D3DCLEAR_TARGET: u32 = 1;
        const D3DCLEAR_ZBUFFER: u32 = 2;
        let mut d3d = state().d3d.borrow_mut();
        let d3d = &mut *d3d;
        let Some((width, height)) = d3d.device.as_ref().map(|d| {
            let t = d.target.borrow();
            (t.width, t.height)
        }) else {
            return DD::OK;
        };
        let background = d3d
            .backgrounds
            .get(&this)
            .and_then(|m| d3d.materials.get(m))
            .copied()
            .unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let rects: Vec<[u32; 4]> = (0..dwCount)
            .map(|i| {
                let r = |o: u32| ctx.memory.read::<i32>(lpRects + i * 16 + o).max(0) as u32;
                [r(0), r(4), r(8).min(width), r(12).min(height)]
            })
            .collect();
        let covers_all = rects.iter().any(|r| *r == [0, 0, width, height]);
        if dwFlags & D3DCLEAR_TARGET != 0 && covers_all {
            // Whatever the game drew is about to be painted over.
            d3d.cpu_dirty = false;
        }
        let Some(gpu) = d3d.gpu() else {
            return DD::OK;
        };
        gpu.set_target_size(width, height);
        let target = d3d.device.as_ref().unwrap().target.clone();
        sync_to_gpu(&ctx.memory, d3d, &target.borrow());
        let gpu = d3d.gpu.as_mut().unwrap();
        for rect in rects {
            gpu.clear(
                Some(rect),
                (dwFlags & D3DCLEAR_TARGET != 0).then_some(background),
                (dwFlags & D3DCLEAR_ZBUFFER != 0).then_some(1.0),
            );
        }
        d3d.gpu_dirty = true;
        d3d.gpu_frame = true;
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn AddLight(_ctx: &mut Context, _this: u32, _lpDirect3DLight: u32) -> DD {
        note("AddLight ignored".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DeleteLight(_ctx: &mut Context, _this: u32, _lpDirect3DLight: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn NextLight(
        _ctx: &mut Context,
        _this: u32,
        _lpDirect3DLight: u32,
        _lplpDirect3DLight: u32,
        _dwFlags: u32,
    ) -> DD {
        todo!()
    }

    object_new!();
}

pub mod IDirect3DMaterial {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 9] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "SetMaterial",
        "GetMaterial",
        "GetHandle",
        "Reserve",
        "Unreserve",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DMaterial", this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, this: u32) -> u32 {
        state().d3d.borrow_mut().materials.remove(&this);
        0
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpDirect3D: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetMaterial(ctx: &mut Context, this: u32, lpMat: u32) -> DD {
        // D3DMATERIAL: dwSize, then the diffuse D3DCOLORVALUE (r, g, b, a).
        let c = |i: u32| ctx.memory.read::<f32>(lpMat + 4 + i * 4);
        let diffuse = [c(0), c(1), c(2), c(3)];
        state().d3d.borrow_mut().materials.insert(this, diffuse);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetMaterial(_ctx: &mut Context, _this: u32, _lpMat: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetHandle(ctx: &mut Context, this: u32, _lpDirect3DDevice: u32, lpHandle: u32) -> DD {
        ctx.memory.write::<u32>(lpHandle, this);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Reserve(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unreserve(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    object_new!();
}

pub mod IDirect3DLight {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 6] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "SetLight",
        "GetLight",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DLight", this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        0
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpDirect3D: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetLight(_ctx: &mut Context, _this: u32, _lpLight: u32) -> DD {
        note("SetLight ignored".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetLight(_ctx: &mut Context, _this: u32, _lpLight: u32) -> DD {
        todo!()
    }

    object_new!();
}

pub mod IDirect3DTexture {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 8] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "GetHandle",
        "PaletteChanged",
        "Load",
        "Unload",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DTexture", this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, this: u32) -> u32 {
        state().d3d.borrow_mut().textures.remove(&this);
        0
    }

    #[win32_derive::dllexport]
    pub fn Initialize(
        _ctx: &mut Context,
        _this: u32,
        _lpD3DDevice: u32,
        _lpDDSurface: u32,
    ) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetHandle(ctx: &mut Context, this: u32, _lpDirect3DDevice: u32, lpHandle: u32) -> DD {
        // The texture's own pointer serves as its handle.
        ctx.memory.write::<u32>(lpHandle, this);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn PaletteChanged(_ctx: &mut Context, _this: u32, _dwStart: u32, _dwCount: u32) -> DD {
        // Palette changes are noticed through the palette's generation.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Load(ctx: &mut Context, this: u32, lpD3DTexture: u32) -> DD {
        // Copy a texture (typically one in system memory) into this one.
        let surfaces = {
            let d3d = state().d3d.borrow();
            let surf = state().surf.borrow();
            let lookup = |texture| {
                d3d.textures
                    .get(&texture)
                    .and_then(|s| surf.get(s))
                    .cloned()
            };
            lookup(this).zip(lookup(lpD3DTexture))
        };
        let Some((dst, src)) = surfaces else {
            return DD::ERR_GENERIC;
        };
        if Rc::ptr_eq(&dst, &src) {
            return DD::OK;
        }
        let (src_pixels, size, palette, key) = {
            let mut src = src.borrow_mut();
            let addr = src.lock(&mut ctx.memory);
            (
                addr,
                src.width * src.height * src.bytes_per_pixel,
                src.palette.clone(),
                src.src_color_key,
            )
        };
        let mut dst = dst.borrow_mut();
        let dst_size = dst.width * dst.height * dst.bytes_per_pixel;
        let dst_pixels = dst.lock(&mut ctx.memory);
        let (src_pixels, dst_pixels) = (src_pixels as usize, dst_pixels as usize);
        ctx.memory.bytes.copy_within(
            src_pixels..src_pixels + size.min(dst_size) as usize,
            dst_pixels,
        );
        if palette.is_some() {
            dst.palette = palette;
        }
        dst.src_color_key = key;
        dst.generation = crate::ddraw::next_generation();
        dst.shared.generation.set(crate::ddraw::next_generation());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unload(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    object_new!();
}

/// IDirectDrawSurface::QueryInterface for the Direct3D interfaces a surface
/// can hand out: a device (rendering into the surface) or a texture.
pub fn surface_query_interface(
    ctx: &mut Context,
    surface: u32,
    iid: &GUID,
    ppv: u32,
) -> Option<DD> {
    let addr = if *iid == IID_IDirect3DHALDevice {
        let target = state().surf.borrow().get(&surface)?.clone();
        let addr = new_object(ctx, IDirect3DDevice::new);
        let mut d3d = state().d3d.borrow_mut();
        {
            let t = target.borrow();
            d3d.note(format!(
                "device on a {}x{}x{} surface",
                t.width,
                t.height,
                t.bytes_per_pixel * 8
            ));
        }
        d3d.device = Some(Device {
            target,
            render_states: default_render_states(),
            vertices: Vec::new(),
        });
        // Whatever the surface holds so far goes under the first triangles.
        d3d.cpu_dirty = true;
        d3d.gpu_dirty = false;
        d3d.gpu_frame = false;
        d3d.snapshot = None;
        addr
    } else if *iid == IID_IDirect3DTexture {
        let addr = new_object(ctx, IDirect3DTexture::new);
        state().d3d.borrow_mut().textures.insert(addr, surface);
        addr
    } else {
        return None;
    };
    ctx.memory.write::<u32>(ppv, addr);
    Some(DD::OK)
}

/// IDirectDraw::QueryInterface(IID_IDirect3D).
pub fn ddraw_query_interface(ctx: &mut Context, ppv: u32) -> DD {
    let addr = new_object(ctx, IDirect3D::new);
    ctx.memory.write::<u32>(ppv, addr);
    DD::OK
}
