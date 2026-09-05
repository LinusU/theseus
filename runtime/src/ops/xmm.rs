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
