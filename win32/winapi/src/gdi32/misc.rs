use runtime::Context;

use crate::{POINT, Ptr, RECT, gdi32::HDC, stub};

pub type HGDIOBJ = u32;

#[win32_derive::dllexport]
pub fn DeleteObject(_ctx: &mut Context, _ho: HGDIOBJ) -> bool {
    stub!(true)
}

#[win32_derive::dllexport]
pub fn GetSystemPaletteEntries(
    ctx: &mut Context,
    _hdc: HDC,
    iStart: u32,
    cEntries: u32,
    pPalEntries: crate::Ptr<u8>,
) -> u32 {
    // PALETTEENTRY { peRed, peGreen, peBlue, peFlags }: report a gray ramp.
    let mut addr = pPalEntries.addr;
    for i in iStart..iStart + cEntries {
        let level = (i & 0xff) as u8;
        for value in [level, level, level, 0] {
            ctx.memory.write::<u8>(addr, value);
            addr += 1;
        }
    }
    cEntries
}

#[derive(Debug, win32_derive::ABIEnum)]
pub enum GetDeviceCapsArg {
    DRIVERVERSION = 0,
    TECHNOLOGY = 2,
    HORZSIZE = 4,
    VERTSIZE = 6,
    HORZRES = 8,
    VERTRES = 10,
    BITSPIXEL = 12,
    PLANES = 14,
    NUMBRUSHES = 16,
    NUMPENS = 18,
    NUMMARKERS = 20,
    NUMFONTS = 22,
    NUMCOLORS = 24,
    PDEVICESIZE = 26,
    CURVECAPS = 28,
    LINECAPS = 30,
    POLYGONALCAPS = 32,
    TEXTCAPS = 34,
    CLIPCAPS = 36,
    RASTERCAPS = 38,
    ASPECTX = 40,
    ASPECTY = 42,
    ASPECTXY = 44,
    LOGPIXELSX = 88,
    LOGPIXELSY = 90,
    SIZEPALETTE = 104,
    NUMRESERVED = 106,
    COLORRES = 108,
    PHYSICALWIDTH = 110,
    PHYSICALHEIGHT = 111,
    PHYSICALOFFSETX = 112,
    PHYSICALOFFSETY = 113,
    SCALINGFACTORX = 114,
    SCALINGFACTORY = 115,
    VREFRESH = 116,
    DESKTOPVERTRES = 117,
    DESKTOPHORZRES = 118,
    BLTALIGNMENT = 119,
}

#[win32_derive::dllexport]
pub fn GetDeviceCaps(_ctx: &mut Context, _hdc: HDC, index: GetDeviceCapsArg) -> i32 {
    use GetDeviceCapsArg::*;
    // A 640x480 display at 96 dpi.
    match index {
        DRIVERVERSION => 0x400,
        TECHNOLOGY => 1, // DT_RASDISPLAY
        HORZSIZE => 169, // mm
        VERTSIZE => 127,
        HORZRES | DESKTOPHORZRES => 640,
        VERTRES | DESKTOPVERTRES => 480,
        BITSPIXEL => crate::gdi32::DESKTOP_BPP as i32,
        PLANES => 1,
        NUMBRUSHES | NUMPENS | NUMCOLORS => -1, // unlimited / more than 8 bits
        NUMMARKERS | NUMFONTS | PDEVICESIZE => 0,
        CURVECAPS => 0x1ff,
        LINECAPS => 0xfe,
        POLYGONALCAPS => 0xff,
        TEXTCAPS => 0x8004, // TC_OP_CHARACTER | TC_RA_ABLE
        CLIPCAPS => 1,      // CP_RECTANGLE
        // RC_BITBLT | RC_BITMAP64 | RC_DI_BITMAP | RC_DIBTODEV | RC_STRETCHBLT | RC_STRETCHDIB
        RASTERCAPS => 0x2a89,
        ASPECTX | ASPECTY => 36,
        ASPECTXY => 51,
        LOGPIXELSX | LOGPIXELSY => 96,
        SIZEPALETTE | NUMRESERVED => 0,
        COLORRES => crate::gdi32::DESKTOP_BPP as i32,
        VREFRESH => 60,
        BLTALIGNMENT => 0,
        PHYSICALWIDTH | PHYSICALHEIGHT | PHYSICALOFFSETX | PHYSICALOFFSETY | SCALINGFACTORX
        | SCALINGFACTORY => 0,
    }
}

#[win32_derive::dllexport]
pub fn SaveDC(_ctx: &mut Context, _hdc: HDC) -> i32 {
    1
}

#[win32_derive::dllexport]
pub fn RestoreDC(_ctx: &mut Context, _hdc: HDC, _nSavedDC: i32) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn SetBkColor(_ctx: &mut Context, _hdc: HDC, _color: u32) -> u32 {
    0x00ff_ffff // previous color: white
}

#[win32_derive::dllexport]
pub fn SetTextColor(_ctx: &mut Context, _hdc: HDC, _color: u32) -> u32 {
    0 // previous color: black
}

#[win32_derive::dllexport]
pub fn SetMapMode(_ctx: &mut Context, _hdc: HDC, _iMode: i32) -> i32 {
    1 // previous mode: MM_TEXT
}

// The mapping mode stays MM_TEXT with a viewport/window origin of 0; these
// report that back and otherwise do nothing.

#[win32_derive::dllexport]
pub fn SetViewportOrgEx(ctx: &mut Context, _hdc: HDC, _x: i32, _y: i32, lppt: Ptr<POINT>) -> bool {
    if lppt.addr != 0 {
        lppt.write(&mut ctx.memory, POINT::default());
    }
    true
}

#[win32_derive::dllexport]
pub fn OffsetViewportOrgEx(
    ctx: &mut Context,
    _hdc: HDC,
    _x: i32,
    _y: i32,
    lppt: Ptr<POINT>,
) -> bool {
    if lppt.addr != 0 {
        lppt.write(&mut ctx.memory, POINT::default());
    }
    true
}

#[win32_derive::dllexport]
pub fn SetViewportExtEx(ctx: &mut Context, _hdc: HDC, _x: i32, _y: i32, lpsz: Ptr<POINT>) -> bool {
    if lpsz.addr != 0 {
        lpsz.write(&mut ctx.memory, POINT { x: 1, y: 1 });
    }
    true
}

#[win32_derive::dllexport]
pub fn ScaleViewportExtEx(
    ctx: &mut Context,
    _hdc: HDC,
    _xn: i32,
    _dx: i32,
    _yn: i32,
    _yd: i32,
    lpsz: Ptr<POINT>,
) -> bool {
    if lpsz.addr != 0 {
        lpsz.write(&mut ctx.memory, POINT { x: 1, y: 1 });
    }
    true
}

#[win32_derive::dllexport]
pub fn SetWindowExtEx(ctx: &mut Context, _hdc: HDC, _x: i32, _y: i32, lpsz: Ptr<POINT>) -> bool {
    if lpsz.addr != 0 {
        lpsz.write(&mut ctx.memory, POINT { x: 1, y: 1 });
    }
    true
}

#[win32_derive::dllexport]
pub fn ScaleWindowExtEx(
    ctx: &mut Context,
    _hdc: HDC,
    _xn: i32,
    _xd: i32,
    _yn: i32,
    _yd: i32,
    lpsz: Ptr<POINT>,
) -> bool {
    if lpsz.addr != 0 {
        lpsz.write(&mut ctx.memory, POINT { x: 1, y: 1 });
    }
    true
}

#[win32_derive::dllexport]
pub fn GetClipBox(ctx: &mut Context, _hdc: HDC, lprect: Ptr<RECT>) -> i32 {
    const SIMPLEREGION: i32 = 2;
    let rect = match crate::user32::state().window.borrow().as_ref() {
        Some(window) => window.borrow().rect(),
        None => RECT {
            left: 0,
            top: 0,
            right: 640,
            bottom: 480,
        },
    };
    lprect.write(&mut ctx.memory, rect);
    SIMPLEREGION
}

#[win32_derive::dllexport]
pub fn PtVisible(_ctx: &mut Context, _hdc: HDC, _x: i32, _y: i32) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn RectVisible(_ctx: &mut Context, _hdc: HDC, _lprect: Ptr<RECT>) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn TextOutA(ctx: &mut Context, _hdc: HDC, x: i32, y: i32, lpString: Ptr<u8>, c: i32) -> bool {
    let text =
        String::from_utf8_lossy(&ctx.memory[lpString.addr..][..c.max(0) as usize]).to_string();
    log::warn!("TextOutA({x}, {y}, {text:?}): text not drawn");
    true
}

#[win32_derive::dllexport]
pub fn ExtTextOutA(
    ctx: &mut Context,
    _hdc: HDC,
    x: i32,
    y: i32,
    _options: u32,
    _lprect: Ptr<RECT>,
    lpString: Ptr<u8>,
    c: u32,
    _lpDx: Ptr<i32>,
) -> bool {
    let text = String::from_utf8_lossy(&ctx.memory[lpString.addr..][..c as usize]).to_string();
    log::warn!("ExtTextOutA({x}, {y}, {text:?}): text not drawn");
    true
}

#[win32_derive::dllexport]
pub fn CreateBitmap(
    _ctx: &mut Context,
    nWidth: i32,
    nHeight: i32,
    nPlanes: u32,
    nBitCount: u32,
    _lpBits: u32,
) -> u32 {
    log::warn!("CreateBitmap({nWidth}x{nHeight}, {nPlanes} planes, {nBitCount} bpp): unsupported");
    0
}

#[win32_derive::dllexport]
pub fn Escape(
    _ctx: &mut Context,
    _hdc: HDC,
    iEscape: i32,
    _cjIn: i32,
    _pvIn: u32,
    _pvOut: u32,
) -> i32 {
    // Printer escapes; MFC queries these when setting up printing.
    log::warn!("Escape({iEscape}): unsupported");
    0
}
