fn binop_ps(a: [u32; 4], b: [u32; 4], op: impl Fn(f32, f32) -> f32) -> [u32; 4] {
    std::array::from_fn(|i| op(f32::from_bits(a[i]), f32::from_bits(b[i])).to_bits())
}

fn bitop_ps(a: [u32; 4], b: [u32; 4], op: impl Fn(u32, u32) -> u32) -> [u32; 4] {
    std::array::from_fn(|i| op(a[i], b[i]))
}

pub fn addps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_ps(a, b, |a, b| a + b)
}

pub fn subps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_ps(a, b, |a, b| a - b)
}

pub fn mulps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_ps(a, b, |a, b| a * b)
}

pub fn divps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_ps(a, b, |a, b| a / b)
}

pub fn andps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| a & b)
}

pub fn andnps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| !a & b)
}

pub fn orps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| a | b)
}

pub fn xorps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| a ^ b)
}

fn unary_ps(a: [u32; 4], op: impl Fn(f32) -> f32) -> [u32; 4] {
    std::array::from_fn(|i| op(f32::from_bits(a[i])).to_bits())
}

pub fn sqrtps(a: [u32; 4]) -> [u32; 4] {
    unary_ps(a, |a| a.sqrt())
}

/// RSQRTPS and RCPPS are defined by Intel as approximations. The emulated host
/// uses the full-precision `1.0 / sqrt(x)` and `1.0 / x` results, which is
/// accurate enough to unblock translation; games that depend on the specific
/// low-precision seed value may need a more faithful approximation later.
pub fn rsqrtps(a: [u32; 4]) -> [u32; 4] {
    unary_ps(a, |a| 1.0 / a.sqrt())
}

pub fn rcpps(a: [u32; 4]) -> [u32; 4] {
    unary_ps(a, |a| 1.0 / a)
}

pub fn minps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_ps(a, b, |a, b| {
        if a.is_nan() {
            b
        } else if b.is_nan() {
            a
        } else {
            a.min(b)
        }
    })
}

pub fn maxps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_ps(a, b, |a, b| {
        if a.is_nan() {
            b
        } else if b.is_nan() {
            a
        } else {
            a.max(b)
        }
    })
}

pub fn shufps(a: [u32; 4], b: [u32; 4], imm8: u8) -> [u32; 4] {
    [
        a[(imm8 & 0x3) as usize],
        a[((imm8 >> 2) & 0x3) as usize],
        b[((imm8 >> 4) & 0x3) as usize],
        b[((imm8 >> 6) & 0x3) as usize],
    ]
}

pub fn unpcklps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [a[0], b[0], a[1], b[1]]
}

pub fn unpckhps(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [a[2], b[2], a[3], b[3]]
}

pub fn movss(dst: [u32; 4], src: u32) -> [u32; 4] {
    [src, dst[1], dst[2], dst[3]]
}

pub fn movhlps(dst: [u32; 4], src: [u32; 4]) -> [u32; 4] {
    [src[2], src[3], dst[2], dst[3]]
}

pub fn movlhps(dst: [u32; 4], src: [u32; 4]) -> [u32; 4] {
    [dst[0], dst[1], src[0], src[1]]
}

pub fn movlps(dst: [u32; 4], src: [u32; 2]) -> [u32; 4] {
    [src[0], src[1], dst[2], dst[3]]
}

pub fn movhps(dst: [u32; 4], src: [u32; 2]) -> [u32; 4] {
    [dst[0], dst[1], src[0], src[1]]
}

pub fn low_qword(xmm: [u32; 4]) -> [u32; 2] {
    [xmm[0], xmm[1]]
}

pub fn high_qword(xmm: [u32; 4]) -> [u32; 2] {
    [xmm[2], xmm[3]]
}

fn scalar_binop(a: u32, b: u32, op: impl Fn(f32, f32) -> f32) -> u32 {
    op(f32::from_bits(a), f32::from_bits(b)).to_bits()
}

pub fn addss(dst: [u32; 4], src: u32) -> [u32; 4] {
    [
        scalar_binop(dst[0], src, |a, b| a + b),
        dst[1],
        dst[2],
        dst[3],
    ]
}

pub fn subss(dst: [u32; 4], src: u32) -> [u32; 4] {
    [
        scalar_binop(dst[0], src, |a, b| a - b),
        dst[1],
        dst[2],
        dst[3],
    ]
}

pub fn mulss(dst: [u32; 4], src: u32) -> [u32; 4] {
    [
        scalar_binop(dst[0], src, |a, b| a * b),
        dst[1],
        dst[2],
        dst[3],
    ]
}

pub fn divss(dst: [u32; 4], src: u32) -> [u32; 4] {
    [
        scalar_binop(dst[0], src, |a, b| a / b),
        dst[1],
        dst[2],
        dst[3],
    ]
}

fn scalar_unary(a: u32, op: impl Fn(f32) -> f32) -> u32 {
    op(f32::from_bits(a)).to_bits()
}

pub fn sqrtss(dst: [u32; 4], src: u32) -> [u32; 4] {
    [scalar_unary(src, |a| a.sqrt()), dst[1], dst[2], dst[3]]
}

pub fn rsqrtss(dst: [u32; 4], src: u32) -> [u32; 4] {
    [
        scalar_unary(src, |a| 1.0 / a.sqrt()),
        dst[1],
        dst[2],
        dst[3],
    ]
}

pub fn rcpss(dst: [u32; 4], src: u32) -> [u32; 4] {
    [scalar_unary(src, |a| 1.0 / a), dst[1], dst[2], dst[3]]
}

pub fn minss(dst: [u32; 4], src: u32) -> [u32; 4] {
    let a = f32::from_bits(dst[0]);
    let b = f32::from_bits(src);
    let res = if a.is_nan() {
        b
    } else if b.is_nan() {
        a
    } else {
        a.min(b)
    };
    [res.to_bits(), dst[1], dst[2], dst[3]]
}

pub fn maxss(dst: [u32; 4], src: u32) -> [u32; 4] {
    let a = f32::from_bits(dst[0]);
    let b = f32::from_bits(src);
    let res = if a.is_nan() {
        b
    } else if b.is_nan() {
        a
    } else {
        a.max(b)
    };
    [res.to_bits(), dst[1], dst[2], dst[3]]
}

pub fn cvtsi2ss(dst: [u32; 4], src: u32) -> [u32; 4] {
    [((src as i32) as f32).to_bits(), dst[1], dst[2], dst[3]]
}

pub fn cvtss2si(src: u32) -> u32 {
    (f32::from_bits(src).round() as i32) as u32
}

pub fn cvttss2si(src: u32) -> u32 {
    (f32::from_bits(src) as i32) as u32
}

pub fn cvtpi2ps(dst: [u32; 4], src: u64) -> [u32; 4] {
    let low = (src as u32) as i32;
    let high = (src >> 32) as i32;
    [
        (low as f32).to_bits(),
        (high as f32).to_bits(),
        dst[2],
        dst[3],
    ]
}

fn cvt_f32_to_i32(src: u32, truncate: bool) -> i32 {
    let f = f32::from_bits(src);
    if truncate { f as i32 } else { f.round() as i32 }
}

pub fn cvtps2pi(src: [u32; 2]) -> u64 {
    let low = cvt_f32_to_i32(src[0], false) as u64 as u32 as u64;
    let high = (cvt_f32_to_i32(src[1], false) as u64) << 32;
    low | high
}

pub fn cvttps2pi(src: [u32; 2]) -> u64 {
    let low = cvt_f32_to_i32(src[0], true) as u64 as u32 as u64;
    let high = (cvt_f32_to_i32(src[1], true) as u64) << 32;
    low | high
}

pub fn cmpss(dst: [u32; 4], src: u32, predicate: u8) -> [u32; 4] {
    let a = f32::from_bits(dst[0]);
    let b = f32::from_bits(src);
    let result = match predicate {
        0 => a == b,
        1 => a < b,
        2 => a <= b,
        3 => a.is_nan() || b.is_nan(),
        4 => a != b,
        5 => !(a < b),
        6 => !(a <= b),
        7 => !a.is_nan() && !b.is_nan(),
        _ => false,
    };
    [if result { 0xffff_ffff } else { 0 }, dst[1], dst[2], dst[3]]
}

pub fn cmpps(a: [u32; 4], b: [u32; 4], predicate: u8) -> [u32; 4] {
    std::array::from_fn(|i| {
        let a = f32::from_bits(a[i]);
        let b = f32::from_bits(b[i]);
        let result = match predicate {
            0 => a == b,
            1 => a < b,
            2 => a <= b,
            3 => a.is_nan() || b.is_nan(),
            4 => a != b,
            5 => !(a < b),
            6 => !(a <= b),
            7 => !a.is_nan() && !b.is_nan(),
            _ => false,
        };
        if result { 0xffff_ffff } else { 0 }
    })
}
