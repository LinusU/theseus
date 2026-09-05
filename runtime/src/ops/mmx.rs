trait Unpack<T> {
    fn unpack(self) -> T;
}

impl Unpack<[u32; 2]> for u64 {
    fn unpack(self) -> [u32; 2] {
        [(self >> 0) as u32, (self >> 32) as u32]
    }
}

impl Unpack<[i32; 2]> for u64 {
    fn unpack(self) -> [i32; 2] {
        [(self >> 0) as i32, (self >> 32) as i32]
    }
}

impl Unpack<[u16; 4]> for u64 {
    fn unpack(self) -> [u16; 4] {
        [
            (self >> 0) as u16,
            (self >> 16) as u16,
            (self >> 32) as u16,
            (self >> 48) as u16,
        ]
    }
}

impl Unpack<[i16; 4]> for u64 {
    fn unpack(self) -> [i16; 4] {
        let x: [u16; 4] = self.unpack();
        [x[0] as i16, x[1] as i16, x[2] as i16, x[3] as i16]
    }
}

impl Unpack<[i8; 8]> for u64 {
    fn unpack(self) -> [i8; 8] {
        let x: [u8; 8] = self.unpack();
        x.map(|b| b as i8)
    }
}

impl Unpack<[u8; 8]> for u64 {
    fn unpack(self) -> [u8; 8] {
        self.to_le_bytes()
    }
}

impl Unpack<[u16; 2]> for u32 {
    fn unpack(self) -> [u16; 2] {
        [(self >> 0) as u16, (self >> 16) as u16]
    }
}

impl Unpack<[u8; 4]> for u32 {
    fn unpack(self) -> [u8; 4] {
        self.to_le_bytes()
    }
}

trait Pack {
    type Target;
    fn pack(self) -> Self::Target;
}

impl Pack for [u32; 2] {
    type Target = u64;
    fn pack(self) -> u64 {
        (self[0] as u64) | ((self[1] as u64) << 32)
    }
}

impl Pack for [i16; 4] {
    type Target = u64;
    fn pack(self) -> u64 {
        self.map(|b| b as u16).pack()
    }
}

impl Pack for [i32; 2] {
    type Target = u64;
    fn pack(self) -> u64 {
        self.map(|b| b as u32).pack()
    }
}

impl Pack for [u16; 4] {
    type Target = u64;
    fn pack(self) -> u64 {
        (self[0] as u64)
            | ((self[1] as u64) << 16)
            | ((self[2] as u64) << 32)
            | ((self[3] as u64) << 48)
    }
}

impl Pack for [u8; 8] {
    type Target = u64;
    fn pack(self) -> u64 {
        u64::from_le_bytes(self)
    }
}

impl Pack for [i8; 8] {
    type Target = u64;
    fn pack(self) -> u64 {
        self.map(|b| b as u8).pack()
    }
}

impl Pack for [u16; 2] {
    type Target = u32;
    fn pack(self) -> u32 {
        (self[0] as u32) | ((self[1] as u32) << 16)
    }
}

impl Pack for [u8; 4] {
    type Target = u32;
    fn pack(self) -> u32 {
        u32::from_le_bytes(self)
    }
}

pub fn paddsb(x: u64, y: u64) -> u64 {
    let x: [i8; 8] = x.unpack();
    let y: [i8; 8] = y.unpack();
    [
        x[0].saturating_add(y[0]),
        x[1].saturating_add(y[1]),
        x[2].saturating_add(y[2]),
        x[3].saturating_add(y[3]),
        x[4].saturating_add(y[4]),
        x[5].saturating_add(y[5]),
        x[6].saturating_add(y[6]),
        x[7].saturating_add(y[7]),
    ]
    .pack()
}

pub fn paddsw(x: u64, y: u64) -> u64 {
    let x: [i16; 4] = x.unpack();
    let y: [i16; 4] = y.unpack();
    [
        x[0].saturating_add(y[0]),
        x[1].saturating_add(y[1]),
        x[2].saturating_add(y[2]),
        x[3].saturating_add(y[3]),
    ]
    .pack()
}

pub fn paddw(x: u64, y: u64) -> u64 {
    let x: [u16; 4] = x.unpack();
    let y: [u16; 4] = y.unpack();
    [
        x[0].wrapping_add(y[0]),
        x[1].wrapping_add(y[1]),
        x[2].wrapping_add(y[2]),
        x[3].wrapping_add(y[3]),
    ]
    .pack()
}

pub fn paddb(x: u64, y: u64) -> u64 {
    let x: [u8; 8] = x.unpack();
    let y: [u8; 8] = y.unpack();
    [
        x[0].wrapping_add(y[0]),
        x[1].wrapping_add(y[1]),
        x[2].wrapping_add(y[2]),
        x[3].wrapping_add(y[3]),
        x[4].wrapping_add(y[4]),
        x[5].wrapping_add(y[5]),
        x[6].wrapping_add(y[6]),
        x[7].wrapping_add(y[7]),
    ]
    .pack()
}

pub fn paddd(x: u64, y: u64) -> u64 {
    let x: [u32; 2] = x.unpack();
    let y: [u32; 2] = y.unpack();
    [x[0].wrapping_add(y[0]), x[1].wrapping_add(y[1])].pack()
}

pub fn paddusb(x: u64, y: u64) -> u64 {
    let x: [u8; 8] = x.unpack();
    let y: [u8; 8] = y.unpack();
    [
        x[0].saturating_add(y[0]),
        x[1].saturating_add(y[1]),
        x[2].saturating_add(y[2]),
        x[3].saturating_add(y[3]),
        x[4].saturating_add(y[4]),
        x[5].saturating_add(y[5]),
        x[6].saturating_add(y[6]),
        x[7].saturating_add(y[7]),
    ]
    .pack()
}

pub fn punpcklbw(x: u32, y: u32) -> u64 {
    let x: [u8; 4] = x.unpack();
    let y: [u8; 4] = y.unpack();
    [x[0], y[0], x[1], y[1], x[2], y[2], x[3], y[3]].pack()
}

pub fn punpcklwd(x: u32, y: u32) -> u64 {
    let x: [u16; 2] = x.unpack();
    let y: [u16; 2] = y.unpack();
    [x[0], y[0], x[1], y[1]].pack()
}

pub fn punpckldq(x: u32, y: u32) -> u64 {
    (x as u64) | ((y as u64) << 32)
}

pub fn punpckhbw(x: u64, y: u64) -> u64 {
    let x: [u8; 8] = x.unpack();
    let y: [u8; 8] = y.unpack();
    [x[4], y[4], x[5], y[5], x[6], y[6], x[7], y[7]].pack()
}

pub fn punpckhwd(x: u64, y: u64) -> u64 {
    let x: [u16; 4] = x.unpack();
    let y: [u16; 4] = y.unpack();
    [x[2], y[2], x[3], y[3]].pack()
}

pub fn punpckhdq(x: u64, y: u64) -> u64 {
    let x: [u32; 2] = x.unpack();
    let y: [u32; 2] = y.unpack();
    [x[1], y[1]].pack()
}

pub fn pcmpeqb(x: u64, y: u64) -> u64 {
    let x: [u8; 8] = x.unpack();
    let y: [u8; 8] = y.unpack();
    let mut out = [0u8; 8];
    for i in 0..8 {
        out[i] = if x[i] == y[i] { u8::MAX } else { 0 };
    }
    out.pack()
}
pub fn pcmpeqw(x: u64, y: u64) -> u64 {
    let x: [u16; 4] = x.unpack();
    let y: [u16; 4] = y.unpack();
    let mut out = [0u16; 4];
    for i in 0..4 {
        out[i] = if x[i] == y[i] { u16::MAX } else { 0 };
    }
    out.pack()
}
pub fn pcmpeqd(x: u64, y: u64) -> u64 {
    let x: [u32; 2] = x.unpack();
    let y: [u32; 2] = y.unpack();
    let mut out = [0u32; 2];
    for i in 0..2 {
        out[i] = if x[i] == y[i] { u32::MAX } else { 0 };
    }
    out.pack()
}
pub fn pcmpgtb(x: u64, y: u64) -> u64 {
    let x: [i8; 8] = x.unpack();
    let y: [i8; 8] = y.unpack();
    let mut out = [0i8; 8];
    for i in 0..8 {
        out[i] = if x[i] > y[i] { -1 } else { 0 };
    }
    out.pack()
}
pub fn pcmpgtw(x: u64, y: u64) -> u64 {
    let x: [i16; 4] = x.unpack();
    let y: [i16; 4] = y.unpack();
    let mut out = [0i16; 4];
    for i in 0..4 {
        out[i] = if x[i] > y[i] { -1 } else { 0 };
    }
    out.pack()
}
pub fn pcmpgtd(x: u64, y: u64) -> u64 {
    let x: [i32; 2] = x.unpack();
    let y: [i32; 2] = y.unpack();
    let mut out = [0i32; 2];
    for i in 0..2 {
        out[i] = if x[i] > y[i] { -1 } else { 0 };
    }
    out.pack()
}

pub fn packsswb(x: u64, y: u64) -> u64 {
    fn saturate(x: i16) -> i8 {
        x.clamp(i8::MIN as i16, i8::MAX as i16) as i8
    }
    let x: [i16; 4] = x.unpack();
    let y: [i16; 4] = y.unpack();
    [
        saturate(x[0]),
        saturate(x[1]),
        saturate(x[2]),
        saturate(x[3]),
        saturate(y[0]),
        saturate(y[1]),
        saturate(y[2]),
        saturate(y[3]),
    ]
    .pack()
}

pub fn packssdw(x: u64, y: u64) -> u64 {
    fn saturate(x: i32) -> i16 {
        x.clamp(i16::MIN as i32, i16::MAX as i32) as i16
    }
    let x: [i32; 2] = x.unpack();
    let y: [i32; 2] = y.unpack();
    [
        saturate(x[0]),
        saturate(x[1]),
        saturate(y[0]),
        saturate(y[1]),
    ]
    .pack()
}

pub fn psllw(x: u64, y: u64) -> u64 {
    if y > 15 {
        return 0;
    }
    let x: [u16; 4] = x.unpack();
    [x[0] << y, x[1] << y, x[2] << y, x[3] << y].pack()
}

pub fn pslld(x: u64, y: u64) -> u64 {
    if y > 31 {
        return 0;
    }
    let x: [u32; 2] = x.unpack();
    [x[0] << y, x[1] << y].pack()
}

pub fn psllq(x: u64, y: u64) -> u64 {
    if y > 63 {
        return 0;
    }
    x << y
}

pub fn psrld(x: u64, y: u64) -> u64 {
    if y > 31 {
        return 0;
    }
    let x: [u32; 2] = x.unpack();
    [x[0] >> y, x[1] >> y].pack()
}

pub fn psrlq(x: u64, y: u64) -> u64 {
    if y > 63 {
        return 0;
    }
    x >> y
}

pub fn psrad(x: u64, y: u64) -> u64 {
    let x: [i32; 2] = x.unpack();
    let shifted = if y >= 32 {
        x.map(|lane| if lane < 0 { -1 } else { 0 })
    } else {
        x.map(|lane| lane >> y)
    };
    shifted.pack()
}

pub fn pmaddwd(x: u64, y: u64) -> u64 {
    let x: [i16; 4] = x.unpack();
    let y: [i16; 4] = y.unpack();
    [
        (x[0] as i32 * y[0] as i32).wrapping_add(x[1] as i32 * y[1] as i32),
        (x[2] as i32 * y[2] as i32).wrapping_add(x[3] as i32 * y[3] as i32),
    ]
    .pack()
}

/// PEXTRW extracts the selected word lane, zero-extended to 32 bits.
pub fn pextrw(x: u64, sel: u8) -> u32 {
    ((x >> ((sel & 3) * 16)) as u16) as u32
}

/// PINSRW replaces the selected word lane with the low word of the source.
pub fn pinsrw(x: u64, y: u16, sel: u8) -> u64 {
    let shift = (sel & 3) * 16;
    (x & !(0xffff_u64 << shift)) | ((y as u64) << shift)
}

/// PSHUFW copies word lanes selected by the four 2-bit fields in `imm`.
pub fn pshufw(x: u64, imm: u8) -> u64 {
    let x: [u16; 4] = x.unpack();
    [
        x[(imm & 3) as usize],
        x[((imm >> 2) & 3) as usize],
        x[((imm >> 4) & 3) as usize],
        x[((imm >> 6) & 3) as usize],
    ]
    .pack()
}

/// PMOVMSKB packs the sign bit of each byte lane: bit i = byte i's MSB.
pub fn pmovmskb(x: u64) -> u32 {
    x.to_le_bytes()
        .iter()
        .enumerate()
        .fold(0u32, |acc, (i, b)| acc | (((b >> 7) as u32) << i))
}

pub fn pmullw(x: u64, y: u64) -> u64 {
    let x: [u16; 4] = x.unpack();
    let y: [u16; 4] = y.unpack();
    [
        x[0].wrapping_mul(y[0]),
        x[1].wrapping_mul(y[1]),
        x[2].wrapping_mul(y[2]),
        x[3].wrapping_mul(y[3]),
    ]
    .pack()
}

pub fn psrlw(x: u64, y: u64) -> u64 {
    if y > 15 {
        return 0;
    }
    let x: [u16; 4] = x.unpack();
    [x[0] >> y, x[1] >> y, x[2] >> y, x[3] >> y].pack()
}

pub fn packuswb(x: u64, y: u64) -> u64 {
    fn saturate(x: i16) -> u8 {
        x.clamp(0, 0xFF) as u8
    }
    let x: [i16; 4] = x.unpack();
    let y: [i16; 4] = y.unpack();
    [
        saturate(x[0]),
        saturate(x[1]),
        saturate(x[2]),
        saturate(x[3]),
        saturate(y[0]),
        saturate(y[1]),
        saturate(y[2]),
        saturate(y[3]),
    ]
    .pack()
}

pub fn psubusb(x: u64, y: u64) -> u64 {
    let x: [u8; 8] = x.unpack();
    let y: [u8; 8] = y.unpack();
    [
        x[0].saturating_sub(y[0]),
        x[1].saturating_sub(y[1]),
        x[2].saturating_sub(y[2]),
        x[3].saturating_sub(y[3]),
        x[4].saturating_sub(y[4]),
        x[5].saturating_sub(y[5]),
        x[6].saturating_sub(y[6]),
        x[7].saturating_sub(y[7]),
    ]
    .pack()
}

pub fn psubb(x: u64, y: u64) -> u64 {
    let x: [u8; 8] = x.unpack();
    let y: [u8; 8] = y.unpack();
    [
        x[0].wrapping_sub(y[0]),
        x[1].wrapping_sub(y[1]),
        x[2].wrapping_sub(y[2]),
        x[3].wrapping_sub(y[3]),
        x[4].wrapping_sub(y[4]),
        x[5].wrapping_sub(y[5]),
        x[6].wrapping_sub(y[6]),
        x[7].wrapping_sub(y[7]),
    ]
    .pack()
}

pub fn psubd(x: u64, y: u64) -> u64 {
    let x: [u32; 2] = x.unpack();
    let y: [u32; 2] = y.unpack();
    [x[0].wrapping_sub(y[0]), x[1].wrapping_sub(y[1])].pack()
}

pub fn psubsb(x: u64, y: u64) -> u64 {
    let x: [i8; 8] = x.unpack();
    let y: [i8; 8] = y.unpack();
    [
        x[0].saturating_sub(y[0]),
        x[1].saturating_sub(y[1]),
        x[2].saturating_sub(y[2]),
        x[3].saturating_sub(y[3]),
        x[4].saturating_sub(y[4]),
        x[5].saturating_sub(y[5]),
        x[6].saturating_sub(y[6]),
        x[7].saturating_sub(y[7]),
    ]
    .pack()
}

pub fn psubsw(x: u64, y: u64) -> u64 {
    let x: [i16; 4] = x.unpack();
    let y: [i16; 4] = y.unpack();
    [
        x[0].saturating_sub(y[0]),
        x[1].saturating_sub(y[1]),
        x[2].saturating_sub(y[2]),
        x[3].saturating_sub(y[3]),
    ]
    .pack()
}

pub fn psubw(x: u64, y: u64) -> u64 {
    let x: [u16; 4] = x.unpack();
    let y: [u16; 4] = y.unpack();
    [
        x[0].wrapping_sub(y[0]),
        x[1].wrapping_sub(y[1]),
        x[2].wrapping_sub(y[2]),
        x[3].wrapping_sub(y[3]),
    ]
    .pack()
}

pub fn psraw(x: u64, y: u64) -> u64 {
    let x: [i16; 4] = x.unpack();
    let shifted = if y >= 16 {
        x.map(|lane| if lane < 0 { -1 } else { 0 })
    } else {
        x.map(|lane| lane >> y)
    };
    shifted.pack()
}

#[cfg(test)]
mod tests {
    use super::{
        packssdw, packsswb, paddb, paddd, paddw, pcmpeqb, pcmpeqd, pcmpgtb, pextrw, pinsrw,
        pmaddwd, pmovmskb, pshufw, pslld, psllw, psrad, psraw, psrld, psrlq, psubb, psubd, psubsb,
        psubsw, punpckhbw, punpckhwd, punpckldq, punpcklwd,
    };

    #[test]
    fn paddb_and_paddd_wrap_each_lane_independently() {
        assert_eq!(
            paddb(0x00ff_00ff_00ff_00ff, 0x0101_0101_0101_0101),
            0x0100_0100_0100_0100
        );
        assert_eq!(
            paddd(0xffff_fffe_0000_0001, 0x0000_0002_0000_0003),
            0x0000_0000_0000_0004
        );
    }

    #[test]
    fn paddw_wraps_each_word_independently() {
        assert_eq!(
            paddw(0x0001_ffff_7fff_8000, 0x0001_0002_0001_8000),
            0x0002_0001_8000_0000
        );
    }

    #[test]
    fn psubb_and_psubd_wrap_each_lane_independently() {
        assert_eq!(psubb(0, 0x0101_0101_0101_0101), 0xffff_ffff_ffff_ffff);
        assert_eq!(
            psubd(0x0000_0001_0000_0000, 0x0000_0002_0000_0001),
            0xffff_ffff_ffff_ffff
        );
    }

    #[test]
    fn psubsb_and_psubsw_saturate_signed_lanes() {
        assert_eq!(psubsb(0x7f, 0xff), 0x7f);
        assert_eq!(psubsb(0x80, 1), 0x80);
        assert_eq!(psubsw(0x7fff, 0xffff), 0x7fff);
        assert_eq!(psubsw(0x8000, 1), 0x8000);
    }

    #[test]
    fn psraw_saturates_large_shift_counts_to_the_sign_bit() {
        assert_eq!(psraw(0x8000_7fff_ffff_0001, 16), 0xffff_0000_ffff_0000);
        assert_eq!(
            psraw(0x8000_7fff_ffff_0001, u64::MAX),
            0xffff_0000_ffff_0000
        );
    }

    #[test]
    fn pcmpeq_and_pcmpgt_fill_matching_lanes() {
        assert_eq!(
            pcmpeqb(0x0807_0605_0403_0201, 0x0807_0605_0403_0201),
            u64::MAX
        );
        assert_eq!(pcmpeqb(0x00ff_0000_00ff_0000, 0), 0xff00_ffff_ff00_ffff);
        assert_eq!(pcmpgtb(0xffff_0000_0000_0100, 0), 0x0000_0000_0000_ff00);
        assert_eq!(
            pcmpeqd(0x0000_0001_0000_0002, 0x0000_0001_0000_0003),
            0xffff_ffff_0000_0000
        );
    }

    #[test]
    fn packsswb_and_packssdw_saturate_signed_lanes() {
        // words [1, -300, -2, 0x7fff] -> bytes [1, -128, -2, 127]
        assert_eq!(packsswb(0x7fff_fffe_fed4_0001, 0), 0x0000_0000_7ffe_8001);
        // dwords [0x7fff_ffff, -1] -> words [32767, -1]
        assert_eq!(packssdw(0xffff_ffff_7fff_ffff, 0), 0x0000_0000_ffff_7fff);
    }

    #[test]
    fn unpack_interleaves_lanes() {
        assert_eq!(punpckldq(0x1122_3344, 0x5566_7788), 0x5566_7788_1122_3344);
        assert_eq!(punpcklwd(0x1122_3344, 0x5566_7788), 0x5566_1122_7788_3344);
        assert_eq!(
            punpckhwd(0xaabb_ccdd_eeff_0011, 0x8877_6655_4433_2211),
            0x8877_aabb_6655_ccdd
        );
        assert_eq!(
            punpckhbw(0x0807_0605_0403_0201, 0x1817_1615_1413_1211),
            0x1808_1707_1606_1505
        );
    }

    #[test]
    fn shift_helpers_shift_each_lane() {
        assert_eq!(psllw(0x0001_0002_0004_0008, 4), 0x0010_0020_0040_0080);
        assert_eq!(psllw(0xffff, 16), 0);
        assert_eq!(pslld(0xffff_ffff_0000_0001, 8), 0xffff_ff00_0000_0100);
        assert_eq!(psrld(0xffff_ffff_0000_0100, 8), 0x00ff_ffff_0000_0001);
        assert_eq!(psrlq(0x8000_0000_0000_0000, 63), 1);
        assert_eq!(psrad(0x8000_0000_0000_0001, 31), 0xffff_ffff_0000_0000);
    }

    #[test]
    fn pextrw_pinsrw_pshufw_move_word_lanes() {
        assert_eq!(pextrw(0x0004_0003_0002_0001, 2), 3);
        assert_eq!(pextrw(0x8000_0000_0000_0000, 3), 0x8000);
        assert_eq!(
            pinsrw(0x0004_0003_0002_0001, 0xabcd, 1),
            0x0004_0003_abcd_0001
        );
        assert_eq!(pinsrw(u64::MAX, 0, 0), 0xffff_ffff_ffff_0000);
        // imm 0x1b selects lanes 3,2,1,0, reversing the order.
        assert_eq!(pshufw(0x0004_0003_0002_0001, 0x1b), 0x0001_0002_0003_0004);
        assert_eq!(pshufw(0x0004_0003_0002_0001, 0xff), 0x0004_0004_0004_0004);
    }

    #[test]
    fn pmaddwd_and_pmovmskb() {
        // words [1, 2, 3, 4] * [5, 6, 7, 8] -> (5+12, 21+32)
        assert_eq!(
            pmaddwd(0x0004_0003_0002_0001, 0x0008_0007_0006_0005),
            0x0000_0035_0000_0011
        );
        assert_eq!(pmovmskb(0x8000_0000_0000_00ff), 0x81);
        assert_eq!(pmovmskb(0x0000_0000_0000_0000), 0);
    }
}
