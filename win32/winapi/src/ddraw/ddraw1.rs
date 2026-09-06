use runtime::Context;
use zerocopy::{FromBytes, IntoBytes};

use crate::{
    ddraw::{DD, GUID, get_pixel_format, state, types::*},
    gdi32::HDC,
    heap::Heap,
    kernel32,
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

    pub const VTABLE_FUNCS: [runtime::ContFn; 23] = [
        QueryInterface_stdcall,
        AddRef_stdcall,
        Release_stdcall,
        Compact_stdcall,
        CreateClipper_stdcall,
        CreatePalette_stdcall,
        CreateSurface_stdcall,
        DuplicateSurface_stdcall,
        EnumDisplayModes_stdcall,
        EnumSurfaces_stdcall,
        FlipToGDISurface_stdcall,
        GetCaps_stdcall,
        GetDisplayMode_stdcall,
        GetFourCCCodes_stdcall,
        GetGDISurface_stdcall,
        GetMonitorFrequency_stdcall,
        GetScanLine_stdcall,
        GetVerticalBlankStatus_stdcall,
        Initialize_stdcall,
        RestoreDisplayMode_stdcall,
        SetCooperativeLevel_stdcall,
        SetDisplayMode_stdcall,
        WaitForVerticalBlank_stdcall,
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, _this: u32, riid: u32, _ppvObject: u32) -> DD {
        let iid = crate::Ptr::<GUID>::new(riid).read(&ctx.memory);
        log::warn!("IDirectDraw::QueryInterface({iid:?}): not supported");
        DD::E_NOINTERFACE
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
        // Nothing to compact: emulated surfaces are not real video memory.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateClipper(
        ctx: &mut Context,
        this: u32,
        dwFlags: u32,
        lplpDDClipper: u32,
        pUnkOuter: u32,
    ) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::CreateClipper(
            ctx,
            this,
            dwFlags,
            lplpDDClipper,
            pUnkOuter,
        )
    }

    #[win32_derive::dllexport]
    pub fn CreatePalette(
        ctx: &mut Context,
        _this: u32,
        flags: Result<DDPCAPS, u32>,
        lpEntries: u32,
        lplpPal: u32,
        pUnkOuter: u32,
    ) -> DD {
        if pUnkOuter != 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let Ok(flags) = flags else {
            return DD::ERR_INVALIDPARAMS;
        };
        crate::ddraw::create_palette(ctx, flags.bits(), lpEntries, lplpPal, |ctx| {
            IDirectDrawPalette::new(ctx, &mut kernel32::lock().process_heap)
        })
    }

    #[win32_derive::dllexport]
    pub fn CreateSurface(
        ctx: &mut Context,
        this: u32,
        desc: u32,
        lplpDDSurface: u32,
        _pUnkOuter: u32,
    ) -> DD {
        let Some(mut ddraw) = state().get_ddraw(this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let Some(desc) = crate::Ptr::<DDSURFACEDESC>::new(desc).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if desc.dwSize != std::mem::size_of::<DDSURFACEDESC>() as u32 {
            return DD::ERR_INVALIDPARAMS;
        }
        let desc2 = DDSURFACEDESC2::from_desc(&desc);
        let mut state = kernel32::lock();
        let Some(surface) = ddraw.create_surface(&desc2, &mut || {
            IDirectDrawSurface::new(ctx, &mut state.process_heap)
        }) else {
            return DD::ERR_GENERIC;
        };
        let addr = surface.borrow().addr;
        if crate::Ptr::<u32>::new(lplpDDSurface)
            .write(&mut ctx.memory, addr)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }

        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DuplicateSurface(
        ctx: &mut Context,
        this: u32,
        lpDDSurface: u32,
        lplpDupDDSurface: u32,
    ) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::DuplicateSurface(
            ctx,
            this,
            lpDDSurface,
            lplpDupDDSurface,
        )
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
        // A filter desc limits enumeration to modes matching its DDSD fields.
        let filter = if lpSurfaceDesc != 0 {
            let Ok((desc, _)) = <DDSURFACEDESC>::read_from_prefix(&ctx.memory[lpSurfaceDesc..])
            else {
                return DD::ERR_INVALIDPARAMS;
            };
            if desc.dwSize != std::mem::size_of::<DDSURFACEDESC>() as u32 {
                return DD::ERR_INVALIDPARAMS;
            }
            Some(desc)
        } else {
            None
        };

        // Report the standard display modes; games match these against their
        // internal mode tables by width/height/bit count.
        const RESOLUTIONS: &[(u32, u32)] = &[(640, 480), (800, 600), (1024, 768)];
        // Only depths the surface code can actually convert to rgba.
        const BIT_DEPTHS: &[u32] = &[8, 16, 32];

        for &(width, height) in RESOLUTIONS {
            for &bpp in BIT_DEPTHS {
                if let Some(filter) = &filter {
                    if filter.dwFlags.contains(DDSD::WIDTH) && filter.dwWidth != width {
                        continue;
                    }
                    if filter.dwFlags.contains(DDSD::HEIGHT) && filter.dwHeight != height {
                        continue;
                    }
                    if filter.dwFlags.contains(DDSD::PIXELFORMAT)
                        && filter.ddpfPixelFormat.dwRGBBitCount != bpp
                    {
                        continue;
                    }
                }
                // DDPF_RGB = 0x40, DDPF_PALETTEINDEXED8 = 0x20.
                let (flags, r, g, b) = match bpp {
                    8 => (0x40 | 0x20, 0, 0, 0),
                    16 => (0x40, 0xF800, 0x07E0, 0x001F), // 5-6-5
                    _ => (0x40, 0xFF0000, 0x00FF00, 0x0000FF), // 24/32
                };
                let desc = DDSURFACEDESC {
                    dwSize: std::mem::size_of::<DDSURFACEDESC>() as u32,
                    dwFlags: DDSD::WIDTH | DDSD::HEIGHT | DDSD::PIXELFORMAT | DDSD::PITCH,
                    dwWidth: width,
                    dwHeight: height,
                    lPitch_dwLinearSize: width * bpp.div_ceil(8),
                    ddpfPixelFormat: DDPIXELFORMAT {
                        dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
                        dwFlags: flags,
                        dwFourCC: 0,
                        dwRGBBitCount: bpp,
                        dwRBitMask: r,
                        dwGBitMask: g,
                        dwBBitMask: b,
                        dwRGBAlphaBitMask: 0,
                    },
                    ..Default::default()
                };

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
    pub fn EnumSurfaces(
        ctx: &mut Context,
        _this: u32,
        dwFlags: u32,
        _lpDDSD: u32,
        lpContext: u32,
        lpEnumSurfacesCallback: u32,
    ) -> DD {
        // Only DDENUMSURFACES_DOESEXIST lists live surfaces; MATCH/ALL would
        // enumerate hypothetical surfaces, which the emulated model reports
        // none of.
        const DDENUMSURFACES_DOESEXIST: u32 = 0x1;
        if dwFlags & DDENUMSURFACES_DOESEXIST == 0 || lpEnumSurfacesCallback == 0 {
            return DD::OK;
        }
        let addrs: Vec<u32> = state().surf.borrow().keys().cloned().collect();
        for addr in addrs {
            let desc = {
                let surfaces = state().surf.borrow();
                let Some(surface) = surfaces.get(&addr) else {
                    continue; // released by an earlier callback
                };
                let surface = surface.borrow();
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
            let desc_addr = kernel32::lock()
                .process_heap
                .alloc(&mut ctx.memory, desc.dwSize);
            let Some(buf) = ctx
                .memory
                .bytes
                .get_mut(desc_addr as usize..(desc_addr + desc.dwSize) as usize)
            else {
                kernel32::lock()
                    .process_heap
                    .free(&mut ctx.memory, desc_addr);
                return DD::ERR_INVALIDPARAMS;
            };
            if desc.write_to(buf).is_err() {
                kernel32::lock()
                    .process_heap
                    .free(&mut ctx.memory, desc_addr);
                return DD::ERR_INVALIDPARAMS;
            }
            let callback = ctx.indirect(lpEnumSurfacesCallback);
            ctx.call32_x86(callback, vec![addr, desc_addr, lpContext]);
            let ret = ctx.cpu.regs.eax;
            kernel32::lock()
                .process_heap
                .free(&mut ctx.memory, desc_addr);
            if ret == 0 {
                return DD::OK; // DDENUMRET_CANCEL
            }
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn FlipToGDISurface(ctx: &mut Context, this: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::FlipToGDISurface(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, this: u32, lpDDDriverCaps: u32, lpDDEmulCaps: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::GetCaps(ctx, this, lpDDDriverCaps, lpDDEmulCaps)
    }

    #[win32_derive::dllexport]
    pub fn GetDisplayMode(ctx: &mut Context, this: u32, lpDDSurfaceDesc: u32) -> DD {
        if lpDDSurfaceDesc == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let Ok((desc, _)) = <DDSURFACEDESC>::read_from_prefix(&ctx.memory[lpDDSurfaceDesc..])
        else {
            return DD::ERR_INVALIDPARAMS;
        };
        if desc.dwSize != std::mem::size_of::<DDSURFACEDESC>() as u32 {
            return DD::ERR_INVALIDPARAMS;
        }

        let Some(ddraw) = state().get_ddraw(this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let (width, height) = match &ddraw.window {
            Some(window) => {
                let window = window.borrow();
                (window.width, window.height)
            }
            None => (640, 480),
        };
        let bpp = ddraw.bytes_per_pixel * 8;
        drop(ddraw);

        let (flags, r, g, b) = match bpp {
            8 => (0x40 | 0x20, 0, 0, 0),
            16 => (0x40, 0xF800, 0x07E0, 0x001F),
            _ => (0x40, 0x0000_00FF, 0x0000_FF00, 0x00FF_0000),
        };
        let desc = DDSURFACEDESC {
            dwSize: std::mem::size_of::<DDSURFACEDESC>() as u32,
            dwFlags: DDSD::WIDTH | DDSD::HEIGHT | DDSD::PIXELFORMAT | DDSD::PITCH,
            dwWidth: width,
            dwHeight: height,
            lPitch_dwLinearSize: width * bpp.div_ceil(8),
            ddpfPixelFormat: DDPIXELFORMAT {
                dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
                dwFlags: flags,
                dwFourCC: 0,
                dwRGBBitCount: bpp,
                dwRBitMask: r,
                dwGBitMask: g,
                dwBBitMask: b,
                dwRGBAlphaBitMask: 0,
            },
            ..DDSURFACEDESC::default()
        };
        if crate::Ptr::<DDSURFACEDESC>::new(lpDDSurfaceDesc)
            .write(&mut ctx.memory, desc)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetFourCCCodes(ctx: &mut Context, this: u32, lpNumCodes: u32, lpCodes: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::GetFourCCCodes(ctx, this, lpNumCodes, lpCodes)
    }

    #[win32_derive::dllexport]
    pub fn GetGDISurface(ctx: &mut Context, this: u32, lplpGDIDDSSurface: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::GetGDISurface(ctx, this, lplpGDIDDSSurface)
    }

    #[win32_derive::dllexport]
    pub fn GetMonitorFrequency(ctx: &mut Context, this: u32, lpdwFrequency: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::GetMonitorFrequency(ctx, this, lpdwFrequency)
    }

    #[win32_derive::dllexport]
    pub fn GetScanLine(ctx: &mut Context, this: u32, lpdwScanLine: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::GetScanLine(ctx, this, lpdwScanLine)
    }

    #[win32_derive::dllexport]
    pub fn GetVerticalBlankStatus(ctx: &mut Context, this: u32, lpbIsInVB: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::GetVerticalBlankStatus(ctx, this, lpbIsInVB)
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpGUID: u32) -> DD {
        // Nothing to do: the object is fully constructed when it's created.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn RestoreDisplayMode(ctx: &mut Context, this: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDraw7::RestoreDisplayMode(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn SetCooperativeLevel(_ctx: &mut Context, this: u32, hwnd: HWND, flags: u32) -> DD {
        let Some(mut ddraw) = state().get_ddraw(this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        ddraw.set_cooperative_level(hwnd, flags);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetDisplayMode(ctx: &mut Context, this: u32, width: u32, height: u32, bpp: u32) -> DD {
        let Some(mut ddraw) = state().get_ddraw(this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if let Some(window) = &ddraw.window {
            window.borrow_mut().resize(ctx, width, height);
        }
        if bpp == 0 || !bpp.is_multiple_of(8) {
            return DD::ERR_INVALIDPARAMS;
        }
        ddraw.bytes_per_pixel = bpp / 8;
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn WaitForVerticalBlank(_ctx: &mut Context, _this: u32, _dwFlags: u32, _hEvent: u32) -> DD {
        DD::OK // pretend the vblank already happened
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        log::debug!("ddraw1 object at {addr:#x}, vtable {:#x}", unsafe {
            VTABLE
        });
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

    pub const VTABLE_FUNCS: [runtime::ContFn; 36] = [
        QueryInterface_stdcall,
        AddRef_stdcall,
        Release_stdcall,
        AddAttachedSurface_stdcall,
        AddOverlayDirtyRect_stdcall,
        Blt_stdcall,
        BltBatch_stdcall,
        BltFast_stdcall,
        DeleteAttachedSurface_stdcall,
        EnumAttachedSurfaces_stdcall,
        EnumOverlayZOrders_stdcall,
        Flip_stdcall,
        GetAttachedSurface_stdcall,
        GetBltStatus_stdcall,
        GetCaps_stdcall,
        GetClipper_stdcall,
        GetColorKey_stdcall,
        GetDC_stdcall,
        GetFlipStatus_stdcall,
        GetOverlayPosition_stdcall,
        GetPalette_stdcall,
        GetPixelFormat_stdcall,
        GetSurfaceDesc_stdcall,
        Initialize_stdcall,
        IsLost_stdcall,
        Lock_stdcall,
        ReleaseDC_stdcall,
        Restore_stdcall,
        SetClipper_stdcall,
        SetColorKey_stdcall,
        SetOverlayPosition_stdcall,
        SetPalette_stdcall,
        Unlock_stdcall,
        UpdateOverlay_stdcall,
        UpdateOverlayDisplay_stdcall,
        UpdateOverlayZOrder_stdcall,
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, _this: u32, riid: u32, _ppvObject: u32) -> DD {
        let iid = crate::Ptr::<GUID>::new(riid).read(&ctx.memory);
        log::warn!("IDirectDrawSurface::QueryInterface({iid:?}): not supported");
        DD::E_NOINTERFACE
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
    pub fn AddAttachedSurface(ctx: &mut Context, this: u32, lpDDSAttachedSurface: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::AddAttachedSurface(
            ctx,
            this,
            lpDDSAttachedSurface,
        )
    }

    #[win32_derive::dllexport]
    pub fn AddOverlayDirtyRect(ctx: &mut Context, this: u32, lpRect: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::AddOverlayDirtyRect(ctx, this, lpRect)
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
        crate::ddraw::ddraw::blt(
            ctx,
            this,
            lpDstRect,
            lpDDSrcSurface,
            lpSrcRect,
            dwFlags,
            lpDDBLTFX,
        )
    }

    #[win32_derive::dllexport]
    pub fn BltBatch(
        ctx: &mut Context,
        this: u32,
        lpDDBltBatch: u32,
        dwCount: u32,
        dwFlags: u32,
    ) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::BltBatch(
            ctx,
            this,
            lpDDBltBatch,
            dwCount,
            dwFlags,
        )
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
        crate::ddraw::ddraw::blt_fast(ctx, this, dwX, dwY, lpDDSrcSurface, lpSrcRect, dwTrans)
    }

    #[win32_derive::dllexport]
    pub fn DeleteAttachedSurface(
        ctx: &mut Context,
        this: u32,
        dwFlags: u32,
        lpDDSAttachedSurface: u32,
    ) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::DeleteAttachedSurface(
            ctx,
            this,
            dwFlags,
            lpDDSAttachedSurface,
        )
    }

    #[win32_derive::dllexport]
    pub fn EnumAttachedSurfaces(
        ctx: &mut Context,
        this: u32,
        lpContext: u32,
        lpEnumSurfacesCallback: u32,
    ) -> DD {
        if lpEnumSurfacesCallback == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let attached: Vec<u32> = {
            let surfaces = state().surf.borrow();
            let Some(surface) = surfaces.get(&this) else {
                return DD::ERR_INVALIDPARAMS;
            };
            surface
                .borrow()
                .attachments
                .iter()
                .map(|s| s.borrow().addr)
                .collect()
        };
        for addr in attached {
            let desc = {
                let surfaces = state().surf.borrow();
                let Some(surface) = surfaces.get(&addr) else {
                    continue;
                };
                let surface = surface.borrow();
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
            let desc_addr = kernel32::lock()
                .process_heap
                .alloc(&mut ctx.memory, desc.dwSize);
            let Some(buf) = ctx
                .memory
                .bytes
                .get_mut(desc_addr as usize..(desc_addr + desc.dwSize) as usize)
            else {
                kernel32::lock()
                    .process_heap
                    .free(&mut ctx.memory, desc_addr);
                return DD::ERR_INVALIDPARAMS;
            };
            if desc.write_to(buf).is_err() {
                kernel32::lock()
                    .process_heap
                    .free(&mut ctx.memory, desc_addr);
                return DD::ERR_INVALIDPARAMS;
            }
            let callback = ctx.indirect(lpEnumSurfacesCallback);
            ctx.call32_x86(callback, vec![addr, desc_addr, lpContext]);
            let ret = ctx.cpu.regs.eax;
            kernel32::lock()
                .process_heap
                .free(&mut ctx.memory, desc_addr);
            if ret == 0 {
                return DD::OK; // DDENUMRET_CANCEL
            }
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EnumOverlayZOrders(
        ctx: &mut Context,
        this: u32,
        dwFlags: u32,
        lpContext: u32,
        lpfnCallback: u32,
    ) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::EnumOverlayZOrders(
            ctx,
            this,
            dwFlags,
            lpContext,
            lpfnCallback,
        )
    }

    #[win32_derive::dllexport]
    pub fn Flip(
        ctx: &mut Context,
        this: u32,
        _lpDDSurfaceTargetOverride: u32,
        _dwFlags: u32,
    ) -> DD {
        let result = {
            let surfaces = state().surf.borrow_mut();
            let Some(surface) = surfaces.get(&this) else {
                return DD::ERR_INVALIDPARAMS;
            };
            surface.borrow_mut().flip(&mut ctx.memory)
        };
        // A frame flip is the one thing a game does every frame no matter what
        // it's doing, so it's where we keep the audio mixer fed.
        crate::dsound::pump(ctx);
        result
    }

    #[win32_derive::dllexport]
    pub fn GetAttachedSurface(
        ctx: &mut Context,
        this: u32,
        lpDDSCaps: u32,
        lplpDDAttachedSurface: u32,
    ) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::GetAttachedSurface(
            ctx,
            this,
            lpDDSCaps,
            lplpDDAttachedSurface,
        )
    }

    #[win32_derive::dllexport]
    pub fn GetBltStatus(ctx: &mut Context, this: u32, dwFlags: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::GetBltStatus(ctx, this, dwFlags)
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, this: u32, lpDDSCaps: u32) -> DD {
        if lpDDSCaps == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        ctx.memory
            .write::<u32>(lpDDSCaps, surface.borrow().caps.dwCaps.bits());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetClipper(ctx: &mut Context, this: u32, lplpDDClipper: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::GetClipper(ctx, this, lplpDDClipper)
    }

    #[win32_derive::dllexport]
    pub fn GetColorKey(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
        crate::ddraw::ddraw::get_color_key(ctx, this, dwFlags, lpDDColorKey)
    }

    #[win32_derive::dllexport]
    pub fn SetColorKey(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
        crate::ddraw::ddraw::set_color_key(ctx, this, dwFlags, lpDDColorKey)
    }

    #[win32_derive::dllexport]
    pub fn GetDC(ctx: &mut Context, this: u32, lphDC: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::GetDC(ctx, this, lphDC)
    }

    #[win32_derive::dllexport]
    pub fn GetFlipStatus(ctx: &mut Context, this: u32, dwFlags: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::GetFlipStatus(ctx, this, dwFlags)
    }

    #[win32_derive::dllexport]
    pub fn GetOverlayPosition(ctx: &mut Context, this: u32, lplX: u32, lplY: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::GetOverlayPosition(ctx, this, lplX, lplY)
    }

    #[win32_derive::dllexport]
    pub fn GetPalette(ctx: &mut Context, this: u32, lplpDDPalette: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::GetPalette(ctx, this, lplpDDPalette)
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
            let Some(surface) = surfaces.get(&this) else {
                return DD::ERR_INVALIDPARAMS;
            };
            let surface = surface.borrow();
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
        if crate::Ptr::<DDSURFACEDESC>::new(lpDDSurfaceDesc)
            .write(&mut ctx.memory, desc)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpDD: u32, _lpDDSurfaceDesc: u32) -> DD {
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
        _rect: u32,
        lpDesc: u32,
        _flags: u32,
        _unused: u32,
    ) -> DD {
        let surfaces = state().surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut surface = surface.borrow_mut();
        // A non-null rect locks a subregion; we always hand out the whole
        // surface, matching the ddraw7 Lock below.

        let Some(pixels) = surface.lock(&mut ctx.memory) else {
            return DD::ERR_OUTOFMEMORY;
        };
        let desc = DDSURFACEDESC {
            dwSize: std::mem::size_of::<DDSURFACEDESC>() as u32,
            lPitch_dwLinearSize: surface.width * surface.bytes_per_pixel,
            lpSurface: pixels,
            ..DDSURFACEDESC::default()
        };
        if crate::Ptr::<DDSURFACEDESC>::new(lpDesc)
            .write(&mut ctx.memory, desc)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn ReleaseDC(ctx: &mut Context, this: u32, hDC: HDC) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::ReleaseDC(ctx, this, hDC)
    }

    #[win32_derive::dllexport]
    pub fn Restore(ctx: &mut Context, this: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::Restore(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn SetClipper(ctx: &mut Context, this: u32, lpDDClipper: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::SetClipper(ctx, this, lpDDClipper)
    }

    #[win32_derive::dllexport]
    pub fn SetOverlayPosition(ctx: &mut Context, this: u32, lX: i32, lY: i32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::SetOverlayPosition(ctx, this, lX, lY)
    }

    #[win32_derive::dllexport]
    pub fn SetPalette(_ctx: &mut Context, this: u32, lpPalette: u32) -> DD {
        let state = state();
        let surfaces = state.surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let palettes = state.palette.borrow();
        let Some(palette) = palettes.get(&lpPalette) else {
            return DD::ERR_INVALIDPARAMS;
        };
        surface.borrow_mut().palette = Some(palette.clone());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unlock(ctx: &mut Context, this: u32, _lpRect: u32) -> DD {
        let surfaces = state().surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        // unlock presents window-backed surfaces itself.
        surface.borrow_mut().unlock(&mut ctx.memory);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlay(
        ctx: &mut Context,
        this: u32,
        lpSrcRect: u32,
        lpDDDestSurface: u32,
        lpDestRect: u32,
        dwFlags: u32,
        lpDDOverlayFx: u32,
    ) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::UpdateOverlay(
            ctx,
            this,
            lpSrcRect,
            lpDDDestSurface,
            lpDestRect,
            dwFlags,
            lpDDOverlayFx,
        )
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlayDisplay(ctx: &mut Context, this: u32, dwFlags: u32) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::UpdateOverlayDisplay(ctx, this, dwFlags)
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlayZOrder(
        ctx: &mut Context,
        this: u32,
        dwFlags: u32,
        lpDDSurfaceReference: u32,
    ) -> DD {
        crate::ddraw::ddraw7::IDirectDrawSurface7::UpdateOverlayZOrder(
            ctx,
            this,
            dwFlags,
            lpDDSurfaceReference,
        )
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        log::debug!("ddraw1 object at {addr:#x}, vtable {:#x}", unsafe {
            VTABLE
        });
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

    pub const VTABLE_FUNCS: [runtime::ContFn; 7] = [
        QueryInterface_stdcall,
        AddRef_stdcall,
        Release_stdcall,
        GetCaps_stdcall,
        GetEntries_stdcall,
        Initialize_stdcall,
        SetEntries_stdcall,
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
        let Some(bytes) = ctx.memory.bytes.get(lpEntries as usize..) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let Ok((new_entries, _)) =
            <[PALETTEENTRY]>::ref_from_prefix_with_elems(bytes, dwCount as usize)
        else {
            return DD::ERR_INVALIDPARAMS;
        };
        let new_entries = new_entries.to_vec();
        let palettes = state().palette.borrow_mut();
        let Some(palette) = palettes.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut palette = palette.borrow_mut();
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
        log::debug!("ddraw1 object at {addr:#x}, vtable {:#x}", unsafe {
            VTABLE
        });
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }
}
