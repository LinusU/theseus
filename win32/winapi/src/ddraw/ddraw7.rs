use std::rc::Rc;

use runtime::*;

use crate::{
    Ptr, RECT,
    ddraw::{GUID, state, types::*},
    gdi32,
    gdi32::HDC,
    heap::Heap,
    kernel32,
    user32::HWND,
};

pub const IID_IDirectDraw7: GUID = GUID::new(
    0x15e65ec0,
    0x3b9c,
    0x11d2,
    [0xb9, 0x2f, 0x00, 0x60, 0x97, 0x97, 0xea, 0x5b],
);

pub mod IDirectDraw7 {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 30] = [
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
        "GetAvailableVidMem",
        "GetSurfaceFromDC",
        "RestoreAllSurfaces",
        "TestCooperativeLevel",
        "GetDeviceIdentifier",
        "StartModeTest",
        "EvaluateMode",
    ];

    pub const VTABLE_FUNCS: [runtime::ContFn; 30] = [
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
        GetAvailableVidMem_stdcall,
        GetSurfaceFromDC_stdcall,
        RestoreAllSurfaces_stdcall,
        TestCooperativeLevel_stdcall,
        GetDeviceIdentifier_stdcall,
        StartModeTest_stdcall,
        EvaluateMode_stdcall,
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, _this: u32, riid: u32, ppv: u32) -> DD {
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(iid) = crate::Ptr::<GUID>::new(riid).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if iid == crate::ddraw::GUID::new(0, 0, 0, [0; 8]) || iid == IID_IDirectDraw7 {
            ctx.memory.write::<u32>(ppv, _this);
            // QueryInterface AddRefs the returned interface pointer.
            if let Some(mut ddraw) = state().get_ddraw(_this) {
                ddraw.refs += 1;
            }
            return DD::OK;
        }
        if iid == crate::ddraw::d3d7::IID_IDirect3D7 {
            let mut kernel32 = kernel32::lock();
            let Some(addr) = crate::ddraw::d3d7::IDirect3D7::new(ctx, &mut kernel32.process_heap)
            else {
                return DD::ERR_OUTOFMEMORY;
            };
            drop(kernel32);
            ctx.memory.write::<u32>(ppv, addr);
            return DD::OK;
        }
        ctx.memory.write::<u32>(ppv, 0);
        DD::E_NOINTERFACE
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, this: u32) -> u32 {
        let Some(mut ddraw) = state().get_ddraw(this) else {
            return 0;
        };
        ddraw.refs += 1;
        ddraw.refs
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, this: u32) -> u32 {
        let mut slot = state().ddraw.borrow_mut();
        let Some(ddraw) = slot.as_mut() else {
            return 0;
        };
        if ddraw.addr != this {
            return 0;
        }
        ddraw.refs = ddraw.refs.saturating_sub(1);
        if ddraw.refs > 0 {
            return ddraw.refs;
        }
        // The last release destroys the object, dropping its cooperative-level
        // window binding. Surfaces created through it live in `state().surf`
        // under their own reference counts.
        *slot = None;
        0
    }

    #[win32_derive::dllexport]
    pub fn Compact(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateClipper(
        _ctx: &mut Context,
        _this: u32,
        _flags: u32,
        _lplpClipper: u32,
        _pUnkOuter: u32,
    ) -> DD {
        // Clippers only constrain windowed-mode blits; the emulated display is
        // exclusive-mode, so no clipper object model exists.
        DD::ERR_NODIRECTDRAWHW
    }

    #[win32_derive::dllexport]
    pub fn CreatePalette(
        ctx: &mut Context,
        _this: u32,
        flags: u32,
        lpColorTable: u32,
        lplpPalette: u32,
        _pUnkOuter: u32,
    ) -> DD {
        crate::ddraw::create_palette(ctx, flags, lpColorTable, lplpPalette, |ctx| {
            crate::ddraw::ddraw1::IDirectDrawPalette::new(ctx, &mut kernel32::lock().process_heap)
        })
    }

    #[win32_derive::dllexport]
    pub fn CreateSurface(
        ctx: &mut Context,
        this: u32,
        lpDDSurfaceDesc2: u32,
        lplpDDSurface: u32,
        _pUnkOuter: u32,
    ) -> DD {
        let Some(mut ddraw) = state().get_ddraw(this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let Some(desc) = crate::Ptr::<DDSURFACEDESC2>::new(lpDDSurfaceDesc2).read(&ctx.memory)
        else {
            return DD::ERR_INVALIDPARAMS;
        };
        if desc.dwSize != std::mem::size_of::<DDSURFACEDESC2>() as u32 {
            return DD::ERR_INVALIDPARAMS;
        }

        let mut lock = kernel32::lock();
        let Some(surface) = ddraw.create_surface(&desc, &mut || {
            IDirectDrawSurface7::new(ctx, &mut lock.process_heap)
        }) else {
            return DD::ERR_GENERIC;
        };
        let s = surface.borrow();
        log::debug!(
            "CreateSurface: {:#x} {}x{} {}bpp caps={:#x} flags={:#x} mips={} lpSurface={:#x} pf_flags={:#x} fourcc={:#x} masks={:#x},{:#x},{:#x},{:#x}",
            s.addr,
            s.width,
            s.height,
            s.bytes_per_pixel,
            s.caps.dwCaps.bits(),
            desc.dwFlags.bits(),
            desc.dwMipMapCount_dwRefreshRate_dwSrcVBHandle,
            desc.lpSurface,
            desc.ddpfPixelFormat.dwFlags,
            desc.ddpfPixelFormat.dwFourCC,
            desc.ddpfPixelFormat.dwRBitMask,
            desc.ddpfPixelFormat.dwGBitMask,
            desc.ddpfPixelFormat.dwBBitMask,
            desc.ddpfPixelFormat.dwRGBAlphaBitMask,
        );
        if crate::Ptr::<u32>::new(lplpDDSurface)
            .write(&mut ctx.memory, s.addr)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }

        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DuplicateSurface(
        _ctx: &mut Context,
        _this: u32,
        _lpDDSurface: u32,
        _lplpDupDDSurface: u32,
    ) -> DD {
        // Surfaces own their pixel storage; there is no shared-backing model a
        // duplicate could point at.
        DD::ERR_CANTDUPLICATE
    }

    #[win32_derive::dllexport]
    pub fn EnumDisplayModes(
        ctx: &mut Context,
        _this: u32,
        _flags: u32,
        lpSurfaceDesc2: u32,
        lpContext: u32,
        lpEnumCallback: u32,
    ) -> DD {
        // A null-page callback would dispatch to a missing block and halt.
        if lpEnumCallback < 0x1000 {
            return DD::ERR_INVALIDPARAMS;
        }
        // A filter desc limits enumeration to modes matching its DDSD fields.
        let filter = if lpSurfaceDesc2 != 0 {
            if !crate::ddraw::guest_range(
                ctx,
                lpSurfaceDesc2,
                std::mem::size_of::<DDSURFACEDESC2>() as u32,
            ) {
                return DD::ERR_INVALIDPARAMS;
            }
            let Some(desc) = Ptr::<DDSURFACEDESC2>::new(lpSurfaceDesc2).read(&ctx.memory) else {
                return DD::ERR_INVALIDPARAMS;
            };
            if desc.dwSize != std::mem::size_of::<DDSURFACEDESC2>() as u32 {
                return DD::ERR_INVALIDPARAMS;
            }
            Some(desc)
        } else {
            None
        };
        let filter_matches =
            |desc: &DDSURFACEDESC2, width: u32, height: u32, bpp: u32, refresh: u32| {
                if desc.dwFlags.contains(DDSD::WIDTH) && desc.dwWidth != width {
                    return false;
                }
                if desc.dwFlags.contains(DDSD::HEIGHT) && desc.dwHeight != height {
                    return false;
                }
                if desc.dwFlags.contains(DDSD::PIXELFORMAT)
                    && desc.ddpfPixelFormat.dwRGBBitCount != bpp
                {
                    return false;
                }
                if desc.dwFlags.contains(DDSD::REFRESHRATE)
                    && desc.dwMipMapCount_dwRefreshRate_dwSrcVBHandle != refresh
                {
                    return false;
                }
                true
            };

        const RESOLUTIONS: &[(u32, u32)] = &[(640, 480), (800, 600), (1024, 768)];
        const BIT_DEPTHS: &[u32] = &[8, 16, 32];
        const REFRESH_RATES: &[u32] = &[60, 75, 85];

        for &(width, height) in RESOLUTIONS {
            for &bpp in BIT_DEPTHS {
                for &refresh in REFRESH_RATES {
                    if let Some(filter) = &filter
                        && !filter_matches(filter, width, height, bpp, refresh)
                    {
                        continue;
                    }
                    // Modes must advertise the byte order surfaces actually
                    // store: 32bpp memory is RGBA order (R in the low byte),
                    // not the usual Windows BGRA.
                    let mut ddpfPixelFormat = crate::ddraw::ddraw::surface_pixel_format(bpp / 8);
                    ddpfPixelFormat.dwRGBAlphaBitMask = 0; // modes carry no alpha
                    let desc = DDSURFACEDESC2 {
                        dwSize: std::mem::size_of::<DDSURFACEDESC2>() as u32,
                        dwFlags: DDSD::WIDTH
                            | DDSD::HEIGHT
                            | DDSD::PIXELFORMAT
                            | DDSD::PITCH
                            | DDSD::REFRESHRATE,
                        dwWidth: width,
                        dwHeight: height,
                        lPitch_dwLinearSize: width * bpp.div_ceil(8),
                        dwMipMapCount_dwRefreshRate_dwSrcVBHandle: refresh,
                        ddpfPixelFormat,
                        ..Default::default()
                    };

                    let Some(desc_addr) = kernel32::lock()
                        .process_heap
                        .try_alloc(&mut ctx.memory, desc.dwSize)
                    else {
                        return DD::ERR_OUTOFMEMORY;
                    };
                    ctx.memory.write(desc_addr, desc);
                    let callback = ctx.indirect(lpEnumCallback);
                    ctx.call32_x86(callback, vec![desc_addr, lpContext]);
                    let ret = ctx.cpu.regs.eax;
                    kernel32::lock()
                        .process_heap
                        .free(&mut ctx.memory, desc_addr);

                    if ret == 0 {
                        return DD::OK;
                    }
                }
            }
        }

        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EnumSurfaces(
        ctx: &mut Context,
        _this: u32,
        flags: u32,
        _lpSurfaceDesc2: u32,
        lpContext: u32,
        lpEnumCallback: u32,
    ) -> DD {
        // DDENUMSURFACES match flags describe surfaces that *could* be
        // created; only DOESEXIST reports live ones. The teardown path wants
        // every live surface, so matching is not implemented.
        const DDENUMSURFACES_DOESEXIST: u32 = 0x1;
        // A null-page callback would dispatch to a missing block and halt.
        if flags & DDENUMSURFACES_DOESEXIST == 0 || lpEnumCallback < 0x1000 {
            return DD::OK;
        }
        // Snapshot the list: the callback may release surfaces mid-walk.
        let addrs: Vec<u32> = state().surf.borrow().keys().copied().collect();
        for addr in addrs {
            let desc = {
                let surfaces = state().surf.borrow();
                let Some(surface) = surfaces.get(&addr) else {
                    continue;
                };
                let surface = surface.borrow();
                DDSURFACEDESC2 {
                    dwSize: std::mem::size_of::<DDSURFACEDESC2>() as u32,
                    dwFlags: DDSD::WIDTH | DDSD::HEIGHT | DDSD::PITCH | DDSD::PIXELFORMAT,
                    dwHeight: surface.height,
                    dwWidth: surface.width,
                    lPitch_dwLinearSize: surface.width * surface.bytes_per_pixel,
                    ddpfPixelFormat: surface.pixel_format.clone(),
                    ..Default::default()
                }
            };
            let Some(desc_addr) = kernel32::lock().process_heap.try_alloc(
                &mut ctx.memory,
                std::mem::size_of::<DDSURFACEDESC2>() as u32,
            ) else {
                return DD::ERR_OUTOFMEMORY;
            };
            ctx.memory.write(desc_addr, desc);
            let callback = ctx.indirect(lpEnumCallback);
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
    pub fn FlipToGDISurface(_ctx: &mut Context, _this: u32) -> DD {
        // GDI output already lands on the visible primary in the emulated
        // single-window model, so there is nothing to flip.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, _this: u32, lpDDDriverCaps: u32, lpDDEmulCaps: u32) -> DD {
        let all_caps = 0xFF7FFFFF;
        let all_caps2 = 0xFFFFFFFF;
        let dds = DDSCAPS2 {
            dwCaps: DDSCAPS::from_bits_truncate(all_caps),
            dwCaps2: all_caps2,
            dwCaps3: 0,
            dwCaps4: 0,
        };
        let mut caps = DDCAPS_DX7 {
            dwSize: std::mem::size_of::<DDCAPS_DX7>() as u32,
            dwCaps: all_caps,
            dwCaps2: all_caps2,
            dwCKeyCaps: 0xFFFFFFFF,
            dwFXCaps: 0xFFFFFFFF,
            dwFXAlphaCaps: 0xFFFFFFFF,
            dwPalCaps: 0xFFFFFFFF,
            dwSVCaps: 0xFFFFFFFF,
            dwAlphaBltConstBitDepths: 0xFFFFFFFF,
            dwAlphaBltPixelBitDepths: 0xFFFFFFFF,
            dwAlphaBltSurfaceBitDepths: 0xFFFFFFFF,
            dwAlphaOverlayConstBitDepths: 0xFFFFFFFF,
            dwAlphaOverlayPixelBitDepths: 0xFFFFFFFF,
            dwAlphaOverlaySurfaceBitDepths: 0xFFFFFFFF,
            dwZBufferBitDepths: 0xFFFFFFFF,
            dwVidMemTotal: 256 * 1024 * 1024,
            dwVidMemFree: 256 * 1024 * 1024,
            dwMaxVisibleOverlays: 0,
            dwCurrVisibleOverlays: 0,
            dwNumFourCCCodes: 0,
            dwAlignBoundarySrc: 0,
            dwAlignSizeSrc: 0,
            dwAlignBoundaryDest: 0,
            dwAlignSizeDest: 0,
            dwAlignStrideAlign: 0,
            dwRops: [0xFFFFFFFF; 8],
            ddsOldCaps: DDSCAPS::from_bits_truncate(all_caps),
            dwMinOverlayStretch: 1,
            dwMaxOverlayStretch: 0x7FFFFFFF,
            dwMinLiveVideoStretch: 1,
            dwMaxLiveVideoStretch: 0x7FFFFFFF,
            dwMinHwCodecStretch: 1,
            dwMaxHwCodecStretch: 0x7FFFFFFF,
            dwReserved1: 0,
            dwReserved2: 0,
            dwReserved3: 0,
            dwSVBCaps: all_caps,
            dwSVBCKeyCaps: 0xFFFFFFFF,
            dwSVBFXCaps: 0xFFFFFFFF,
            dwSVBRops: [0xFFFFFFFF; 8],
            dwVSBCaps: all_caps,
            dwVSBCKeyCaps: 0xFFFFFFFF,
            dwVSBFXCaps: 0xFFFFFFFF,
            dwVSBRops: [0xFFFFFFFF; 8],
            dwSSBCaps: all_caps,
            dwSSBCKeyCaps: 0xFFFFFFFF,
            dwSSBFXCaps: 0xFFFFFFFF,
            dwSSBRops: [0xFFFFFFFF; 8],
            dwMaxVideoPorts: 0,
            dwCurrVideoPorts: 0,
            dwSVBCaps2: all_caps2,
            dwNLVBCaps: all_caps,
            dwNLVBCaps2: all_caps2,
            dwNLVBCKeyCaps: 0xFFFFFFFF,
            dwNLVBFXCaps: 0xFFFFFFFF,
            dwNLVBRops: [0xFFFFFFFF; 8],
            ddsCaps: dds,
        };
        let caps_size = std::mem::size_of::<DDCAPS_DX7>() as u32;
        if !crate::ddraw::guest_range(ctx, lpDDDriverCaps, caps_size)
            || (lpDDEmulCaps != 0 && !crate::ddraw::guest_range(ctx, lpDDEmulCaps, caps_size))
        {
            return DD::ERR_INVALIDPARAMS;
        }
        ctx.memory.write(lpDDDriverCaps, caps);
        if lpDDEmulCaps != 0 {
            caps.dwCaps &= !0x00004000; // claim 3D is hardware only
            ctx.memory.write(lpDDEmulCaps, caps);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetDisplayMode(ctx: &mut Context, this: u32, lpDDSurfaceDesc2: u32) -> DD {
        if lpDDSurfaceDesc2 == 0
            || !crate::ddraw::guest_range(
                ctx,
                lpDDSurfaceDesc2,
                std::mem::size_of::<DDSURFACEDESC2>() as u32,
            )
        {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(desc) = Ptr::<DDSURFACEDESC2>::new(lpDDSurfaceDesc2).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if desc.dwSize != std::mem::size_of::<DDSURFACEDESC2>() as u32 {
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
        let desc = DDSURFACEDESC2 {
            dwSize: std::mem::size_of::<DDSURFACEDESC2>() as u32,
            dwFlags: DDSD::WIDTH
                | DDSD::HEIGHT
                | DDSD::PIXELFORMAT
                | DDSD::PITCH
                | DDSD::REFRESHRATE,
            dwWidth: width,
            dwHeight: height,
            lPitch_dwLinearSize: width * bpp.div_ceil(8),
            dwMipMapCount_dwRefreshRate_dwSrcVBHandle: 60,
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
        ctx.memory.write(lpDDSurfaceDesc2, desc);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetFourCCCodes(ctx: &mut Context, _this: u32, lpNumCodes: u32, _lpCodes: u32) -> DD {
        if lpNumCodes == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        // The emulated hardware exposes no FOURCC surface formats.
        if crate::Ptr::<u32>::new(lpNumCodes)
            .write(&mut ctx.memory, 0)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetGDISurface(ctx: &mut Context, _this: u32, lplpGDISurface: u32) -> DD {
        if lplpGDISurface == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        // The surface GDI writes to is the primary, i.e. the window-backed one.
        let surfaces = state().surf.borrow();
        let gdi_surface = surfaces
            .values()
            .find(|s| matches!(s.borrow().target, crate::ddraw::Target::Window(_)))
            .map(|s| s.borrow().addr);
        match gdi_surface {
            Some(addr) => {
                if crate::Ptr::<u32>::new(lplpGDISurface)
                    .write(&mut ctx.memory, addr)
                    .is_none()
                {
                    return DD::ERR_INVALIDPARAMS;
                }
                DD::OK
            }
            None => DD::ERR_NOTFOUND,
        }
    }

    #[win32_derive::dllexport]
    pub fn GetMonitorFrequency(ctx: &mut Context, _this: u32, lpdwFrequency: u32) -> DD {
        if lpdwFrequency == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        // The emulated display refreshes at the same 60 Hz reported by
        // GetDisplayMode.
        if crate::Ptr::<u32>::new(lpdwFrequency)
            .write(&mut ctx.memory, 60)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetScanLine(ctx: &mut Context, _this: u32, lpdwScanLine: u32) -> DD {
        if lpdwScanLine == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        // There is no real raster; report the first line.
        if crate::Ptr::<u32>::new(lpdwScanLine)
            .write(&mut ctx.memory, 0)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetVerticalBlankStatus(ctx: &mut Context, _this: u32, lpbIsInVB: u32) -> DD {
        if lpbIsInVB == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        if crate::Ptr::<u32>::new(lpbIsInVB)
            .write(&mut ctx.memory, 0)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpGUID: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn RestoreDisplayMode(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
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
    pub fn SetDisplayMode(
        ctx: &mut Context,
        this: u32,
        width: u32,
        height: u32,
        bpp: u32,
        _refresh: u32,
        _flags: u32,
    ) -> DD {
        log::debug!("SetDisplayMode: {width}x{height} {bpp}bpp");
        let Some(mut ddraw) = state().get_ddraw(this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if let Some(window) = &ddraw.window {
            window.borrow_mut().resize(ctx, width, height);
        }
        ddraw.bytes_per_pixel = bpp.div_ceil(8);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn WaitForVerticalBlank(_ctx: &mut Context, _this: u32, _flags: u32, _hEvent: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetAvailableVidMem(
        ctx: &mut Context,
        _this: u32,
        _lpDDSCaps2: u32,
        lpdwTotal: u32,
        lpdwFree: u32,
    ) -> DD {
        if lpdwTotal != 0 && !crate::ddraw::guest_range(ctx, lpdwTotal, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        if lpdwFree != 0 && !crate::ddraw::guest_range(ctx, lpdwFree, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        if lpdwTotal != 0 {
            crate::Ptr::<u32>::new(lpdwTotal).write(&mut ctx.memory, 256 * 1024 * 1024);
        }
        if lpdwFree != 0 {
            crate::Ptr::<u32>::new(lpdwFree).write(&mut ctx.memory, 256 * 1024 * 1024);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetSurfaceFromDC(_ctx: &mut Context, _this: u32, _hdc: HDC, _lplpDDSurface: u32) -> DD {
        DD::ERR_GENERIC
    }

    #[win32_derive::dllexport]
    pub fn RestoreAllSurfaces(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn TestCooperativeLevel(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetDeviceIdentifier(
        ctx: &mut Context,
        _this: u32,
        lpDDDeviceIdentifier: u32,
        _flags: u32,
    ) -> DD {
        if !crate::ddraw::guest_range(
            ctx,
            lpDDDeviceIdentifier,
            std::mem::size_of::<DDDEVICEIDENTIFIER2>() as u32,
        ) {
            return DD::ERR_INVALIDPARAMS;
        }
        let mut sz_driver = [0u8; 512];
        let driver = b"nv4disp.dll";
        sz_driver[..driver.len()].copy_from_slice(driver);

        let mut sz_description = [0u8; 512];
        let description = b"NVIDIA GeForce2 GTS";
        sz_description[..description.len()].copy_from_slice(description);

        let info = DDDEVICEIDENTIFIER2 {
            szDriver: sz_driver,
            szDescription: sz_description,
            liDriverVersion: 0x0000000100000000,
            dwVendorId: 0x10de,
            dwDeviceId: 0x0151,
            dwSubSysId: 0,
            dwRevision: 0,
            guidDeviceIdentifier: GUID::new(0, 0, 0, [0; 8]),
            dwWHQLLevel: 1,
            dwReserved1: 0,
            dwReserved2: 0,
            dwReserved3: 0,
            dwReserved4: 0,
        };
        ctx.memory.write(lpDDDeviceIdentifier, info);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn StartModeTest(
        _ctx: &mut Context,
        _this: u32,
        _lpModesToTest: u32,
        _numEntries: u32,
        _flags: u32,
    ) -> DD {
        // Refresh-rate testing is not meaningful on the emulated display;
        // report that no test could be initiated.
        DD::ERR_TESTFINISHED
    }

    #[win32_derive::dllexport]
    pub fn EvaluateMode(
        _ctx: &mut Context,
        _this: u32,
        _flags: u32,
        _pSecondsUntilTimeout: u32,
    ) -> DD {
        // No StartModeTest sequence is ever in progress.
        DD::ERR_TESTFINISHED
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> Option<u32> {
        if unsafe { VTABLE } == 0 {
            return None;
        }
        let addr = heap.try_alloc(&mut ctx.memory, 4)?;
        ctx.memory.write(addr, unsafe { VTABLE });
        Some(addr)
    }
}

const IID_IDIRECTDRAWSURFACE7: GUID = GUID::new(
    0x06675a80,
    0x3b9b,
    0x11d2,
    [0xb9, 0x2f, 0x00, 0x60, 0x97, 0x97, 0xea, 0x5b],
);

/// Reachability over the attachment graph, given a child-lookup callback.
/// A `true` result means `to` is already reachable from `from`, so adding
/// a `this -> from` edge would close a cycle.
fn attachment_reaches(children: &dyn Fn(u32) -> Vec<u32>, from: u32, to: u32) -> bool {
    let mut seen = std::collections::HashSet::new();
    let mut stack = vec![from];
    while let Some(a) = stack.pop() {
        if a == to {
            return true;
        }
        if seen.insert(a) {
            stack.extend(children(a));
        }
    }
    false
}

pub mod IDirectDrawSurface7 {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 49] = [
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
        "GetDDInterface",
        "PageLock",
        "PageUnlock",
        "SetSurfaceDesc",
        "SetPrivateData",
        "GetPrivateData",
        "FreePrivateData",
        "GetUniquenessValue",
        "ChangeUniquenessValue",
        "SetPriority",
        "GetPriority",
        "SetLOD",
        "GetLOD",
    ];

    pub const VTABLE_FUNCS: [runtime::ContFn; 49] = [
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
        GetDDInterface_stdcall,
        PageLock_stdcall,
        PageUnlock_stdcall,
        SetSurfaceDesc_stdcall,
        SetPrivateData_stdcall,
        GetPrivateData_stdcall,
        FreePrivateData_stdcall,
        GetUniquenessValue_stdcall,
        ChangeUniquenessValue_stdcall,
        SetPriority_stdcall,
        GetPriority_stdcall,
        SetLOD_stdcall,
        GetLOD_stdcall,
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppv: u32) -> DD {
        if !crate::ddraw::guest_range(ctx, ppv, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(iid) = crate::Ptr::<GUID>::new(riid).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if iid == crate::ddraw::GUID::new(0, 0, 0, [0; 8]) || iid == IID_IDIRECTDRAWSURFACE7 {
            ctx.memory.write::<u32>(ppv, this);
            return DD::OK;
        }
        ctx.memory.write::<u32>(ppv, 0);
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
    pub fn AddAttachedSurface(_ctx: &mut Context, this: u32, lpDDSAttachedSurface: u32) -> DD {
        if this == lpDDSAttachedSurface {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow_mut();
        let (Some(attached), Some(surface)) = (
            surfaces.get(&lpDDSAttachedSurface).cloned(),
            surfaces.get(&this),
        ) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let attached_addr = attached.borrow().addr;
        // Attachment graphs are trees: an edge that lets `attached` already
        // reach `this` would close a cycle and make chain walks like
        // `IDirect3DDevice7::Load`'s mip cascade loop forever.
        if attachment_reaches(
            &|addr| {
                surfaces
                    .get(&addr)
                    .map(|s| {
                        s.borrow()
                            .attachments
                            .iter()
                            .map(|c| c.borrow().addr)
                            .collect()
                    })
                    .unwrap_or_default()
            },
            attached_addr,
            this,
        ) {
            return DD::ERR_INVALIDPARAMS;
        }
        let mut surface = surface.borrow_mut();
        if surface
            .attachments
            .iter()
            .any(|s| s.borrow().addr == attached_addr)
        {
            return DD::ERR_GENERIC; // already attached
        }
        // `attached` remains the flip-chain link; the full set lives in
        // `attachments` so z-buffers don't clobber the back buffer.
        if surface.attached.is_none() {
            surface.attached = Some(attached.clone());
        }
        surface.attachments.push(attached);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn AddOverlayDirtyRect(_ctx: &mut Context, _this: u32, _lpRect: u32) -> DD {
        // No emulated surface is an overlay.
        DD::ERR_NOTAOVERLAYSURFACE
    }

    #[win32_derive::dllexport]
    pub fn Blt(
        ctx: &mut Context,
        this: u32,
        lpDestRect: u32,
        lpDDSrcSurface: u32,
        lpSrcRect: u32,
        dwFlags: u32,
        lpDDBltFx: u32,
    ) -> DD {
        crate::ddraw::ddraw::blt(
            ctx,
            this,
            lpDestRect,
            lpDDSrcSurface,
            lpSrcRect,
            dwFlags,
            lpDDBltFx,
        )
    }

    #[win32_derive::dllexport]
    pub fn BltBatch(
        _ctx: &mut Context,
        _this: u32,
        _lpDDBltBatch: u32,
        _dwCount: u32,
        _dwFlags: u32,
    ) -> DD {
        // BltBatch is documented as unimplemented; its blits already happened
        // synchronously through Blt, so acknowledge it.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn BltFast(
        ctx: &mut Context,
        this: u32,
        dwX: u32,
        dwY: u32,
        lpDDSrcSurface: u32,
        lpSrcRect: Ptr<RECT>,
        dwTrans: u32,
    ) -> DD {
        let result = crate::ddraw::ddraw::blt_fast(
            ctx,
            this,
            dwX,
            dwY,
            lpDDSrcSurface,
            lpSrcRect.addr,
            dwTrans,
        );
        // Apps on this interface draw to an offscreen surface and expect to see
        // the result without flipping, so refresh its texture here.
        let surfaces = state().surf.borrow();
        if let Some(surface) = surfaces.get(&this) {
            surface.borrow_mut().unlock(&mut ctx.memory);
        }
        result
    }

    #[win32_derive::dllexport]
    pub fn DeleteAttachedSurface(
        _ctx: &mut Context,
        this: u32,
        _dwFlags: u32,
        lpDDSAttachedSurface: u32,
    ) -> DD {
        let surfaces = state().surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut surface = surface.borrow_mut();
        let Some(pos) = surface
            .attachments
            .iter()
            .position(|s| s.borrow().addr == lpDDSAttachedSurface)
        else {
            return DD::ERR_GENERIC; // not attached
        };
        surface.attachments.remove(pos);
        if surface
            .attached
            .as_ref()
            .is_some_and(|s| s.borrow().addr == lpDDSAttachedSurface)
        {
            surface.attached = surface.attachments.first().cloned();
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EnumAttachedSurfaces(
        ctx: &mut Context,
        this: u32,
        lpContext: u32,
        lpEnumSurfacesCallback: u32,
    ) -> DD {
        // A null-page callback would dispatch to a missing block and halt.
        if lpEnumSurfacesCallback < 0x1000 {
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
                DDSURFACEDESC2 {
                    dwSize: std::mem::size_of::<DDSURFACEDESC2>() as u32,
                    dwFlags: DDSD::WIDTH | DDSD::HEIGHT | DDSD::PITCH | DDSD::PIXELFORMAT,
                    dwHeight: surface.height,
                    dwWidth: surface.width,
                    lPitch_dwLinearSize: surface.width * surface.bytes_per_pixel,
                    ddpfPixelFormat: surface.pixel_format.clone(),
                    ..Default::default()
                }
            };
            let Some(desc_addr) = kernel32::lock().process_heap.try_alloc(
                &mut ctx.memory,
                std::mem::size_of::<DDSURFACEDESC2>() as u32,
            ) else {
                return DD::ERR_OUTOFMEMORY;
            };
            ctx.memory.write(desc_addr, desc);
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
        _ctx: &mut Context,
        _this: u32,
        _dwFlags: u32,
        _lpContext: u32,
        _lpfnCallback: u32,
    ) -> DD {
        // There are no overlays in the emulated display, so the callback is
        // never invoked.
        DD::OK
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
        _lpDDSCaps: u32,
        lplpDDAttachedSurface: u32,
    ) -> DD {
        if !crate::ddraw::guest_range(ctx, lplpDDAttachedSurface, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let surface = surface.borrow();
        let Some(attached) = surface
            .attached
            .as_ref()
            .or_else(|| surface.attachments.first())
        else {
            log::debug!("GetAttachedSurface {this:#x}: nothing attached");
            return DD::ERR_GENERIC; // nothing attached
        };
        log::debug!(
            "GetAttachedSurface {this:#x} -> {:#x}",
            attached.borrow().addr
        );
        ctx.memory
            .write(lplpDDAttachedSurface, attached.borrow().addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetBltStatus(_ctx: &mut Context, _this: u32, dwFlags: u32) -> DD {
        // DDGBS_CANBLT | DDGBS_ISBLTDONE.
        if dwFlags & !0x3 != 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        // Every blit runs synchronously, so the surface can always blit and no
        // blit is ever still drawing.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, this: u32, lpDDSCaps: u32) -> DD {
        if !crate::ddraw::guest_range(ctx, lpDDSCaps, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        ctx.memory.write(lpDDSCaps, surface.borrow().caps);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetClipper(ctx: &mut Context, this: u32, lplpDDClipper: u32) -> DD {
        if !crate::ddraw::guest_range(ctx, lplpDDClipper, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        match surface.borrow().clipper {
            Some(clipper) => {
                ctx.memory.write::<u32>(lplpDDClipper, clipper);
                DD::OK
            }
            None => {
                ctx.memory.write::<u32>(lplpDDClipper, 0);
                DD::ERR_NOCLIPPERATTACHED
            }
        }
    }

    #[win32_derive::dllexport]
    pub fn GetColorKey(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
        crate::ddraw::ddraw::get_color_key(ctx, this, dwFlags, lpDDColorKey)
    }

    #[win32_derive::dllexport]
    pub fn GetDC(ctx: &mut Context, this: u32, lphDC: u32) -> DD {
        let surfaces = state().surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if !crate::ddraw::guest_range(ctx, lphDC, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        let mut surface = surface.borrow_mut();
        let (width, height, bpp) = (surface.width, surface.height, surface.bytes_per_pixel);
        let bitmap = if bpp == 4 {
            let Some(pixels) = surface.lock(&mut ctx.memory) else {
                return DD::ERR_OUTOFMEMORY;
            };
            gdi32::Bitmap::new_simple(width, height, pixels)
        } else {
            // GDI draws 32-bit, so a DC over a narrower surface gets a scratch
            // RGBA buffer that ReleaseDC converts back.
            let rgba = surface
                .to_rgba(&ctx.memory, &surface.palette)
                .map(|px| px.into_owned());
            // Surface dimensions are bounded at creation, so only heap
            // exhaustion can fail this allocation.
            let Some(scratch) = kernel32::lock()
                .process_heap
                .try_alloc(&mut ctx.memory, width * height * 4)
            else {
                return DD::ERR_OUTOFMEMORY;
            };
            if let Some(rgba) = rgba
                && let Some(buf) = ctx
                    .memory
                    .bytes
                    .get_mut(scratch as usize..(scratch + rgba.len() as u32) as usize)
            {
                buf.copy_from_slice(&rgba);
            }
            gdi32::Bitmap::new_simple(width, height, scratch)
        };
        let scratch = (bpp != 4).then_some(bitmap.pixels);
        let dc = gdi32::lock().new_memory_dc(bitmap);
        if let Some(scratch) = scratch {
            state()
                .surface_dcs
                .borrow_mut()
                .insert(dc.to_raw(), scratch);
        }
        if crate::Ptr::<u32>::new(lphDC)
            .write(&mut ctx.memory, dc.to_raw())
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetFlipStatus(_ctx: &mut Context, _this: u32, dwFlags: u32) -> DD {
        // DDGFS_CANFLIP | DDGFS_ISFLIPDONE.
        if dwFlags & !0x3 != 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        // Flips are synchronous, so the previous flip is always done.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetOverlayPosition(_ctx: &mut Context, _this: u32, _lplX: u32, _lplY: u32) -> DD {
        DD::ERR_NOTAOVERLAYSURFACE
    }

    #[win32_derive::dllexport]
    pub fn GetPalette(ctx: &mut Context, this: u32, lplpDDPalette: u32) -> DD {
        if !crate::ddraw::guest_range(ctx, lplpDDPalette, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let palette = surface.borrow().palette.clone();
        drop(surfaces);
        let Some(palette) = palette else {
            ctx.memory.write::<u32>(lplpDDPalette, 0);
            return DD::ERR_GENERIC; // DDERR_NOPALETTEATTACHED
        };
        let addr = state()
            .palette
            .borrow()
            .iter()
            .find_map(|(&addr, p)| Rc::ptr_eq(p, &palette).then_some(addr))
            .unwrap_or(0);
        ctx.memory.write::<u32>(lplpDDPalette, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetPixelFormat(ctx: &mut Context, this: u32, lpDDPixelFormat: u32) -> DD {
        if !crate::ddraw::guest_range(
            ctx,
            lpDDPixelFormat,
            std::mem::size_of::<DDPIXELFORMAT>() as u32,
        ) {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        // The surface records the declared format at creation (or a
        // byte-depth fallback), so z-buffers and 1555/4444 textures report
        // what they actually are rather than a synthesized RGB format.
        let pixel_format = surface.borrow().pixel_format.clone();
        ctx.memory.write(lpDDPixelFormat, pixel_format);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetSurfaceDesc(ctx: &mut Context, this: u32, lpDDSurfaceDesc2: u32) -> DD {
        if !crate::ddraw::guest_range(
            ctx,
            lpDDSurfaceDesc2,
            std::mem::size_of::<DDSURFACEDESC2>() as u32,
        ) {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let surface = surface.borrow();
        let size = ctx.memory.read::<u32>(lpDDSurfaceDesc2);
        if size != std::mem::size_of::<DDSURFACEDESC2>() as u32 {
            return DD::ERR_INVALIDPARAMS;
        }
        ctx.memory.write(
            lpDDSurfaceDesc2,
            DDSURFACEDESC2 {
                dwSize: std::mem::size_of::<DDSURFACEDESC2>() as u32,
                dwFlags: DDSD::WIDTH | DDSD::HEIGHT,
                dwWidth: surface.width,
                dwHeight: surface.height,
                ..Default::default()
            },
        );

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
        lpDestRect: u32,
        lpDDSurfaceDesc2: u32,
        _dwFlags: u32,
        _hEvent: u32,
    ) -> DD {
        if !crate::ddraw::guest_range(
            ctx,
            lpDDSurfaceDesc2,
            std::mem::size_of::<DDSURFACEDESC2>() as u32,
        ) {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut surface = surface.borrow_mut();
        let Some(pixels) = surface.lock(&mut ctx.memory) else {
            return DD::ERR_OUTOFMEMORY;
        };
        // A non-null rect locks a subregion: lpSurface is that region's
        // first pixel, while lPitch still spans whole surface rows.
        let Some(pixels) = crate::ddraw::ddraw::lock_offset(
            ctx,
            lpDestRect,
            surface.width,
            surface.height,
            surface.bytes_per_pixel,
            pixels,
        ) else {
            return DD::ERR_INVALIDPARAMS;
        };
        log::debug!(
            "Lock {this:#x} {}x{} {}bpp caps={:#x} -> {pixels:#x}",
            surface.width,
            surface.height,
            surface.bytes_per_pixel,
            surface.caps.dwCaps.bits(),
        );
        ctx.memory.write(
            lpDDSurfaceDesc2,
            DDSURFACEDESC2 {
                dwSize: std::mem::size_of::<DDSURFACEDESC2>() as u32,
                dwFlags: DDSD::WIDTH
                    | DDSD::HEIGHT
                    | DDSD::PITCH
                    | DDSD::PIXELFORMAT
                    | DDSD::LPSURFACE,
                dwWidth: surface.width,
                dwHeight: surface.height,
                lPitch_dwLinearSize: surface.width * surface.bytes_per_pixel,
                lpSurface: pixels,
                ddpfPixelFormat: surface.pixel_format.clone(),
                ..Default::default()
            },
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn ReleaseDC(ctx: &mut Context, this: u32, hDC: HDC) -> DD {
        let scratch = state().surface_dcs.borrow_mut().remove(&hDC.to_raw());
        let surfaces = state().surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut surface = surface.borrow_mut();
        gdi32::lock().release_dc(&mut ctx.memory, hDC);
        if let Some(scratch) = scratch {
            // GetDC gave the game a 32-bit scratch buffer; convert it back to
            // the surface's depth now.
            let (width, height) = (surface.width, surface.height);
            let len = (width as usize)
                .checked_mul(height as usize)
                .and_then(|n| n.checked_mul(4));
            let start = scratch as usize;
            let end = len.and_then(|len| start.checked_add(len));
            let rgba = end
                .and_then(|end| ctx.memory.bytes.get(start..end))
                .map(|b| b.to_vec())
                .unwrap_or_default();
            kernel32::lock().process_heap.free(&mut ctx.memory, scratch);
            if let Some(dst) = surface.lock(&mut ctx.memory) {
                surface.write_rgba(&mut ctx.memory, &rgba, dst);
            }
        }
        surface.unlock(&mut ctx.memory);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Restore(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetClipper(_ctx: &mut Context, this: u32, lpDDClipper: u32) -> DD {
        let surfaces = state().surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        // The clipper pointer is opaque to us — CreateClipper never succeeds,
        // so the only value a caller can legitimately pass is zero to detach.
        surface.borrow_mut().clipper = (lpDDClipper != 0).then_some(lpDDClipper);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetColorKey(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
        crate::ddraw::ddraw::set_color_key(ctx, this, dwFlags, lpDDColorKey)
    }

    #[win32_derive::dllexport]
    pub fn SetOverlayPosition(_ctx: &mut Context, _this: u32, _lX: i32, _lY: i32) -> DD {
        DD::ERR_NOTAOVERLAYSURFACE
    }

    #[win32_derive::dllexport]
    pub fn SetPalette(_ctx: &mut Context, this: u32, lpDDPalette: u32) -> DD {
        let state = state();
        let palettes = state.palette.borrow();
        let Some(palette) = palettes.get(&lpDDPalette) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let surfaces = state.surf.borrow_mut();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        surface.borrow_mut().palette = Some(palette.clone());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unlock(ctx: &mut Context, this: u32, _lpSurfaceData: u32) -> DD {
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
        _ctx: &mut Context,
        _this: u32,
        _lpSrcRect: u32,
        _lpDDDestSurface: u32,
        _lpDestRect: u32,
        _dwFlags: u32,
        _lpDDOverlayFx: u32,
    ) -> DD {
        DD::ERR_NOTAOVERLAYSURFACE
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlayDisplay(_ctx: &mut Context, _this: u32, _dwFlags: u32) -> DD {
        DD::ERR_NOTAOVERLAYSURFACE
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlayZOrder(
        _ctx: &mut Context,
        _this: u32,
        _dwFlags: u32,
        _lpDDSurfaceReference: u32,
    ) -> DD {
        DD::ERR_NOTAOVERLAYSURFACE
    }

    #[win32_derive::dllexport]
    pub fn GetDDInterface(ctx: &mut Context, _this: u32, lplpDD: u32) -> DD {
        if lplpDD == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        // There is a single DirectDraw object for the process.
        let addr = state().ddraw.borrow().as_ref().map(|d| d.addr);
        match addr {
            Some(addr) => {
                ctx.memory.write::<u32>(lplpDD, addr);
                DD::OK
            }
            None => DD::ERR_NOTFOUND,
        }
    }

    #[win32_derive::dllexport]
    pub fn PageLock(_ctx: &mut Context, _this: u32, _dwFlags: u32) -> DD {
        // Emulated surface memory never pages, so locking it is a no-op.
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn PageUnlock(_ctx: &mut Context, _this: u32, _dwFlags: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetSurfaceDesc(
        _ctx: &mut Context,
        _this: u32,
        _lpDDSurfaceDesc2: u32,
        _dwFlags: u32,
    ) -> DD {
        // SetSurfaceDesc exists so a caller can hand a surface user-allocated
        // backing memory; emulated surfaces own their buffers.
        DD::ERR_INVALIDSURFACETYPE
    }

    #[win32_derive::dllexport]
    pub fn SetPrivateData(
        ctx: &mut Context,
        this: u32,
        guidTag: u32,
        lpData: u32,
        cbSize: u32,
        _dwFlags: u32,
    ) -> DD {
        if guidTag == 0
            || lpData == 0
            || cbSize == 0
            || !crate::ddraw::guest_range(ctx, guidTag, std::mem::size_of::<GUID>() as u32)
            || !crate::ddraw::guest_range(ctx, lpData, cbSize)
        {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(tag) = Ptr::<GUID>::new(guidTag).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let Some(end) = (lpData as usize).checked_add(cbSize as usize) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let Some(data) = ctx
            .memory
            .bytes
            .get(lpData as usize..end)
            .map(|b| b.to_vec())
        else {
            return DD::ERR_INVALIDPARAMS;
        };
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        surface.borrow_mut().private_data.insert(tag, data);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetPrivateData(
        ctx: &mut Context,
        this: u32,
        guidTag: u32,
        lpBuffer: u32,
        lpcbBufferSize: u32,
    ) -> DD {
        if guidTag == 0
            || lpcbBufferSize == 0
            || !crate::ddraw::guest_range(ctx, guidTag, std::mem::size_of::<GUID>() as u32)
        {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(tag) = Ptr::<GUID>::new(guidTag).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let Some(data) = surface.borrow().private_data.get(&tag).cloned() else {
            return DD::ERR_NOTFOUND;
        };
        let Some(size) = Ptr::<u32>::new(lpcbBufferSize).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if size < data.len() as u32 {
            // Report the needed size, as the API contract requires.
            if Ptr::<u32>::new(lpcbBufferSize)
                .write(&mut ctx.memory, data.len() as u32)
                .is_none()
            {
                return DD::ERR_INVALIDPARAMS;
            }
            return DD::ERR_MOREDATA;
        }
        if !crate::ddraw::guest_range(ctx, lpBuffer, data.len() as u32) {
            return DD::ERR_INVALIDPARAMS;
        }
        if let Some(dst) = ctx
            .memory
            .bytes
            .get_mut(lpBuffer as usize..)
            .and_then(|b| b.get_mut(..data.len()))
        {
            dst.copy_from_slice(&data);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn FreePrivateData(ctx: &mut Context, this: u32, guidTag: u32) -> DD {
        if guidTag == 0
            || !crate::ddraw::guest_range(ctx, guidTag, std::mem::size_of::<GUID>() as u32)
        {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(tag) = Ptr::<GUID>::new(guidTag).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if surface.borrow_mut().private_data.remove(&tag).is_none() {
            return DD::ERR_NOTFOUND;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetUniquenessValue(ctx: &mut Context, this: u32, lpValue: u32) -> DD {
        if lpValue == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let uniqueness = surface.borrow().uniqueness;
        if crate::Ptr::<u32>::new(lpValue)
            .write(&mut ctx.memory, uniqueness)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn ChangeUniquenessValue(_ctx: &mut Context, this: u32) -> DD {
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        surface.borrow_mut().uniqueness += 1;
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetPriority(_ctx: &mut Context, this: u32, dwPriority: u32) -> DD {
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        surface.borrow_mut().priority = dwPriority;
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetPriority(ctx: &mut Context, this: u32, lpdwPriority: u32) -> DD {
        if lpdwPriority == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let priority = surface.borrow().priority;
        if crate::Ptr::<u32>::new(lpdwPriority)
            .write(&mut ctx.memory, priority)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetLOD(_ctx: &mut Context, this: u32, dwMaxLOD: u32) -> DD {
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        surface.borrow_mut().max_lod = dwMaxLOD;
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetLOD(ctx: &mut Context, this: u32, lpdwMaxLOD: u32) -> DD {
        if lpdwMaxLOD == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let surfaces = state().surf.borrow();
        let Some(surface) = surfaces.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let max_lod = surface.borrow().max_lod;
        if crate::Ptr::<u32>::new(lpdwMaxLOD)
            .write(&mut ctx.memory, max_lod)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    pub static mut VTABLE: u32 = 0;
    pub fn new(ctx: &mut Context, heap: &mut Heap) -> Option<u32> {
        if unsafe { VTABLE } == 0 {
            return None;
        }
        let addr = heap.try_alloc(&mut ctx.memory, 4)?;
        ctx.memory.write(addr, unsafe { VTABLE });
        Some(addr)
    }
}

#[cfg(test)]
mod tests {
    use super::attachment_reaches;

    #[test]
    fn attachment_reaches_detects_would_be_cycles() {
        // Chain 1 -> 2 -> 3: attaching 3 back to 1 or 2 closes a cycle.
        let chain = |a: u32| match a {
            1 => vec![2],
            2 => vec![3],
            _ => vec![],
        };
        assert!(attachment_reaches(&chain, 1, 3));
        assert!(attachment_reaches(&chain, 1, 1));
        assert!(!attachment_reaches(&chain, 3, 1));
        assert!(!attachment_reaches(&chain, 1, 4));
    }

    #[test]
    fn attachment_reaches_terminates_on_a_preexisting_cycle() {
        // A graph that is already cyclic must not spin the walk.
        let cyclic = |a: u32| if a == 2 { vec![2] } else { vec![1] };
        assert!(attachment_reaches(&cyclic, 2, 2));
        assert!(!attachment_reaches(&cyclic, 2, 4));
    }
}
