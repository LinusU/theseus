//! The DirectX 3 era interfaces, IDirectDraw2 and IDirectDrawSurface2.
//!
//! They extend IDirectDraw/IDirectDrawSurface with a few methods and change
//! SetDisplayMode's arguments; everything else delegates to the DirectX 1
//! implementation in ddraw1.rs, which owns the state. A program gets one of
//! these by calling QueryInterface on the older interface, which hands out a
//! second pointer (with this vtable) to the same object.
//!
//! The delegating functions here are generated from ddraw1.rs's signatures;
//! keep them in sync if those change.

use runtime::Context;

use crate::{
    ddraw::{DD, GUID, ddraw1::*, state, types::*},
    heap::Heap,
    user32::HWND,
};

pub const IID_IDirectDraw2: GUID = GUID((
    0xb3a6f3e0,
    0x2b43,
    0x11cf,
    [0xa2, 0xde, 0x00, 0xaa, 0x00, 0xb9, 0x33, 0x56],
));
pub const IID_IDirectDrawSurface2: GUID = GUID((
    0x57805885,
    0x6eec,
    0x11cf,
    [0x94, 0x41, 0xa8, 0x23, 0x03, 0xc1, 0x0e, 0x27],
));

pub mod IDirectDraw2 {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 24] = [
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
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        IDirectDraw::QueryInterface(ctx, this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(ctx: &mut Context, this: u32) -> u32 {
        IDirectDraw::AddRef(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn Release(ctx: &mut Context, this: u32) -> u32 {
        IDirectDraw::Release(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn Compact(ctx: &mut Context, this: u32) -> DD {
        IDirectDraw::Compact(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn CreateClipper(ctx: &mut Context, this: u32, dwFlags: u32, lplpDDClipper: u32, pUnkOuter: u32) -> DD {
        IDirectDraw::CreateClipper(ctx, this, dwFlags, lplpDDClipper, pUnkOuter)
    }

    #[win32_derive::dllexport]
    pub fn CreatePalette(
        ctx: &mut Context,
        this: u32,
        flags: DDPCAPS,
        lpEntries: u32,
        lplpPal: u32,
        pUnkOuter: u32,
    ) -> DD {
        IDirectDraw::CreatePalette(ctx, this, flags, lpEntries, lplpPal, pUnkOuter)
    }

    #[win32_derive::dllexport]
    pub fn CreateSurface(
        ctx: &mut Context,
        this: u32,
        desc: u32,
        lplpDDSurface: u32,
        pUnkOuter: u32,
    ) -> DD {
        IDirectDraw::CreateSurface(ctx, this, desc, lplpDDSurface, pUnkOuter)
    }

    #[win32_derive::dllexport]
    pub fn DuplicateSurface(ctx: &mut Context, this: u32, lpDDSurface: u32, lplpDDDuplicateSurface: u32) -> DD {
        IDirectDraw::DuplicateSurface(ctx, this, lpDDSurface, lplpDDDuplicateSurface)
    }

    #[win32_derive::dllexport]
    pub fn EnumDisplayModes(
        ctx: &mut Context,
        this: u32,
        dwFlags: u32,
        lpSurfaceDesc: u32,
        lpContext: u32,
        lpEnumCallback: u32,
    ) -> DD {
        IDirectDraw::EnumDisplayModes(ctx, this, dwFlags, lpSurfaceDesc, lpContext, lpEnumCallback)
    }

    #[win32_derive::dllexport]
    pub fn EnumSurfaces(
        ctx: &mut Context,
        this: u32,
        dwFlags: u32,
        lpDDSD2: u32,
        lpContext: u32,
        lpEnumCallback: u32,
    ) -> DD {
        IDirectDraw::EnumSurfaces(ctx, this, dwFlags, lpDDSD2, lpContext, lpEnumCallback)
    }

    #[win32_derive::dllexport]
    pub fn FlipToGDISurface(ctx: &mut Context, this: u32) -> DD {
        IDirectDraw::FlipToGDISurface(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, this: u32, lpDDDriverCaps: u32, lpDDHELCaps: u32) -> DD {
        IDirectDraw::GetCaps(ctx, this, lpDDDriverCaps, lpDDHELCaps)
    }

    #[win32_derive::dllexport]
    pub fn GetDisplayMode(ctx: &mut Context, this: u32, lpDDSurfaceDesc: u32) -> DD {
        IDirectDraw::GetDisplayMode(ctx, this, lpDDSurfaceDesc)
    }

    #[win32_derive::dllexport]
    pub fn GetFourCCCodes(ctx: &mut Context, this: u32, lpNumCodes: u32, lpCodes: u32) -> DD {
        IDirectDraw::GetFourCCCodes(ctx, this, lpNumCodes, lpCodes)
    }

    #[win32_derive::dllexport]
    pub fn GetGDISurface(ctx: &mut Context, this: u32, lplpGDIDDSSurface: u32) -> DD {
        IDirectDraw::GetGDISurface(ctx, this, lplpGDIDDSSurface)
    }

    #[win32_derive::dllexport]
    pub fn GetMonitorFrequency(ctx: &mut Context, this: u32, lpdwFrequency: u32) -> DD {
        IDirectDraw::GetMonitorFrequency(ctx, this, lpdwFrequency)
    }

    #[win32_derive::dllexport]
    pub fn GetScanLine(ctx: &mut Context, this: u32, lpdwScanLine: u32) -> DD {
        IDirectDraw::GetScanLine(ctx, this, lpdwScanLine)
    }

    #[win32_derive::dllexport]
    pub fn GetVerticalBlankStatus(ctx: &mut Context, this: u32, lpbIsInVB: u32) -> DD {
        IDirectDraw::GetVerticalBlankStatus(ctx, this, lpbIsInVB)
    }

    #[win32_derive::dllexport]
    pub fn Initialize(
        ctx: &mut Context,
        this: u32,
        lpDD: u32,
        dwFlags: u32,
        lpDDColorTable: u32,
    ) -> DD {
        IDirectDraw::Initialize(ctx, this, lpDD, dwFlags, lpDDColorTable)
    }

    #[win32_derive::dllexport]
    pub fn RestoreDisplayMode(ctx: &mut Context, this: u32) -> DD {
        IDirectDraw::RestoreDisplayMode(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn SetCooperativeLevel(ctx: &mut Context, this: u32, hwnd: HWND, flags: u32) -> DD {
        IDirectDraw::SetCooperativeLevel(ctx, this, hwnd, flags)
    }

    /// IDirectDraw2 adds refresh rate and flags to SetDisplayMode.
    #[win32_derive::dllexport]
    pub fn SetDisplayMode(
        ctx: &mut Context,
        this: u32,
        width: u32,
        height: u32,
        bpp: u32,
        _dwRefreshRate: u32,
        _dwFlags: u32,
    ) -> DD {
        IDirectDraw::SetDisplayMode(ctx, this, width, height, bpp)
    }

    #[win32_derive::dllexport]
    pub fn WaitForVerticalBlank(ctx: &mut Context, this: u32, dwFlags: u32, hEvent: u32) -> DD {
        IDirectDraw::WaitForVerticalBlank(ctx, this, dwFlags, hEvent)
    }

    #[win32_derive::dllexport]
    pub fn GetAvailableVidMem(
        ctx: &mut Context,
        _this: u32,
        _lpDDSCaps: u32,
        lpdwTotal: u32,
        lpdwFree: u32,
    ) -> DD {
        for addr in [lpdwTotal, lpdwFree] {
            if addr != 0 {
                ctx.memory.write::<u32>(addr, 16 << 20);
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

pub mod IDirectDrawSurface2 {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 39] = [
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
        "SetColorKey",
        "GetDC",
        "GetFlipStatus",
        "GetOverlayPosition",
        // IDirectDrawSurface2 callers that use the IDirectDrawSurface v1
        // offsets expect GetPixelFormat at 0x54.  Put it there too, and keep
        // the v2 0x58 slot.  GetPalette is unused by poptb.
        "GetPixelFormat",
        "GetPixelFormat",
        "GetSurfaceDesc",
        "Initialize",
        "IsLost",
        "Lock",
        "ReleaseDC",
        "Restore",
        "SetClipper",
        "SetOverlayPosition",
        "SetPalette",
        "Unlock",
        "UpdateOverlay",
        "UpdateOverlayDisplay",
        "UpdateOverlayZOrder",
        "GetDDInterface",
        "PageLock",
        "PageUnlock",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        IDirectDrawSurface::QueryInterface(ctx, this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(ctx: &mut Context, this: u32) -> u32 {
        IDirectDrawSurface::AddRef(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn Release(ctx: &mut Context, this: u32) -> u32 {
        IDirectDrawSurface::Release(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn AddAttachedSurface(ctx: &mut Context, this: u32, lpDDSAttached: u32) -> DD {
        IDirectDrawSurface::AddAttachedSurface(ctx, this, lpDDSAttached)
    }

    #[win32_derive::dllexport]
    pub fn AddOverlayDirtyRect(ctx: &mut Context, this: u32, lpRect: u32) -> DD {
        IDirectDrawSurface::AddOverlayDirtyRect(ctx, this, lpRect)
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
        IDirectDrawSurface::Blt(
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
        IDirectDrawSurface::BltBatch(ctx, this, lpDDBltBatch, dwCount, dwFlags)
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
        IDirectDrawSurface::BltFast(ctx, this, dwX, dwY, lpDDSrcSurface, lpSrcRect, dwTrans)
    }

    #[win32_derive::dllexport]
    pub fn DeleteAttachedSurface(
        ctx: &mut Context,
        this: u32,
        dwFlags: u32,
        lpDDSAttached: u32,
    ) -> DD {
        IDirectDrawSurface::DeleteAttachedSurface(ctx, this, dwFlags, lpDDSAttached)
    }

    #[win32_derive::dllexport]
    pub fn EnumAttachedSurfaces(
        ctx: &mut Context,
        this: u32,
        lpContext: u32,
        lpEnumCallback: u32,
    ) -> DD {
        IDirectDrawSurface::EnumAttachedSurfaces(ctx, this, lpContext, lpEnumCallback)
    }

    #[win32_derive::dllexport]
    pub fn EnumOverlayZOrders(
        ctx: &mut Context,
        this: u32,
        dwFlags: u32,
        lpContext: u32,
        lpfnCallback: u32,
    ) -> DD {
        IDirectDrawSurface::EnumOverlayZOrders(ctx, this, dwFlags, lpContext, lpfnCallback)
    }

    #[win32_derive::dllexport]
    pub fn Flip(ctx: &mut Context, this: u32, lpDDSurfaceTargetOverride: u32, dwFlags: u32) -> DD {
        IDirectDrawSurface::Flip(ctx, this, lpDDSurfaceTargetOverride, dwFlags)
    }

    #[win32_derive::dllexport]
    pub fn GetAttachedSurface(
        ctx: &mut Context,
        this: u32,
        lpDDSCaps: u32,
        lplpDDAttachedSurface: u32,
    ) -> DD {
        IDirectDrawSurface::GetAttachedSurface(ctx, this, lpDDSCaps, lplpDDAttachedSurface)
    }

    #[win32_derive::dllexport]
    pub fn GetBltStatus(ctx: &mut Context, this: u32, dwFlags: u32) -> DD {
        IDirectDrawSurface::GetBltStatus(ctx, this, dwFlags)
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, this: u32, lpDDSCaps: u32) -> DD {
        IDirectDrawSurface::GetCaps(ctx, this, lpDDSCaps)
    }

    #[win32_derive::dllexport]
    pub fn GetClipper(ctx: &mut Context, this: u32, lplpDDClipper: u32) -> DD {
        IDirectDrawSurface::GetClipper(ctx, this, lplpDDClipper)
    }

    #[win32_derive::dllexport]
    pub fn GetColorKey(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
        IDirectDrawSurface::GetColorKey(ctx, this, dwFlags, lpDDColorKey)
    }

    #[win32_derive::dllexport]
    pub fn SetColorKey(ctx: &mut Context, this: u32, dwFlags: u32, lpDDColorKey: u32) -> DD {
        IDirectDrawSurface::SetColorKey(ctx, this, dwFlags, lpDDColorKey)
    }

    #[win32_derive::dllexport]
    pub fn GetDC(ctx: &mut Context, this: u32, lphDC: u32) -> DD {
        IDirectDrawSurface::GetDC(ctx, this, lphDC)
    }

    #[win32_derive::dllexport]
    pub fn GetFlipStatus(ctx: &mut Context, this: u32, dwFlags: u32) -> DD {
        IDirectDrawSurface::GetFlipStatus(ctx, this, dwFlags)
    }

    #[win32_derive::dllexport]
    pub fn GetOverlayPosition(ctx: &mut Context, this: u32, lplX: u32, lplY: u32) -> DD {
        IDirectDrawSurface::GetOverlayPosition(ctx, this, lplX, lplY)
    }

    #[win32_derive::dllexport]
    pub fn GetPalette(ctx: &mut Context, this: u32, lplpDDPalette: u32) -> DD {
        IDirectDrawSurface::GetPalette(ctx, this, lplpDDPalette)
    }

    #[win32_derive::dllexport]
    pub fn GetPixelFormat(ctx: &mut Context, this: u32, lpDDPixelFormat: u32) -> DD {
        IDirectDrawSurface::GetPixelFormat(ctx, this, lpDDPixelFormat)
    }

    #[win32_derive::dllexport]
    pub fn GetSurfaceDesc(ctx: &mut Context, this: u32, lpDDSurfaceDesc: u32) -> DD {
        IDirectDrawSurface::GetSurfaceDesc(ctx, this, lpDDSurfaceDesc)
    }

    #[win32_derive::dllexport]
    pub fn Initialize(
        ctx: &mut Context,
        this: u32,
        lpDD: u32,
        dwFlags: u32,
        lpDDColorTable: u32,
    ) -> DD {
        IDirectDrawSurface::Initialize(ctx, this, lpDD, dwFlags, lpDDColorTable)
    }

    #[win32_derive::dllexport]
    pub fn IsLost(ctx: &mut Context, this: u32) -> DD {
        IDirectDrawSurface::IsLost(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn Lock(
        ctx: &mut Context,
        this: u32,
        rect: u32,
        lpDesc: u32,
        flags: u32,
        unused: u32,
    ) -> DD {
        IDirectDrawSurface::Lock(ctx, this, rect, lpDesc, flags, unused)
    }

    #[win32_derive::dllexport]
    pub fn ReleaseDC(ctx: &mut Context, this: u32, hDC: u32) -> DD {
        IDirectDrawSurface::ReleaseDC(ctx, this, hDC)
    }

    #[win32_derive::dllexport]
    pub fn Restore(ctx: &mut Context, this: u32) -> DD {
        IDirectDrawSurface::Restore(ctx, this)
    }

    #[win32_derive::dllexport]
    pub fn SetClipper(ctx: &mut Context, this: u32, lpDDClipper: u32) -> DD {
        IDirectDrawSurface::SetClipper(ctx, this, lpDDClipper)
    }

    #[win32_derive::dllexport]
    pub fn SetOverlayPosition(ctx: &mut Context, this: u32, lX: u32, lY: u32) -> DD {
        IDirectDrawSurface::SetOverlayPosition(ctx, this, lX, lY)
    }

    #[win32_derive::dllexport]
    pub fn SetPalette(ctx: &mut Context, this: u32, lpPalette: u32) -> DD {
        IDirectDrawSurface::SetPalette(ctx, this, lpPalette)
    }

    #[win32_derive::dllexport]
    pub fn Unlock(ctx: &mut Context, this: u32, lpRect: u32) -> DD {
        IDirectDrawSurface::Unlock(ctx, this, lpRect)
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlay(
        ctx: &mut Context,
        this: u32,
        lpDestRect: u32,
        lpDDOverlay: u32,
        lpSrcRect: u32,
        dwFlags: u32,
        lpDDOverlayFx: u32,
    ) -> DD {
        IDirectDrawSurface::UpdateOverlay(ctx, this, lpDestRect, lpDDOverlay, lpSrcRect, dwFlags, lpDDOverlayFx)
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlayDisplay(ctx: &mut Context, this: u32, dwFlags: u32) -> DD {
        IDirectDrawSurface::UpdateOverlayDisplay(ctx, this, dwFlags)
    }

    #[win32_derive::dllexport]
    pub fn UpdateOverlayZOrder(ctx: &mut Context, this: u32, dwFlags: u32, lpDDSReference: u32) -> DD {
        IDirectDrawSurface::UpdateOverlayZOrder(ctx, this, dwFlags, lpDDSReference)
    }

    #[win32_derive::dllexport]
    pub fn GetDDInterface(ctx: &mut Context, _this: u32, lplpDD: u32) -> DD {
        let addr = state().ddraw.borrow().as_ref().unwrap().addr;
        ctx.memory.write::<u32>(lplpDD, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn PageLock(_ctx: &mut Context, _this: u32, _dwFlags: u32) -> DD {
        DD::OK // system memory never pages here
    }

    #[win32_derive::dllexport]
    pub fn PageUnlock(_ctx: &mut Context, _this: u32, _dwFlags: u32) -> DD {
        DD::OK
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }
}
