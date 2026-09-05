use std::sync::Arc;

use runtime::Context;

use crate::{
    Ptr,
    gdi32::{self, Bitmap, COLORREF, HBRUSH, HDC, HFONT, HGDIOBJ, HPEN},
};

/// A `None` color is a NULL_BRUSH: it draws nothing when selected.
#[derive(Debug, Clone)]
pub struct Brush(pub Option<COLORREF>);

/// A `None` color is a NULL_PEN: it draws nothing when selected.
#[derive(Debug, Clone)]
pub struct Pen(pub Option<COLORREF>);

#[derive(Debug, Clone, Default)]
pub struct Font {
    pub height: i32,
    pub width: i32,
    pub weight: i32,
    pub face: String,
}

#[win32_derive::dllexport]
pub fn CreatePen(
    _ctx: &mut Context,
    iStyle: u32, /* PEN_STYLE */
    cWidth: i32,
    color: COLORREF,
) -> HPEN {
    assert_eq!(iStyle, 0); // PS_SOLID
    assert_eq!(cWidth, 1);
    let pen = Pen(Some(color));
    gdi32::lock().objects.add(Object::Pen(pen))
}

#[win32_derive::dllexport]
pub fn CreateSolidBrush(_ctx: &mut Context, color: COLORREF) -> HBRUSH {
    gdi32::lock().objects.add(Object::Brush(Brush(Some(color))))
}

#[win32_derive::dllexport]
pub fn CreateFontA(
    ctx: &mut Context,
    nHeight: i32,
    nWidth: i32,
    _nEscapement: i32,
    _nOrientation: i32,
    fnWeight: i32,
    _fdwItalic: u32,
    _fdwUnderline: u32,
    _fdwStrikeOut: u32,
    _fdwCharSet: u32,
    _fdwOutputPrecision: u32,
    _fdwClipPrecision: u32,
    _fdwQuality: u32,
    _fdwPitchAndFamily: u32,
    lpszFace: Ptr<u8>,
) -> HFONT {
    let face = if lpszFace.addr == 0 {
        String::new()
    } else {
        ctx.memory.read_str(lpszFace.addr).to_owned()
    };
    gdi32::lock().objects.add(Object::Font(Font {
        height: nHeight,
        width: nWidth,
        weight: fnWeight,
        face,
    }))
}

/// The emulated display is non-palettized, so a palette is only a
/// placeholder handle; its entries are never realized.
#[derive(Debug, Clone, Copy)]
pub struct Palette;

pub enum Object {
    Bitmap(Arc<Bitmap>),
    Brush(Brush),
    Pen(Pen),
    Font(Font),
    Palette(Palette),
}

impl Object {
    pub fn unwrap_brush(&self) -> Brush {
        let Object::Brush(brush) = self else { panic!() };
        brush.clone()
    }
}

#[repr(C)]
#[derive(zerocopy::Immutable, zerocopy::IntoBytes)]
pub struct BITMAP {
    bmType: u32,
    bmWidth: u32,
    bmHeight: u32,
    bmWidthBytes: u32,
    bmPlanes: u16,
    bmBitsPixel: u16,
    bmBits: u32,
}

#[win32_derive::dllexport]
pub fn GetObjectA(ctx: &mut Context, handle: HGDIOBJ, size: u32, lpOut: Ptr<BITMAP>) -> u32 {
    let state = gdi32::lock();
    let object = state.objects.get(handle).unwrap();
    let Object::Bitmap(bitmap) = object else {
        panic!();
    };
    assert!(size == std::mem::size_of::<BITMAP>() as u32);
    let fields = BITMAP {
        bmType: 0,
        bmWidth: bitmap.width,
        bmHeight: bitmap.height,
        bmWidthBytes: 0,
        bmPlanes: 0,
        bmBitsPixel: bitmap.bit_count as u16,
        bmBits: 0,
    };
    lpOut.write(&mut ctx.memory, fields).unwrap();
    size
}

#[derive(Debug, win32_derive::ABIEnum)]
pub enum GetStockObjectArg {
    WHITE_BRUSH = 0,
    LTGRAY_BRUSH = 1,
    GRAY_BRUSH = 2,
    DKGRAY_BRUSH = 3,
    BLACK_BRUSH = 4,
    NULL_BRUSH = 5,
    WHITE_PEN = 6,
    BLACK_PEN = 7,
    NULL_PEN = 8,
    OEM_FIXED_FONT = 10,
    ANSI_FIXED_FONT = 11,
    ANSI_VAR_FONT = 12,
    SYSTEM_FONT = 13,
    DEVICE_DEFAULT_FONT = 14,
    DEFAULT_PALETTE = 15,
    SYSTEM_FIXED_FONT = 16,
    DEFAULT_GUI_FONT = 17,
    DC_BRUSH = 18,
    DC_PEN = 19,
}

#[win32_derive::dllexport]
pub fn GetStockObject(_ctx: &mut Context, i: GetStockObjectArg) -> HGDIOBJ {
    use GetStockObjectArg::*;
    let rgb = |r, g, b| Some(COLORREF::from_rgb(r, g, b));
    let object = match i {
        WHITE_BRUSH => Object::Brush(Brush(rgb(0xff, 0xff, 0xff))),
        LTGRAY_BRUSH => Object::Brush(Brush(rgb(0xc0, 0xc0, 0xc0))),
        GRAY_BRUSH => Object::Brush(Brush(rgb(0x80, 0x80, 0x80))),
        DKGRAY_BRUSH => Object::Brush(Brush(rgb(0x40, 0x40, 0x40))),
        BLACK_BRUSH => Object::Brush(Brush(rgb(0x00, 0x00, 0x00))),
        NULL_BRUSH => Object::Brush(Brush(None)),
        WHITE_PEN => Object::Pen(Pen(rgb(0xff, 0xff, 0xff))),
        BLACK_PEN => Object::Pen(Pen(rgb(0x00, 0x00, 0x00))),
        NULL_PEN => Object::Pen(Pen(None)),
        // The emulated font model does not distinguish stock faces; they all
        // share the default metrics used by the text path.
        OEM_FIXED_FONT | ANSI_FIXED_FONT | ANSI_VAR_FONT | SYSTEM_FONT | DEVICE_DEFAULT_FONT
        | SYSTEM_FIXED_FONT | DEFAULT_GUI_FONT => Object::Font(Font::default()),
        // DC_BRUSH/DC_PEN are stock objects whose colors come from
        // SetDCBrushColor/SetDCPenColor; neither is implemented, so report the
        // documented defaults (white brush, black pen).
        DC_BRUSH => Object::Brush(Brush(rgb(0xff, 0xff, 0xff))),
        DC_PEN => Object::Pen(Pen(rgb(0x00, 0x00, 0x00))),
        DEFAULT_PALETTE => Object::Palette(Palette),
    };
    gdi32::lock().objects.add(object)
}

#[win32_derive::dllexport]
pub fn SelectObject(_ctx: &mut Context, hdc: HDC, h: HGDIOBJ) -> HGDIOBJ {
    if h.is_null_or_invalid() {
        log::warn!("SelectObject: ignoring null select, likely from a prior stub");
        return HGDIOBJ::null();
    }
    let state = &mut *gdi32::lock();
    let dc = state.dcs.get_mut(hdc).unwrap();
    let object = state.objects.get(h).unwrap();
    match object {
        Object::Bitmap(bitmap) => {
            let prev = dc.bitmap.0;
            dc.bitmap = (h, bitmap.clone());
            prev
        }
        Object::Pen(pen) => {
            let prev = dc.pen.0;
            dc.pen = (h, pen.clone());
            prev
        }
        Object::Brush(brush) => {
            let prev = dc.brush.0;
            dc.brush = (h, brush.clone());
            prev
        }
        Object::Font(font) => {
            let prev = dc.font.0;
            dc.font = (h, font.clone());
            prev
        }
        Object::Palette(_) => {
            // Palettes are selected with SelectPalette, not SelectObject.
            HGDIOBJ::null()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GetStockObject, GetStockObjectArg, Object};
    use crate::gdi32;
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
    fn stock_objects_cover_every_variant() {
        use GetStockObjectArg::*;
        let mut ctx = context();
        for i in [
            WHITE_BRUSH,
            LTGRAY_BRUSH,
            GRAY_BRUSH,
            DKGRAY_BRUSH,
            BLACK_BRUSH,
            NULL_BRUSH,
            WHITE_PEN,
            BLACK_PEN,
            NULL_PEN,
            OEM_FIXED_FONT,
            ANSI_FIXED_FONT,
            ANSI_VAR_FONT,
            SYSTEM_FONT,
            DEVICE_DEFAULT_FONT,
            SYSTEM_FIXED_FONT,
            DEFAULT_GUI_FONT,
            DC_BRUSH,
            DC_PEN,
            DEFAULT_PALETTE,
        ] {
            assert!(!GetStockObject(&mut ctx, i).is_null());
        }
    }

    #[test]
    fn null_stock_objects_are_hollow() {
        let mut ctx = context();
        let null_brush = GetStockObject(&mut ctx, GetStockObjectArg::NULL_BRUSH);
        let null_pen = GetStockObject(&mut ctx, GetStockObjectArg::NULL_PEN);

        let state = gdi32::lock();
        let Object::Brush(brush) = state.objects.get(null_brush).unwrap() else {
            panic!()
        };
        assert!(brush.0.is_none());
        let Object::Pen(pen) = state.objects.get(null_pen).unwrap() else {
            panic!()
        };
        assert!(pen.0.is_none());
    }
}
