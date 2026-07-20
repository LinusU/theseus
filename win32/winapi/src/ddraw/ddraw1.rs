use std::{cell::RefCell, rc::Rc};

use runtime::Context;
use zerocopy::{FromBytes, IntoBytes};

use crate::{
    RECT,
    ddraw::{DD, Palette, get_pixel_format, state, types::*},
    heap::Heap,
    kernel32, stub,
    user32::HWND,
};

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
    pub fn QueryInterface(_ctx: &mut Context, _this: u32, _riid: u32, _ppvObject: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Compact(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
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
        const BIT_DEPTHS: &[u32] = &[8, 16, 24];

        for &(width, height) in RESOLUTIONS {
            for &bpp in BIT_DEPTHS {
                let mut desc = DDSURFACEDESC::default();
                desc.dwSize = std::mem::size_of::<DDSURFACEDESC>() as u32;
                desc.dwFlags = DDSD::WIDTH | DDSD::HEIGHT | DDSD::PIXELFORMAT | DDSD::PITCH;
                desc.dwWidth = width;
                desc.dwHeight = height;
                desc.lPitch_dwLinearSize = width * bpp.div_ceil(8);

                // DDPF_RGB = 0x40, DDPF_PALETTEINDEXED8 = 0x20.
                let (flags, r, g, b) = match bpp {
                    8 => (0x40 | 0x20, 0, 0, 0),
                    16 => (0x40, 0xF800, 0x07E0, 0x001F), // 5-6-5
                    _ => (0x40, 0xFF0000, 0x00FF00, 0x0000FF), // 24/32
                };
                desc.ddpfPixelFormat = DDPIXELFORMAT {
                    dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
                    dwFlags: flags,
                    dwFourCC: 0,
                    dwRGBBitCount: bpp,
                    dwRBitMask: r,
                    dwGBitMask: g,
                    dwBBitMask: b,
                    dwRGBAlphaBitMask: 0,
                };

                let desc_addr = kernel32::lock()
                    .process_heap
                    .alloc(&mut ctx.memory, desc.dwSize);
                ctx.memory.write(desc_addr, desc);
                let callback = ctx.indirect(lpEnumCallback);
                ctx.call32_x86(callback, vec![desc_addr, lpContext]);
                let ret = ctx.cpu.regs.eax;
                kernel32::lock().process_heap.free(&mut ctx.memory, desc_addr);

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
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetDisplayMode(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetFourCCCodes(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetGDISurface(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetMonitorFrequency(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetScanLine(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetVerticalBlankStatus(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn RestoreDisplayMode(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
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
    pub fn QueryInterface(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn AddAttachedSurface(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn AddOverlayDirtyRect(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    fn full_rect(width: u32, height: u32) -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        }
    }

    fn clip_rect(rect: RECT, width: u32, height: u32) -> RECT {
        rect.clip(&full_rect(width, height))
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
    fn blit_copy(
        ctx: &mut Context,
        dst_ptr: u32,
        dst_rect: Option<RECT>,
        src_ptr: u32,
        src_rect: Option<RECT>,
    ) {
        let src_rc = state().surf.borrow_mut().get(&src_ptr).unwrap().clone();
        let dst_rc = state().surf.borrow_mut().get(&dst_ptr).unwrap().clone();

        let (rows, row_bytes, row_count, bpp) = {
            let mut src = src_rc.borrow_mut();
            let addr = src.lock(&mut ctx.memory);
            let bpp = src.bytes_per_pixel;
            let stride = src.width * bpp;
            let rect = clip_rect(
                src_rect.unwrap_or_else(|| full_rect(src.width, src.height)),
                src.width,
                src.height,
            );
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
        let rect = clip_rect(
            dst_rect.unwrap_or_else(|| full_rect(dst.width, dst.height)),
            dst.width,
            dst.height,
        );
        // No stretching: copy 1:1, clipped to both rects.
        let copy_bytes = row_bytes.min(((rect.right - rect.left).max(0) as u32 * bpp) as usize);
        let copy_rows = row_count.min((rect.bottom - rect.top).max(0) as usize);
        for i in 0..copy_rows {
            let dst_start = addr + (rect.top + i as i32) as u32 * stride + rect.left as u32 * bpp;
            ctx.memory[dst_start..][..copy_bytes]
                .copy_from_slice(&rows[i * row_bytes..][..copy_bytes]);
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
        const DDBLT_WAIT: u32 = 0x0100_0000;
        const KNOWN: u32 = DDBLT_COLORFILL | DDBLT_WAIT;
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
            let rect = clip_rect(
                dst_rect.unwrap_or_else(|| full_rect(dst.width, dst.height)),
                dst.width,
                dst.height,
            );
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

        let src_rect = read_rect(ctx, lpSrcRect);
        blit_copy(ctx, this, dst_rect, lpDDSrcSurface, src_rect);
        DD::OK
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
        if dwTrans & !0x10 != 0 {
            // e.g. DDBLTFAST_SRCCOLORKEY; transparency not implemented yet.
            log::warn!("BltFast: ignoring flags {dwTrans:#x}");
        }
        let src_rect = read_rect(ctx, lpSrcRect);
        let (w, h) = match &src_rect {
            Some(r) => ((r.right - r.left).max(0), (r.bottom - r.top).max(0)),
            None => {
                let src = state().surf.borrow_mut().get(&lpDDSrcSurface).unwrap().clone();
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
        blit_copy(ctx, this, Some(dst_rect), lpDDSrcSurface, src_rect);
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
        let surfaces = state().surf.borrow_mut();
        let mut surface = surfaces.get(&this).unwrap().borrow_mut();
        surface.flip(&mut ctx.memory);
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
    pub fn GetBltStatus(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
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
    pub fn GetColorKey(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetDC(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetFlipStatus(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
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
    pub fn GetPixelFormat(ctx: &mut Context, _this: u32, lpDDPixelFormat: u32) -> DD {
        ctx.memory.write(lpDDPixelFormat, get_pixel_format());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetSurfaceDesc(ctx: &mut Context, this: u32, lpDDSurfaceDesc: u32) -> DD {
        let desc = {
            let surfaces = state().surf.borrow_mut();
            let surface = surfaces.get(&this).unwrap().borrow();
            let bpp = surface.bytes_per_pixel * 8;
            let pixel_format = if bpp == 8 {
                DDPIXELFORMAT {
                    dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
                    dwFlags: 0x40 | 0x20, // DDPF_RGB | DDPF_PALETTEINDEXED8
                    dwFourCC: 0,
                    dwRGBBitCount: 8,
                    dwRBitMask: 0,
                    dwGBitMask: 0,
                    dwBBitMask: 0,
                    dwRGBAlphaBitMask: 0,
                }
            } else {
                get_pixel_format()
            };
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
    pub fn Initialize(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
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
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetClipper(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetColorKey(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
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
        surface.unlock(&mut ctx.memory);
        // An app drawing straight to the primary surface expects it on screen.
        surface.present(&mut ctx.memory);
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
    pub fn QueryInterface(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetEntries(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32) -> DD {
        todo!()
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
        let new_entries =
            <[PALETTEENTRY]>::ref_from_prefix_with_elems(&ctx.memory[lpEntries..], dwCount as usize)
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
