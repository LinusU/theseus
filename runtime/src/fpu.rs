//! FPU registers.

use bitflags::bitflags;

bitflags! {
    #[derive(Clone, Copy)]
    pub struct Status: u16 {
        const C3 = 1 << 14;
        const C2 = 1 << 10;
        const C1 = 1 << 9;
        const C0 = 1 << 8;
    }
}

pub struct FPU {
    /// FPU ST0 through ST7 registers.
    pub st: [f64; 8],
    /// Index of top of FPU stack; 8 when stack empty.
    pub st_top: usize,
    /// Condition code bits (C0..C3 in status-word positions) from the last
    /// comparison or fxam.
    pub cc: Status,
    /// Control word, as managed by fldcw/fnstcw. We only round-trip the value;
    /// precision/rounding control bits are not honored.
    pub control: u16,
}

impl Default for FPU {
    fn default() -> Self {
        Self {
            st: [0.; 8],
            st_top: 8,
            cc: Status::C3,
            control: 0x037f,
        }
    }
}

impl FPU {
    fn exception(_msg: &str) {
        // TODO: modify state bits etc.
        // At least ignoring these may allow programs to make some progress.
        // See note in https://github.com/joncampbell123/dosbox-x/issues/94 ,
        // "I've seen DOSBox SVN bail out on perfectly good demoscene programs because
        // of [not allowing underflow]."
        // Don't log because anatyda underflows thousands of times, eek.
        // log::warn!("{}", msg);
    }

    /// Get st(0), the current top of the FPU stack.
    pub fn st0(&mut self) -> &mut f64 {
        &mut self.st[self.st_top]
    }

    pub fn push(&mut self, val: f64) {
        if self.st_top == 0 {
            Self::exception("fpu stack overflow");
            return;
        }
        self.st_top -= 1;
        self.st[self.st_top] = val;
    }

    pub fn pop(&mut self) {
        if self.st_top == 8 {
            Self::exception("fpu stack underflow");
            return;
        }
        self.st_top += 1;
    }

    /// Index in self.st for a given ST0, ST1 etc reg.
    fn st_offset(&self, ofs: usize) -> usize {
        let new = self.st_top + ofs;
        if new >= 8 {
            Self::exception("fpu stack underflow");
            return 7;
        }
        new
    }

    pub fn swap(&mut self, o1: usize, o2: usize) {
        let o1 = self.st_offset(o1);
        let o2 = self.st_offset(o2);
        self.st.swap(o1, o2);
    }

    pub fn get(&self, ofs: usize) -> f64 {
        self.st[self.st_offset(ofs)]
    }

    pub fn set(&mut self, ofs: usize, val: f64) {
        self.st[self.st_offset(ofs)] = val;
    }

    /// Record a comparison result (fcom and friends) in the condition codes.
    pub fn set_cmp(&mut self, cmp: std::cmp::Ordering) {
        self.cc = match cmp {
            std::cmp::Ordering::Less => Status::C0,
            std::cmp::Ordering::Equal => Status::C3,
            std::cmp::Ordering::Greater => Status::empty(),
        };
    }

    /// fxam: classify a value into the condition codes.
    pub fn xam(&mut self, val: f64) {
        let mut cc = if val.is_nan() {
            Status::C0
        } else if val.is_infinite() {
            Status::C0 | Status::C2
        } else if val == 0.0 {
            Status::C3
        } else if val.is_subnormal() {
            Status::C2 | Status::C3
        } else {
            Status::C2 // normal finite
        };
        if val.is_sign_negative() {
            cc |= Status::C1;
        }
        self.cc = cc;
    }

    pub fn status(&self) -> u16 {
        // Our status register impl doesn't include st_top so include it here.
        let mut status = self.cc.bits();
        status |= (self.st_top as u16 & 0b111) << 11;
        status
    }

    /// Round per the control word's rounding-control bits, as fist/frndint do.
    /// The C runtime sets truncation around its double-to-int conversions, so
    /// honoring these matters.
    pub fn round(&self, val: f64) -> f64 {
        match (self.control >> 10) & 3 {
            0 => val.round_ties_even(),
            1 => val.floor(),
            2 => val.ceil(),
            _ => val.trunc(),
        }
    }
}

/// Decode an x87 80-bit extended precision float, as stored by `fstp tbyte ptr`.
/// We model the FPU with f64, so the low 11 bits of the mantissa are lost.
pub fn f80_to_f64(bytes: [u8; 10]) -> f64 {
    let mant = u64::from_le_bytes(bytes[..8].try_into().unwrap());
    let se = u16::from_le_bytes(bytes[8..].try_into().unwrap());
    let sign = if se & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (se & 0x7fff) as i32;
    if exp == 0x7fff {
        return if mant << 1 == 0 {
            sign * f64::INFINITY
        } else {
            f64::NAN
        };
    }
    if mant == 0 {
        return sign * 0.0;
    }
    // Denormals (exp == 0) use the same exponent as exp == 1.
    let exp = if exp == 0 { 1 } else { exp };
    // mant is an integer with the binary point after the top bit.
    sign * scale2(mant as f64, exp - 16383 - 63)
}

/// `val * 2^exp`, in steps so that an intermediate power of two doesn't
/// overflow or underflow f64 before the multiply.
fn scale2(mut val: f64, mut exp: i32) -> f64 {
    while exp > 1000 {
        val *= 2f64.powi(1000);
        exp -= 1000;
    }
    while exp < -1000 {
        val *= 2f64.powi(-1000);
        exp += 1000;
    }
    val * 2f64.powi(exp)
}

/// Encode an f64 as an x87 80-bit extended precision float, for `fld tbyte ptr`.
pub fn f64_to_f80(val: f64) -> [u8; 10] {
    let bits = val.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1 << 52) - 1);
    let (mant, exp80): (u64, u16) = if exp == 0x7ff {
        // inf or nan
        let mant = if frac == 0 { 1 << 63 } else { 0xc000_0000_0000_0000 };
        (mant, 0x7fff)
    } else if exp == 0 {
        if frac == 0 {
            (0, 0)
        } else {
            // Subnormal: value is frac * 2^-1074; normalize so the top bit is set.
            let lz = frac.leading_zeros();
            ((frac << lz), (15372 - lz) as u16)
        }
    } else {
        ((1 << 63) | (frac << 11), (exp - 1023 + 16383) as u16)
    };
    let mut out = [0u8; 10];
    out[..8].copy_from_slice(&mant.to_le_bytes());
    out[8..].copy_from_slice(&(sign | exp80).to_le_bytes());
    out
}

pub fn read_f80(mem: &crate::Memory, addr: u32) -> f64 {
    f80_to_f64(mem[addr..addr + 10].try_into().unwrap())
}

pub fn write_f80(mem: &mut crate::Memory, addr: u32, val: f64) {
    mem[addr..addr + 10].copy_from_slice(&f64_to_f80(val));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f80_roundtrip() {
        for &v in &[
            0.0,
            -0.0,
            1.0,
            -1.0,
            3.14159,
            1e300,
            -1e-300,
            5e-324, // smallest subnormal
            f64::MAX,
            f64::MIN_POSITIVE,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ] {
            let back = f80_to_f64(f64_to_f80(v));
            assert_eq!(back.to_bits(), v.to_bits(), "{v}");
        }
        assert!(f80_to_f64(f64_to_f80(f64::NAN)).is_nan());
        // 1.0 as the x87 stores it: mantissa 0x8000000000000000, exponent 0x3fff.
        let one = [0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f];
        assert_eq!(f80_to_f64(one), 1.0);
        assert_eq!(f64_to_f80(1.0), one);
    }
}
