use std::{cell::RefCell, rc::Rc};

use runtime::Context;
use zerocopy::{FromBytes, IntoBytes};

use crate::{
    RECT,
    ddraw::{ColorKey, DD, GUID, Palette, ddraw2, get_pixel_format, state, types::*},
    heap::Heap,
    kernel32, stub,
    user32::HWND,
};

pub const IID_IUnknown: GUID = GUID((
    0x00000000,
    0x0000,
    0x0000,
    [0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
));
pub const IID_IDirectDraw: GUID = GUID((
    0x6c14db80,
    0xa733,
    0x11ce,
    [0xa5, 0x21, 0x00, 0x20, 0xaf, 0x0b, 0xe5, 0x60],
));
pub const IID_IDirectDrawSurface: GUID = GUID((
    0x6c14db81,
    0xa733,
    0x11ce,
    [0xa5, 0x21, 0x00, 0x20, 0xaf, 0x0b, 0xe5, 0x60],
));

/// A DDSURFACEDESC describing a display mode.
pub fn mode_desc(width: u32, height: u32, bpp: u32) -> DDSURFACEDESC {
    let mut desc = DDSURFACEDESC::default();
    desc.dwSize = std::mem::size_of::<DDSURFACEDESC>() as u32;
    desc.dwFlags = DDSD::WIDTH | DDSD::HEIGHT | DDSD::PIXELFORMAT | DDSD::PITCH;
    desc.dwWidth = width;
    desc.dwHeight = height;
    desc.lPitch_dwLinearSize = width * bpp.div_ceil(8);
    desc.ddpfPixelFormat = get_pixel_format(bpp);
    desc
}

/// Fill in a DDCAPS (of whatever size the caller declared) for GetCaps.
fn write_caps(ctx: &mut Context, addr: u32) {
    let size = ctx.memory.read::<u32>(addr);
    if size < 8 {
        return;
    }
    ctx.memory[addr + 4..addr + size].fill(0);
    let mut write = |offset: u32, value: u32| {
        if offset + 4 <= size {
            ctx.memory.write::<u32>(addr + offset, value);
        }
    };
    // DDCAPS_BLT | DDCAPS_BLTSTRETCH | DDCAPS_GDI | DDCAPS_PALETTE | DDCAPS_COLORKEY
    // | DDCAPS_BLTCOLORFILL | DDCAPS_CANCLIP | DDCAPS_CANBLTSYSMEM
    write(
        0x04,
        0x40 | 0x200 | 0x400 | 0x8000 | 0x400000 | 0x4000000 | 0x20000000 | 0x80000000,
    );
    write(0x0c, 0x1 | 0x800); // dwCKeyCaps: DDCKEYCAPS_DESTBLT | DDCKEYCAPS_SRCBLT
    write(0x18, 0x4 | 0x10 | 0x40); // dwPalCaps: 8BIT | PRIMARYSURFACE | ALLOW256
    write(0x3c, 16 << 20); // dwVidMemTotal
    write(0x40, 16 << 20); // dwVidMemFree
    // dwRops[]: SRCCOPY (rop index 0xcc) is supported.
    write(0x64 + (0xcc / 32) * 4, 1 << (0xcc % 32));
    // ddsCaps: OFFSCREENPLAIN | PRIMARYSURFACE | FLIP | BACKBUFFER | COMPLEX
    // | PALETTE | SYSTEMMEMORY | VIDEOMEMORY
    write(
        0x84,
        0x40 | 0x200 | 0x10 | 0x4 | 0x8 | 0x100 | 0x800 | 0x4000,
    );
}

pub mod IDirectDraw {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 23] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Compact",
        "CreateClipper",
        "CreatePalette",
        "CreateSurface",
        "DuplicateSurface",
        "EnumDisplayModes",
        "EnumSurfaces",
        "FlipToGDISurface",
        "GetCaps",
        "GetDisplayMode",
        "GetFourCCCodes",
        "GetGDISurface",
        "GetMonitorFrequency",
        "GetScanLine",
        "GetVerticalBlankStatus",
        "Initialize",
        "RestoreDisplayMode",
        "SetCooperativeLevel",
        "SetDisplayMode",
        "WaitForVerticalBlank",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        let iid = crate::Ptr::<GUID>::new(riid).read(&ctx.memory).unwrap();
        match iid {
            IID_IUnknown | IID_IDirectDraw => {
                ctx.memory.write::<u32>(ppvObject, this);
                DD::OK
            }
            ddraw2::IID_IDirectDraw2 => {
                // A second interface pointer onto the same object, with the
                // IDirectDraw2 vtable.
                let addr = {
                    let mut kernel32 = kernel32::lock();
                    ddraw2::IDirectDraw2::new(ctx, &mut kernel32.process_heap)
                };
                state().get_ddraw(this).aliases.push(addr);
                ctx.memory.write::<u32>(ppvObject, addr);
                DD::OK
            }
            _ => {
                // Includes IID_IDirect3D: no 3D hardware, so games use their
                // software renderers.
                log::warn!("IDirectDraw::QueryInterface({iid:?}): not supported");
                DD::E_NOINTERFACE
            }
        }
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        // We don't reference count; the single DirectDraw object lives as long
        // as the process.
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        0
    }

    #[win32_derive::dllexport]
    pub fn Compact(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateClipper(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn CreatePalette(
        ctx: &mut Context,
        _this: u32,
        flags: DDPCAPS,
        lpEntries: u32,
        lplpPal: u32,
        pUnkOuter: u32,
    ) -> DD {
        assert_eq!(pUnkOuter, 0);
        assert!(flags.contains(DDPCAPS::_8BIT));

        let mut kernel32 = kernel32::lock();
        let ptr = IDirectDrawPalette::new(ctx, &mut kernel32.process_heap);

        let entries = <[PALETTEENTRY]>::ref_from_prefix_with_elems(&ctx.memory[lpEntries..], 256)
            .unwrap()
            .0;
        state().palette.borrow_mut().insert(
            ptr,
            Rc::new(RefCell::new(Palette {
                entries: entries.into_iter().cloned().collect(),
            })),
        );
        ctx.memory.write::<u32>(lplpPal, ptr);

        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateSurface(
        ctx: &mut Context,
        this: u32,
        desc: u32,
        lplpDDSurface: u32,
        _pUnkOuter: u32,
    ) -> DD {
        let mut ddraw = state().get_ddraw(this);
        let desc = <DDSURFACEDESC>::ref_from_prefix(&ctx.memory[desc..])
            .unwrap()
            .0;
        let desc2 = DDSURFACEDESC2::from_desc(&desc);
        let mut state = kernel32::lock();
        let surface = ddraw.create_surface(&desc2, &mut || {
            IDirectDrawSurface::new(ctx, &mut state.process_heap)
        });
        ctx.memory.write(lplpDDSurface, surface.borrow().addr);

        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DuplicateSurface(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn EnumDisplayModes(
        ctx: &mut Context,
        _this: u32,
        _dwFlags: u32,
        lpSurfaceDesc: u32,
        lpContext: u32,
        lpEnumCallback: u32,
    ) -> DD {
        if lpSurfaceDesc != 0 {
            todo!("EnumDisplayModes with a filter desc");
        }

        // Report the standard display modes; games match these against their
        // internal mode tables by width/height/bit count.
        const RESOLUTIONS: &[(u32, u32)] = &[(640, 480), (800, 600), (1024, 768)];
        // Only depths the surface code can actually convert to rgba.
        const BIT_DEPTHS: &[u32] = &[8, 16, 32];

        for &(width, height) in RESOLUTIONS {
            for &bpp in BIT_DEPTHS {
                let desc = mode_desc(width, height, bpp);
                let desc_addr = kernel32::lock()
                    .process_heap
                    .alloc(&mut ctx.memory, desc.dwSize);
                ctx.memory.write(desc_addr, desc);
                let callback = ctx.indirect(lpEnumCallback);
                ctx.call32_x86(callback, vec![desc_addr, lpContext]);
                let ret = ctx.cpu.regs.eax;
                kernel32::lock()
                    .process_heap
                    .free(&mut ctx.memory, desc_addr);

                // DDENUMRET_CANCEL (0) means stop enumerating.
                if ret == 0 {
                    return DD::OK;
                }
            }
        }

        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EnumSurfaces(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn FlipToGDISurface(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, _this: u32, lpDDDriverCaps: u32, lpDDHELCaps: u32) -> DD {
        // The same capabilities for the "hardware" and the emulation layer:
        // everything is software here anyway.
        for addr in [lpDDDriverCaps, lpDDHELCaps] {
            if addr != 0 {
                write_caps(ctx, addr);
            }
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetDisplayMode(ctx: &mut Context, this: u32, lpDDSurfaceDesc: u32) -> DD {
        let (width, height, bpp) = {
            let ddraw = state().get_ddraw(this);
            let bpp = ddraw.bytes_per_pixel * 8;
            match &ddraw.window {
                Some(window) => {
                    let window = window.borrow();
                    (window.width, window.height, bpp)
                }
                None => (640, 480, bpp),
            }
        };
        ctx.memory
            .write(lpDDSurfaceDesc, mode_desc(width, height, bpp));
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetFourCCCodes(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetGDISurface(_ctx: &mut Context, _this: u32, _lplpGDIDDSSurface: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetMonitorFrequency(ctx: &mut Context, _this: u32, lpdwFrequency: u32) -> DD {
        ctx.memory.write::<u32>(lpdwFrequency, 60);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetScanLine(ctx: &mut Context, _this: u32, lpdwScanLine: u32) -> DD {
        ctx.memory.write::<u32>(lpdwScanLine, 0);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetVerticalBlankStatus(ctx: &mut Context, _this: u32, lpbIsInVB: u32) -> DD {
        // Always in the vertical blank: a program waiting for it never spins.
        ctx.memory.write::<u32>(lpbIsInVB, 1);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Initialize(
        _ctx: &mut Context,
        _this: u32,
        _lpDD: u32,
        _dwFlags: u32,
        _lpDDColorTable: u32,
    ) -> DD {
        // Nothing to do: the object is fully constructed when it's created.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn RestoreDisplayMode(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetCooperativeLevel(_ctx: &mut Context, this: u32, hwnd: HWND, flags: u32) -> DD {
        state().get_ddraw(this).set_cooperative_level(hwnd, flags);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetDisplayMode(ctx: &mut Context, this: u32, width: u32, height: u32, bpp: u32) -> DD {
        let mut ddraw = state().get_ddraw(this);
        ddraw
            .window
            .as_ref()
            .unwrap()
            .borrow_mut()
            .resize(ctx, width, height);
        assert!(bpp % 8 == 0);
        ddraw.bytes_per_pixel = bpp / 8;
        stub!(DD::OK)
    }

    #[win32_derive::dllexport]
    pub fn WaitForVerticalBlank(_ctx: &mut Context, _this: u32, _dwFlags: u32, _hEvent: u32) -> DD {
        DD::OK // pretend the vblank already happened
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }
}

pub mod IDirectDrawSurface {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 36] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "AddAttachedSurface",
        "AddOverlayDirtyRect",
        "Blt",
        "BltBatch",
        "BltFast",
        "DeleteAttachedSurface",
        "EnumAttachedSurfaces",
        "EnumOverlayZOrders",
        "Flip",
        "GetAttachedSurface",
        "GetBltStatus",
        "GetCaps",
        "GetClipper",
        "GetColorKey",
        "GetDC",
        "GetFlipStatus",
        "GetOverlayPosition",
        "GetPalette",
        "GetPixelFormat",
        "GetSurfaceDesc",
        "Initialize",
        "IsLost",
        "Lock",
        "ReleaseDC",
        "Restore",
        "SetClipper",
        "SetColorKey",
        "SetOverlayPosition",
        "SetPalette",
        "Unlock",
        "UpdateOverlay",
        "UpdateOverlayDisplay",
        "UpdateOverlayZOrder",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        let iid = crate::Ptr::<GUID>::new(riid).read(&ctx.memory).unwrap();
        match iid {
            IID_IUnknown | IID_IDirectDrawSurface => {
                ctx.memory.write::<u32>(ppvObject, this);
                DD::OK
            }
            ddraw2::IID_IDirectDrawSurface2 => {
                let Some(surface) = state().surf.borrow().get(&this).cloned() else {
                    return DD::E_NOINTERFACE;
                };
                let addr = {
                    let mut kernel32 = kernel32::lock();
                    ddraw2::IDirectDrawSurface2::new(ctx, &mut kernel32.process_heap)
                };
                // The new pointer names the same surface.
                state().surf.borrow_mut().insert(addr, surface);
                ctx.memory.write::<u32>(ppvObject, addr);
                DD::OK
            }
            _ => {
                log::warn!("IDirectDrawSurface::QueryInterface({iid:?}): not supported");
                DD::E_NOINTERFACE
            }
        }
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, this: u32) -> u32 {
        match state().surf.borrow_mut().get(&this) {
            Some(surface) => {
                let mut surface = surface.borrow_mut();
                surface.refs += 1;
                surface.refs
            }
            None => 0,
        }
    }

    #[win32_derive::dllexport]
    pub fn Release(ctx: &mut Context, this: u32) -> u32 {
        let surfaces = state().surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return 0;
        };
        let remaining = {
            let mut surface = surface.borrow_mut();
            surface.refs = surface.refs.saturating_sub(1);
            surface.refs
        };
        drop(surfaces);
        if remaining > 0 {
            return remaining;
        }
        let Some(surface) = state().surf.borrow_mut().remove(&this) else {
            return 0;
        };
        // Games recreate surfaces when changing screens, so returning the
        // pixels keeps the heap from growing without bound.
        if let Some(pixels) = surface.borrow_mut().pixels.take() {
            kernel32::lock().process_heap.free(&mut ctx.memory, pixels);
        }
        0
    }

    #[win32_derive::dllexport]
    pub fn AddAttachedSurface(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn AddOverlayDirtyRect(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    fn read_rect(ctx: &Context, addr: u32) -> Option<RECT> {
        if addr == 0 {
            None
        } else {
            crate::Ptr::<RECT>::new(addr).read(&ctx.memory)
        }
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
    ) {
        let src_rc = state().surf.borrow_mut().get(&src_ptr).unwrap().clone();
        let dst_rc = state().surf.borrow_mut().get(&dst_ptr).unwrap().clone();

        let (rows, row_bytes, row_count, bpp) = {
            let mut src = src_rc.borrow_mut();
            let addr = src.lock(&mut ctx.memory);
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
                rows.extend_from_slice(&ctx.memory[start..][..row_bytes]);
            }
            (rows, row_bytes, row_count, bpp)
        };

        let mut dst = dst_rc.borrow_mut();
        if dst.bytes_per_pixel != bpp {
            log::warn!("blit between different pixel formats");
            return;
        }
        let addr = dst.lock(&mut ctx.memory);
        let stride = dst.width * bpp;
        let want = dst_rect.unwrap_or_else(|| RECT::from_size(dst.width, dst.height));
        let rect = want.clip_to_size(dst.width, dst.height);
        // Whatever the clip took off the top and left has to come off the
        // source as well, otherwise the image slides instead of being cropped.
        let skip_x = (rect.left - want.left).max(0) as usize * bpp as usize;
        let skip_y = (rect.top - want.top).max(0) as usize;
        // No stretching: copy 1:1, clipped to both rects.
        let copy_bytes = row_bytes
            .saturating_sub(skip_x)
            .min(((rect.right - rect.left).max(0) as u32 * bpp) as usize);
        let copy_rows = row_count
            .saturating_sub(skip_y)
            .min((rect.bottom - rect.top).max(0) as usize);
        for i in 0..copy_rows {
            let dst_start = addr + (rect.top + i as i32) as u32 * stride + rect.left as u32 * bpp;
            let row = &rows[(i + skip_y) * row_bytes + skip_x..][..copy_bytes];
            match color_key {
                None => ctx.memory[dst_start..][..copy_bytes].copy_from_slice(row),
                Some(key) => {
                    for (x, pixel) in row.chunks_exact(bpp as usize).enumerate() {
                        let value = match bpp {
                            1 => pixel[0] as u32,
                            2 => u16::from_le_bytes(pixel.try_into().unwrap()) as u32,
                            4 => u32::from_le_bytes(pixel.try_into().unwrap()),
                            _ => {
                                log::warn!("colorkey blit at {bpp} bytes per pixel");
                                return;
                            }
                        };
                        if key.matches(value) {
                            continue;
                        }
                        let at = dst_start + x as u32 * bpp;
                        ctx.memory[at..][..bpp as usize].copy_from_slice(pixel);
                    }
                }
            }
        }
        dst.present(&mut ctx.memory);
    }

    #[win32_derive::dllexport]
    pub fn Blt(
        ctx: &mut Context,
        this: u32,
        lpDstRect: u32,
        lpDDSrcSurface: u32,
        lpSrcRect: u32,
        dwFlags: u32,
        lpDDBLTFX: u32,
    ) -> DD {
        const DDBLT_COLORFILL: u32 = 0x0400;
        const DDBLT_KEYSRC: u32 = 0x8000;
        const DDBLT_KEYSRCOVERRIDE: u32 = 0x0001_0000;
        const DDBLT_WAIT: u32 = 0x0100_0000;
        const KNOWN: u32 = DDBLT_COLORFILL | DDBLT_KEYSRC | DDBLT_KEYSRCOVERRIDE | DDBLT_WAIT;
        if dwFlags & !KNOWN != 0 {
            log::warn!("Blt: ignoring flags {:#x}", dwFlags & !KNOWN);
        }

        let dst_rect = read_rect(ctx, lpDstRect);
        if dwFlags & DDBLT_COLORFILL != 0 {
            // DDBLTFX.dwFillColor is at offset 80.
            let color = ctx.memory.read::<u32>(lpDDBLTFX + 80);
            let dst_rc = state().surf.borrow_mut().get(&this).unwrap().clone();
            let mut dst = dst_rc.borrow_mut();
            let bpp = dst.bytes_per_pixel;
            let rect = dst_rect
                .unwrap_or_else(|| RECT::from_size(dst.width, dst.height))
                .clip_to_size(dst.width, dst.height);
            let addr = dst.lock(&mut ctx.memory);
            let stride = dst.width * bpp;
            for y in rect.top..rect.bottom {
                let start = addr + y as u32 * stride + rect.left as u32 * bpp;
                let width_bytes = ((rect.right - rect.left).max(0) as u32 * bpp) as usize;
                match bpp {
                    1 => ctx.memory[start..][..width_bytes].fill(color as u8),
                    4 => {
                        for x in 0..(rect.right - rect.left).max(0) as u32 {
                            ctx.memory.write::<u32>(start + x * 4, color);
                        }
                    }
                    _ => todo!("Blt colorfill bpp {bpp}"),
                }
            }
            dst.present(&mut ctx.memory);
            return DD::OK;
        }

        let color_key = if dwFlags & DDBLT_KEYSRCOVERRIDE != 0 {
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

        let src_rect = read_rect(ctx, lpSrcRect);
        blit_copy(ctx, this, dst_rect, lpDDSrcSurface, src_rect, color_key);
        DD::OK
    }

    fn surface_src_color_key(surface: u32) -> Option<ColorKey> {
        let surfaces = state().surf.borrow();
        let key = surfaces.get(&surface)?.borrow().src_color_key;
        if key.is_none() {
            log::warn!("blit asked for a source color key, but none is set");
        }
        key
    }

    #[win32_derive::dllexport]
    pub fn BltBatch(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn BltFast(
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
        if dwTrans & DDBLTFAST_DESTCOLORKEY != 0 {
            // Would need to test the destination pixel rather than the source;
            // no caller has needed it.
            log::warn!("BltFast: destination color key not supported");
        }
        let color_key = if dwTrans & DDBLTFAST_SRCCOLORKEY != 0 {
            surface_src_color_key(lpDDSrcSurface)
        } else {
            None
        };

        let src_rect = read_rect(ctx, lpSrcRect);
        let (w, h) = match &src_rect {
            Some(r) => ((r.right - r.left).max(0), (r.bottom - r.top).max(0)),
            None => {
                let src = state()
                    .surf
                    .borrow_mut()
                    .get(&lpDDSrcSurface)
                    .unwrap()
                    .clone();
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
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DeleteAttachedSurface(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn EnumAttachedSurfaces(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn EnumOverlayZOrders(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Flip(
        ctx: &mut Context,
        this: u32,
        _lpDDSurfaceTargetOverride: u32,
        _dwFlags: u32,
    ) -> DD {
        {
            let surfaces = state().surf.borrow_mut();
            let mut surface = surfaces.get(&this).unwrap().borrow_mut();
            surface.flip(&mut ctx.memory);
        }
        // A frame flip is the one thing a game does every frame no matter what
        // it's doing, so it's where we keep the audio mixer fed.
        crate::dsound::pump(ctx);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetAttachedSurface(
        ctx: &mut Context,
        this: u32,
        _lpDDSCaps: u32,
        lplpDDAttachedSurface: u32,
    ) -> DD {
        let surfaces = state().surf.borrow_mut();
        let surface = surfaces.get(&this).unwrap().borrow();
        ctx.memory.write(
            lplpDDAttachedSurface,
            surface.attached.as_ref().unwrap().borrow().addr,
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetBltStatus(_ctx: &mut Context, _this: u32, _dwFlags: u32) -> DD {
        DD::OK // blits complete synchronously
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, _this: u32, lpDDSCaps: u32) -> DD {
        let caps = DDSCAPS::BACKBUFFER | DDSCAPS::COMPLEX | DDSCAPS::FLIP | DDSCAPS::VIDEOMEMORY;
        ctx.memory.write::<u32>(lpDDSCaps, caps.bits());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetClipper(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetColorKey(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
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
        ctx.memory.write::<u32>(lpDDColorKey, key.low);
        ctx.memory.write::<u32>(lpDDColorKey + 4, key.high);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetDC(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetFlipStatus(_ctx: &mut Context, _this: u32, _dwFlags: u32) -> DD {
        DD::OK // flips complete synchronously
    }

    #[win32_derive::dllexport]
    pub fn GetOverlayPosition(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetPalette(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetPixelFormat(ctx: &mut Context, this: u32, lpDDPixelFormat: u32) -> DD {
        let bpp = state().surf.borrow().get(&this).unwrap().borrow().bytes_per_pixel * 8;
        ctx.memory.write(lpDDPixelFormat, get_pixel_format(bpp));
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetSurfaceDesc(ctx: &mut Context, this: u32, lpDDSurfaceDesc: u32) -> DD {
        let desc = {
            let surfaces = state().surf.borrow_mut();
            let surface = surfaces.get(&this).unwrap().borrow();
            let pixel_format = get_pixel_format(surface.bytes_per_pixel * 8);
            DDSURFACEDESC {
                dwSize: std::mem::size_of::<DDSURFACEDESC>() as u32,
                dwFlags: DDSD::WIDTH | DDSD::HEIGHT | DDSD::PITCH | DDSD::PIXELFORMAT,
                dwWidth: surface.width,
                dwHeight: surface.height,
                lPitch_dwLinearSize: surface.width * surface.bytes_per_pixel,
                ddpfPixelFormat: pixel_format,
                ..DDSURFACEDESC::default()
            }
        };
        desc.write_to_prefix(&mut ctx.memory[lpDDSurfaceDesc..])
            .unwrap();
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Initialize(
        _ctx: &mut Context,
        _this: u32,
        _lpDD: u32,
        _dwFlags: u32,
        _lpDDColorTable: u32,
    ) -> DD {
        // Nothing to do: the object is fully constructed when it's created.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn IsLost(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK // our surfaces are never lost
    }

    #[win32_derive::dllexport]
    pub fn Lock(
        ctx: &mut Context,
        this: u32,
        rect: u32,
        lpDesc: u32,
        _flags: u32,
        _unused: u32,
    ) -> DD {
        let surfaces = state().surf.borrow_mut();
        let mut surface = surfaces.get(&this).unwrap().borrow_mut();
        assert_eq!(rect, 0);

        let pixels = surface.lock(&mut ctx.memory);
        let desc = DDSURFACEDESC {
            dwSize: std::mem::size_of::<DDSURFACEDESC>() as u32,
            lPitch_dwLinearSize: surface.width * surface.bytes_per_pixel,
            lpSurface: pixels,
            ..DDSURFACEDESC::default()
        };
        desc.write_to_prefix(&mut ctx.memory[lpDesc..]).unwrap();
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn ReleaseDC(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Restore(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK // never lost, nothing to restore
    }

    #[win32_derive::dllexport]
    pub fn SetClipper(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetColorKey(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
        let key = if lpDDColorKey == 0 {
            None
        } else {
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
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetOverlayPosition(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetPalette(_ctx: &mut Context, this: u32, lpPalette: u32) -> DD {
        let state = state();
        let surfaces = state.surf.borrow_mut();
        let mut surface = surfaces.get(&this).unwrap().borrow_mut();
        let palettes = state.palette.borrow_mut();
        let palette = palettes.get(&lpPalette).unwrap();
        surface.palette = Some(palette.clone());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unlock(ctx: &mut Context, this: u32, _lpRect: u32) -> DD {
        let surfaces = state().surf.borrow_mut();
        let mut surface = surfaces.get(&this).unwrap().borrow_mut();
        // unlock presents window-backed surfaces itself.
        surface.unlock(&mut ctx.memory);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlay(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlayDisplay(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlayZOrder(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }
}

pub mod IDirectDrawPalette {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 7] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "GetCaps",
        "GetEntries",
        "Initialize",
        "SetEntries",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, _this: u32, riid: u32, _ppvObject: u32) -> DD {
        let iid = crate::Ptr::<GUID>::new(riid).read(&ctx.memory);
        log::warn!("IDirectDrawPalette::QueryInterface({iid:?}): not supported");
        DD::E_NOINTERFACE
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(ctx: &mut Context, this: u32) -> u32 {
        // Surfaces hold their own reference to the palette, so dropping it from
        // the table doesn't disturb anything still displaying it.
        state().palette.borrow_mut().remove(&this);
        kernel32::lock().process_heap.free(&mut ctx.memory, this);
        0
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, _this: u32, lpdwCaps: u32) -> DD {
        // We only ever create 8-bit palettes with all 256 entries settable.
        let caps = DDPCAPS::_8BIT | DDPCAPS::ALLOW256;
        ctx.memory.write::<u32>(lpdwCaps, caps.bits());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetEntries(
        ctx: &mut Context,
        this: u32,
        _dwFlags: u32,
        dwBase: u32,
        dwNumEntries: u32,
        lpEntries: u32,
    ) -> DD {
        let palettes = state().palette.borrow();
        let Some(palette) = palettes.get(&this) else {
            return DD::ERR_GENERIC;
        };
        let palette = palette.borrow();
        for i in 0..dwNumEntries {
            let Some(entry) = palette.entries.get((dwBase + i) as usize) else {
                break;
            };
            ctx.memory.write(lpEntries + i * 4, entry.clone());
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Initialize(
        _ctx: &mut Context,
        _this: u32,
        _lpDD: u32,
        _dwFlags: u32,
        _lpDDColorTable: u32,
    ) -> DD {
        // Nothing to do: the object is fully constructed when it's created.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetEntries(
        ctx: &mut Context,
        this: u32,
        _dwFlags: u32,
        dwStartingEntry: u32,
        dwCount: u32,
        lpEntries: u32,
    ) -> DD {
        let new_entries = <[PALETTEENTRY]>::ref_from_prefix_with_elems(
            &ctx.memory[lpEntries..],
            dwCount as usize,
        )
        .unwrap()
        .0
        .to_vec();
        let palettes = state().palette.borrow_mut();
        let mut palette = palettes.get(&this).unwrap().borrow_mut();
        for (i, entry) in new_entries.into_iter().enumerate() {
            let index = dwStartingEntry as usize + i;
            if index < palette.entries.len() {
                palette.entries[index] = entry;
            }
        }
        DD::OK
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }
}
