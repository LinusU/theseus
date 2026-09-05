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

pub fn cvtsi2sd(dst: [u32; 4], src: u32) -> [u32; 4] {
    let mut out = dst;
    set_qword(&mut out, 0, (src as i32) as f64);
    out
}

pub fn cvtsd2si(src: [u32; 2]) -> u32 {
    (qword2(src).round() as i32) as u32
}

pub fn cvttsd2si(src: [u32; 2]) -> u32 {
    (qword2(src) as i32) as u32
}

pub fn cvtsd2ss(dst: [u32; 4], src: [u32; 2]) -> [u32; 4] {
    let mut out = dst;
    out[0] = (qword2(src) as f32).to_bits();
    out
}

pub fn cvtss2sd(dst: [u32; 4], src: u32) -> [u32; 4] {
    let mut out = dst;
    set_qword(&mut out, 0, f32::from_bits(src) as f64);
    out
}

pub fn cvtpd2ps(src: [u32; 4]) -> [u32; 4] {
    [
        (qword(src, 0) as f32).to_bits(),
        (qword(src, 1) as f32).to_bits(),
        0,
        0,
    ]
}

pub fn cvtps2pd(src: [u32; 4]) -> [u32; 4] {
    let mut out = [0u32; 4];
    set_qword(&mut out, 0, f32::from_bits(src[0]) as f64);
    set_qword(&mut out, 1, f32::from_bits(src[1]) as f64);
    out
}

pub fn cvtdq2ps(src: [u32; 4]) -> [u32; 4] {
    std::array::from_fn(|i| (src[i] as i32 as f32).to_bits())
}

pub fn cvtps2dq(src: [u32; 4]) -> [u32; 4] {
    std::array::from_fn(|i| (f32::from_bits(src[i]).round() as i32) as u32)
}

pub fn cvttps2dq(src: [u32; 4]) -> [u32; 4] {
    std::array::from_fn(|i| (f32::from_bits(src[i]) as i32) as u32)
}

pub fn cvtdq2pd(src: [u32; 4]) -> [u32; 4] {
    let mut out = [0u32; 4];
    set_qword(&mut out, 0, (src[0] as i32) as f64);
    set_qword(&mut out, 1, (src[1] as i32) as f64);
    out
}

pub fn cvtpd2dq(src: [u32; 4]) -> [u32; 4] {
    [
        (qword(src, 0).round() as i32) as u32,
        (qword(src, 1).round() as i32) as u32,
        0,
        0,
    ]
}

pub fn cvttpd2dq(src: [u32; 4]) -> [u32; 4] {
    [
        (qword(src, 0) as i32) as u32,
        (qword(src, 1) as i32) as u32,
        0,
        0,
    ]
}

fn to_bytes(a: [u32; 4]) -> [u8; 16] {
    let mut out = [0u8; 16];
    for i in 0..4 {
        for j in 0..4 {
            out[i * 4 + j] = (a[i] >> (j * 8)) as u8;
        }
    }
    out
}

fn from_bytes(b: [u8; 16]) -> [u32; 4] {
    let mut out = [0u32; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i] |= (b[i * 4 + j] as u32) << (j * 8);
        }
    }
    out
}

pub fn pslldq(a: [u32; 4], count: u64) -> [u32; 4] {
    if count >= 16 {
        return [0; 4];
    }
    let bytes = to_bytes(a);
    let count = count as usize;
    let mut out = [0u8; 16];
    for i in 0..(16 - count) {
        out[i + count] = bytes[i];
    }
    from_bytes(out)
}

pub fn psrldq(a: [u32; 4], count: u64) -> [u32; 4] {
    if count >= 16 {
        return [0; 4];
    }
    let bytes = to_bytes(a);
    let count = count as usize;
    let mut out = [0u8; 16];
    for i in 0..(16 - count) {
        out[i] = bytes[i + count];
    }
    from_bytes(out)
}

fn to_words(a: [u32; 4]) -> [u16; 8] {
    let mut out = [0u16; 8];
    for i in 0..4 {
        out[i * 2] = a[i] as u16;
        out[i * 2 + 1] = (a[i] >> 16) as u16;
    }
    out
}

fn from_words(w: [u16; 8]) -> [u32; 4] {
    let mut out = [0u32; 4];
    for i in 0..4 {
        out[i] = (w[i * 2] as u32) | ((w[i * 2 + 1] as u32) << 16);
    }
    out
}

fn to_qwords(a: [u32; 4]) -> [u64; 2] {
    [
        (a[0] as u64) | ((a[1] as u64) << 32),
        (a[2] as u64) | ((a[3] as u64) << 32),
    ]
}

fn from_qwords(q: [u64; 2]) -> [u32; 4] {
    [
        q[0] as u32,
        (q[0] >> 32) as u32,
        q[1] as u32,
        (q[1] >> 32) as u32,
    ]
}

pub fn psllw_xmm(a: [u32; 4], count: u64) -> [u32; 4] {
    let words = to_words(a);
    let out = words.map(|w| if count >= 16 { 0 } else { w << (count as u32) });
    from_words(out)
}

pub fn pslld_xmm(a: [u32; 4], count: u64) -> [u32; 4] {
    let out = a.map(|w| if count >= 32 { 0 } else { w << (count as u32) });
    out
}

pub fn psllq_xmm(a: [u32; 4], count: u64) -> [u32; 4] {
    let q = to_qwords(a);
    let out = q.map(|v| if count >= 64 { 0 } else { v << count });
    from_qwords(out)
}

pub fn psrlw_xmm(a: [u32; 4], count: u64) -> [u32; 4] {
    let words = to_words(a);
    let out = words.map(|w| if count >= 16 { 0 } else { w >> (count as u32) });
    from_words(out)
}

pub fn psrld_xmm(a: [u32; 4], count: u64) -> [u32; 4] {
    a.map(|w| if count >= 32 { 0 } else { w >> (count as u32) })
}

pub fn psrlq_xmm(a: [u32; 4], count: u64) -> [u32; 4] {
    let q = to_qwords(a);
    let out = q.map(|v| if count >= 64 { 0 } else { v >> count });
    from_qwords(out)
}

pub fn psraw_xmm(a: [u32; 4], count: u64) -> [u32; 4] {
    let words = to_words(a);
    let out = words.map(|w| {
        if count >= 16 {
            if (w as i16) < 0 { 0xffff } else { 0 }
        } else {
            ((w as i16) >> (count as u32)) as u16
        }
    });
    from_words(out)
}

pub fn punpcklbw_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_bytes(a);
    let b = to_bytes(b);
    let mut out = [0u8; 16];
    for i in 0..8 {
        out[i * 2] = a[i];
        out[i * 2 + 1] = b[i];
    }
    from_bytes(out)
}

pub fn punpcklwd_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_words(a);
    let b = to_words(b);
    let mut out = [0u16; 8];
    for i in 0..4 {
        out[i * 2] = a[i];
        out[i * 2 + 1] = b[i];
    }
    from_words(out)
}

pub fn punpckldq_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [a[0], b[0], a[1], b[1]]
}

pub fn punpckhbw_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_bytes(a);
    let b = to_bytes(b);
    let mut out = [0u8; 16];
    for i in 0..8 {
        out[i * 2] = a[i + 8];
        out[i * 2 + 1] = b[i + 8];
    }
    from_bytes(out)
}

pub fn punpckhwd_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_words(a);
    let b = to_words(b);
    let mut out = [0u16; 8];
    for i in 0..4 {
        out[i * 2] = a[i + 4];
        out[i * 2 + 1] = b[i + 4];
    }
    from_words(out)
}

pub fn punpckhdq_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [a[2], b[2], a[3], b[3]]
}

pub fn punpcklqdq_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [a[0], a[1], b[0], b[1]]
}

pub fn punpckhqdq_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    [a[2], a[3], b[2], b[3]]
}

pub fn pshufd_xmm(src: [u32; 4], imm: u8) -> [u32; 4] {
    [
        src[(imm & 3) as usize],
        src[((imm >> 2) & 3) as usize],
        src[((imm >> 4) & 3) as usize],
        src[((imm >> 6) & 3) as usize],
    ]
}

pub fn pshuflw_xmm(src: [u32; 4], imm: u8) -> [u32; 4] {
    let w = to_words(src);
    let mut out = w;
    for i in 0..4 {
        out[i] = w[((imm >> (i * 2)) & 3) as usize];
    }
    from_words(out)
}

pub fn paddb_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_bytes(a);
    let b = to_bytes(b);
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = a[i].wrapping_add(b[i]);
    }
    from_bytes(out)
}

pub fn paddw_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_words(a);
    let b = to_words(b);
    let mut out = [0u16; 8];
    for i in 0..8 {
        out[i] = a[i].wrapping_add(b[i]);
    }
    from_words(out)
}

pub fn paddd_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    std::array::from_fn(|i| a[i].wrapping_add(b[i]))
}

pub fn paddq_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_qwords(a);
    let b = to_qwords(b);
    let mut out = [0u64; 2];
    for i in 0..2 {
        out[i] = a[i].wrapping_add(b[i]);
    }
    from_qwords(out)
}

pub fn psubb_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_bytes(a);
    let b = to_bytes(b);
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = a[i].wrapping_sub(b[i]);
    }
    from_bytes(out)
}

pub fn psubw_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_words(a);
    let b = to_words(b);
    let mut out = [0u16; 8];
    for i in 0..8 {
        out[i] = a[i].wrapping_sub(b[i]);
    }
    from_words(out)
}

pub fn psubd_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    std::array::from_fn(|i| a[i].wrapping_sub(b[i]))
}

pub fn psubq_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let a = to_qwords(a);
    let b = to_qwords(b);
    let mut out = [0u64; 2];
    for i in 0..2 {
        out[i] = a[i].wrapping_sub(b[i]);
    }
    from_qwords(out)
}

pub fn pand_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| a & b)
}

pub fn pandn_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| !a & b)
}

pub fn por_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| a | b)
}

pub fn pxor_xmm(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| a ^ b)
}

pub fn pshufhw_xmm(src: [u32; 4], imm: u8) -> [u32; 4] {
    let w = to_words(src);
    let mut out = w;
    for i in 0..4 {
        out[i + 4] = w[4 + ((imm >> (i * 2)) & 3) as usize];
    }
    from_words(out)
}

pub fn psrad_xmm(a: [u32; 4], count: u64) -> [u32; 4] {
    a.map(|w| {
        if count >= 32 {
            if (w as i32) < 0 { 0xffff_ffff } else { 0 }
        } else {
            ((w as i32) >> (count as u32)) as u32
        }
    })
}

pub fn movsd(dst: [u32; 4], src: [u32; 2]) -> [u32; 4] {
    [src[0], src[1], dst[2], dst[3]]
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

fn compare_unordered(a: f32, b: f32) -> (bool, bool, bool) {
    // (CF, ZF, PF): (less, equal, unordered)
    if a.is_nan() || b.is_nan() {
        (true, true, true)
    } else if a == b {
        (false, true, false)
    } else if a < b {
        (true, false, false)
    } else {
        (false, false, false)
    }
}

pub fn comiss_update_flags(flags: crate::Flags, a: u32, b: u32) -> crate::Flags {
    let (cf, zf, pf) = compare_unordered(f32::from_bits(a), f32::from_bits(b));
    let mut flags = flags;
    flags.set(crate::Flags::CF, cf);
    flags.set(crate::Flags::ZF, zf);
    flags.set(crate::Flags::PF, pf);
    flags.remove(crate::Flags::OF | crate::Flags::SF | crate::Flags::AF);
    flags
}

pub fn ucomiss_update_flags(flags: crate::Flags, a: u32, b: u32) -> crate::Flags {
    // For emulation purposes the same unordered comparison is used as COMISS;
    // the real instructions differ only in SIMD invalid-operation exception
    // signaling, which is not modeled.
    comiss_update_flags(flags, a, b)
}

pub fn movmskps(src: [u32; 4]) -> u32 {
    ((src[0] >> 31) & 1) | ((src[1] >> 30) & 2) | ((src[2] >> 29) & 4) | ((src[3] >> 28) & 8)
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

fn qword(a: [u32; 4], n: usize) -> f64 {
    let low = a[n * 2] as u64;
    let high = (a[n * 2 + 1] as u64) << 32;
    f64::from_bits(low | high)
}

fn qword2(src: [u32; 2]) -> f64 {
    f64::from_bits((src[0] as u64) | ((src[1] as u64) << 32))
}

fn set_qword(out: &mut [u32; 4], n: usize, value: f64) {
    let bits = value.to_bits();
    out[n * 2] = bits as u32;
    out[n * 2 + 1] = (bits >> 32) as u32;
}

fn binop_pd(a: [u32; 4], b: [u32; 4], op: impl Fn(f64, f64) -> f64) -> [u32; 4] {
    let mut out = [0u32; 4];
    for n in 0..2 {
        set_qword(&mut out, n, op(qword(a, n), qword(b, n)));
    }
    out
}

pub fn addpd(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_pd(a, b, |a, b| a + b)
}

pub fn subpd(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_pd(a, b, |a, b| a - b)
}

pub fn mulpd(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_pd(a, b, |a, b| a * b)
}

pub fn divpd(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    binop_pd(a, b, |a, b| a / b)
}

pub fn andpd(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| a & b)
}

pub fn andnpd(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| !a & b)
}

pub fn orpd(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| a | b)
}

pub fn xorpd(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    bitop_ps(a, b, |a, b| a ^ b)
}

fn scalar_binop_pd(dst: [u32; 4], src: [u32; 2], op: impl Fn(f64, f64) -> f64) -> [u32; 4] {
    let mut out = dst;
    set_qword(&mut out, 0, op(qword(dst, 0), qword2(src)));
    out
}

pub fn addsd(dst: [u32; 4], src: [u32; 2]) -> [u32; 4] {
    scalar_binop_pd(dst, src, |a, b| a + b)
}

pub fn subsd(dst: [u32; 4], src: [u32; 2]) -> [u32; 4] {
    scalar_binop_pd(dst, src, |a, b| a - b)
}

pub fn mulsd(dst: [u32; 4], src: [u32; 2]) -> [u32; 4] {
    scalar_binop_pd(dst, src, |a, b| a * b)
}

pub fn divsd(dst: [u32; 4], src: [u32; 2]) -> [u32; 4] {
    scalar_binop_pd(dst, src, |a, b| a / b)
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

fn cmppd_result(a: f64, b: f64, predicate: u8) -> bool {
    match predicate {
        0 => a == b,
        1 => a < b,
        2 => a <= b,
        3 => a.is_nan() || b.is_nan(),
        4 => a != b,
        5 => !(a < b),
        6 => !(a <= b),
        7 => !a.is_nan() && !b.is_nan(),
        _ => false,
    }
}

pub fn cmppd(a: [u32; 4], b: [u32; 4], predicate: u8) -> [u32; 4] {
    let mut out = [0u32; 4];
    for n in 0..2 {
        let bits = if cmppd_result(qword(a, n), qword(b, n), predicate) {
            0xffff_ffff_ffff_ffffu64
        } else {
            0
        };
        out[n * 2] = bits as u32;
        out[n * 2 + 1] = (bits >> 32) as u32;
    }
    out
}

pub fn cmpsd(dst: [u32; 4], src: [u32; 2], predicate: u8) -> [u32; 4] {
    let mut out = dst;
    let bits = if cmppd_result(qword(dst, 0), qword2(src), predicate) {
        0xffff_ffff_ffff_ffffu64
    } else {
        0
    };
    out[0] = bits as u32;
    out[1] = (bits >> 32) as u32;
    out
}

fn compare_unordered_f64(a: f64, b: f64) -> (bool, bool, bool) {
    if a.is_nan() || b.is_nan() {
        (true, true, true)
    } else if a == b {
        (false, true, false)
    } else if a < b {
        (true, false, false)
    } else {
        (false, false, false)
    }
}

pub fn comisd_update_flags(flags: crate::Flags, a: [u32; 2], b: [u32; 2]) -> crate::Flags {
    let (cf, zf, pf) = compare_unordered_f64(qword2(a), qword2(b));
    let mut flags = flags;
    flags.set(crate::Flags::CF, cf);
    flags.set(crate::Flags::ZF, zf);
    flags.set(crate::Flags::PF, pf);
    flags.remove(crate::Flags::OF | crate::Flags::SF | crate::Flags::AF);
    flags
}

pub fn ucomisd_update_flags(flags: crate::Flags, a: [u32; 2], b: [u32; 2]) -> crate::Flags {
    comisd_update_flags(flags, a, b)
}

pub fn movmskpd(src: [u32; 4]) -> u32 {
    ((src[1] >> 31) & 1) | (((src[3] >> 31) & 1) << 1)
}
