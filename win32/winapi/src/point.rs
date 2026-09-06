#[repr(C)]
#[derive(
    Copy, Clone, Debug, Default, zerocopy::FromBytes, zerocopy::IntoBytes, zerocopy::Immutable,
)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

impl POINT {
    // Guest coordinate arithmetic wraps rather than panicking on overflow,
    // matching the unchecked C arithmetic of the real API.
    pub fn add(&self, delta: POINT) -> POINT {
        POINT {
            x: self.x.wrapping_add(delta.x),
            y: self.y.wrapping_add(delta.y),
        }
    }

    pub fn sub(&self, delta: POINT) -> POINT {
        POINT {
            x: self.x.wrapping_sub(delta.x),
            y: self.y.wrapping_sub(delta.y),
        }
    }

    pub fn mul(&self, o: POINT) -> POINT {
        POINT {
            x: self.x.wrapping_mul(o.x),
            y: self.y.wrapping_mul(o.y),
        }
    }

    pub fn div(&self, o: POINT) -> POINT {
        POINT {
            x: self.x.checked_div(o.x).unwrap_or(0),
            y: self.y.checked_div(o.y).unwrap_or(0),
        }
    }
}
