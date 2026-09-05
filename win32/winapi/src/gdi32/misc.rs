use runtime::Context;

use crate::{gdi32::HDC, stub};

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
    // Capabilities of the emulated 640x480 32bpp raster display at 60 Hz and
    // 96 DPI, matching the user32 display-mode model.
    match index {
        DRIVERVERSION => 0,
        TECHNOLOGY => 1, // DT_RASDISPLAY
        HORZSIZE => 169, // mm: 640 px at 96 DPI
        VERTSIZE => 127, // mm: 480 px at 96 DPI
        HORZRES => 640,
        VERTRES => 480,
        BITSPIXEL => 32,
        PLANES => 1,
        NUMBRUSHES | NUMPENS => -1, // not limited by the device
        NUMMARKERS | NUMFONTS => 0, // no device markers or device fonts
        NUMCOLORS => -1,            // true color
        PDEVICESIZE => 0,
        CURVECAPS => 0,
        LINECAPS => 0x4,      // LC_POLYLINE
        POLYGONALCAPS => 0x2, // PC_RECTANGLE
        TEXTCAPS => 0x1,      // TC_OP_CHARACTER
        CLIPCAPS => 0x1,      // CP_RECTANGLE
        RASTERCAPS => 0x1,    // RC_BITBLT
        ASPECTX | ASPECTY => 36,
        ASPECTXY => 51,
        LOGPIXELSX | LOGPIXELSY => 96,
        SIZEPALETTE | NUMRESERVED | COLORRES => 0, // non-palettized device
        PHYSICALWIDTH | PHYSICALHEIGHT | PHYSICALOFFSETX | PHYSICALOFFSETY => 0,
        SCALINGFACTORX | SCALINGFACTORY => 0,
        VREFRESH => 60,
        DESKTOPHORZRES => 640,
        DESKTOPVERTRES => 480,
        BLTALIGNMENT => 0, // no preferred blit alignment
    }
}

#[cfg(test)]
mod tests {
    use super::{GetDeviceCaps, GetDeviceCapsArg};
    use crate::gdi32::HDC;
    use runtime::{BlockCache, CPU, Context, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    #[test]
    fn device_caps_match_the_emulated_display() {
        let mut ctx = context();
        let hdc = HDC::null();
        assert_eq!(GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::HORZRES), 640);
        assert_eq!(GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::VERTRES), 480);
        assert_eq!(
            GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::BITSPIXEL),
            32
        );
        assert_eq!(GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::VREFRESH), 60);
        assert_eq!(
            GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::NUMCOLORS),
            -1
        );
        assert_eq!(
            GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::SIZEPALETTE),
            0
        );
    }
}
