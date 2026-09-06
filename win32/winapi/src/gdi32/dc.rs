use std::sync::Arc;

use runtime::Context;

use crate::{
    HANDLE, Handles, POINT, Ptr,
    gdi32::{
        self, Bitmap, Brush, COLORREF, Font, HBITMAP, HBRUSH, HGDIOBJ, HPEN, Object, Pen, State,
    },
};

pub type HDC = HANDLE;

impl State {
    pub fn new_memory_dc(&mut self, bitmap: Bitmap) -> HDC {
        let (hbitmap, bitmap) = self.new_bitmap_handle(bitmap);
        let dc = DC::new(hbitmap, bitmap, &mut self.objects);
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
    pub brush: (HBRUSH, Brush),
    pub font: (HGDIOBJ, Font),
    rop2: R2,
    bk_mode: i32,
    text_color: COLORREF,
    bk_color: COLORREF,
    pos: POINT,
    layout: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, zerocopy::Immutable, zerocopy::IntoBytes)]
pub struct SIZE {
    pub cx: i32,
    pub cy: i32,
}

impl DC {
    /// A fresh DC selects the stock objects Windows gives it (black pen,
    /// white brush, system font), so SelectObject reports a real previous
    /// handle rather than null.
    pub fn new(hbitmap: HBITMAP, bitmap: Arc<Bitmap>, objects: &mut Handles<Object>) -> Self {
        let pen = Pen(Some(COLORREF::default()));
        let brush = Brush(Some(COLORREF::from_rgb(0xff, 0xff, 0xff)));
        let font = Font::default();
        DC {
            bitmap: (hbitmap, bitmap),
            pen: (objects.add(Object::Pen(pen.clone())), pen),
            brush: (objects.add(Object::Brush(brush.clone())), brush),
            font: (objects.add(Object::Font(font.clone())), font),
            rop2: R2::COPYPEN,
            bk_mode: 2,
            text_color: COLORREF::default(),
            bk_color: COLORREF::from_rgb(0xff, 0xff, 0xff),
            pos: POINT::default(),
            layout: 0, // LAYOUT_LTR
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
    gdi32::lock().dcs.remove(hdc).is_some()
}

#[win32_derive::dllexport]
pub fn GetLayout(_ctx: &mut Context, _hdc: HDC) -> u32 {
    0 // LTR
}

#[win32_derive::dllexport]
pub fn SetBkMode(_ctx: &mut Context, hdc: HDC, mode: i32) -> i32 {
    if !matches!(mode, 1 | 2) {
        return 0;
    }
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return 0;
    };
    std::mem::replace(&mut dc.bk_mode, mode)
}

#[win32_derive::dllexport]
pub fn SetTextColor(_ctx: &mut Context, hdc: HDC, color: COLORREF) -> COLORREF {
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return COLORREF(u32::MAX);
    };
    std::mem::replace(&mut dc.text_color, color)
}

#[win32_derive::dllexport]
pub fn SetBkColor(_ctx: &mut Context, hdc: HDC, color: COLORREF) -> COLORREF {
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return COLORREF(u32::MAX);
    };
    std::mem::replace(&mut dc.bk_color, color)
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

pub fn text_extent_for_dc(hdc: HDC, count: usize) -> Option<SIZE> {
    let state = gdi32::lock();
    state.dcs.get(hdc).map(|dc| text_extent(&dc.font.1, count))
}

fn glyph(c: u8) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        b' ' => [0, 0, 0, 0, 0, 0, 0],
        b'0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        b'1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        b'2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        b'3' => [0x1e, 0x01, 0x01, 0x0e, 0x01, 0x01, 0x1e],
        b'4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        b'5' => [0x1f, 0x10, 0x10, 0x1e, 0x01, 0x01, 0x1e],
        b'6' => [0x0e, 0x10, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        b'7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        b'9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x01, 0x0e],
        b'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        b'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        b'C' => [0x0f, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0f],
        b'D' => [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e],
        b'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        b'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        b'G' => [0x0f, 0x10, 0x10, 0x17, 0x11, 0x11, 0x0f],
        b'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        b'I' => [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        b'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0c],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        b'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        b'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        b'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        b'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        b'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        b'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1b, 0x11],
        b'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        b'Y' => [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
        b'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        b'.' => [0, 0, 0, 0, 0, 0x0c, 0x0c],
        b',' => [0, 0, 0, 0, 0x0c, 0x0c, 0x08],
        b'-' => [0, 0, 0, 0x1f, 0, 0, 0],
        b'_' => [0, 0, 0, 0, 0, 0, 0x1f],
        b':' => [0, 0x0c, 0x0c, 0, 0x0c, 0x0c, 0],
        b'!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0, 0x04],
        _ => [0x1f, 0x11, 0x15, 0x11, 0x15, 0x11, 0x1f],
    }
}

fn draw_pixel(pixels: &mut [u8], bitmap: &Bitmap, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= bitmap.width as i32 || y >= bitmap.height as i32 {
        return;
    }
    let offset = ((y as u32 * bitmap.stride()) + x as u32 * 4) as usize;
    pixels[offset..][..4].copy_from_slice(&color);
}

fn fill_pixels(
    pixels: &mut [u8],
    bitmap: &Bitmap,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: [u8; 4],
) {
    for py in y..y.saturating_add(height) {
        for px in x..x.saturating_add(width) {
            draw_pixel(pixels, bitmap, px, py, color);
        }
    }
}

#[win32_derive::dllexport]
pub fn TextOutA(ctx: &mut Context, hdc: HDC, x: i32, y: i32, lpString: Ptr<u8>, c: i32) -> bool {
    if c < 0 {
        return false;
    }
    let count = c as usize;
    if count != 0 {
        let Some(end) = lpString.addr.checked_add(c as u32) else {
            return false;
        };
        if lpString.addr < 0x1000 || end as usize > ctx.memory.bytes.len() {
            return false;
        }
    }
    let string = ctx.memory[lpString.addr..][..count].to_vec();
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return false;
    };
    let bitmap = dc.bitmap.1.clone();
    if !bitmap.is_simple() {
        return false;
    }
    let font = dc.font.1.clone();
    let size = text_extent(&font, 1);
    let text_color = dc.text_color.to_pixel();
    let bk_color = dc.bk_color.to_pixel();
    let opaque = dc.bk_mode == 2;
    let scale_x = (size.cx / 5).clamp(1, 64);
    let scale_y = (size.cy / 7).clamp(1, 64);
    let Some(pixels) = bitmap.pixels_mut(&mut ctx.memory) else {
        return false;
    };
    for (index, character) in string.into_iter().enumerate() {
        let origin_x = x.saturating_add(size.cx.saturating_mul(index as i32));
        if opaque {
            fill_pixels(pixels, &bitmap, origin_x, y, size.cx, size.cy, bk_color);
        }
        for (row, bits) in glyph(character).into_iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) == 0 {
                    continue;
                }
                fill_pixels(
                    pixels,
                    &bitmap,
                    origin_x.saturating_add(column * scale_x),
                    y.saturating_add(row as i32 * scale_y),
                    scale_x,
                    scale_y,
                    text_color,
                );
            }
        }
    }
    true
}

#[win32_derive::dllexport]
pub fn Rectangle(
    ctx: &mut Context,
    hdc: HDC,
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> bool {
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return false;
    };
    let bitmap = dc.bitmap.1.clone();
    if !bitmap.is_simple() {
        return false;
    }
    let (left, right) = (left.min(right), left.max(right));
    let (top, bottom) = (top.min(bottom), top.max(bottom));
    let width = right.saturating_sub(left);
    let height = bottom.saturating_sub(top);
    if width == 0 || height == 0 {
        return true;
    }
    let fill = dc.brush.1.0.map(|c| c.to_pixel());
    let border = dc.pen.1.0.map(|c| c.to_pixel());
    let Some(pixels) = bitmap.pixels_mut(&mut ctx.memory) else {
        return false;
    };
    if let Some(fill) = fill {
        fill_pixels(pixels, &bitmap, left, top, width, height, fill);
    }
    if let Some(border) = border {
        for x in left..right {
            draw_pixel(pixels, &bitmap, x, top, border);
            draw_pixel(pixels, &bitmap, x, bottom - 1, border);
        }
        for y in top..bottom {
            draw_pixel(pixels, &bitmap, left, y, border);
            draw_pixel(pixels, &bitmap, right - 1, y, border);
        }
    }
    true
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
    let Some(size) = text_extent_for_dc(hdc, count) else {
        return false;
    };
    lpSize.write(&mut ctx.memory, size).is_some()
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
pub fn SetROP2(_ctx: &mut Context, hdc: HDC, rop2: u32) -> i32 {
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return 0;
    };
    let Ok(rop2) = R2::try_from(rop2) else {
        log::warn!("SetROP2({hdc:?}, {rop2}): unknown R2");
        return 0;
    };
    std::mem::replace(&mut dc.rop2, rop2) as i32
}

#[win32_derive::dllexport]
pub fn LineTo(ctx: &mut Context, hdc: HDC, x: i32, y: i32) -> bool {
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return false;
    };
    let bitmap = dc.bitmap();
    if !bitmap.is_simple() {
        return false;
    }
    let stride = bitmap.stride();
    let width = bitmap.width;
    let height = bitmap.height;

    let pen = dc.pen.1.0;
    // The binary raster operation combining pen and destination pixel,
    // or None when the pixel is left untouched.
    let rop = |d: u32| -> Option<u32> {
        use R2::*;
        Some(match dc.rop2 {
            NOP => return None,
            BLACK => 0,
            WHITE => 0x00ff_ffff,
            NOT => !d,
            _ => match pen.map(|p| p.as_win32()) {
                // A NULL pen draws nothing for pen-dependent operations.
                None => return None,
                Some(p) => match dc.rop2 {
                    NOTMERGEPEN => !(d | p),
                    MASKNOTPEN => d & !p,
                    NOTCOPYPEN => !p,
                    MASKPENNOT => p & !d,
                    XORPEN => d ^ p,
                    NOTMASKPEN => !(d & p),
                    MASKPEN => d & p,
                    NOTXORPEN => !(d ^ p),
                    MERGENOTPEN => d | !p,
                    COPYPEN => p,
                    MERGEPENNOT => p | !d,
                    MERGEPEN => d | p,
                    _ => return None,
                },
            },
        })
    };

    // Bresenham rasterization covering horizontal, vertical, and
    // diagonal segments, with per-pixel clipping.
    let mut x0 = dc.pos.x;
    let mut y0 = dc.pos.y;
    let dx = (x - x0).abs();
    let sx = if x0 < x { 1 } else { -1 };
    let dy = -(y - y0).abs();
    let sy = if y0 < y { 1 } else { -1 };
    let mut err = dx + dy;

    let Some(pixels) = bitmap.pixels_mut(&mut ctx.memory) else {
        return false;
    };
    loop {
        if x0 >= 0 && y0 >= 0 && (x0 as u32) < width && (y0 as u32) < height {
            let i = (y0 as u32 * stride + x0 as u32 * 4) as usize;
            let d = u32::from_le_bytes(pixels[i..][..4].try_into().unwrap());
            if let Some(v) = rop(d) {
                pixels[i..][..4].copy_from_slice(&((v & 0x00ff_ffff) | 0xff00_0000).to_le_bytes());
            }
        }
        if x0 == x && y0 == y {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }

    dc.pos = POINT { x, y };

    true
}

#[win32_derive::dllexport]
pub fn MoveToEx(ctx: &mut Context, hdc: HDC, x: i32, y: i32, lppt: Ptr<POINT>) -> bool {
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return false;
    };
    dc.pos = POINT { x, y };
    if lppt.addr != 0 && lppt.write(&mut ctx.memory, dc.pos).is_none() {
        return false;
    }
    true
}

#[win32_derive::dllexport]
pub fn SetLayout(_ctx: &mut Context, hdc: HDC, l: u32 /* DC_LAYOUT */) -> u32 {
    // RTL mirroring is not modeled, but the layout is recorded so the
    // documented "previous layout" return value stays accurate.
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return 0xFFFF_FFFF; // GDI_ERROR
    };
    std::mem::replace(&mut dc.layout, l)
}

#[win32_derive::dllexport]
pub fn SetPixel(ctx: &mut Context, hdc: HDC, x: i32, y: i32, color: COLORREF) -> COLORREF {
    let mut state = gdi32::lock();
    let Some(dc) = state.dcs.get_mut(hdc) else {
        return COLORREF(0xFFFF_FFFF); // CLR_INVALID
    };
    let bitmap = dc.bitmap();
    if !bitmap.is_simple()
        || x < 0
        || y < 0
        || x as u32 >= bitmap.width
        || y as u32 >= bitmap.height
    {
        return COLORREF(0xFFFF_FFFF); // CLR_INVALID
    }
    let Some(pixels) = bitmap.pixels_mut(&mut ctx.memory) else {
        return COLORREF(0xFFFF_FFFF); // CLR_INVALID
    };
    let i = (y as u32 * bitmap.stride() + x as u32 * 4) as usize;
    pixels[i..][..4].copy_from_slice(&color.to_pixel());
    color
}

#[cfg(test)]
mod tests {
    use super::{COLORREF, Font, SIZE, SetPixel, text_extent};
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

    #[test]
    fn set_pixel_rejects_out_of_bounds_bitmap() {
        let mut ctx = context();
        // 2x2 bitmap at 0x3FFC needs 16 bytes, extending past 0x4000.
        let bitmap = gdi32::Bitmap::new_simple(2, 2, 0x3FFC);
        let hdc = gdi32::lock().new_memory_dc(bitmap);
        let result = SetPixel(&mut ctx, hdc, 0, 0, COLORREF::from_rgb(0, 0, 0));
        assert_eq!(result.as_win32(), 0xFFFF_FFFF);
    }
}
