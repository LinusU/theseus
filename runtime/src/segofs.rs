#[repr(C)]
#[derive(
    zerocopy::FromBytes, zerocopy::IntoBytes, Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord,
)]
pub struct SegOfs {
    pub seg: u16,
    pub ofs: u16,
}

impl SegOfs {
    pub const fn new(seg: u16, ofs: u16) -> SegOfs {
        SegOfs { seg, ofs }
    }

    pub const fn abs(&self) -> u32 {
        segofs(self.seg, self.ofs)
    }

    pub const fn with_ofs(&self, ofs: u16) -> SegOfs {
        SegOfs { seg: self.seg, ofs }
    }
}

impl From<(u16, u16)> for SegOfs {
    fn from((seg, ofs): (u16, u16)) -> Self {
        SegOfs::new(seg, ofs)
    }
}

impl std::fmt::Display for SegOfs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{seg:04x}:{ofs:04x}", seg = self.seg, ofs = self.ofs)
    }
}

/// Combine a seg:ofs address into a single flat u32 address.
pub const fn segofs(seg: u16, off: u16) -> u32 {
    ((seg as u32) << 4) + (off as u32)
}
