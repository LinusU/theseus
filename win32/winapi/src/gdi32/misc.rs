use runtime::Context;

use crate::{HANDLE, gdi32::HDC};

pub type HGDIOBJ = u32;

#[win32_derive::dllexport]
pub fn DeleteObject(ctx: &mut Context, ho: HGDIOBJ) -> bool {
    let mut state = crate::gdi32::lock();
    let handle = HANDLE::from_raw(ho);
    // Deleting a stock object is a documented harmless no-op; the shared
    // handle must survive because other DCs may still have it selected.
    if state.is_stock_object(handle) {
        return true;
    }
    // Deleting an object that is still selected into a DC fails.
    let selected = state.dcs.iter().any(|(_, dc)| {
        dc.bitmap.0 == handle || dc.pen.0 == handle || dc.brush.0 == handle || dc.font.0 == handle
    });
    if selected {
        return false;
    }
    let Some(object) = state.objects.remove(handle) else {
        return false;
    };
    // A heap-allocated bitmap's pixel buffer goes back to the process heap.
    if let crate::gdi32::Object::Bitmap(bitmap) = object
        && state.heap_bitmap_pixels.remove(&bitmap.pixels)
    {
        crate::kernel32::lock()
            .process_heap
            .free(&mut ctx.memory, bitmap.pixels);
    }
    true
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
    if !crate::ddraw::guest_range(ctx, pPalEntries.addr, cEntries.saturating_mul(4)) {
        return 0;
    }
    let mut addr = pPalEntries.addr;
    for i in 0..cEntries {
        let level = (iStart.wrapping_add(i) & 0xff) as u8;
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
pub fn GetDeviceCaps(_ctx: &mut Context, _hdc: HDC, index: u32) -> i32 {
    let Ok(index) = GetDeviceCapsArg::try_from(index) else {
        log::warn!("GetDeviceCaps({index}): unknown device capability");
        return 0;
    };
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
    use super::{DeleteObject, GetDeviceCaps, GetDeviceCapsArg};
    use crate::gdi32::{self, HDC};
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
        assert_eq!(
            GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::HORZRES as u32),
            640
        );
        assert_eq!(
            GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::VERTRES as u32),
            480
        );
        assert_eq!(
            GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::BITSPIXEL as u32),
            32
        );
        assert_eq!(
            GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::VREFRESH as u32),
            60
        );
        assert_eq!(
            GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::NUMCOLORS as u32),
            -1
        );
        assert_eq!(
            GetDeviceCaps(&mut ctx, hdc, GetDeviceCapsArg::SIZEPALETTE as u32),
            0
        );
    }

    #[test]
    fn delete_object_ignores_a_stock_object() {
        let mut ctx = context();
        let stock = crate::gdi32::GetStockObject(
            &mut ctx,
            crate::gdi32::GetStockObjectArg::BLACK_PEN as u32,
        );
        // Deleting a stock object is a harmless no-op: the shared handle
        // keeps resolving for the DCs that have it selected.
        assert!(DeleteObject(&mut ctx, stock.to_raw()));
        assert!(gdi32::lock().objects.get(stock).is_some());
        assert_eq!(
            crate::gdi32::GetStockObject(
                &mut ctx,
                crate::gdi32::GetStockObjectArg::BLACK_PEN as u32
            ),
            stock
        );
    }

    #[test]
    fn delete_object_rejects_an_object_selected_into_a_dc() {
        let mut ctx = context();
        let bitmap = gdi32::Bitmap::new_simple(2, 2, 0x3000);
        let hdc = gdi32::lock().new_memory_dc(bitmap);
        let hbitmap = gdi32::lock().dcs.get(hdc).unwrap().bitmap.0;

        // A selected object cannot be deleted, and survives the attempt.
        assert!(!DeleteObject(&mut ctx, hbitmap.to_raw()));
        assert!(gdi32::lock().objects.get(hbitmap).is_some());

        // Once the DC is gone the delete succeeds, and fails when repeated.
        gdi32::lock().release_dc(&mut ctx.memory, hdc);
        assert!(DeleteObject(&mut ctx, hbitmap.to_raw()));
        assert!(!DeleteObject(&mut ctx, hbitmap.to_raw()));
    }
}
