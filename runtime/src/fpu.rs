//! FPU registers.

use bitflags::bitflags;

bitflags! {
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
    /// The result of the last fcmp, used to generate status word.
    pub cmp: std::cmp::Ordering,
    /// Control word, as managed by fldcw/fnstcw. We only round-trip the value;
    /// precision/rounding control bits are not honored.
    pub control: u16,
}

impl Default for FPU {
    fn default() -> Self {
        Self {
            st: [0.; 8],
            st_top: 8,
            cmp: std::cmp::Ordering::Equal,
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

    /// Load an x87 80-bit packed-BCD value onto the register stack.
    pub fn push_bcd(&mut self, bytes: [u8; 10]) {
        let mut value = 0.0;
        for byte in bytes[..9].iter().rev() {
            value = value * 100.0 + ((byte >> 4) * 10 + (byte & 0x0f)) as f64;
        }
        if bytes[9] & 0x80 != 0 {
            value = -value;
        }
        self.push(value);
    }

    /// Load an x87 80-bit extended-precision value, approximated as an f64.
    pub fn push_f80(&mut self, bytes: [u8; 10]) {
        let significand = u64::from_le_bytes(bytes[..8].try_into().unwrap());
        let sign_and_exponent = u16::from_le_bytes(bytes[8..].try_into().unwrap());
        let exponent = sign_and_exponent & 0x7fff;
        let negative = sign_and_exponent & 0x8000 != 0;

        let mut value = match (exponent, significand) {
            (0, 0) => 0.0,
            (0, _) => (significand as f64 / (1u64 << 63) as f64) * 2f64.powi(1 - 16383),
            (0x7fff, 0x8000_0000_0000_0000) => f64::INFINITY,
            (0x7fff, _) => f64::NAN,
            _ => (significand as f64 / (1u64 << 63) as f64) * 2f64.powi(exponent as i32 - 16383),
        };
        if negative {
            value = -value;
        }
        self.push(value);
    }

    /// Store and pop ST(0) as an x87 80-bit packed-BCD value.
    pub fn pop_bcd(&mut self) -> [u8; 10] {
        let value = self.round(self.get(0));
        self.pop();

        let mut bytes = [0; 10];
        let mut magnitude = value.abs() as u128;
        for byte in &mut bytes[..9] {
            let low = magnitude % 10;
            magnitude /= 10;
            let high = magnitude % 10;
            magnitude /= 10;
            *byte = low as u8 | ((high as u8) << 4);
        }
        if value.is_sign_negative() {
            bytes[9] = 0x80;
        }
        bytes
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

    pub fn status(&self) -> u16 {
        let status = match self.cmp {
            std::cmp::Ordering::Less => Status::C0,
            std::cmp::Ordering::Equal => Status::C3,
            std::cmp::Ordering::Greater => Status::empty(),
        };
        // Our status register impl doesn't include st_top so include it here.
        let mut status = status.bits();
        status |= (self.st_top as u16 & 0b111) << 11;
        status
    }

    pub fn round(&self, val: f64) -> f64 {
        // TODO: rounding modes?
        // This implements default rounding mode of round towards even.
        val.round_ties_even()
    }

    /// Split ST(0) into an exponent in ST(0) and a significand in ST(1).
    pub fn extract(&mut self) {
        let value = self.get(0);
        let (significand, exponent) = if value == 0.0 {
            (value, f64::NEG_INFINITY)
        } else {
            let exponent = value.abs().log2().floor();
            (value / 2.0f64.powf(exponent), exponent)
        };
        self.set(0, significand);
        self.push(exponent);
    }
}

#[cfg(test)]
mod tests {
    use super::FPU;

    #[test]
    fn loads_packed_bcd() {
        let mut fpu = FPU::default();
        fpu.push_bcd([0x45, 0x23, 0x01, 0, 0, 0, 0, 0, 0, 0x80]);
        assert_eq!(fpu.get(0), -12345.0);
    }

    #[test]
    fn round_trips_packed_bcd() {
        let mut fpu = FPU::default();
        fpu.push(-987654321.0);
        let packed = fpu.pop_bcd();
        fpu.push_bcd(packed);
        assert_eq!(fpu.get(0), -987654321.0);
    }

    #[test]
    fn loads_extended_precision() {
        let mut fpu = FPU::default();
        fpu.push_f80([0, 0, 0, 0, 0, 0, 0, 0x80, 0xff, 0x3f]);
        assert_eq!(fpu.get(0), 1.0);
    }

    #[test]
    fn extracts_exponent_and_significand() {
        let mut fpu = FPU::default();
        fpu.push(-12.0);
        fpu.extract();
        assert_eq!(fpu.get(0), 3.0);
        assert_eq!(fpu.get(1), -1.5);
    }
}
