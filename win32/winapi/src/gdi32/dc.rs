use std::sync::Arc;

use runtime::Context;

use crate::{
    HANDLE, POINT, Ptr,
    gdi32::{self, Bitmap, COLORREF, Font, HBITMAP, HGDIOBJ, HPEN, Pen, State},
    stub,
};

pub type HDC = HANDLE;

impl State {
    pub fn new_memory_dc(&mut self, bitmap: Bitmap) -> HDC {
        let (hbitmap, bitmap) = self.new_bitmap_handle(bitmap);
        let dc = DC::new(hbitmap, bitmap);
        self.dcs.add(dc)
    }

    pub fn release_dc(&mut self, hdc: HDC) {
        self.dcs.remove(hdc);
    }
}

pub struct DC {
    /// Store the HBITMAP as well as the Bitmap itself so that when it is switched via SelectObject we can return it.
    pub bitmap: (HBITMAP, Arc<Bitmap>),
    pub pen: (HPEN, Pen),
    pub font: (HGDIOBJ, Font),
    rop2: R2,
    pos: POINT,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, zerocopy::Immutable, zerocopy::IntoBytes)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}

impl DC {
    pub fn new(hbitmap: HBITMAP, bitmap: Arc<Bitmap>) -> Self {
        DC {
            bitmap: (hbitmap, bitmap),
            pen: (HPEN::null(), Pen(COLORREF::default())),
            font: (HGDIOBJ::null(), Font::default()),
            rop2: R2::COPYPEN,
            pos: POINT::default(),
        }
    }

    pub fn bitmap(&self) -> &Arc<Bitmap> {
        &self.bitmap.1
    }
}

#[win32_derive::dllexport]
pub fn CreateCompatibleDC(_ctx: &mut Context, hdc: HDC) -> HDC {
    // 1x1 monochrome bitmap
    let bitmap = Bitmap {
        width: 1,
        height: 1,
        is_bottom_up: false,
        bit_count: 1,
        palette: Box::new([COLORREF::default()]),
        pixels: 0,
    };
    let new_hdc = gdi32::lock().new_memory_dc(bitmap);
    if hdc.is_null() {
        // memory DC compatible with screen
        new_hdc
    } else {
        // memory DC compatible with hdc
        new_hdc
    }
}

#[win32_derive::dllexport]
pub fn DeleteDC(_ctx: &mut Context, _hdc: HDC) -> bool {
    stub!(true)
}

#[win32_derive::dllexport]
pub fn GetLayout(_ctx: &mut Context, _hdc: HDC) -> u32 {
    0 // LTR
}

fn text_extent(font: &Font, count: usize) -> SIZE {
    let height = if font.height == 0 {
        16
    } else {
        font.height.unsigned_abs().min(i32::MAX as u32) as i32
    };
    let width = if font.width == 0 {
        (height / 2).max(1)
    } else {
        font.width.unsigned_abs().min(i32::MAX as u32) as i32
    };
    SIZE {
        cx: width.saturating_mul(count.min(i32::MAX as usize) as i32),
        cy: height,
    }
}

#[win32_derive::dllexport]
pub fn GetTextExtentPoint32A(
    ctx: &mut Context,
    hdc: HDC,
    lpString: Ptr<u8>,
    cchString: i32,
    lpSize: Ptr<SIZE>,
) -> bool {
    if cchString < 0 || lpSize.addr < 0x1000 {
        return false;
    }
    let count = cchString as usize;
    if count != 0 {
        let Some(end) = lpString.addr.checked_add(cchString as u32) else {
            return false;
        };
        if lpString.addr < 0x1000 || end as usize > ctx.memory.bytes.len() {
            return false;
        }
    }
    let state = gdi32::lock();
    let Some(dc) = state.dcs.get(hdc) else {
        return false;
    };
    lpSize
        .write(&mut ctx.memory, text_extent(&dc.font.1, count))
        .is_some()
}

#[derive(Debug, win32_derive::ABIEnum)]
pub enum R2 {
    BLACK = 1,
    NOTMERGEPEN = 2,
    MASKNOTPEN = 3,
    NOTCOPYPEN = 4,
    MASKPENNOT = 5,
    NOT = 6,
    XORPEN = 7,
    NOTMASKPEN = 8,
    MASKPEN = 9,
    NOTXORPEN = 10,
    NOP = 11,
    MERGENOTPEN = 12,
    COPYPEN = 13,
    MERGEPENNOT = 14,
    MERGEPEN = 15,
    WHITE = 16,
}

#[win32_derive::dllexport]
pub fn SetROP2(_ctx: &mut Context, hdc: HDC, rop2: R2) -> i32 {
    let mut state = gdi32::lock();
    let dc = state.dcs.get_mut(hdc).unwrap();
    std::mem::replace(&mut dc.rop2, rop2) as i32
}

fn ascending(a: i32, b: i32) -> std::ops::RangeInclusive<i32> {
    if a < b { a..=b } else { b..=a }
}

#[win32_derive::dllexport]
pub fn LineTo(ctx: &mut Context, hdc: HDC, x: i32, y: i32) -> bool {
    let mut state = gdi32::lock();
    let dc = state.dcs.get_mut(hdc).unwrap();
    let bitmap = dc.bitmap();
    assert!(bitmap.is_simple());

    let color = match dc.rop2 {
        R2::COPYPEN => dc.pen.1.0,
        R2::WHITE => COLORREF::from_rgb(0xff, 0xff, 0xff),
        _ => todo!("{:?}", dc.rop2),
    };

    let pixels = bitmap.pixels_mut(&mut ctx.memory);
    if x == dc.pos.x {
        for y in ascending(dc.pos.y, y) {
            let i = ((y as u32 * bitmap.stride()) + (x as u32 * 4)) as usize;
            pixels[i..][..4].copy_from_slice(&color.to_pixel());
        }
    } else if y == dc.pos.y {
        for x in ascending(dc.pos.x, x) {
            let i = ((y as u32 * bitmap.stride()) + (x as u32 * 4)) as usize;
            pixels[i..][..4].copy_from_slice(&color.to_pixel());
        }
    } else {
        todo!(); // only axis-aligned supported for now
    }

    dc.pos = POINT { x, y };

    true
}

#[win32_derive::dllexport]
pub fn MoveToEx(ctx: &mut Context, hdc: HDC, x: i32, y: i32, lppt: Ptr<POINT>) -> bool {
    let mut state = gdi32::lock();
    let dc = state.dcs.get_mut(hdc).unwrap();
    dc.pos = POINT { x, y };
    if lppt.addr != 0 {
        lppt.write(&mut ctx.memory, dc.pos).unwrap();
    }
    true
}

#[win32_derive::dllexport]
pub fn SetLayout(_ctx: &mut Context, _hdc: HDC, _l: u32 /* DC_LAYOUT */) -> u32 {
    todo!()
}

#[win32_derive::dllexport]
pub fn SetPixel(_ctx: &mut Context, _hdc: HDC, _x: i32, _y: i32, _color: COLORREF) -> COLORREF {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::{Font, SIZE, text_extent};

    #[test]
    fn text_extent_uses_requested_font_metrics() {
        let font = Font {
            height: -12,
            width: 7,
            weight: 400,
            face: String::new(),
        };
        assert_eq!(text_extent(&font, 5), SIZE { cx: 35, cy: 12 });
    }

    #[test]
    fn text_extent_defaults_zero_metrics() {
        assert_eq!(text_extent(&Font::default(), 3), SIZE { cx: 24, cy: 16 });
    }
}
