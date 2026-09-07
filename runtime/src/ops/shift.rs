use super::int::Int;
use crate::Flags;

pub fn shl<I: Int + num_traits::WrappingShl>(x: I, y: u8, flags: &mut Flags) -> I {
    let bits = I::bits() as u8;
    assert!(I::bits() < 64);
    let y = y % 32;
    if y == 0 {
        return x;
    }

    // Carry is the highest bit that will be shifted out.
    let cf = if y <= bits {
        (x >> (bits - y) as usize & I::one()).is_one()
    } else {
        false
    };
    let val = if y < bits { x << y as usize } else { I::zero() };

    flags.set(Flags::CF, cf);
    let msb = val.high_bit().is_one();
    flags.set(Flags::SF, msb);
    // Note: OF only defined for 1-bit rotates.
    // "For left shifts, the OF flag is set to 0 if the mostsignificant bit of the result is the
    // same as the CF flag (that is, the top two bits of the original operand were the same) [...]"
    flags.set(
        Flags::OF,
        x.shr(I::bits() - 1).is_one() ^ (x.shr(I::bits() - 2) & I::one()).is_one(),
    );
    flags.set(Flags::ZF, val.is_zero());
    flags.set(Flags::PF, val.low_byte().count_ones().is_multiple_of(2));

    val
}

pub fn shld16(x: u16, y: u16, count: u8, flags: &mut Flags) -> u16 {
    let count = u32::from(count % 16);
    if count == 0 {
        return x;
    }
    flags.set(Flags::CF, ((x >> (16 - count)) & 1) != 0);
    if count == 1 {
        flags.set(Flags::OF, (x >> 15) != ((x >> 14) & 1));
    }
    let result = (((x as u32) << count) | ((y as u32) >> (16 - count))) as u16;
    flags.set(Flags::PF, result.low_byte().count_ones().is_multiple_of(2));
    flags.set(Flags::SF, (result >> 15) != 0);
    flags.set(Flags::ZF, result == 0);
    result
}

pub fn shld(x: u32, y: u32, count: u8, flags: &mut Flags) -> u32 {
    let count = count % 32;
    if count == 0 {
        return x;
    }
    // "CF flag is filled with the last bit shifted out of the destination operand"
    flags.set(Flags::CF, ((x >> (32 - count)) & 1) != 0);
    if count == 1 {
        // "OF flag is set if a sign change occurred"
        flags.set(Flags::OF, (x >> 31) != ((x >> 30) & 1));
    }
    let result = (x << count) | (y >> (32 - count));
    flags.set(Flags::PF, result.low_byte().count_ones().is_multiple_of(2));
    flags.set(Flags::SF, (result >> 31) != 0);
    flags.set(Flags::ZF, result == 0);
    result
}

pub fn shrd16(x: u16, y: u16, count: u8, flags: &mut Flags) -> u16 {
    let count = u32::from(count % 16);
    if count == 0 {
        return x;
    }
    flags.set(Flags::CF, ((x >> (count - 1)) & 1) != 0);
    let result = (((x as u32) >> count) | ((y as u32) << (16 - count))) as u16;
    if count == 1 {
        flags.set(Flags::OF, ((x >> 15) & 1) != ((result >> 15) & 1));
    }
    flags.set(Flags::PF, result.low_byte().count_ones().is_multiple_of(2));
    flags.set(Flags::SF, (result >> 15) != 0);
    flags.set(Flags::ZF, result == 0);
    result
}

pub fn shrd(x: u32, y: u32, count: u8, flags: &mut Flags) -> u32 {
    let count = count % 32;
    if count == 0 {
        return x;
    }
    flags.set(Flags::CF, ((x >> (count - 1)) & 1) != 0);
    let result = (x >> count) | (y << (32 - count));
    if count == 1 {
        // For a 1-bit shrd, OF is set if the sign bit changed.
        flags.set(Flags::OF, ((x >> 31) & 1) != ((result >> 31) & 1));
    }
    flags.set(Flags::PF, result.low_byte().count_ones().is_multiple_of(2));
    flags.set(Flags::SF, (result >> 31) != 0);
    flags.set(Flags::ZF, result == 0);
    result
}

pub fn shr<I: Int>(x: I, y: u8, flags: &mut Flags) -> I {
    assert!(I::bits() < 64);
    let y = y % 32;
    if y == 0 {
        return x; // Don't affect flags.
    }

    let val = if y < I::bits() as u8 {
        x >> y as usize
    } else {
        I::zero()
    };
    let cf = if y <= I::bits() as u8 {
        ((x >> (y - 1) as usize) & I::one()).is_one()
    } else {
        false
    };
    flags.set(Flags::CF, cf);
    flags.set(Flags::SF, false); // ?
    flags.set(Flags::ZF, val.is_zero());

    // Note: OF state undefined for shifts > 1 bit.
    flags.set(Flags::OF, x.high_bit().is_one());
    flags.set(Flags::PF, val.low_byte().count_ones().is_multiple_of(2));
    val
}

pub fn sar<I: Int>(x: I, y: u8, flags: &mut Flags) -> I {
    assert!(I::bits() < 64);
    let y = y % 32;
    if y == 0 {
        return x;
    }

    // Past the operand width the register is already all sign bits, so each
    // further shift drops a sign bit: CF = the original sign, not zero like
    // the zero-filled shl/shr paths.
    let cf = if y <= I::bits() as u8 {
        x.shr(y as usize - 1).bitand(I::one()).is_one()
    } else {
        x.high_bit().is_one()
    };
    flags.set(Flags::CF, cf);
    // Note: OF only defined for 1-bit rotates.
    flags.set(Flags::OF, false);
    // There's a random "u32" type in the num-traits signed_shr signature, so cast here.
    let result = if y < I::bits() as u8 {
        x.signed_shr(y as u32)
    } else if x.high_bit().is_one() {
        !I::zero()
    } else {
        I::zero()
    };

    flags.set(Flags::SF, result.high_bit().is_one());
    flags.set(Flags::ZF, result.is_zero());
    flags.set(Flags::PF, result.low_byte().count_ones().is_multiple_of(2));
    result
}

pub fn rol<I: Int>(x: I, y: u8, flags: &mut Flags) -> I {
    // The count masks to 5 bits; a nonzero masked count updates CF even
    // when the effective rotation (mod operand width) is zero, e.g.
    // `rol al, 8` leaves AL alone but still writes CF = LSB(AL).
    let masked = usize::from(y) % 32;
    if masked == 0 {
        return x;
    }
    let result = x.rotate_left((masked % I::bits()) as u32);
    let carry = (result & I::one()).is_one();
    flags.set(Flags::CF, carry);
    // Note: OF only defined for 1-bit rotates.
    if masked == 1 {
        flags.set(Flags::OF, carry ^ (result.high_bit()).is_one());
    }
    result
}

pub fn ror<I: Int>(x: I, y: u8, flags: &mut Flags) -> I {
    let masked = usize::from(y) % 32;
    if masked == 0 {
        return x;
    }
    let result = x.rotate_right((masked % I::bits()) as u32);
    flags.set(Flags::CF, result.high_bit().is_one());
    // Note: OF only defined for 1-bit rotates.
    if masked == 1 {
        flags.set(
            Flags::OF,
            result.high_bit().is_one() ^ ((result >> (I::bits() - 2)) & I::one()).is_one(),
        );
    }
    result
}

pub fn rcl<I: Int>(x: I, y: u8, flags: &mut Flags) -> I {
    assert!(I::bits() < 64);
    let y = y % 32;
    let count = y as usize % (I::bits() + 1);
    if count == 0 {
        return x;
    }

    let width = I::bits() + 1;
    let mask = (1u64 << width) - 1;
    let x = (x.to_u64().unwrap() << 1) | u64::from(flags.contains(Flags::CF));
    let x = ((x << count) | (x >> (width - count))) & mask;
    let result = I::from(x >> 1).unwrap();

    flags.set(Flags::CF, (x & 1) != 0);
    // Note: OF only defined for 1-bit rotates.
    flags.set(
        Flags::OF,
        flags.contains(Flags::CF) ^ result.high_bit().is_one(),
    );
    result
}

pub fn rcr<I: Int>(x: I, y: u8, flags: &mut Flags) -> I {
    assert!(I::bits() < 64);
    let y = y % 32;
    let count = y as usize % (I::bits() + 1);
    if count == 0 {
        return x;
    }

    let bits = I::bits();
    let width = bits + 1;
    let mask = (1u64 << width) - 1;
    let result_mask = (1u64 << bits) - 1;
    let x = x.to_u64().unwrap() | (u64::from(flags.contains(Flags::CF)) << bits);
    let x = ((x >> count) | (x << (width - count))) & mask;
    let result = I::from(x & result_mask).unwrap();

    flags.set(Flags::CF, ((x >> bits) & 1) != 0);
    // Note: OF only defined for 1-bit rotates.
    flags.set(
        Flags::OF,
        result.high_bit().is_one() ^ ((result >> (bits - 2)) & I::one()).is_one(),
    );
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrd() {
        let mut flags = Flags::default();
        assert_eq!(super::shrd(0x8000_0001, 0, 1, &mut flags), 0x4000_0000);
        assert_eq!("CF PF OF", flags.to_string());

        let mut flags = Flags::default();
        assert_eq!(
            super::shrd(0x1234_5678, 0xfedc_ba98, 4, &mut flags),
            0x8123_4567
        );
        assert_eq!("CF SF", flags.to_string());
    }

    #[test]
    fn shld16_uses_the_16_bit_source_boundary() {
        let mut flags = Flags::default();
        assert_eq!(super::shld16(0x8001, 0xffff, 1, &mut flags), 0x0003);
        assert_eq!("CF PF OF", flags.to_string());
    }

    #[test]
    fn shrd16_uses_the_16_bit_source_boundary() {
        let mut flags = Flags::default();
        assert_eq!(super::shrd16(0x0001, 0x0003, 1, &mut flags), 0x8000);
        assert_eq!("CF PF SF OF", flags.to_string());
    }

    #[test]
    fn large_shifts_saturate_to_the_operand_width() {
        let mut flags = Flags::default();
        assert_eq!(super::shr(0x80u8, 8, &mut flags), 0);
        assert_eq!(super::sar(0x80u8, 8, &mut flags), 0xff);
    }

    #[test]
    fn sar_past_the_operand_width_reports_the_sign_in_cf() {
        // `sar al, 9..31` has already pushed every original bit out; each
        // remaining shift drops a sign-fill bit, so CF = the sign.
        for count in 9..32u8 {
            let mut flags = Flags::default();
            assert_eq!(super::sar(0x80u8, count, &mut flags), 0xff);
            assert!(flags.contains(Flags::CF));

            let mut flags = Flags::default();
            assert_eq!(super::sar(0x7fu8, count, &mut flags), 0);
            assert!(!flags.contains(Flags::CF));

            let mut flags = Flags::default();
            assert_eq!(super::sar(0x8000u16, count.max(17), &mut flags), 0xffff);
            assert!(flags.contains(Flags::CF));
        }
        // The boundary count still reports the last real bit shifted out.
        let mut flags = Flags::default();
        assert_eq!(super::sar(0x80u8, 8, &mut flags), 0xff);
        assert!(flags.contains(Flags::CF));
        let mut flags = Flags::default();
        assert_eq!(super::sar(0x40u8, 8, &mut flags), 0x00);
        assert!(!flags.contains(Flags::CF));
    }

    #[test]
    fn rcl() {
        let mut flags = Flags::CF;
        assert_eq!(super::rcl(0b1000_0000u8, 1, &mut flags), 0b0000_0001);
        assert_eq!("CF OF", flags.to_string());

        let mut flags = Flags::default();
        assert_eq!(super::rcl(0b1010_0001u8, 3, &mut flags), 0b0000_1010);
        assert_eq!("CF OF", flags.to_string());

        let mut flags = Flags::CF | Flags::OF;
        assert_eq!(super::rcl(0x1234_5678u32, 32, &mut flags), 0x1234_5678);
        assert_eq!("CF OF", flags.to_string());

        let mut flags = Flags::CF | Flags::OF;
        assert_eq!(super::rcl(0x1234u16, 17, &mut flags), 0x1234);
        assert_eq!("CF OF", flags.to_string());
    }

    #[test]
    fn ror() {
        let mut flags = Flags::default();
        assert_eq!(super::ror(0b0000_0001u8, 1, &mut flags), 0b1000_0000);
        assert_eq!("CF OF", flags.to_string());

        let mut flags = Flags::CF | Flags::OF;
        assert_eq!(super::ror(0b0000_0010u8, 1, &mut flags), 0b0000_0001);
        assert_eq!("", flags.to_string());

        // OF is undefined for counts above 1 and is left untouched.
        let mut flags = Flags::default();
        assert_eq!(super::ror(0x1234_5678u32, 4, &mut flags), 0x8123_4567);
        assert_eq!("CF", flags.to_string());

        let mut flags = Flags::CF | Flags::OF;
        assert_eq!(super::ror(0x1234_5678u32, 32, &mut flags), 0x1234_5678);
        assert_eq!("CF OF", flags.to_string());
    }

    #[test]
    fn rotates_by_operand_width_preserve_flags() {
        let mut flags = Flags::CF | Flags::OF;
        assert_eq!(super::rol(0x81u8, 8, &mut flags), 0x81);
        assert_eq!("CF OF", flags.to_string());

        let mut flags = Flags::CF | Flags::OF;
        assert_eq!(super::ror(0x8001u16, 16, &mut flags), 0x8001);
        assert_eq!("CF OF", flags.to_string());
    }

    #[test]
    fn full_width_rotates_still_update_cf() {
        // `rol al, 8` rotates a full width: AL is unchanged but the masked
        // count is nonzero, so CF still receives LSB(AL).
        let mut flags = Flags::CF;
        assert_eq!(super::rol(0x80u8, 8, &mut flags), 0x80);
        assert_eq!("", flags.to_string());

        let mut flags = Flags::default();
        assert_eq!(super::rol(0x01u8, 8, &mut flags), 0x01);
        assert_eq!("CF", flags.to_string());

        // `ror al, 8` likewise writes CF = MSB(AL).
        let mut flags = Flags::CF;
        assert_eq!(super::ror(0x01u8, 8, &mut flags), 0x01);
        assert_eq!("", flags.to_string());

        let mut flags = Flags::default();
        assert_eq!(super::ror(0x80u8, 24, &mut flags), 0x80);
        assert_eq!("CF", flags.to_string());

        // A zero masked count still leaves every flag alone.
        let mut flags = Flags::CF | Flags::OF;
        assert_eq!(super::rol(0x01u8, 32, &mut flags), 0x01);
        assert_eq!("CF OF", flags.to_string());
    }

    #[test]
    fn rcr() {
        let mut flags = Flags::CF;
        assert_eq!(super::rcr(0b0000_0001u8, 1, &mut flags), 0b1000_0000);
        assert_eq!("CF OF", flags.to_string());

        let mut flags = Flags::default();
        assert_eq!(super::rcr(0b1000_0101u8, 3, &mut flags), 0b0101_0000);
        assert_eq!("CF OF", flags.to_string());

        let mut flags = Flags::CF | Flags::OF;
        assert_eq!(super::rcr(0x1234_5678u32, 32, &mut flags), 0x1234_5678);
        assert_eq!("CF OF", flags.to_string());

        let mut flags = Flags::CF | Flags::OF;
        assert_eq!(super::rcr(0x1234u16, 17, &mut flags), 0x1234);
        assert_eq!("CF OF", flags.to_string());
    }
}
