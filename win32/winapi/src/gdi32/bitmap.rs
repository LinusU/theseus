use std::sync::Arc;

use runtime::Context;

pub use crate::bitmap_format::Bitmap;
use crate::{
    HANDLE, Ptr,
    gdi32::{self, HDC, Object, State},
    kernel32,
};

impl State {
    pub fn new_bitmap_handle(&mut self, bitmap: Bitmap) -> (HBITMAP, Arc<Bitmap>) {
        let bitmap = Arc::new(bitmap);
        let hbitmap = self.objects.add(Object::Bitmap(bitmap.clone()));
        (hbitmap, bitmap)
    }
}

#[win32_derive::dllexport]
pub fn BitBlt(
    ctx: &mut Context,
    hdc: HDC,
    x: i32,
    y: i32,
    cx: i32,
    cy: i32,
    hdcSrc: HDC,
    x1: i32,
    y1: i32,
    rop: u32, /* ROP_CODE */
) -> bool {
    StretchBlt(ctx, hdc, x, y, cx, cy, hdcSrc, x1, y1, cx, cy, rop)
}

#[win32_derive::dllexport]
pub fn StretchBlt(
    ctx: &mut Context,
    hdcDest: HDC,
    xDest: i32,
    yDest: i32,
    wDest: i32,
    hDest: i32,
    hdcSrc: HDC,
    xSrc: i32,
    ySrc: i32,
    wSrc: i32,
    hSrc: i32,
    rop: u32, /* ROP_CODE */
) -> bool {
    if rop != 0xcc0020 {
        return false;
    }

    let state = gdi32::lock();
    let Some(dc_src) = state.dcs.get(hdcSrc) else {
        return false;
    };
    let bmp_src = &dc_src.bitmap.1;

    let Some(dc_dst) = state.dcs.get(hdcDest) else {
        return false;
    };
    let bmp_dst = &dc_dst.bitmap.1;
    if !bmp_dst.is_simple() {
        return false;
    }

    let Ok([pixels_src, pixels_dst]) = ctx
        .memory
        .bytes
        .get_disjoint_mut([bmp_src.pixels_range(), bmp_dst.pixels_range()])
    else {
        return false;
    };

    // stretching not implemented yet
    if wDest != wSrc || hDest != hSrc {
        return false;
    }

    let xSrc = xSrc as u32;
    let ySrc = ySrc as u32;
    let xDst = xDest as u32;
    let yDst = yDest as u32;
    let wSrc = wSrc as u32;
    let hSrc = hSrc as u32;
    let wDst = wDest as u32;
    if xSrc + wSrc > bmp_src.width
        || ySrc + hSrc > bmp_src.height
        || xDst + wDst > bmp_dst.width
        || yDst + hSrc > bmp_dst.height
    {
        return false;
    }

    for y in 0..hDest as u32 {
        let dst = &mut pixels_dst[(((yDst + y) * bmp_dst.stride()) + (xDst * 4)) as usize..]
            [..wDst as usize * 4];
        let y_src = ySrc + y;
        bmp_src.read_pixels(
            pixels_src,
            if bmp_src.is_bottom_up {
                hSrc - y_src - 1
            } else {
                y_src
            },
            xSrc,
            xSrc + wSrc,
            dst,
        );
    }

    true
}

pub type HBITMAP = HANDLE;

#[cfg(test)]
mod tests {
    use super::{SetDIBitsToDevice, StretchBlt};
    use crate::{Ptr, gdi32};
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
    fn stretch_blt_rejects_out_of_bounds_bitmap() {
        let mut ctx = context();
        // A 2x2 32bpp bitmap at 0x3FFC needs 16 bytes, extending past 0x4000.
        let bitmap = gdi32::Bitmap::new_simple(2, 2, 0x3FFC);
        let hdc = gdi32::lock().new_memory_dc(bitmap);
        assert!(!StretchBlt(
            &mut ctx, hdc, 0, 0, 1, 1, hdc, 0, 0, 1, 1, 0xcc0020
        ));
    }

    fn dib_info32(ctx: &mut Context, addr: u32) {
        let header: [u8; 40] = [
            0x28, 0x00, 0x00, 0x00, // biSize = 40
            0x01, 0x00, 0x00, 0x00, // biWidth = 1
            0x01, 0x00, 0x00, 0x00, // biHeight = 1
            0x01, 0x00, // biPlanes = 1
            0x20, 0x00, // biBitCount = 32
            0x00, 0x00, 0x00, 0x00, // biCompression = 0
            0x00, 0x00, 0x00, 0x00, // biSizeImage = 0
            0x00, 0x00, 0x00, 0x00, // biXPelsPerMeter = 0
            0x00, 0x00, 0x00, 0x00, // biYPelsPerMeter = 0
            0x00, 0x00, 0x00, 0x00, // biClrUsed = 0
            0x00, 0x00, 0x00, 0x00, // biClrImportant = 0
        ];
        ctx.memory.bytes[addr as usize..addr as usize + 40].copy_from_slice(&header);
    }

    #[test]
    fn set_di_bits_rejects_out_of_bounds_source() {
        let mut ctx = context();
        // Destination: 2x2 bitmap with plenty of room.
        let hdc = gdi32::lock().new_memory_dc(gdi32::Bitmap::new_simple(2, 2, 0x1000));
        // DIB header at 0x100 describes a 1x1 32bpp bottom-up bitmap.
        dib_info32(&mut ctx, 0x100);
        // Source bits at 0x3FFD need 4 bytes, extending one byte past 0x4000.
        let lpvBits = Ptr::new(0x3FFD);
        let lpbmi = Ptr::new(0x100);
        assert_eq!(
            SetDIBitsToDevice(&mut ctx, hdc, 0, 0, 1, 1, 0, 0, 0, 1, lpvBits, lpbmi, 0),
            0
        );
    }
}

#[win32_derive::dllexport]
pub fn CreateCompatibleBitmap(ctx: &mut Context, _hdc: HDC, cx: i32, cy: i32) -> HBITMAP {
    // Negative or zero dimensions and overflowing or unallocatable sizes
    // all report failure with a null bitmap handle.
    let (Ok(w), Ok(h)) = (u32::try_from(cx), u32::try_from(cy)) else {
        return HANDLE::null();
    };
    let Some(size) = w.checked_mul(h).and_then(|px| px.checked_mul(4)) else {
        return HANDLE::null();
    };
    let Some(pixels) = kernel32::lock()
        .process_heap
        .try_alloc(&mut ctx.memory, size.max(4))
    else {
        return HANDLE::null();
    };
    let bitmap = Bitmap::new_simple(w, h, pixels);
    gdi32::lock().new_bitmap_handle(bitmap).0
}

#[win32_derive::dllexport]
pub fn SetDIBitsToDevice(
    ctx: &mut Context,
    hdc: HDC,
    xDest: u32,
    yDest: u32,
    w: u32,
    h: u32,
    xSrc: u32,
    ySrc: u32,
    StartScan: u32,
    cLines: u32,
    lpvBits: Ptr<u8>,
    lpbmi: Ptr<u8>, /* BITMAPINFO */
    ColorUse: u32,  /* DIB_USAGE */
) -> u32 {
    let (bmp_src, _) = Bitmap::parse(&ctx.memory[lpbmi.addr..]);

    if StartScan != 0 || ColorUse != 0 || cLines != h {
        return 0;
    }
    if bmp_src.width == 0 || bmp_src.height == 0 {
        return 0;
    }

    let state = gdi32::lock();
    let Some(dc_dst) = state.dcs.get(hdc) else {
        return 0;
    };
    let bmp_dst = &dc_dst.bitmap.1;
    if !bmp_dst.is_simple() {
        return 0;
    }

    if xSrc + w > bmp_src.width
        || ySrc + h > bmp_src.height
        || xDest + w > bmp_dst.width
        || yDest + h > bmp_dst.height
    {
        return 0;
    }

    let src_end = lpvBits.addr as u64 + (h as u64 * bmp_src.stride() as u64);
    let Ok([pixels_src, pixels_dst]) = ctx.memory.bytes.get_disjoint_mut([
        lpvBits.addr as usize..src_end as usize,
        bmp_dst.pixels_range(),
    ]) else {
        return 0;
    };

    // for i in (0..pixels_src.len()).step_by(bmp_src.stride() as usize) {
    //     log::info!("{:x?}", &pixels_src[i..][..bmp_src.stride() as usize]);
    // }

    for y in 0..h {
        let dst = &mut pixels_dst[((yDest + y) * bmp_dst.stride() + xDest * 4) as usize..]
            [..w as usize * 4];
        let y_src = ySrc + y;
        bmp_src.read_pixels(
            pixels_src,
            if bmp_src.is_bottom_up {
                h - y_src - 1
            } else {
                y_src
            },
            xSrc,
            xSrc + w,
            dst,
        );
    }

    cLines
}
