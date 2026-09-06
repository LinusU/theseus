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
    // Rops that read no source pixels never consult hdcSrc, which may be
    // null for them.
    match rop {
        0x0000_0042 | 0x00ff_0062 | 0x0055_0009 => {
            // BLACKNESS / WHITENESS / DSTINVERT
            return rop_fill(ctx, hdcDest, xDest, yDest, wDest, hDest, rop);
        }
        0x00cc_0020 => {} // SRCCOPY
        _ => return false,
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

    if wDest == wSrc && hDest == hSrc {
        // GDI clips a partially offscreen blit rather than failing it:
        // compute the pixels skipped at the near edges and the shared
        // length that remains, in i64 so a negative or huge start
        // coordinate cannot wrap the check.
        let x_skip = (-(xDest as i64)).max(-(xSrc as i64)).max(0);
        let y_skip = (-(yDest as i64)).max(-(ySrc as i64)).max(0);
        let width = (wDest as i64 - x_skip)
            .min(bmp_dst.width as i64 - xDest as i64 - x_skip)
            .min(bmp_src.width as i64 - xSrc as i64 - x_skip);
        let height = (hDest as i64 - y_skip)
            .min(bmp_dst.height as i64 - yDest as i64 - y_skip)
            .min(bmp_src.height as i64 - ySrc as i64 - y_skip);
        if width <= 0 || height <= 0 {
            // A fully clipped blit is well-formed but draws nothing.
            return true;
        }
        let (w, h) = (width as u32, height as u32);
        let x_src = (xSrc as i64 + x_skip) as u32;
        let y_src = (ySrc as i64 + y_skip) as u32;
        let x_dst = (xDest as i64 + x_skip) as u32;
        let y_dst = (yDest as i64 + y_skip) as u32;

        for y in 0..h {
            let dst = &mut pixels_dst
                [(y_dst + y) as usize * bmp_dst.stride() as usize + x_dst as usize * 4..]
                [..w as usize * 4];
            let y_src = y_src + y;
            bmp_src.read_pixels(
                pixels_src,
                if bmp_src.is_bottom_up {
                    // GDI coordinates are top-down; a bottom-up source's
                    // last buffer row is the image's first.
                    bmp_src.height - y_src - 1
                } else {
                    y_src
                },
                x_src,
                x_src + w,
                dst,
            );
        }
        return true;
    }

    // A different-size blit scales the source: clip the destination to its
    // bitmap, then nearest-neighbor map each remaining pixel back into the
    // source. Samples that land outside the source are left undrawn.
    if wSrc <= 0 || hSrc <= 0 || wDest <= 0 || hDest <= 0 {
        return false;
    }
    let x_skip = (-(xDest as i64)).max(0);
    let y_skip = (-(yDest as i64)).max(0);
    let width = (wDest as i64 - x_skip).min(bmp_dst.width as i64 - xDest as i64 - x_skip);
    let height = (hDest as i64 - y_skip).min(bmp_dst.height as i64 - yDest as i64 - y_skip);
    if width <= 0 || height <= 0 {
        return true;
    }
    let x_dst = (xDest as i64 + x_skip) as u32;
    for dy in 0..height {
        let y_dst = (yDest as i64 + y_skip + dy) as u32;
        let sy = ySrc as i64 + (y_skip + dy) * hSrc as i64 / hDest as i64;
        if !(0..bmp_src.height as i64).contains(&sy) {
            continue;
        }
        let row = if bmp_src.is_bottom_up {
            (bmp_src.height as i64 - 1 - sy) as u32
        } else {
            sy as u32
        };
        let dst = &mut pixels_dst
            [(y_dst as usize * bmp_dst.stride() as usize + x_dst as usize * 4)..]
            [..width as usize * 4];
        for i in 0..width {
            let sx = xSrc as i64 + (x_skip + i) * wSrc as i64 / wDest as i64;
            if !(0..bmp_src.width as i64).contains(&sx) {
                continue;
            }
            bmp_src.read_pixels(
                pixels_src,
                row,
                sx as u32,
                sx as u32 + 1,
                &mut dst[i as usize * 4..][..4],
            );
        }
    }

    true
}

/// The fill-style raster ops (BLACKNESS, WHITENESS, DSTINVERT) ignore the
/// source DC; the destination rect is clipped to the bitmap the same way
/// the copy path clips.
fn rop_fill(
    ctx: &mut Context,
    hdc: HDC,
    x_dest: i32,
    y_dest: i32,
    w: i32,
    h: i32,
    rop: u32,
) -> bool {
    let state = gdi32::lock();
    let Some(dc) = state.dcs.get(hdc) else {
        return false;
    };
    let bmp = &dc.bitmap.1;
    if !bmp.is_simple() {
        return false;
    }
    let x_skip = (-(x_dest as i64)).max(0);
    let y_skip = (-(y_dest as i64)).max(0);
    let w = (w as i64 - x_skip).min(bmp.width as i64 - x_dest as i64 - x_skip);
    let h = (h as i64 - y_skip).min(bmp.height as i64 - y_dest as i64 - y_skip);
    if w <= 0 || h <= 0 {
        // A fully clipped fill is well-formed but draws nothing.
        return true;
    }
    let Some(pixels) = ctx.memory.bytes.get_mut(bmp.pixels_range()) else {
        return false;
    };
    let x = (x_dest as i64 + x_skip) as u32;
    let y = (y_dest as i64 + y_skip) as u32;
    for row in 0..h as u32 {
        let dst = &mut pixels[((y + row) * bmp.stride() + x * 4) as usize..][..w as usize * 4];
        match rop {
            0x0000_0042 => dst.fill(0),
            0x00ff_0062 => dst.fill(0xff),
            _ => dst.iter_mut().for_each(|b| *b = !*b),
        }
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

    #[test]
    fn stretch_blt_reads_a_bottom_up_subregion() {
        let mut ctx = context();
        // A 1x4 32bpp bottom-up source at 0x2000: buffer row r holds the
        // image's r-th row up from the bottom.
        for row in 0..4u32 {
            ctx.memory.write::<u32>(0x2000 + row * 4, 0x100 + row);
        }
        let src = crate::bitmap_format::Bitmap {
            width: 1,
            height: 4,
            is_bottom_up: true,
            bit_count: 32,
            palette: Box::new([]),
            pixels: 0x2000,
        };
        let src_dc = gdi32::lock().new_memory_dc(src);
        let dst_dc = gdi32::lock().new_memory_dc(gdi32::Bitmap::new_simple(1, 1, 0x3000));
        // Image row 2 in top-down GDI coordinates is buffer row 4-2-1 = 1.
        assert!(StretchBlt(
            &mut ctx, dst_dc, 0, 0, 1, 1, src_dc, 0, 2, 1, 1, 0xcc0020
        ));
        assert_eq!(ctx.memory.read::<u32>(0x3000), 0x101);
    }

    #[test]
    fn stretch_blt_supports_the_no_source_rops() {
        let mut ctx = context();
        let hdc = gdi32::lock().new_memory_dc(gdi32::Bitmap::new_simple(2, 1, 0x3000));
        ctx.memory.write::<u32>(0x3000, 0x11223344);
        // BLACKNESS fills without consulting the (null) source DC.
        assert!(StretchBlt(
            &mut ctx,
            hdc,
            0,
            0,
            2,
            1,
            crate::HANDLE::null(),
            0,
            0,
            2,
            1,
            0x42
        ));
        assert_eq!(ctx.memory.read::<u32>(0x3000), 0);
        assert_eq!(ctx.memory.read::<u32>(0x3004), 0);
        // WHITENESS fills with 0xff, and DSTINVERT flips it back to zero.
        assert!(StretchBlt(
            &mut ctx,
            hdc,
            0,
            0,
            2,
            1,
            crate::HANDLE::null(),
            0,
            0,
            2,
            1,
            0xff0062
        ));
        assert_eq!(ctx.memory.read::<u32>(0x3000), 0xffff_ffff);
        assert!(StretchBlt(
            &mut ctx,
            hdc,
            0,
            0,
            2,
            1,
            crate::HANDLE::null(),
            0,
            0,
            2,
            1,
            0x550009
        ));
        assert_eq!(ctx.memory.read::<u32>(0x3000), 0);
    }

    #[test]
    fn stretch_blt_clips_negative_destination_coordinates() {
        let mut ctx = context();
        // A 3x1 32bpp top-down source at 0x2000.
        for (col, value) in [0xaau32, 0xbb, 0xcc].iter().enumerate() {
            ctx.memory.write::<u32>(0x2000 + col as u32 * 4, *value);
        }
        let src_dc = gdi32::lock().new_memory_dc(gdi32::Bitmap::new_simple(3, 1, 0x2000));
        let dst_dc = gdi32::lock().new_memory_dc(gdi32::Bitmap::new_simple(2, 1, 0x3000));
        ctx.memory.bytes[0x3000..0x3008].fill(0x77);
        // xDest=-1 with w=2: only source column 1 lands on destination
        // column 0, and the rest of the destination is left alone.
        assert!(StretchBlt(
            &mut ctx, dst_dc, -1, 0, 2, 1, src_dc, 0, 0, 2, 1, 0xcc0020
        ));
        assert_eq!(ctx.memory.read::<u32>(0x3000), 0xbb);
        assert_eq!(ctx.memory.read::<u32>(0x3004), 0x77777777);
    }

    #[test]
    fn stretch_blt_scales_a_source_nearest_neighbor() {
        let mut ctx = context();
        ctx.memory.write::<u32>(0x2000, 0x11);
        ctx.memory.write::<u32>(0x2004, 0x22);
        let src_dc = gdi32::lock().new_memory_dc(gdi32::Bitmap::new_simple(2, 1, 0x2000));
        let dst_dc = gdi32::lock().new_memory_dc(gdi32::Bitmap::new_simple(4, 1, 0x3000));
        // A 2-wide source doubles to 4 destination pixels.
        assert!(StretchBlt(
            &mut ctx, dst_dc, 0, 0, 4, 1, src_dc, 0, 0, 2, 1, 0xcc0020
        ));
        assert_eq!(ctx.memory.read::<u32>(0x3000), 0x11);
        assert_eq!(ctx.memory.read::<u32>(0x3004), 0x11);
        assert_eq!(ctx.memory.read::<u32>(0x3008), 0x22);
        assert_eq!(ctx.memory.read::<u32>(0x300c), 0x22);
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

    #[test]
    fn set_di_bits_reads_a_bottom_up_subregion() {
        let mut ctx = context();
        let hdc = gdi32::lock().new_memory_dc(gdi32::Bitmap::new_simple(1, 1, 0x2000));
        // A 1x2 32bpp bottom-up DIB: header at 0x100, pixels at 0x300 with
        // buffer row 0 holding the image's bottom row.
        let mut header = [0u8; 40];
        header[0..4].copy_from_slice(&40u32.to_le_bytes()); // biSize
        header[4..8].copy_from_slice(&1u32.to_le_bytes()); // biWidth
        header[8..12].copy_from_slice(&2u32.to_le_bytes()); // biHeight
        header[12..14].copy_from_slice(&1u16.to_le_bytes()); // biPlanes
        header[14..16].copy_from_slice(&32u16.to_le_bytes()); // biBitCount
        ctx.memory.bytes[0x100..0x100 + 40].copy_from_slice(&header);
        ctx.memory.write::<u32>(0x300, 0xbb); // image row 0 (bottom)
        ctx.memory.write::<u32>(0x304, 0xaa); // image row 1 (top)
        // ySrc=1 selects the top row of the image: buffer row 1.
        assert_eq!(
            SetDIBitsToDevice(
                &mut ctx,
                hdc,
                0,
                0,
                1,
                1,
                0,
                1,
                0,
                1,
                Ptr::new(0x300),
                Ptr::new(0x100),
                0
            ),
            1
        );
        assert_eq!(ctx.memory.read::<u32>(0x2000), 0xaa);
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
    let Some(header) = ctx.memory.bytes.get(lpbmi.addr as usize..) else {
        return 0;
    };
    let (bmp_src, _) = Bitmap::parse(header);

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

    // Widen to u64 so a huge start coordinate cannot wrap past the check.
    if xSrc as u64 + w as u64 > bmp_src.width as u64
        || ySrc as u64 + h as u64 > bmp_src.height as u64
        || xDest as u64 + w as u64 > bmp_dst.width as u64
        || yDest as u64 + h as u64 > bmp_dst.height as u64
    {
        return 0;
    }

    // The read spans the DIB rows the blit touches: lpvBits is the start of
    // the pixel array and ySrc + h is the last row read, so a nonzero ySrc
    // needs more than h rows of buffer.
    let src_end = lpvBits.addr as u64 + ((ySrc + h) as u64 * bmp_src.stride() as u64);
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
        let dst = &mut pixels_dst
            [(yDest + y) as usize * bmp_dst.stride() as usize + xDest as usize * 4..]
            [..w as usize * 4];
        let y_src = if bmp_src.is_bottom_up {
            // ySrc counts up from the bottom of a bottom-up DIB, so the
            // destination's first row reads the top of the region.
            ySrc + h - 1 - y
        } else {
            ySrc + y
        };
        bmp_src.read_pixels(pixels_src, y_src, xSrc, xSrc + w, dst);
    }

    cLines
}
