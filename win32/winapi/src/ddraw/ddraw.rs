use std::{cell::RefCell, rc::Rc};

use runtime::*;
use zerocopy::FromBytes;

use super::types::*;
use crate::{
    RECT,
    ddraw::{GUID, ddraw1, ddraw7, state},
    kernel32,
    user32::{self, HWND},
};

pub struct DirectDraw {
    pub addr: u32,
    /// COM reference count. An app that releases a DirectDraw object and
    /// creates a new one expects the old one's window binding to die with it.
    pub refs: u32,
    pub bytes_per_pixel: u32,
    pub window: Option<Rc<RefCell<user32::Window>>>,
}

impl DirectDraw {
    pub fn set_cooperative_level(&mut self, hwnd: HWND, _flags: u32) {
        // A null hwnd resets the device to normal mode and unbinds it from
        // the cooperative-level window. A non-null hwnd binds the current
        // window — the model has only one — which may not exist yet in a
        // headless session or before the app's window is created.
        self.window = if hwnd.is_null() {
            None
        } else {
            user32::state().window.borrow().as_ref().cloned()
        };
    }
}

/// Whether `addr..addr + bytes` is a range a guest may supply: outside the
/// null page and fully inside emulated memory.
pub(crate) fn guest_range(ctx: &Context, addr: u32, bytes: u32) -> bool {
    addr >= 0x1000
        && (addr as usize)
            .checked_add(bytes as usize)
            .is_some_and(|end| end <= ctx.memory.bytes.len())
}

struct SurfaceParams {
    is_primary: bool,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
    caps: DDSCAPS2,
    pixel_format: DDPIXELFORMAT,
}

impl DirectDraw {
    /// Surfaces target the cooperative-level window, so this returns `None`
    /// when no window was bound yet rather than fabricating a display.
    pub fn create_surface(
        &mut self,
        desc: &DDSURFACEDESC2,
        new_pointer: &mut dyn FnMut() -> Option<u32>,
    ) -> Option<Rc<RefCell<Surface>>> {
        let is_primary = desc.dwFlags.contains(DDSD::CAPS)
            && desc.ddsCaps.dwCaps.contains(DDSCAPS::PRIMARYSURFACE);

        let window = self.window.as_ref()?.borrow();
        let width = if desc.dwFlags.contains(DDSD::WIDTH) {
            desc.dwWidth
        } else {
            window.width
        };
        let height = if desc.dwFlags.contains(DDSD::HEIGHT) {
            desc.dwHeight
        } else {
            window.height
        };
        drop(window);

        // An offscreen surface takes the display mode's format unless the app
        // asks for a specific one, which is what lets a palettized game blit
        // between its buffers without conversion.
        let bytes_per_pixel = if desc.dwFlags.contains(DDSD::PIXELFORMAT) {
            let bits = desc.ddpfPixelFormat.dwRGBBitCount;
            if bits == 0 {
                self.bytes_per_pixel
            } else {
                bits.div_ceil(8)
            }
        } else {
            self.bytes_per_pixel
        };

        // Surface pixels come out of the 64 MiB process heap on first Lock,
        // and a non-32bpp DC needs a width*height*4 scratch buffer, so
        // reject dimensions that can never be backed rather than panicking
        // on an out-of-memory or overflowing allocation later.
        const MAX_SURFACE_BYTES: u64 = 64 << 20;
        const MAX_SURFACE_DIMENSION: u32 = 8192;
        if width > MAX_SURFACE_DIMENSION || height > MAX_SURFACE_DIMENSION {
            log::warn!("ddraw: rejecting {width}x{height} surface outside host texture limits");
            return None;
        }
        let pixels = width as u64 * height as u64;
        let worst = pixels * u64::from(bytes_per_pixel.max(4));
        if worst > MAX_SURFACE_BYTES {
            log::warn!(
                "ddraw: rejecting {width}x{height}x{bytes_per_pixel} surface needing {worst} bytes"
            );
            return None;
        }

        let caps = if desc.dwFlags.contains(DDSD::CAPS) {
            desc.ddsCaps
        } else if is_primary {
            DDSCAPS2 {
                dwCaps: DDSCAPS::PRIMARYSURFACE,
                ..Default::default()
            }
        } else {
            DDSCAPS2::default()
        };

        // Keep the requested pixel format so the rasterizer can decode
        // alpha-bearing textures (1555/4444) instead of assuming 565, and
        // so a depth surface keeps its declared z/stencil bit masks.
        let pixel_format = if desc.dwFlags.contains(DDSD::PIXELFORMAT)
            && desc.ddpfPixelFormat.dwFlags & (0x40 | 0x400) != 0
        // DDPF_RGB | DDPF_ZBUFFER
        {
            desc.ddpfPixelFormat.clone()
        } else {
            // Unspecified formats take a format matching the surface's
            // actual byte depth.
            surface_pixel_format(bytes_per_pixel)
        };

        let surface = self.create_one_surface(
            new_pointer()?,
            &SurfaceParams {
                is_primary,
                width,
                height,
                bytes_per_pixel,
                caps,
                pixel_format: pixel_format.clone(),
            },
        )?;

        if desc.dwFlags.contains(DDSD::CKSRCBLT) {
            surface.borrow_mut().src_color_key = Some(ColorKey {
                low: desc.ddckCKSrcBlt.dwColorSpaceLowValue,
                high: desc.ddckCKSrcBlt.dwColorSpaceHighValue,
            });
        }
        if desc.dwFlags.contains(DDSD::CKDESTBLT) {
            surface.borrow_mut().dst_color_key = Some(ColorKey {
                low: desc.ddckCKDestBlt.dwColorSpaceLowValue,
                high: desc.ddckCKDestBlt.dwColorSpaceHighValue,
            });
        }

        if let Some(count) = desc.back_buffer_count() {
            // Only a single back buffer is modeled; anything more fails
            // creation rather than faking a longer flip chain.
            if count != 1 {
                return None;
            }
            // The implicit back buffer shares the primary's caps minus
            // PRIMARYSURFACE, plus the BACKBUFFER role.
            let mut back_caps = caps;
            back_caps.dwCaps &= !DDSCAPS::PRIMARYSURFACE;
            back_caps.dwCaps |= DDSCAPS::BACKBUFFER;
            let back = self.create_one_surface(
                new_pointer()?,
                &SurfaceParams {
                    is_primary: false,
                    width,
                    height,
                    bytes_per_pixel,
                    caps: back_caps,
                    pixel_format: pixel_format.clone(),
                },
            )?;
            back.borrow_mut().primary.replace(surface.clone());
            let mut surface_mut = surface.borrow_mut();
            surface_mut.attached.replace(back.clone());
            surface_mut.attachments.push(back);
        }

        // A complex mipmap texture carries the rest of its chain as attached
        // surfaces, each level half the previous one's size, so the game can
        // walk GetAttachedSurface and fill each level.
        if desc.dwFlags.contains(DDSD::MIPMAPCOUNT) && caps.dwCaps.contains(DDSCAPS::MIPMAP) {
            // dwMipMapCount counts the base level too.
            let levels = desc.dwMipMapCount_dwRefreshRate_dwSrcVBHandle.max(1);
            let mut parent = surface.clone();
            let (mut w, mut h) = (width, height);
            for _ in 1..levels {
                w = (w / 2).max(1);
                h = (h / 2).max(1);
                let level = self.create_one_surface(
                    new_pointer()?,
                    &SurfaceParams {
                        is_primary: false,
                        width: w,
                        height: h,
                        bytes_per_pixel,
                        caps,
                        pixel_format: pixel_format.clone(),
                    },
                )?;
                parent.borrow_mut().attachments.push(level.clone());
                parent = level;
                if w == 1 && h == 1 {
                    break;
                }
            }
        }

        Some(surface)
    }

    fn create_one_surface(
        &mut self,
        addr: u32,
        params: &SurfaceParams,
    ) -> Option<Rc<RefCell<Surface>>> {
        let window = self.window.as_ref()?;
        let target = if params.is_primary {
            Target::Window(window.clone())
        } else {
            let texture = window
                .borrow_mut()
                .host
                .create_surface(params.width, params.height);
            Target::Texture(texture)
        };

        let surf = Rc::new(RefCell::new(Surface {
            addr,
            refs: 1,
            width: params.width,
            height: params.height,
            bytes_per_pixel: params.bytes_per_pixel,
            target,
            caps: params.caps,
            pixel_format: params.pixel_format.clone(),
            primary: Default::default(),
            attached: Default::default(),
            attachments: Vec::new(),
            pixels: None,
            palette: None,
            clipper: None,
            src_color_key: None,
            dst_color_key: None,
            private_data: Default::default(),
            uniqueness: 1,
            priority: 0,
            max_lod: 0,
        }));
        // TODO: move surf to ddraw
        state().surf.borrow_mut().insert(addr, surf.clone());
        Some(surf)
    }
}

pub enum Target {
    Window(Rc<RefCell<user32::Window>>),
    Texture(host::Surface),
}

/// A DDCOLORKEY: the inclusive range of pixel values a blit treats as
/// transparent.
#[derive(Copy, Clone, Debug)]
pub struct ColorKey {
    pub low: u32,
    pub high: u32,
}

impl ColorKey {
    pub fn matches(&self, pixel: u32) -> bool {
        (self.low..=self.high).contains(&pixel)
    }
}

/// Expand 8bpp indexed pixels to ABGR8888 through `entries`. A palette can
/// legally have fewer than 256 entries (DDPCAPS_4BIT etc.); out-of-range
/// indices fall back to black rather than panicking the host.
fn expand_palettized(pixels: &[u8], entries: &[PALETTEENTRY], buf: &mut Vec<u8>) {
    for &p in pixels {
        let entry = entries.get(p as usize);
        // ABGR8888 layout: R,G,B,A in byte order.
        buf.push(entry.map_or(0, |e| e.peRed));
        buf.push(entry.map_or(0, |e| e.peGreen));
        buf.push(entry.map_or(0, |e| e.peBlue));
        buf.push(0);
    }
}

pub struct Surface {
    pub addr: u32,
    /// COM reference count. An app that balances AddRef/Release expects the
    /// surface to outlive the matching Release, so this has to be real.
    pub refs: u32,
    pub width: u32,
    pub height: u32,
    pub bytes_per_pixel: u32,
    pub target: Target,

    // How does surface attachment actually work?
    // Docs are unclear, and wine's comments are also full of speculation and frustration, ha.
    /// Present on surfaces attached to Target::Window
    pub primary: Option<Rc<RefCell<Surface>>>,
    /// Present on Target::Window, TODO should be vec
    pub attached: Option<Rc<RefCell<Surface>>>,
    /// Every surface attached through AddAttachedSurface or an implicit
    /// flipping chain. `attached` stays the front/back flip link while this
    /// tracks the whole set so DeleteAttachedSurface and
    /// EnumAttachedSurfaces can see z-buffers and friends.
    pub attachments: Vec<Rc<RefCell<Surface>>>,

    /// Address of pixel data.
    pub pixels: Option<u32>,

    pub palette: Option<Rc<RefCell<Palette>>>,

    /// The DDSCAPS2 the surface was created with, reported by GetCaps.
    pub caps: DDSCAPS2,

    /// The requested pixel format — the rasterizer needs the channel masks
    /// to decode 1555/4444 textures instead of assuming 565.
    pub pixel_format: DDPIXELFORMAT,

    /// The IDirectDrawClipper interface pointer attached through SetClipper.
    /// Clipper objects are not modeled (they only affect windowed-mode blits),
    /// so the pointer is tracked but never dereferenced.
    pub clipper: Option<u32>,

    /// Pixel values that read as transparent when this surface is the source
    /// of a blit — how sprites get their transparent background.
    pub src_color_key: Option<ColorKey>,
    /// Pixel values that may be overwritten when this surface is the
    /// destination of a blit.
    pub dst_color_key: Option<ColorKey>,

    /// Application data attached through SetPrivateData, keyed by tag GUID.
    pub private_data: std::collections::HashMap<GUID, Vec<u8>>,
    /// Bumped by ChangeUniquenessValue so a caller can tell whether the
    /// surface contents were touched between two queries.
    pub uniqueness: u32,
    /// Texture-management values: retained and reported, but the emulated
    /// device has no eviction policy or mip chain to act on them.
    pub priority: u32,
    pub max_lod: u32,
}

impl Surface {
    /// The guest address of the surface's pixels, allocating them from the
    /// process heap on first use. `None` when the heap cannot satisfy the
    /// request; surface dimensions are bounded at creation, so this only
    /// fails when the heap is exhausted.
    pub fn lock(&mut self, mem: &mut Memory) -> Option<u32> {
        match self.pixels {
            Some(addr) => Some(addr),
            None => {
                let size = self.width * self.height * self.bytes_per_pixel;
                let Some(addr) = kernel32::lock().process_heap.try_alloc(mem, size) else {
                    log::error!(
                        "surface {}x{}x{} pixels do not fit the process heap",
                        self.width,
                        self.height,
                        self.bytes_per_pixel
                    );
                    return None;
                };
                // scribble on pixels so we can see it
                if let Some(buf) = mem
                    .bytes
                    .get_mut(addr as usize..addr as usize + size as usize)
                {
                    buf.fill(0x8F);
                }
                self.pixels = Some(addr);
                Some(addr)
            }
        }
    }

    pub fn unlock(&mut self, mem: &mut Memory) {
        match self.target {
            // Writes to the primary surface go straight to the screen.
            Target::Window(_) => self.present(mem),
            Target::Texture(_) => self.update_texture(mem, &None),
        }
    }

    /// Convert this surface's pixels to the RGBA the host wants, using
    /// `palette` for palettized formats. Borrows the pixels directly when they
    /// are already RGBA. Returns None when there is nothing to show, e.g. an
    /// 8-bit surface with no palette attached yet.
    pub(crate) fn to_rgba<'a>(
        &self,
        mem: &'a Memory,
        palette: &Option<Rc<RefCell<Palette>>>,
    ) -> Option<std::borrow::Cow<'a, [u8]>> {
        let addr = self.pixels?;
        let size = self.width * self.height * self.bytes_per_pixel;
        let Some(pixels) = mem
            .bytes
            .get(addr as usize..)
            .and_then(|b| b.get(..size as usize))
        else {
            log::warn!(
                "surface {}x{}x{} pixels at {addr:#x} out of range",
                self.width,
                self.height,
                self.bytes_per_pixel
            );
            return None;
        };
        Some(match self.bytes_per_pixel {
            1 => {
                let palette = palette.as_ref()?;
                let entries = &palette.borrow().entries;
                let mut buf = Vec::with_capacity(pixels.len() * 4);
                expand_palettized(pixels, entries, &mut buf);
                buf.into()
            }
            2 => {
                // Decode through the surface's declared channel masks so
                // alpha-bearing 1555/4444 textures survive the RGBA round
                // trip; a plain 565 surface lands on the same values.
                let pf = &self.pixel_format;
                let mut buf = Vec::with_capacity(pixels.len() * 2);
                for pixel in pixels.chunks_exact(2) {
                    let v = u16::from_le_bytes([pixel[0], pixel[1]]) as u32;
                    if pf.dwFlags & 0x40 != 0 {
                        buf.push(mask_to_u8(v, pf.dwRBitMask, 0));
                        buf.push(mask_to_u8(v, pf.dwGBitMask, 0));
                        buf.push(mask_to_u8(v, pf.dwBBitMask, 0));
                        buf.push(mask_to_u8(v, pf.dwRGBAlphaBitMask, 255));
                    } else {
                        let (r, g, b) = (v >> 11, (v >> 5) & 0x3f, v & 0x1f);
                        // Replicate the high bits into the low ones so
                        // full-scale values stay full-scale.
                        buf.push((r << 3 | r >> 2) as u8);
                        buf.push((g << 2 | g >> 4) as u8);
                        buf.push((b << 3 | b >> 2) as u8);
                        buf.push(0);
                    }
                }
                buf.into()
            }
            4 => pixels.into(),
            bpp => {
                log::warn!("unsupported surface format: {bpp} bytes per pixel");
                return None;
            }
        })
    }

    /// Convert the RGBA scratch buffer a DC draws in back to this surface's
    /// own format, honoring the declared channel masks so alpha-bearing
    /// 1555/4444 textures keep their layout. Other depths warn and leave the
    /// surface unchanged.
    pub fn write_rgba(&mut self, mem: &mut Memory, rgba: &[u8], dst: u32) {
        match self.bytes_per_pixel {
            2 => {
                let pf = &self.pixel_format;
                let masked = pf.dwFlags & 0x40 != 0;
                for (i, px) in rgba.chunks_exact(4).enumerate() {
                    let v = if masked {
                        u8_to_mask(px[0], pf.dwRBitMask)
                            | u8_to_mask(px[1], pf.dwGBitMask)
                            | u8_to_mask(px[2], pf.dwBBitMask)
                            | u8_to_mask(px[3], pf.dwRGBAlphaBitMask)
                    } else {
                        ((px[0] as u32 >> 3) << 11)
                            | ((px[1] as u32 >> 2) << 5)
                            | (px[2] as u32 >> 3)
                    };
                    mem.write::<u16>(dst + i as u32 * 2, v as u16);
                }
            }
            bpp => {
                log::warn!("ReleaseDC: no RGBA->{}bpp conversion", bpp * 8);
            }
        }
    }

    // App can write pixels to back buffer but attach palette to front buffer,
    // so take palette as an argument.
    fn update_texture(&mut self, mem: &mut Memory, palette: &Option<Rc<RefCell<Palette>>>) {
        let Some(pixels) = self.to_rgba(mem, palette) else {
            return;
        };
        let width = self.width;
        match &mut self.target {
            // A window-target surface has no texture of its own; present()
            // borrows the back buffer's. AddAttachedSurface can make such a
            // surface another surface's flip link, so this is a soft no-op
            // rather than a panic — flip() reports ERR_INVALIDSURFACETYPE.
            Target::Window(_) => {
                log::debug!("update_texture on a window-target surface; nothing to upload")
            }
            Target::Texture(texture) => {
                texture.set_pixels(&pixels, width * 4);
            }
        }
    }

    /// Present this surface's own pixel buffer to the window it targets, used
    /// when an app draws directly to the primary surface (via Lock or Blt)
    /// instead of flipping. No-op for non-primary surfaces.
    pub fn present(&mut self, mem: &mut Memory) {
        let Target::Window(window) = &self.target else {
            return;
        };
        // We have no texture of our own; borrow the back buffer's.
        let Some(back) = self.attached.clone() else {
            return;
        };
        let Some(pixels) = self.to_rgba(mem, &self.palette) else {
            return;
        };
        let mut back = back.borrow_mut();
        let Target::Texture(texture) = &mut back.target else {
            return;
        };
        texture.set_pixels(&pixels, self.width * 4);
        window.borrow_mut().host.render(texture);
    }

    pub fn flip(&mut self, mem: &mut Memory) -> DD {
        // "Flip can be called only for a surface that has the DDSCAPS_FLIP and DDSCAPS_FRONTBUFFER capabilities."
        let Target::Window(window) = &self.target else {
            return DD::ERR_INVALIDSURFACETYPE;
        };

        let Some(back) = self.attached.as_ref() else {
            return DD::ERR_INVALIDSURFACETYPE;
        };
        let mut back = back.borrow_mut();
        // Refresh the back buffer's texture every flip, not just in
        // palettized modes — a palette is only needed to expand indexed
        // pixels, while 16/32bpp buffers convert without one.
        static FLIP_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let flip_n = FLIP_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        log::debug!(
            "flip[{flip_n}]: front={:#x} back={:#x} back.pixels={:#x} {}x{}",
            self.addr,
            back.addr,
            back.pixels.unwrap_or(0),
            back.width,
            back.height,
        );
        back.update_texture(mem, &self.palette);
        // THESEUS_FLIP_DUMP=<path> writes the back buffer's raw guest pixels
        // as a PPM once (on the Nth flip, N from THESEUS_FLIP_DUMP_AT),
        // so we can compare against what the texture shows.
        if let Ok(path) = std::env::var("THESEUS_FLIP_DUMP") {
            let at: u64 = std::env::var("THESEUS_FLIP_DUMP_AT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if flip_n == at
                && let (Some(addr), true) = (back.pixels, back.bytes_per_pixel == 2)
            {
                let (w, h) = (back.width, back.height);
                let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
                for i in 0..(w * h) {
                    let p = mem.read::<u16>(addr + i * 2);
                    out.push((((p >> 11) & 0x1f) << 3) as u8);
                    out.push((((p >> 5) & 0x3f) << 2) as u8);
                    out.push(((p & 0x1f) << 3) as u8);
                }
                let _ = std::fs::write(&path, out);
            }
        }
        let Target::Texture(texture) = &mut back.target else {
            return DD::ERR_INVALIDSURFACETYPE;
        };
        let mut window = window.borrow_mut();
        window.host.render(texture);
        DD::OK
    }
}

pub struct Palette {
    pub entries: Vec<PALETTEENTRY>,
}

/// Shared body for `IDirectDraw::CreatePalette`/`IDirectDraw7::CreatePalette`:
/// reads the caller's initial PALETTEENTRY table (or zero-initializes when
/// absent), registers a palette object, and returns its interface pointer.
/// `new_pointer` allocates the interface object with the right vtable.
pub fn create_palette(
    ctx: &mut Context,
    flags: u32,
    lp_entries: u32,
    lplp_pal: u32,
    new_pointer: impl FnOnce(&mut Context) -> Option<u32>,
) -> DD {
    if !guest_range(ctx, lplp_pal, 4) {
        return DD::ERR_INVALIDPARAMS;
    }
    // DDPCAPS_8BIT/4BIT/2BIT/1BIT choose the table size; absent a depth flag
    // the default is 256.
    let count = if flags & DDPCAPS::_8BIT.bits() != 0 {
        256
    } else if flags & DDPCAPS::_4BIT.bits() != 0 {
        16
    } else if flags & DDPCAPS::_2BIT.bits() != 0 {
        4
    } else if flags & DDPCAPS::_1BIT.bits() != 0 {
        2
    } else {
        256
    };
    let entries = if lp_entries == 0 {
        vec![
            PALETTEENTRY {
                peRed: 0,
                peGreen: 0,
                peBlue: 0,
                peFlags: 0,
            };
            count
        ]
    } else {
        let Some(bytes) = ctx.memory.bytes.get(lp_entries as usize..) else {
            return DD::ERR_INVALIDPARAMS;
        };
        match <[PALETTEENTRY]>::ref_from_prefix_with_elems(bytes, count) {
            Ok((entries, _)) => entries.to_vec(),
            Err(_) => return DD::ERR_INVALIDPARAMS,
        }
    };
    let Some(ptr) = new_pointer(ctx) else {
        return DD::ERR_OUTOFMEMORY;
    };
    state()
        .palette
        .borrow_mut()
        .insert(ptr, Rc::new(RefCell::new(Palette { entries })));
    ctx.memory.write::<u32>(lplp_pal, ptr);
    DD::OK
}

pub fn get_pixel_format() -> DDPIXELFORMAT {
    DDPIXELFORMAT {
        dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
        dwFlags: 0x00000040,
        dwFourCC: 0,
        dwRGBBitCount: 32,
        dwRBitMask: 0x0000_00FF,
        dwGBitMask: 0x0000_FF00,
        dwBBitMask: 0x00FF_0000,
        dwRGBAlphaBitMask: 0xFF00_0000,
    }
}

/// The `DDPIXELFORMAT` matching a surface's byte depth, used when the
/// guest did not declare a format. The 32-bit masks match
/// `get_pixel_format` — surface memory is RGBA byte order, so R sits in
/// the low byte.
pub(crate) fn surface_pixel_format(bpp: u32) -> DDPIXELFORMAT {
    let (flags, count, r, g, b, a) = match bpp {
        1 => (0x40 | 0x20, 8, 0, 0, 0, 0), // DDPF_RGB | DDPF_PALETTEINDEXED8
        2 => (0x40, 16, 0xF800, 0x07E0, 0x001F, 0),
        _ => (0x40, 32, 0x0000_00FF, 0x0000_FF00, 0x00FF_0000, 0xFF00_0000),
    };
    DDPIXELFORMAT {
        dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
        dwFlags: flags,
        dwFourCC: 0,
        dwRGBBitCount: count,
        dwRBitMask: r,
        dwGBitMask: g,
        dwBBitMask: b,
        dwRGBAlphaBitMask: a,
    }
}

#[win32_derive::dllexport]
pub fn DirectDrawCreate(ctx: &mut Context, lpGUID: u32, lplpDD: u32, pUnkOuter: u32) -> DD {
    DirectDrawCreateEx(ctx, lpGUID, lplpDD, 0, pUnkOuter)
}

#[win32_derive::dllexport]
pub fn DirectDrawCreateEx(
    ctx: &mut Context,
    lpGuid: u32,
    lplpDD: u32,
    iid: u32,
    _pUnkOuter: u32,
) -> DD {
    unsafe { super::init_vtables(ctx) };
    if let Some(guid) = crate::Ptr::<GUID>::new(lpGuid).read(&ctx.memory) {
        log::debug!("DirectDrawCreateEx with GUID {guid:?}");
    }
    let iid = if iid == 0 {
        None
    } else {
        let Some(iid) = crate::Ptr::<GUID>::new(iid).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        Some(iid)
    };
    if !guest_range(ctx, lplpDD, 4) {
        return DD::ERR_INVALIDPARAMS;
    }

    let mut kernel32 = kernel32::lock();
    let addr = match iid {
        None => ddraw1::IDirectDraw::new(ctx, &mut kernel32.process_heap),
        Some(ddraw7::IID_IDirectDraw7) => {
            ddraw7::IDirectDraw7::new(ctx, &mut kernel32.process_heap)
        }
        // The emulated object exposes only IDirectDraw and IDirectDraw7.
        Some(_) => return DD::E_NOINTERFACE,
    };
    let Some(addr) = addr else {
        return DD::ERR_OUTOFMEMORY;
    };

    let mut ddraw = state().ddraw.borrow_mut();
    *ddraw = Some(DirectDraw {
        addr,
        refs: 1,
        bytes_per_pixel: 4,
        window: None,
    });

    ctx.memory.write(lplpDD, addr);
    DD::OK
}

pub(crate) fn alloc_string(ctx: &mut Context, s: &str) -> Option<u32> {
    let kernel32 = kernel32::lock();
    let addr = kernel32
        .process_heap
        .try_alloc(&mut ctx.memory, s.len() as u32)?;
    drop(kernel32);
    if let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(addr as usize..addr as usize + s.len())
    {
        dst.copy_from_slice(s.as_bytes());
    }
    Some(addr)
}

#[win32_derive::dllexport]
pub fn DirectDrawEnumerateA(ctx: &mut Context, lpCallback: u32, lpContext: u32) -> DD {
    // A null-page callback would dispatch to a missing block and halt.
    if lpCallback < 0x1000 {
        return DD::ERR_GENERIC;
    }
    let Some(desc) = alloc_string(ctx, "Primary Display Driver\0") else {
        return DD::ERR_OUTOFMEMORY;
    };
    let Some(name) = alloc_string(ctx, "DISPLAY\0") else {
        kernel32::lock().process_heap.free(&mut ctx.memory, desc);
        return DD::ERR_OUTOFMEMORY;
    };
    let callback = ctx.indirect(lpCallback);
    ctx.call32_x86(callback, vec![desc, name, lpContext]);
    DD::OK
}

#[win32_derive::dllexport]
pub fn DirectDrawEnumerateExA(
    ctx: &mut Context,
    lpCallback: u32,
    lpContext: u32,
    _dwFlags: u32,
) -> DD {
    // A null-page callback would dispatch to a missing block and halt.
    if lpCallback < 0x1000 {
        return DD::ERR_GENERIC;
    }
    let Some(guid_addr) = ({
        let kernel32 = kernel32::lock();
        let addr = kernel32
            .process_heap
            .try_alloc(&mut ctx.memory, std::mem::size_of::<GUID>() as u32);
        drop(kernel32);
        addr
    }) else {
        return DD::ERR_OUTOFMEMORY;
    };
    let guid_size = std::mem::size_of::<GUID>();
    if let Some(dst) = ctx
        .memory
        .bytes
        .get_mut(guid_addr as usize..guid_addr as usize + guid_size)
    {
        dst.fill(0);
    }
    let Some(desc) = alloc_string(ctx, "Primary Display Driver\0") else {
        kernel32::lock()
            .process_heap
            .free(&mut ctx.memory, guid_addr);
        return DD::ERR_OUTOFMEMORY;
    };
    let Some(name) = alloc_string(ctx, "DISPLAY\0") else {
        let kernel32 = kernel32::lock();
        kernel32.process_heap.free(&mut ctx.memory, guid_addr);
        kernel32.process_heap.free(&mut ctx.memory, desc);
        return DD::ERR_OUTOFMEMORY;
    };
    let callback = ctx.indirect(lpCallback);
    ctx.call32_x86(callback, vec![guid_addr, desc, name, lpContext, 0]);
    DD::OK
}

pub fn read_rect(ctx: &Context, addr: u32) -> Option<RECT> {
    if addr == 0 {
        None
    } else {
        crate::Ptr::<RECT>::new(addr).read(&ctx.memory)
    }
}

/// The `lpSurface` value for a Lock restricted to the rect at `rect_addr`:
/// DirectDraw hands back the address of the region's first pixel while the
/// reported pitch still spans whole surface rows. `None` when a non-null
/// rect pointer is unreadable.
pub(crate) fn lock_offset(
    ctx: &Context,
    rect_addr: u32,
    width: u32,
    height: u32,
    bytes_per_pixel: u32,
    pixels: u32,
) -> Option<u32> {
    if rect_addr == 0 {
        return Some(pixels);
    }
    let rect = read_rect(ctx, rect_addr)?.clip_to_size(width, height);
    // A rect entirely outside the surface clips to an empty region whose
    // origin can sit past the edge; clamp so the returned address stays
    // inside the allocation.
    let top = (rect.top.max(0) as u32).min(height);
    let left = (rect.left.max(0) as u32).min(width);
    let pitch = width * bytes_per_pixel;
    Some(pixels + top * pitch + left * bytes_per_pixel)
}

/// Copy a rect between two surfaces (which may be the same one; the copy
/// stages through a temporary buffer).
///
/// With a `color_key`, source pixels inside its range are left alone in the
/// destination, which is how sprites get transparent backgrounds.
pub fn blit_copy(
    ctx: &mut Context,
    dst_ptr: u32,
    dst_rect: Option<RECT>,
    src_ptr: u32,
    src_rect: Option<RECT>,
    color_key: Option<ColorKey>,
    dst_color_key: Option<ColorKey>,
) -> DD {
    let (src_rc, dst_rc) = {
        let surfaces = state().surf.borrow();
        let (Some(src), Some(dst)) = (surfaces.get(&src_ptr), surfaces.get(&dst_ptr)) else {
            return DD::ERR_INVALIDPARAMS;
        };
        (src.clone(), dst.clone())
    };

    // THESEUS_SRC_DUMP=<path> writes the Nth full-screen blit's raw source
    // pixels as a PPM (N from THESEUS_SRC_DUMP_AT), for comparing what the
    // game drew with what the screen shows.
    if let Ok(path) = std::env::var("THESEUS_SRC_DUMP") {
        let src = src_rc.borrow();
        if let (Some(addr), 2) = (src.pixels, src.bytes_per_pixel)
            && src.width == 640
            && src.height == 480
        {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let at: u64 = std::env::var("THESEUS_SRC_DUMP_AT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == at {
                log::warn!("src dump: src={src_ptr:#x} pixels={addr:#x} dst={dst_ptr:#x}");
                let (w, h) = (src.width, src.height);
                let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
                for i in 0..(w * h) {
                    let p = ctx.memory.read::<u16>(addr + i * 2);
                    out.push((((p >> 11) & 0x1f) << 3) as u8);
                    out.push((((p >> 5) & 0x3f) << 2) as u8);
                    out.push(((p & 0x1f) << 3) as u8);
                }
                let _ = std::fs::write(&path, out);
            }
        }
    }

    let (rows, row_bytes, row_count, src_fmt) = {
        let mut src = src_rc.borrow_mut();
        let Some(addr) = src.lock(&mut ctx.memory) else {
            return DD::ERR_OUTOFMEMORY;
        };
        let bpp = src.bytes_per_pixel;
        let stride = src.width * bpp;
        let rect = src_rect
            .unwrap_or_else(|| RECT::from_size(src.width, src.height))
            .clip_to_size(src.width, src.height);
        let row_bytes = ((rect.right - rect.left).max(0) as u32 * bpp) as usize;
        let row_count = (rect.bottom - rect.top).max(0) as usize;
        let mut rows = Vec::with_capacity(row_bytes * row_count);
        for y in rect.top..rect.bottom {
            let start = addr + y as u32 * stride + rect.left as u32 * bpp;
            let Some(row) = ctx
                .memory
                .bytes
                .get(start as usize..start as usize + row_bytes)
            else {
                return DD::ERR_INVALIDPARAMS;
            };
            rows.extend_from_slice(row);
        }
        (rows, row_bytes, row_count, PixelFmt::new(&src))
    };

    let mut dst = dst_rc.borrow_mut();
    let dst_fmt = PixelFmt::new(&dst);
    let Some(addr) = dst.lock(&mut ctx.memory) else {
        return DD::ERR_OUTOFMEMORY;
    };
    let want = dst_rect.unwrap_or_else(|| RECT::from_size(dst.width, dst.height));
    let rect = want.clip_to_size(dst.width, dst.height);
    let result = if src_fmt.same_layout(&dst_fmt) {
        write_blit(
            &mut ctx.memory,
            addr,
            dst.width * src_fmt.bpp as u32,
            &want,
            &rect,
            &rows,
            row_bytes / src_fmt.bpp,
            row_count,
            src_fmt.bpp as u32,
            color_key,
            dst_color_key,
        )
    } else {
        write_blit_convert(
            &mut ctx.memory,
            addr,
            dst.width * dst_fmt.bpp as u32,
            &want,
            &rect,
            &rows,
            row_bytes / src_fmt.bpp,
            row_count,
            &src_fmt,
            &dst_fmt,
            color_key,
            dst_color_key,
        )
    };
    dst.present(&mut ctx.memory);
    result
}

/// One pixel's little-endian value for color-key comparison and format
/// conversion.
fn pixel_value(pixel: &[u8], bpp: u32) -> Option<u32> {
    if !(1..=4).contains(&bpp) {
        return None;
    }
    Some(
        pixel
            .get(..bpp as usize)?
            .iter()
            .enumerate()
            .fold(0u32, |v, (i, b)| v | (*b as u32) << (8 * i)),
    )
}

/// A surface's pixel layout for cross-format blits: the DDPIXELFORMAT
/// channel masks for RGB formats, plus a copy of the palette for indexed
/// surfaces.
struct PixelFmt {
    bpp: usize,
    /// DDPF_RGB: the channel masks describe red/green/blue/alpha (as
    /// opposed to a z-buffer or FourCC layout).
    rgb: bool,
    r: u32,
    g: u32,
    b: u32,
    a: u32,
    /// The surface's palette entries, needed to decode indexed pixels.
    palette: Option<Vec<PALETTEENTRY>>,
}

impl PixelFmt {
    fn new(surface: &Surface) -> PixelFmt {
        PixelFmt {
            bpp: surface.bytes_per_pixel as usize,
            rgb: surface.pixel_format.dwFlags & 0x40 != 0,
            r: surface.pixel_format.dwRBitMask,
            g: surface.pixel_format.dwGBitMask,
            b: surface.pixel_format.dwBBitMask,
            a: surface.pixel_format.dwRGBAlphaBitMask,
            palette: surface.palette.as_ref().map(|p| p.borrow().entries.clone()),
        }
    }

    /// Whether pixels can be copied verbatim: same depth and, for RGB
    /// formats, the same channel masks. Indexed-to-indexed blits copy
    /// palette indices verbatim; DirectDraw does not remap between
    /// palettes.
    fn same_layout(&self, other: &PixelFmt) -> bool {
        self.bpp == other.bpp
            && (self.bpp == 1
                || !self.rgb && !other.rgb
                || (self.r, self.g, self.b, self.a) == (other.r, other.g, other.b, other.a))
    }

    /// Whether a blit can convert to or from this format: an indexed
    /// surface needs its palette, deeper surfaces need RGB channel masks.
    fn convertible(&self) -> bool {
        if self.bpp == 1 {
            self.palette.is_some()
        } else {
            self.rgb
        }
    }

    /// Decode one pixel to 8-bit RGBA. `None` when the format is not
    /// decodable — a paletteless indexed pixel or a short slice.
    fn decode(&self, pixel: &[u8]) -> Option<(u8, u8, u8, u8)> {
        let v = pixel_value(pixel, self.bpp as u32)?;
        if self.bpp == 1 {
            let e = self.palette.as_deref()?.get(v as usize);
            return e.map(|e| (e.peRed, e.peGreen, e.peBlue, 255));
        }
        Some((
            mask_to_u8(v, self.r, 0),
            mask_to_u8(v, self.g, 0),
            mask_to_u8(v, self.b, 0),
            mask_to_u8(v, self.a, 255),
        ))
    }

    /// Encode 8-bit RGBA as this format's raw pixel value. `None` for a
    /// paletteless indexed destination.
    fn encode(&self, rgba: (u8, u8, u8, u8)) -> Option<u32> {
        if self.bpp == 1 {
            let (r, g, b) = (i32::from(rgba.0), i32::from(rgba.1), i32::from(rgba.2));
            return self
                .palette
                .as_deref()?
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| {
                    let (dr, dg, db) = (
                        i32::from(e.peRed) - r,
                        i32::from(e.peGreen) - g,
                        i32::from(e.peBlue) - b,
                    );
                    dr * dr + dg * dg + db * db
                })
                .map(|(i, _)| i as u32);
        }
        Some(
            u8_to_mask(rgba.0, self.r)
                | u8_to_mask(rgba.1, self.g)
                | u8_to_mask(rgba.2, self.b)
                | u8_to_mask(rgba.3, self.a),
        )
    }
}

/// Scale a DDPIXELFORMAT channel out of `v` to 8 bits; `default` when the
/// format has no such channel (alpha on an opaque format reads as 255).
fn mask_to_u8(v: u32, mask: u32, default: u8) -> u8 {
    if mask == 0 {
        return default;
    }
    let max = (1u64 << mask.count_ones()) - 1;
    ((((v & mask) >> mask.trailing_zeros()) as u64 * 255 + max / 2) / max) as u8
}

/// Scale an 8-bit channel into a DDPIXELFORMAT mask position.
fn u8_to_mask(v: u8, mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }
    let max = (1u64 << mask.count_ones()) - 1;
    ((((v as u64) * max + 127) / 255) as u32) << mask.trailing_zeros() & mask
}

/// Whether a `rect` worth of pixels starting at `addr` with `pitch` bytes per
/// row and `bpp` bytes per pixel fits entirely in emulated memory.
fn dst_range_valid(memory: &Memory, addr: u32, pitch: u32, bpp: u32, rect: &RECT) -> bool {
    if rect.left >= rect.right || rect.top >= rect.bottom {
        return true;
    }
    let last_row = (rect.bottom as u64 - 1) * pitch as u64;
    let last_col = rect.right as u64 * bpp as u64;
    let Some(end) = (addr as u64).checked_add(last_row + last_col) else {
        return false;
    };
    end as usize <= memory.bytes.len()
}

/// Write the staged source rows into a destination surface. `want` is the
/// requested destination rect, already clipped to `rect`; a different-size
/// `want` stretches the source nearest-neighbor, matching what `Blt` does
/// with unequal `lpSrcRect`/`lpDestRect`. A `dst_color_key` writes only
/// where the existing destination pixel is inside the key range.
#[allow(clippy::too_many_arguments)]
fn write_blit(
    memory: &mut Memory,
    addr: u32,
    pitch: u32,
    want: &RECT,
    rect: &RECT,
    rows: &[u8],
    src_w: usize,
    src_h: usize,
    bpp: u32,
    color_key: Option<ColorKey>,
    dst_color_key: Option<ColorKey>,
) -> DD {
    if !dst_range_valid(memory, addr, pitch, bpp, rect) {
        return DD::ERR_INVALIDPARAMS;
    }
    let row_bytes = src_w * bpp as usize;
    // Widen before subtracting: a guest rect's edges can be far enough
    // apart to overflow i32.
    let dst_w = (want.right as i64 - want.left as i64).max(0);
    let dst_h = (want.bottom as i64 - want.top as i64).max(0);
    if dst_w as usize != src_w || dst_h as usize != src_h {
        if dst_w <= 0 || dst_h <= 0 || src_w == 0 || src_h == 0 {
            return DD::OK;
        }
        for dy in rect.top..rect.bottom {
            let sy = ((dy as i64 - want.top as i64) * src_h as i64 / dst_h) as usize;
            for dx in rect.left..rect.right {
                let sx = ((dx as i64 - want.left as i64) * src_w as i64 / dst_w) as usize;
                let start = sy * row_bytes + sx * bpp as usize;
                let pixel = rows.get(start..).and_then(|b| b.get(..bpp as usize));
                let Some(pixel) = pixel else {
                    continue;
                };
                if let Some(key) = &color_key {
                    let Some(value) = pixel_value(pixel, bpp) else {
                        log::warn!("colorkey blit at {bpp} bytes per pixel");
                        return DD::OK;
                    };
                    if key.matches(value) {
                        continue;
                    }
                }
                let at = addr + dy as u32 * pitch + dx as u32 * bpp;
                if !dst_key_allows(memory, at, bpp, &dst_color_key) {
                    continue;
                }
                memory.write_bytes(at, pixel);
            }
        }
        return DD::OK;
    }

    // Whatever the clip took off the top and left has to come off the
    // source as well, otherwise the image slides instead of being cropped.
    let skip_x = (rect.left as i64 - want.left as i64).max(0) as usize * bpp as usize;
    let skip_y = (rect.top as i64 - want.top as i64).max(0) as usize;
    let copy_bytes = row_bytes
        .saturating_sub(skip_x)
        .min(((rect.right - rect.left).max(0) as u32 * bpp) as usize);
    let copy_rows = src_h
        .saturating_sub(skip_y)
        .min((rect.bottom - rect.top).max(0) as usize);
    // A `want` edge far outside the destination can push the source skip
    // past the end of the staged rows; with nothing to write the loop is a
    // no-op, and computing its slice index would panic instead.
    if copy_bytes == 0 || copy_rows == 0 {
        return DD::OK;
    }
    for i in 0..copy_rows {
        let dst_start = addr + (rect.top + i as i32) as u32 * pitch + rect.left as u32 * bpp;
        let row_start = (i + skip_y) * row_bytes + skip_x;
        let row = rows.get(row_start..).and_then(|b| b.get(..copy_bytes));
        let Some(row) = row else {
            continue;
        };
        match (color_key, dst_color_key) {
            (None, None) => memory.write_bytes(dst_start, row),
            (src_key, dst_key) => {
                for (x, pixel) in row.chunks_exact(bpp as usize).enumerate() {
                    if let Some(key) = &src_key {
                        let Some(value) = pixel_value(pixel, bpp) else {
                            log::warn!("colorkey blit at {bpp} bytes per pixel");
                            return DD::OK;
                        };
                        if key.matches(value) {
                            continue;
                        }
                    }
                    let at = dst_start + x as u32 * bpp;
                    if !dst_key_allows(memory, at, bpp, &dst_key) {
                        continue;
                    }
                    memory.write_bytes(at, pixel);
                }
            }
        }
    }
    DD::OK
}

/// A destination color key lets the blit write only where the existing
/// pixel is inside the key range.
fn dst_key_allows(memory: &Memory, at: u32, bpp: u32, dst_color_key: &Option<ColorKey>) -> bool {
    let Some(key) = dst_color_key else {
        return true;
    };
    let Some(pixel) = memory.bytes.get(at as usize..at as usize + bpp as usize) else {
        return false;
    };
    pixel_value(pixel, bpp).is_some_and(|v| key.matches(v))
}

/// Write staged source rows into a destination surface of a different
/// format, converting each pixel through 8-bit RGBA. Handles the same
/// clipping, stretching, and color keys as `write_blit`; the per-pixel
/// conversion cost is only paid on cross-format blits.
#[allow(clippy::too_many_arguments)]
fn write_blit_convert(
    memory: &mut Memory,
    addr: u32,
    pitch: u32,
    want: &RECT,
    rect: &RECT,
    rows: &[u8],
    src_w: usize,
    src_h: usize,
    src: &PixelFmt,
    dst: &PixelFmt,
    color_key: Option<ColorKey>,
    dst_color_key: Option<ColorKey>,
) -> DD {
    if !src.convertible() || !dst.convertible() {
        log::warn!(
            "blit between unconvertible pixel formats ({}bpp -> {}bpp)",
            src.bpp * 8,
            dst.bpp * 8
        );
        return DD::OK;
    }
    if !dst_range_valid(memory, addr, pitch, dst.bpp as u32, rect) {
        return DD::ERR_INVALIDPARAMS;
    }
    let dst_w = (want.right as i64 - want.left as i64).max(0);
    let dst_h = (want.bottom as i64 - want.top as i64).max(0);
    if dst_w <= 0 || dst_h <= 0 || src_w == 0 || src_h == 0 {
        return DD::OK;
    }
    let src_row_bytes = src_w * src.bpp;
    for dy in rect.top..rect.bottom {
        let sy = ((dy as i64 - want.top as i64) * src_h as i64 / dst_h) as usize;
        for dx in rect.left..rect.right {
            let sx = ((dx as i64 - want.left as i64) * src_w as i64 / dst_w) as usize;
            let start = sy * src_row_bytes + sx * src.bpp;
            let Some(pixel) = rows.get(start..).and_then(|b| b.get(..src.bpp)) else {
                continue;
            };
            if let Some(key) = &color_key {
                let Some(value) = pixel_value(pixel, src.bpp as u32) else {
                    log::warn!("colorkey blit at {} bytes per pixel", src.bpp);
                    return DD::OK;
                };
                if key.matches(value) {
                    continue;
                }
            }
            let at = addr + dy as u32 * pitch + dx as u32 * dst.bpp as u32;
            if !dst_key_allows(memory, at, dst.bpp as u32, &dst_color_key) {
                continue;
            }
            let Some(rgba) = src.decode(pixel) else {
                continue;
            };
            let Some(v) = dst.encode(rgba) else {
                continue;
            };
            memory.write_bytes(at, &v.to_le_bytes()[..dst.bpp]);
        }
    }
    DD::OK
}

pub fn surface_src_color_key(surface: u32) -> Option<ColorKey> {
    let surfaces = state().surf.borrow();
    let key = surfaces.get(&surface)?.borrow().src_color_key;
    if key.is_none() {
        log::warn!("blit asked for a source color key, but none is set");
    }
    key
}

pub fn surface_dst_color_key(surface: u32) -> Option<ColorKey> {
    let surfaces = state().surf.borrow();
    let key = surfaces.get(&surface)?.borrow().dst_color_key;
    if key.is_none() {
        log::warn!("blit asked for a destination color key, but none is set");
    }
    key
}

pub fn blt(
    ctx: &mut Context,
    this: u32,
    lpDstRect: u32,
    lpDDSrcSurface: u32,
    lpSrcRect: u32,
    dwFlags: u32,
    lpDDBLTFX: u32,
) -> DD {
    const DDBLT_COLORFILL: u32 = 0x0400;
    const DDBLT_KEYDEST: u32 = 0x2000;
    const DDBLT_KEYSRC: u32 = 0x8000;
    const DDBLT_KEYDESTOVERRIDE: u32 = 0x0004_0000;
    const DDBLT_KEYSRCOVERRIDE: u32 = 0x0001_0000;
    const DDBLT_WAIT: u32 = 0x0100_0000;
    const KNOWN: u32 = DDBLT_COLORFILL
        | DDBLT_KEYDEST
        | DDBLT_KEYSRC
        | DDBLT_KEYDESTOVERRIDE
        | DDBLT_KEYSRCOVERRIDE
        | DDBLT_WAIT;
    if dwFlags & !KNOWN != 0 {
        log::warn!("Blt: ignoring flags {:#x}", dwFlags & !KNOWN);
    }

    let dst_rect = read_rect(ctx, lpDstRect);
    if dwFlags & DDBLT_COLORFILL != 0 {
        if !guest_range(ctx, lpDDBLTFX, 84) {
            return DD::ERR_INVALIDPARAMS;
        }
        // DDBLTFX.dwFillColor is at offset 80.
        let color = ctx.memory.read::<u32>(lpDDBLTFX + 80);
        log::debug!("Blt colorfill: dst={this:#x} rect={dst_rect:?} color={color:#x}");
        let Some(dst_rc) = state().surf.borrow().get(&this).cloned() else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut dst = dst_rc.borrow_mut();
        let bpp = dst.bytes_per_pixel;
        let rect = dst_rect
            .unwrap_or_else(|| RECT::from_size(dst.width, dst.height))
            .clip_to_size(dst.width, dst.height);
        let Some(addr) = dst.lock(&mut ctx.memory) else {
            return DD::ERR_OUTOFMEMORY;
        };
        let stride = dst.width * bpp;
        if !dst_range_valid(&ctx.memory, addr, stride, bpp, &rect) {
            return DD::ERR_INVALIDPARAMS;
        }
        for y in rect.top..rect.bottom {
            let start = addr + y as u32 * stride + rect.left as u32 * bpp;
            let width_bytes = ((rect.right - rect.left).max(0) as u32 * bpp) as usize;
            match bpp {
                1 => {
                    if let Some(dst) = ctx
                        .memory
                        .bytes
                        .get_mut(start as usize..)
                        .and_then(|b| b.get_mut(..width_bytes))
                    {
                        dst.fill(color as u8);
                    }
                }
                2 => {
                    for x in 0..(rect.right - rect.left).max(0) as u32 {
                        ctx.memory.write::<u16>(start + x * 2, color as u16);
                    }
                }
                3 => {
                    for x in 0..(rect.right - rect.left).max(0) as u32 {
                        let pixel_start = start + x * 3;
                        if let Some(dst) = ctx
                            .memory
                            .bytes
                            .get_mut(pixel_start as usize..)
                            .and_then(|b| b.get_mut(..3))
                        {
                            dst.copy_from_slice(&color.to_le_bytes()[..3]);
                        }
                    }
                }
                4 => {
                    for x in 0..(rect.right - rect.left).max(0) as u32 {
                        ctx.memory.write::<u32>(start + x * 4, color);
                    }
                }
                _ => {
                    log::warn!("Blt colorfill unsupported bpp {bpp}");
                    return DD::ERR_GENERIC;
                }
            }
        }
        dst.present(&mut ctx.memory);
        return DD::OK;
    }

    let color_key = if dwFlags & DDBLT_KEYSRCOVERRIDE != 0 {
        if !guest_range(ctx, lpDDBLTFX, 100) {
            return DD::ERR_INVALIDPARAMS;
        }
        // DDBLTFX.ddckSrcColorkey, past the z-buffer and alpha fields.
        Some(ColorKey {
            low: ctx.memory.read::<u32>(lpDDBLTFX + 92),
            high: ctx.memory.read::<u32>(lpDDBLTFX + 96),
        })
    } else if dwFlags & DDBLT_KEYSRC != 0 {
        surface_src_color_key(lpDDSrcSurface)
    } else {
        None
    };
    let dst_color_key = if dwFlags & DDBLT_KEYDESTOVERRIDE != 0 {
        if !guest_range(ctx, lpDDBLTFX, 92) {
            return DD::ERR_INVALIDPARAMS;
        }
        // DDBLTFX.ddckDestColorkey sits just before ddckSrcColorkey.
        Some(ColorKey {
            low: ctx.memory.read::<u32>(lpDDBLTFX + 84),
            high: ctx.memory.read::<u32>(lpDDBLTFX + 88),
        })
    } else if dwFlags & DDBLT_KEYDEST != 0 {
        surface_dst_color_key(this)
    } else {
        None
    };

    let src_rect = read_rect(ctx, lpSrcRect);
    log::debug!("Blt: dst={this:#x} src={lpDDSrcSurface:#x} flags={dwFlags:#x}");
    blit_copy(
        ctx,
        this,
        dst_rect,
        lpDDSrcSurface,
        src_rect,
        color_key,
        dst_color_key,
    )
}

pub fn blt_fast(
    ctx: &mut Context,
    this: u32,
    dwX: u32,
    dwY: u32,
    lpDDSrcSurface: u32,
    lpSrcRect: u32,
    dwTrans: u32,
) -> DD {
    const DDBLTFAST_SRCCOLORKEY: u32 = 0x0001;
    const DDBLTFAST_DESTCOLORKEY: u32 = 0x0002;
    const DDBLTFAST_WAIT: u32 = 0x0010;
    const KNOWN: u32 = DDBLTFAST_SRCCOLORKEY | DDBLTFAST_DESTCOLORKEY | DDBLTFAST_WAIT;
    if dwTrans & !KNOWN != 0 {
        log::warn!("BltFast: ignoring flags {:#x}", dwTrans & !KNOWN);
    }
    let dst_color_key = if dwTrans & DDBLTFAST_DESTCOLORKEY != 0 {
        surface_dst_color_key(this)
    } else {
        None
    };
    let color_key = if dwTrans & DDBLTFAST_SRCCOLORKEY != 0 {
        surface_src_color_key(lpDDSrcSurface)
    } else {
        None
    };

    let src_rect = read_rect(ctx, lpSrcRect);
    log::debug!("BltFast: dst={this:#x} src={lpDDSrcSurface:#x} ({dwX},{dwY})");
    let (w, h) = match &src_rect {
        Some(r) => ((r.right - r.left).max(0), (r.bottom - r.top).max(0)),
        None => {
            let surfaces = state().surf.borrow();
            let Some(src) = surfaces.get(&lpDDSrcSurface) else {
                return DD::ERR_INVALIDPARAMS;
            };
            let src = src.borrow();
            (src.width as i32, src.height as i32)
        }
    };
    let dst_rect = RECT {
        left: dwX as i32,
        top: dwY as i32,
        right: dwX as i32 + w,
        bottom: dwY as i32 + h,
    };
    blit_copy(
        ctx,
        this,
        Some(dst_rect),
        lpDDSrcSurface,
        src_rect,
        color_key,
        dst_color_key,
    )
}

pub fn set_color_key(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
    let key = if lpDDColorKey == 0 {
        None
    } else {
        if !guest_range(ctx, lpDDColorKey, 8) {
            return DD::ERR_INVALIDPARAMS;
        }
        // DDCOLORKEY: dwColorSpaceLowValue, dwColorSpaceHighValue.
        Some(ColorKey {
            low: ctx.memory.read::<u32>(lpDDColorKey),
            high: ctx.memory.read::<u32>(lpDDColorKey + 4),
        })
    };
    let surfaces = state().surf.borrow();
    let Some(surface) = surfaces.get(&this) else {
        return DD::ERR_GENERIC;
    };
    let mut surface = surface.borrow_mut();
    if dwFlags & (DDCKEY_SRCOVERLAY | DDCKEY_DESTOVERLAY) != 0 {
        log::warn!("SetColorKey: overlays are not supported");
    }
    if dwFlags & DDCKEY_DESTBLT != 0 {
        surface.dst_color_key = key;
    } else {
        surface.src_color_key = key;
    }
    log::debug!("SetColorKey: this={this:#x} flags={dwFlags:#x} key={key:?}");
    DD::OK
}

pub fn get_color_key(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
    let key = {
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_GENERIC;
        };
        let surface = surface.borrow();
        if dwFlags & DDCKEY_DESTBLT != 0 {
            surface.dst_color_key
        } else {
            surface.src_color_key
        }
    };
    let Some(key) = key else {
        return DD::ERR_NOCOLORKEY;
    };
    if !guest_range(ctx, lpDDColorKey, 8) {
        return DD::ERR_INVALIDPARAMS;
    }
    ctx.memory.write::<u32>(lpDDColorKey, key.low);
    ctx.memory.write::<u32>(lpDDColorKey + 4, key.high);
    DD::OK
}

#[win32_derive::dllexport]
pub fn DirectDrawEnumerateW(_ctx: &mut Context, _lpCallback: u32, _lpContext: u32) -> DD {
    DD::OK
}

#[win32_derive::dllexport]
pub fn DirectDrawEnumerateExW(
    _ctx: &mut Context,
    _lpCallback: u32,
    _lpContext: u32,
    _dwFlags: u32,
) -> DD {
    DD::OK
}

#[win32_derive::dllexport]
pub fn DirectDrawCreateClipper(
    _ctx: &mut Context,
    _dwFlags: u32,
    _lplpDDClipper: u32,
    _pUnkOuter: u32,
) -> DD {
    DD::ERR_GENERIC
}

#[win32_derive::dllexport]
pub fn GetDXVB(_ctx: &mut Context) -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use super::{
        ColorKey, DirectDraw, PALETTEENTRY, PixelFmt, RECT, Surface, Target, expand_palettized,
        lock_offset, write_blit, write_blit_convert,
    };
    use crate::{
        Ptr,
        ddraw::{
            state,
            types::{DD, DDPIXELFORMAT, DDSCAPS2, DDSD, DDSURFACEDESC2},
        },
        user32::{HWND, Window},
    };
    use runtime::{BlockCache, CPU, Context, Memory};
    use std::{cell::RefCell, rc::Rc};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x20_000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    fn entry(r: u8, g: u8, b: u8) -> PALETTEENTRY {
        PALETTEENTRY {
            peRed: r,
            peGreen: g,
            peBlue: b,
            peFlags: 0,
        }
    }

    fn test_window() -> Rc<RefCell<Window>> {
        let host_window: host::Window = unsafe { std::mem::zeroed() };
        Rc::new(RefCell::new(Window {
            hwnd: HWND::from_raw(1),
            style: 0,
            ex_style: 0,
            dirty: false,
            title: "Test".into(),
            enabled: true,
            visible: true,
            user_data: 0,
            hinstance: 0,
            id: 0,
            subclass_proc: None,
            paint_dc: None,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
            host: host_window,
            pixels: None,
            surface: None,
        }))
    }

    #[test]
    fn cooperative_level_null_hwnd_unbinds_the_window() {
        // SetCooperativeLevel(NULL, DDSCL_NORMAL) unbinds the device window.
        let mut ddraw = DirectDraw {
            addr: 0x4321,
            refs: 1,
            bytes_per_pixel: 2,
            window: Some(test_window()),
        };
        ddraw.set_cooperative_level(HWND::null(), 0x08);
        assert!(ddraw.window.is_none());
    }

    /// A 2x1 surface whose pixels live at `PIXELS` in `ctx.memory`, in the
    /// given 16bpp channel layout.
    fn surf16(pixel_format: DDPIXELFORMAT, target: Rc<RefCell<Window>>) -> Surface {
        Surface {
            addr: 0,
            refs: 1,
            width: 2,
            height: 1,
            bytes_per_pixel: 2,
            target: Target::Window(target),
            primary: None,
            attached: None,
            attachments: Vec::new(),
            pixels: Some(0x2000),
            palette: None,
            clipper: None,
            src_color_key: None,
            dst_color_key: None,
            caps: DDSCAPS2::default(),
            pixel_format,
            private_data: Default::default(),
            uniqueness: 1,
            priority: 0,
            max_lod: 0,
        }
    }

    #[test]
    fn dc_rgba_round_trip_uses_the_surfaces_channel_masks() {
        // GetDC exposes a 2bpp surface as an RGBA buffer and ReleaseDC packs
        // it back; both must use the surface's declared masks, not RGB565,
        // or alpha-bearing textures (1555/4444) come back scrambled.
        let mut ctx = context();
        let window = test_window();
        let fmt = |r: u32, g: u32, b: u32, a: u32| DDPIXELFORMAT {
            dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
            dwFlags: 0x41, // DDPF_RGB | DDPF_ALPHAPIXELS
            dwFourCC: 0,
            dwRGBBitCount: 16,
            dwRBitMask: r,
            dwGBitMask: g,
            dwBBitMask: b,
            dwRGBAlphaBitMask: a,
        };
        for (pixfmt, value) in [
            (fmt(0xF800, 0x07E0, 0x001F, 0x0000), 0xACE1u16), // RGB565
            (fmt(0x7C00, 0x03E0, 0x001F, 0x8000), 0xB54Au16), // A1R5G5B5
            (fmt(0x0F00, 0x00F0, 0x000F, 0xF000), 0xF123u16), // A4R4G4B4
        ] {
            let mut surf = surf16(pixfmt, window.clone());
            ctx.memory.write::<u16>(0x2000, value);
            let rgba = surf.to_rgba(&ctx.memory, &None).unwrap().into_owned();
            surf.write_rgba(&mut ctx.memory, &rgba, 0x3000);
            assert_eq!(ctx.memory.read::<u16>(0x3000), value);
        }
    }

    #[test]
    fn get_surface_desc_reports_the_surfaces_pixel_format() {
        // MM2 queries GetSurfaceDesc before uploading textures; the desc must
        // carry the surface's real pixel format or the game packs texels in a
        // layout the sampler cannot read.
        let mut ctx = context();
        let fmt = DDPIXELFORMAT {
            dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
            dwFlags: 0x41, // DDPF_RGB | DDPF_ALPHAPIXELS
            dwFourCC: 0,
            dwRGBBitCount: 16,
            dwRBitMask: 0x0F00,
            dwGBitMask: 0x00F0,
            dwBBitMask: 0x000F,
            dwRGBAlphaBitMask: 0xF000,
        };
        let surf = surf16(fmt.clone(), test_window());
        state()
            .surf
            .borrow_mut()
            .insert(0x5000, Rc::new(RefCell::new(surf)));
        // The caller sets dwSize before the call, as the API requires.
        ctx.memory
            .write::<u32>(0x4000, std::mem::size_of::<DDSURFACEDESC2>() as u32);
        assert_eq!(
            crate::ddraw::IDirectDrawSurface7::GetSurfaceDesc(&mut ctx, 0x5000, 0x4000),
            DD::OK
        );
        let desc = Ptr::<DDSURFACEDESC2>::new(0x4000)
            .read(&ctx.memory)
            .unwrap();
        assert!(desc.dwFlags.contains(DDSD::PIXELFORMAT | DDSD::CAPS));
        assert_eq!(desc.ddpfPixelFormat.dwRBitMask, 0x0F00);
        assert_eq!(desc.ddpfPixelFormat.dwRGBAlphaBitMask, 0xF000);
        assert!(desc.dwFlags.contains(DDSD::LPSURFACE | DDSD::PITCH));
        assert_eq!(desc.lpSurface, 0x2000);
        assert_eq!(desc.lPitch_dwLinearSize, 4);
        state().surf.borrow_mut().remove(&0x5000);
    }

    #[test]
    fn ddraw_object_is_dropped_on_the_last_release() {
        let mut ctx = context();
        *state().ddraw.borrow_mut() = Some(DirectDraw {
            addr: 0x4321,
            refs: 1,
            bytes_per_pixel: 2,
            window: None,
        });
        assert_eq!(crate::ddraw::IDirectDraw7::AddRef(&mut ctx, 0x4321), 2);
        assert_eq!(crate::ddraw::IDirectDraw7::Release(&mut ctx, 0x4321), 1);
        assert_eq!(crate::ddraw::IDirectDraw7::Release(&mut ctx, 0x4321), 0);
        assert!(state().ddraw.borrow().is_none());
        // Releasing a dead or unknown pointer is a no-op.
        assert_eq!(crate::ddraw::IDirectDraw7::Release(&mut ctx, 0x4321), 0);
        assert_eq!(crate::ddraw::IDirectDraw7::Release(&mut ctx, 0x9999), 0);
    }

    #[test]
    fn get_gdi_surface_returns_not_found_without_a_matching_primary() {
        // GetGDISurface must not return an arbitrary window surface; when the
        // DirectDraw object has no bound window or there is no matching primary
        // surface, it reports DDERR_NOTFOUND.
        let mut ctx = context();
        let ddraw_addr = 0x4321;
        *state().ddraw.borrow_mut() = Some(DirectDraw {
            addr: ddraw_addr,
            refs: 1,
            bytes_per_pixel: 2,
            window: None,
        });
        assert_eq!(
            crate::ddraw::IDirectDraw7::GetGDISurface(&mut ctx, ddraw_addr, 0x4000),
            DD::ERR_NOTFOUND
        );
        state().ddraw.borrow_mut().take();
    }

    #[test]
    fn lock_offset_points_at_the_regions_first_pixel() {
        let mut ctx = context();
        // A 4x4 surface at 0x4000 with 4 bytes per pixel has a 16-byte pitch.
        ctx.memory.write(
            0x1000,
            RECT {
                left: 1,
                top: 2,
                right: 3,
                bottom: 4,
            },
        );
        // No rect locks the whole surface.
        assert_eq!(lock_offset(&ctx, 0, 4, 4, 4, 0x4000), Some(0x4000));
        // The region's first pixel is top*pitch + left*bpp into the surface.
        assert_eq!(
            lock_offset(&ctx, 0x1000, 4, 4, 4, 0x4000),
            Some(0x4000 + 2 * 16 + 4)
        );
        // A rect entirely outside the surface clamps to an in-bounds address.
        ctx.memory.write(
            0x1000,
            RECT {
                left: -5,
                top: 90,
                right: -1,
                bottom: 99,
            },
        );
        assert_eq!(
            lock_offset(&ctx, 0x1000, 4, 4, 4, 0x4000),
            Some(0x4000 + 4 * 16)
        );
        // An unreadable rect is an error, not the base pointer.
        let oob = ctx.memory.bytes.len() as u32;
        assert_eq!(lock_offset(&ctx, oob, 4, 4, 4, 0x4000), None);
    }

    #[test]
    fn write_blit_stretches_a_different_size_rect() {
        let mut ctx = context();
        // A 2x1 32bpp source stretched into a 4x1 destination rect doubles
        // each pixel.
        let rows = [0x11, 0, 0, 0, 0x22, 0, 0, 0];
        let want = RECT {
            left: 0,
            top: 0,
            right: 4,
            bottom: 1,
        };
        let rect = want.clip_to_size(4, 1);
        assert_eq!(
            write_blit(
                &mut ctx.memory,
                0x4000,
                16,
                &want,
                &rect,
                &rows,
                2,
                1,
                4,
                None,
                None
            ),
            DD::OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x4000), 0x11);
        assert_eq!(ctx.memory.read::<u32>(0x4004), 0x11);
        assert_eq!(ctx.memory.read::<u32>(0x4008), 0x22);
        assert_eq!(ctx.memory.read::<u32>(0x400c), 0x22);
    }

    #[test]
    fn write_blit_destination_color_key_writes_only_matching_pixels() {
        let mut ctx = context();
        let rows = [0x11, 0, 0, 0, 0x22, 0, 0, 0];
        let want = RECT {
            left: 0,
            top: 0,
            right: 2,
            bottom: 1,
        };
        let rect = want.clip_to_size(2, 1);
        ctx.memory.write::<u32>(0x4000, 0xaa);
        ctx.memory.write::<u32>(0x4004, 0xbb);
        // Only the 0xaa pixel is inside the key range, so only it is
        // replaced.
        let key = Some(ColorKey {
            low: 0xaa,
            high: 0xaa,
        });
        assert_eq!(
            write_blit(
                &mut ctx.memory,
                0x4000,
                8,
                &want,
                &rect,
                &rows,
                2,
                1,
                4,
                None,
                key
            ),
            DD::OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x4000), 0x11);
        assert_eq!(ctx.memory.read::<u32>(0x4004), 0xbb);
    }

    #[test]
    fn write_blit_clips_a_far_offscreen_rect_without_panicking() {
        let mut ctx = context();
        // A same-size blit whose unclipped destination sits far off the
        // left edge must crop to nothing instead of indexing the staged
        // source rows past their end.
        let rows = [0x11, 0, 0, 0, 0x22, 0, 0, 0];
        let want = RECT {
            left: -1_000_000,
            top: 0,
            right: -999_998,
            bottom: 1,
        };
        let rect = want.clip_to_size(4, 1);
        assert_eq!(
            write_blit(
                &mut ctx.memory,
                0x4000,
                16,
                &want,
                &rect,
                &rows,
                2,
                1,
                4,
                None,
                None
            ),
            DD::OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x4000), 0);
    }

    fn rgb565() -> PixelFmt {
        PixelFmt {
            bpp: 2,
            rgb: true,
            r: 0xF800,
            g: 0x07E0,
            b: 0x001F,
            a: 0,
            palette: None,
        }
    }

    #[test]
    fn write_blit_convert_expands_indexed_pixels_to_565() {
        let mut ctx = context();
        let src = PixelFmt {
            bpp: 1,
            rgb: false,
            r: 0,
            g: 0,
            b: 0,
            a: 0,
            palette: Some(vec![entry(255, 0, 0), entry(0, 255, 0)]),
        };
        let rows = [0u8, 1];
        let want = RECT {
            left: 0,
            top: 0,
            right: 2,
            bottom: 1,
        };
        let rect = want.clip_to_size(2, 1);
        assert_eq!(
            write_blit_convert(
                &mut ctx.memory,
                0x4000,
                4,
                &want,
                &rect,
                &rows,
                2,
                1,
                &src,
                &rgb565(),
                None,
                None
            ),
            DD::OK
        );
        assert_eq!(ctx.memory.read::<u16>(0x4000), 0xF800);
        assert_eq!(ctx.memory.read::<u16>(0x4002), 0x07E0);
    }

    #[test]
    fn write_blit_convert_repositions_channels_to_32bpp() {
        let mut ctx = context();
        // ARGB8888: alpha in the top byte, red next.
        let dst = PixelFmt {
            bpp: 4,
            rgb: true,
            r: 0x00FF_0000,
            g: 0x0000_FF00,
            b: 0x0000_00FF,
            a: 0xFF00_0000,
            palette: None,
        };
        // A single 565 red pixel.
        let rows = 0xF800u16.to_le_bytes();
        let want = RECT {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        };
        let rect = want.clip_to_size(1, 1);
        assert_eq!(
            write_blit_convert(
                &mut ctx.memory,
                0x4000,
                4,
                &want,
                &rect,
                &rows,
                1,
                1,
                &rgb565(),
                &dst,
                None,
                None
            ),
            DD::OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x4000), 0xFFFF_0000);
    }

    #[test]
    fn write_blit_convert_applies_the_source_color_key() {
        let mut ctx = context();
        let src = PixelFmt {
            bpp: 1,
            rgb: false,
            r: 0,
            g: 0,
            b: 0,
            a: 0,
            palette: Some(vec![entry(255, 0, 0), entry(0, 255, 0)]),
        };
        let rows = [0u8, 1];
        let want = RECT {
            left: 0,
            top: 0,
            right: 2,
            bottom: 1,
        };
        let rect = want.clip_to_size(2, 1);
        // Index 0 is transparent; only index 1 lands.
        let key = Some(ColorKey { low: 0, high: 0 });
        assert_eq!(
            write_blit_convert(
                &mut ctx.memory,
                0x4000,
                4,
                &want,
                &rect,
                &rows,
                2,
                1,
                &src,
                &rgb565(),
                key,
                None
            ),
            DD::OK
        );
        assert_eq!(ctx.memory.read::<u16>(0x4000), 0);
        assert_eq!(ctx.memory.read::<u16>(0x4002), 0x07E0);
    }

    #[test]
    fn write_blit_convert_skips_a_paletteless_indexed_source() {
        let mut ctx = context();
        let src = PixelFmt {
            bpp: 1,
            rgb: false,
            r: 0,
            g: 0,
            b: 0,
            a: 0,
            palette: None,
        };
        let rows = [0u8, 1];
        let want = RECT {
            left: 0,
            top: 0,
            right: 2,
            bottom: 1,
        };
        let rect = want.clip_to_size(2, 1);
        assert_eq!(
            write_blit_convert(
                &mut ctx.memory,
                0x4000,
                4,
                &want,
                &rect,
                &rows,
                2,
                1,
                &src,
                &rgb565(),
                None,
                None
            ),
            DD::OK
        );
        assert_eq!(ctx.memory.read::<u32>(0x4000), 0);
    }

    #[test]
    fn palettized_expansion_falls_back_beyond_the_palette() {
        // A 2-entry palette (DDPCAPS_1BIT) indexing pixel values 0..=255:
        // indices past the table read black instead of panicking.
        let entries = vec![entry(10, 20, 30), entry(40, 50, 60)];
        let pixels = [0u8, 1, 2, 255];
        let mut buf = Vec::new();
        expand_palettized(&pixels, &entries, &mut buf);
        assert_eq!(
            buf,
            vec![
                10, 20, 30, 0, // index 0
                40, 50, 60, 0, // index 1
                0, 0, 0, 0, // index 2: out of range -> black
                0, 0, 0, 0, // index 255: out of range -> black
            ]
        );
    }

    #[test]
    fn query_interface_validates_riid_and_ppv() {
        let mut ctx = context();
        const THIS: u32 = 0x1234;
        const PPV: u32 = 0x1000;
        const RIID: u32 = 0x2000;

        // A supported ddraw7 interface writes the object pointer and returns OK.
        ctx.memory
            .write(RIID, crate::ddraw::ddraw7::IID_IDirectDraw7);
        assert_eq!(
            crate::ddraw::IDirectDraw7::QueryInterface(&mut ctx, THIS, RIID, PPV),
            DD::OK
        );
        assert_eq!(ctx.memory.try_read::<u32>(PPV), Some(THIS));

        // An unsupported interface zeros the output pointer and returns E_NOINTERFACE.
        let unknown =
            crate::ddraw::GUID::new(0xdead_beef, 0xcafe, 0xbabe, [1, 2, 3, 4, 5, 6, 7, 8]);
        ctx.memory.write(RIID + 0x20, unknown);
        ctx.memory.write::<u32>(PPV, 0x42);
        assert_eq!(
            crate::ddraw::IDirectDraw7::QueryInterface(&mut ctx, THIS, RIID + 0x20, PPV),
            DD::E_NOINTERFACE
        );
        assert_eq!(ctx.memory.try_read::<u32>(PPV), Some(0));

        // Bad riid or ppv pointers return ERR_INVALIDPARAMS instead of ignoring
        // the failure or returning E_NOINTERFACE.
        assert_eq!(
            crate::ddraw::IDirectDraw7::QueryInterface(&mut ctx, THIS, 0xffff_fff0, PPV),
            DD::ERR_INVALIDPARAMS
        );
        assert_eq!(
            crate::ddraw::IDirectDraw7::QueryInterface(&mut ctx, THIS, RIID, 0xffff_fff0),
            DD::ERR_INVALIDPARAMS
        );

        // The ddraw1 QueryInterface stubs also validate both pointers.
        assert_eq!(
            crate::ddraw::IDirectDraw::QueryInterface(&mut ctx, THIS, RIID, PPV),
            DD::E_NOINTERFACE
        );
        assert_eq!(ctx.memory.try_read::<u32>(PPV), Some(0));
        assert_eq!(
            crate::ddraw::IDirectDraw::QueryInterface(&mut ctx, THIS, 0xffff_fff0, PPV),
            DD::ERR_INVALIDPARAMS
        );
        assert_eq!(
            crate::ddraw::IDirectDraw::QueryInterface(&mut ctx, THIS, RIID, 0xffff_fff0),
            DD::ERR_INVALIDPARAMS
        );
    }
}
