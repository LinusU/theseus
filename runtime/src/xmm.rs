#[derive(Default)]
pub struct XMM {
    pub xmm0: [u32; 4],
    pub xmm1: [u32; 4],
    pub xmm2: [u32; 4],
    pub xmm3: [u32; 4],
    pub xmm4: [u32; 4],
    pub xmm5: [u32; 4],
    pub xmm6: [u32; 4],
    pub xmm7: [u32; 4],
}

impl XMM {
    pub const fn default() -> XMM {
        XMM {
            xmm0: [0; 4],
            xmm1: [0; 4],
            xmm2: [0; 4],
            xmm3: [0; 4],
            xmm4: [0; 4],
            xmm5: [0; 4],
            xmm6: [0; 4],
            xmm7: [0; 4],
        }
    }
}
