//! The bitmap file/memory format and pixel buffers.

use zerocopy::FromBytes;

use crate::{FromABIParam, gdi32::COLORREF};

#[derive(Debug, Eq, PartialEq, win32_derive::ABIEnum)]
pub enum BI {
    RGB = 0,
    RLE8 = 1,
    RLE4 = 2,
    BITFIELDS = 3,
    JPEG = 4,
    PNG = 5,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct BITMAPFILEHEADER {
    pub bfType: u16,
    pub bfSize: u32,
    pub bfReserved1: u16,
    pub bfReserved2: u16,
    pub bfOffBits: u32,
}

#[repr(C)]
#[derive(Debug, Clone, zerocopy::FromBytes)]
pub struct BITMAPCOREHEADER {
    pub bcSize: u32,
    pub bcWidth: u16,
    pub bcHeight: u16,
    pub bcPlanes: u16,
    pub bcBitCount: u16,
}
impl BITMAPCOREHEADER {
    pub fn stride(&self) -> usize {
        // Bitmap row stride is padded out to 4 bytes per row.
        ((((self.bcWidth * self.bcBitCount) as usize) + 31) & !31) >> 3
    }
}

#[repr(C)]
#[derive(Debug, Clone, zerocopy::FromBytes)]
pub struct BITMAPINFOHEADER {
    pub biSize: u32,
    pub biWidth: u32,
    pub biHeight: u32,
    pub biPlanes: u16,
    pub biBitCount: u16,
    pub biCompression: u32,
    pub biSizeImage: u32,
    pub biXPelsPerMeter: u32,
    pub biYPelsPerMeter: u32,
    pub biClrUsed: u32,
    pub biClrImportant: u32,
}

impl BITMAPINFOHEADER {
    pub fn width(&self) -> u32 {
        self.biWidth
    }

    pub fn stride(&self) -> usize {
        // Bitmap row stride is padded out to 4 bytes per row.
        (((self.biWidth as usize * self.biBitCount as usize) + 31) & !31) >> 3
    }

    pub fn height(&self) -> u32 {
        // Height is negative if top-down DIB.
        (self.biHeight as i32).unsigned_abs()
    }

    pub fn is_bottom_up(&self) -> bool {
        (self.biHeight as i32) > 0
    }

    pub fn compression(&self) -> BI {
        BI::from_abi(self.biCompression)
    }
}

pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub is_bottom_up: bool,
    pub bit_count: u8,
    pub palette: Box<[COLORREF]>,
    pub pixels: u32,
}

impl std::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Bitmap {{ {w}x{h} {bpp}bpp bottom_up:{is_bottom_up} palette:{entries} pixels:{pixels:0x} }}",
            w = self.width,
            h = self.height,
            bpp = self.bit_count,
            is_bottom_up = self.is_bottom_up,
            entries = self.palette.len(),
            pixels = self.pixels,
        )
    }
}

impl Bitmap {
    /// A "simple" bitmap is RGBA top-down.
    pub fn new_simple(width: u32, height: u32, pixels: u32) -> Self {
        Self {
            width,
            height,
            is_bottom_up: false,
            bit_count: 32,
            palette: Box::new([]),
            pixels,
        }
    }

    pub fn is_simple(&self) -> bool {
        !self.is_bottom_up && self.bit_count == 32 && self.palette.is_empty()
    }

    pub fn stride(&self) -> u32 {
        // Bitmap row stride is padded out to 4 bytes per row.
        (((self.width * self.bit_count as u32) + 31) & !31) / 8
    }

    pub fn pixels_len(&self) -> usize {
        (self.height * self.stride()) as usize
    }

    pub fn pixels_range(&self) -> std::ops::Range<usize> {
        self.pixels as usize..self.pixels as usize + self.pixels_len()
    }

    pub fn pixels_mut<'a>(&self, memory: &'a mut runtime::Memory) -> Option<&'a mut [u8]> {
        memory.bytes.get_mut(self.pixels_range())
    }

    /// A degenerate bitmap returned when a header can't be parsed: no pixels
    /// and no dimensions, so readers get empty output rather than a panic.
    fn degenerate() -> (Self, &'static [u8]) {
        (
            Bitmap {
                width: 0,
                height: 0,
                is_bottom_up: false,
                bit_count: 32,
                palette: Box::new([]),
                pixels: 0,
            },
            &[],
        )
    }

    // TODO: when parsing a bitmap from memory it's unclear how much memory we'll need
    // to read until we've read the bitmap header.  This means the caller cannot know how
    // big of a slice to provide.
    pub fn parse(buf: &[u8]) -> (Self, &[u8]) {
        use zerocopy::FromBytes;
        let Some((header_size, _)) = <u32>::read_from_prefix(buf).ok() else {
            return Self::degenerate();
        };
        match header_size {
            12 => {
                let Some((header, rest)) = BITMAPCOREHEADER::read_from_prefix(buf).ok() else {
                    return Self::degenerate();
                };
                Self::parseBMPv2(&header, rest)
            }
            // V3 and later headers share the BITMAPINFOHEADER prefix.
            40.. => {
                let Some((header, _)) = BITMAPINFOHEADER::read_from_prefix(buf).ok() else {
                    return Self::degenerate();
                };
                Self::parseBMPv3(&header, &buf[header_size as usize..])
            }
            _ => {
                log::warn!("unsupported bitmap header size {header_size}");
                Self::degenerate()
            }
        }
    }

    /// buf is the bytes following the header.
    fn parseBMPv2<'a>(header: &BITMAPCOREHEADER, buf: &'a [u8]) -> (Self, &'a [u8]) {
        let palette_len = if header.bcBitCount <= 8 {
            2usize.pow(header.bcBitCount as u32)
        } else {
            0 // >8bpp core bitmaps have no color table
        };
        let Some((palette, buf)) = <[[u8; 3]]>::ref_from_prefix_with_elems(buf, palette_len).ok()
        else {
            return Self::degenerate();
        };
        let palette = palette
            .iter()
            .map(|&[b, g, r]| COLORREF::from_rgb(r, g, b))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let need = header.bcHeight as usize * header.stride();
        if buf.len() < need {
            return Self::degenerate();
        }
        let pixels = &buf[..need];
        let bitmap = Bitmap {
            width: header.bcWidth as u32,
            height: header.bcHeight as u32,
            is_bottom_up: true, // MSDN: "BITMAPCOREHEADER bitmaps cannot be top-down bitmaps"
            bit_count: header.bcBitCount as u8,
            palette,
            pixels: 0,
        };
        (bitmap, pixels)
    }

    /// buf is the bytes following the header.
    fn parseBMPv3<'a>(header: &BITMAPINFOHEADER, buf: &'a [u8]) -> (Self, &'a [u8]) {
        let mut buf = buf;
        match header.biCompression {
            x if x == BI::RGB as u32 => {}
            x if x == BI::BITFIELDS as u32 => {
                // Three u32 channel masks precede the palette; the reader
                // assumes the usual RGB555 layout for 16bpp data.
                let Some((_, rest)) = <[u32]>::ref_from_prefix_with_elems(buf, 3).ok() else {
                    return Self::degenerate();
                };
                buf = rest;
            }
            compression => {
                log::warn!("unsupported bitmap compression {compression}");
                return Self::degenerate();
            }
        }
        let palette_len = if header.biClrUsed > 0 {
            header.biClrUsed as usize
        } else if header.biBitCount <= 8 {
            2usize.pow(header.biBitCount as u32)
        } else {
            0 // >8bpp BI_RGB bitmaps have no color table
        };

        let Some((palette, buf)) = <[[u8; 4]]>::ref_from_prefix_with_elems(buf, palette_len).ok()
        else {
            return Self::degenerate();
        };
        let palette = palette
            .iter()
            .map(|&[b, g, r, _]| COLORREF::from_rgb(r, g, b))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let need = header.height() as usize * header.stride();
        let pixels = buf.get(..need).unwrap_or(buf);
        let bitmap = Bitmap {
            width: header.biWidth,
            height: header.height(),
            is_bottom_up: header.is_bottom_up(),
            bit_count: header.biBitCount as u8,
            palette,
            pixels: 0,
        };
        (bitmap, pixels)
    }

    pub fn read_pixels(&self, pixels: &[u8], y: u32, x1: u32, x2: u32, dst: &mut [u8]) {
        // Degenerate or truncated bitmaps may not have a full row available.
        if x2 > self.width || pixels.len() < (y + 1) as usize * self.stride() as usize {
            return;
        }
        let row_pixels = (x2 - x1) as usize;
        let row_bytes = row_pixels * 4;
        if dst.len() < row_bytes {
            return;
        }
        let palette = |index: usize| {
            self.palette
                .get(index)
                .copied()
                .unwrap_or(COLORREF::from_rgb(0, 0, 0))
        };
        match self.bit_count {
            32 => {
                let len = ((x2 - x1) * 4) as usize;
                dst[..len].copy_from_slice(&pixels[(y * self.stride() + x1 * 4) as usize..][..len]);
            }
            8 => {
                let src = &pixels[(y * self.stride()) as usize..];
                for (srci, dsti) in (x1..x2).zip((0..).step_by(4)) {
                    let color = palette(src[srci as usize] as usize);
                    dst[dsti..][..4].copy_from_slice(&color.to_pixel());
                }
            }
            4 => {
                let src = &pixels[(y * self.stride()) as usize..];
                for (srci, dsti) in (x1..x2).zip((0..).step_by(4)) {
                    let color = palette(if srci % 2 == 0 {
                        src[(srci / 2) as usize] >> 4
                    } else {
                        src[(srci / 2) as usize] & 0xf
                    } as usize);
                    dst[dsti..][..4].copy_from_slice(&color.to_pixel());
                }
            }
            1 => {
                let src = &pixels[(y * self.stride()) as usize..];
                for (srci, dsti) in (x1..x2).zip((0..).step_by(4)) {
                    let bit = 7 - (srci % 8);
                    let color = palette(((src[(srci / 8) as usize] >> bit) & 1) as usize);
                    dst[dsti..][..4].copy_from_slice(&color.to_pixel());
                }
            }
            16 => {
                // BI_RGB 16bpp stores pixels as RGB555.
                let src = &pixels[(y * self.stride()) as usize..];
                for (srci, dsti) in (x1..x2).zip((0..).step_by(4)) {
                    let v =
                        u16::from_le_bytes([src[srci as usize * 2], src[srci as usize * 2 + 1]]);
                    let [r5, g5, b5] = [(v >> 10) & 0x1f, (v >> 5) & 0x1f, v & 0x1f];
                    let color =
                        COLORREF::from_rgb((r5 << 3) as u8, (g5 << 3) as u8, (b5 << 3) as u8);
                    dst[dsti..][..4].copy_from_slice(&color.to_pixel());
                }
            }
            24 => {
                let src = &pixels[(y * self.stride()) as usize..];
                for (srci, dsti) in (x1..x2).zip((0..).step_by(4)) {
                    let color = if let Some([b, g, r]) = src
                        .get(srci as usize * 3..)
                        .and_then(|s| s.get(..3))
                        .and_then(|s| <[u8; 3]>::try_from(s).ok())
                    {
                        COLORREF::from_rgb(r, g, b)
                    } else {
                        COLORREF::default()
                    };
                    dst[dsti..][..4].copy_from_slice(&color.to_pixel());
                }
            }
            bit_count => {
                log::warn!("unsupported bitmap bit count {bit_count}; writing black");
                let len = ((x2 - x1) * 4) as usize;
                let dst_len = dst.len();
                dst[..len.min(dst_len)].fill(0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Bitmap;

    fn core_header(bc_bit_count: u16) -> Vec<u8> {
        vec![
            0x0c, 0x00, 0x00, 0x00, // bcSize = 12
            0x01, 0x00, // bcWidth = 1
            0x01, 0x00, // bcHeight = 1
            0x01, 0x00, // bcPlanes = 1
            bc_bit_count as u8,
            (bc_bit_count >> 8) as u8, // bcBitCount
        ]
    }

    #[test]
    fn parse_rejects_truncated_core_header() {
        // A buffer that claims a 12-byte BITMAPCOREHEADER but is too short.
        let buf = [0x0c, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00];
        let (bmp, _) = Bitmap::parse(&buf);
        assert_eq!(bmp.width, 0);
        assert_eq!(bmp.height, 0);
    }

    #[test]
    fn parse_rejects_core_8bpp_without_palette() {
        // 8bpp needs 256*3 palette bytes after the 12-byte header.
        let (bmp, _) = Bitmap::parse(&core_header(8));
        assert_eq!(bmp.width, 0);
        assert_eq!(bmp.height, 0);
    }

    #[test]
    fn parse_rejects_core_24bpp_without_pixels() {
        // 24bpp needs 1*4=4 pixel bytes after the (empty) palette.
        let (bmp, _) = Bitmap::parse(&core_header(24));
        assert_eq!(bmp.width, 0);
        assert_eq!(bmp.height, 0);
    }

    #[test]
    fn read_pixels_rejects_out_of_range_x2() {
        let bmp = Bitmap::new_simple(1, 1, 0);
        let pixels = [0xff, 0x00, 0x00, 0xff];
        let mut dst = [0u8; 8];
        // x2=2 exceeds width=1; should return without panic.
        bmp.read_pixels(&pixels, 0, 0, 2, &mut dst);
        assert_eq!(dst, [0u8; 8]);
    }

    #[test]
    fn read_pixels_rejects_short_dst() {
        let bmp = Bitmap::new_simple(1, 1, 0);
        let pixels = [0xff, 0x00, 0x00, 0xff];
        let mut dst = [0u8; 2];
        // dst is shorter than one 32bpp pixel; should return without panic.
        bmp.read_pixels(&pixels, 0, 0, 1, &mut dst);
        assert_eq!(dst, [0u8; 2]);
    }

    #[test]
    fn read_pixels_24bpp_decodes_a_valid_pixel() {
        let bmp = Bitmap {
            width: 1,
            height: 1,
            is_bottom_up: true,
            bit_count: 24,
            palette: Box::new([]),
            pixels: 0,
        };
        // 24bpp BGR, stride padded to 4 bytes.
        let pixels = [0x00, 0x00, 0xff, 0x00];
        let mut dst = [0u8; 4];
        bmp.read_pixels(&pixels, 0, 0, 1, &mut dst);
        assert_eq!(dst, [0xff, 0x00, 0x00, 0xff]);
    }

    #[test]
    fn read_pixels_24bpp_rejects_x2_past_width() {
        let bmp = Bitmap {
            width: 1,
            height: 1,
            is_bottom_up: true,
            bit_count: 24,
            palette: Box::new([]),
            pixels: 0,
        };
        let pixels = [0x00, 0x00, 0xff, 0x00];
        let mut dst = [0u8; 8];
        bmp.read_pixels(&pixels, 0, 0, 2, &mut dst);
        assert_eq!(dst, [0u8; 8]);
    }
}
