use std::sync::Arc;

use runtime::Context;

use crate::{
    gdi32::{self, Bitmap, Pen, State, COLORREF, HBITMAP, HPEN},
    Ptr, HANDLE, POINT,
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
    rop2: R2,
    pos: POINT,
}

impl DC {
    pub fn new(hbitmap: HBITMAP, bitmap: Arc<Bitmap>) -> Self {
        DC {
            bitmap: (hbitmap, bitmap),
            pen: (HPEN::null(), Pen(COLORREF::default())),
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
pub fn DeleteDC(_ctx: &mut Context, hdc: HDC) -> bool {
    gdi32::lock().release_dc(hdc);
    true
}

#[win32_derive::dllexport]
pub fn GetLayout(_ctx: &mut Context, _hdc: HDC) -> u32 {
    0 // LTR
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
pub fn SetPixel(ctx: &mut Context, hdc: HDC, x: i32, y: i32, color: COLORREF) -> COLORREF {
    let bitmap = {
        let state = gdi32::lock();
        state.dcs.get(hdc).unwrap().bitmap().clone()
    };
    if x < 0 || y < 0 || x as u32 >= bitmap.width || y as u32 >= bitmap.height {
        return color;
    }

    let [r, g, b] = color.to_rgb();
    let offset =
        bitmap.pixels + y as u32 * bitmap.stride() + x as u32 * (bitmap.bit_count as u32 / 8);
    match bitmap.bit_count {
        16 => {
            let pixel = ((r as u16 >> 3) << 10) | ((g as u16 >> 3) << 5) | (b as u16 >> 3);
            ctx.memory.write::<u16>(offset, pixel);
        }
        24 => ctx.memory[offset..][..3].copy_from_slice(&[r, g, b]),
        32 => ctx.memory[offset..][..4].copy_from_slice(&color.to_pixel()),
        _ => log::warn!("SetPixel: unsupported {}-bit bitmap", bitmap.bit_count),
    }
    color
}

#[win32_derive::dllexport]
pub fn GetPixel(ctx: &mut Context, hdc: HDC, x: i32, y: i32) -> COLORREF {
    let bitmap = {
        let state = gdi32::lock();
        state.dcs.get(hdc).unwrap().bitmap().clone()
    };
    if x < 0 || y < 0 || x as u32 >= bitmap.width || y as u32 >= bitmap.height {
        return COLORREF::default();
    }

    let offset =
        bitmap.pixels + y as u32 * bitmap.stride() + x as u32 * (bitmap.bit_count as u32 / 8);
    let [r, g, b] = match bitmap.bit_count {
        16 => {
            let pixel = ctx.memory.read::<u16>(offset);
            [
                (((pixel >> 10) & 0x1f) * 255 / 31) as u8,
                (((pixel >> 5) & 0x1f) * 255 / 31) as u8,
                ((pixel & 0x1f) * 255 / 31) as u8,
            ]
        }
        24 | 32 => [
            ctx.memory[offset],
            ctx.memory[offset + 1],
            ctx.memory[offset + 2],
        ],
        _ => {
            log::warn!("GetPixel: unsupported {}-bit bitmap", bitmap.bit_count);
            [0, 0, 0]
        }
    };
    COLORREF::from_rgb(r, g, b)
}
