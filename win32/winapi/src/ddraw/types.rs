//! Types defined in the DirectDraw API.

use crate::{ABIReturn, ddraw::GUID, dllexport::win32flags};

// TODO: maybe make some shared const fn for errors that sets high bit?
// TODO: share constants with winapi ERROR type?
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum DD {
    OK = 0,
    E_NOTIMPL = 0x80004001,
    E_NOINTERFACE = 0x80004002,
    ERR_GENERIC = 0x80004005,
    ERR_INVALIDPARAMS = 0x80070057,
    /// DDERR_OUTOFMEMORY, which is E_OUTOFMEMORY.
    ERR_OUTOFMEMORY = 0x8007000e,
    ERR_NOCOLORKEY = 0x887600d4,
    ERR_NOTFOUND = 0x887600ff,
    ERR_WASSTILLDRAWING = 0x8876021c,
    ERR_NOCLIPPERATTACHED = 0x88760238,
    ERR_NOPALETTEATTACHED = 0x8876023c,
    ERR_NOTAOVERLAYSURFACE = 0x88760244,
    ERR_CANTDUPLICATE = 0x88760247,
    ERR_INVALIDSURFACETYPE = 0x88760250,
    ERR_MOREDATA = 0x887602b2,
    ERR_TESTFINISHED = 0x887602b4,
    ERR_NODIRECTDRAWHW = 0x88760233,
}

/// DDCKEY_* flags, selecting which of a surface's color keys an operation
/// refers to.
pub const DDCKEY_DESTOVERLAY: u32 = 0x0001;
pub const DDCKEY_DESTBLT: u32 = 0x0002;
pub const DDCKEY_SRCOVERLAY: u32 = 0x0004;
pub const DDCKEY_SRCBLT: u32 = 0x0008;

impl From<DD> for ABIReturn {
    fn from(val: DD) -> ABIReturn {
        (val as u32).into()
    }
}

#[repr(C)]
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    zerocopy::FromBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
    zerocopy::IntoBytes,
)]
pub struct DDSCAPS2 {
    pub dwCaps: DDSCAPS,
    pub dwCaps2: u32,
    pub dwCaps3: u32,
    pub dwCaps4: u32,
}

win32flags! {
    pub struct DDSCAPS {
        const ALPHA = 0x00000002;
        const BACKBUFFER = 0x00000004;
        const COMPLEX = 0x00000008;
        const FLIP = 0x00000010;
        const FRONTBUFFER = 0x00000020;
        const OFFSCREENPLAIN = 0x00000040;
        const OVERLAY = 0x00000080;
        const PALETTE = 0x00000100;
        const PRIMARYSURFACE = 0x00000200;
        const PRIMARYSURFACELEFT = 0x00000400;
        const SYSTEMMEMORY = 0x00000800;
        const TEXTURE = 0x00001000;
        const _3DDEVICE = 0x00002000;
        const VIDEOMEMORY = 0x00004000;
        const VISIBLE = 0x00008000;
        const WRITEONLY = 0x00010000;
        const ZBUFFER = 0x00020000;
        const OWNDC = 0x00040000;
        const LIVEVIDEO = 0x00080000;
        const HWCODEC = 0x00100000;
        const MODEX = 0x00200000;
        const MIPMAP = 0x00400000;
        const ALLOCONLOAD = 0x04000000;
        const VIDEOPORT = 0x08000000;
        const LOCALVIDMEM = 0x10000000;
        const NONLOCALVIDMEM = 0x20000000;
        const STANDARDVGAMODE = 0x40000000;
    }
}

win32flags! {
    pub struct DDSD {
        const CAPS = 0x00000001;
        const HEIGHT = 0x00000002;
        const WIDTH = 0x00000004;
        const PITCH = 0x00000008;
        const BACKBUFFERCOUNT = 0x00000020;
        const ZBUFFERBITDEPTH = 0x00000040;
        const ALPHABITDEPTH = 0x00000080;
        const LPSURFACE = 0x00000800;
        const PIXELFORMAT = 0x00001000;
        const CKDESTOVERLAY = 0x00002000;
        const CKDESTBLT = 0x00004000;
        const CKSRCOVERLAY= 0x00008000;
        const CKSRCBLT = 0x00010000;
        const MIPMAPCOUNT = 0x00020000;
        const REFRESHRATE = 0x00040000;
        const LINEARSIZE = 0x00080000;
        const TEXTURESTAGE = 0x00100000;
        const FVF = 0x00200000;
        const SRCVBHANDLE = 0x00400000;
        const DEPTH = 0x00800000;
    }
}

win32flags! {
    pub struct DDPCAPS {
        const _4BIT = 0x00000001;
        const _8BITENTRIES = 0x00000002;
        const _8BIT = 0x00000004;
        const INITIALIZE = 0x00000008;
        const PRIMARYSURFACE = 0x00000010;
        const PRIMARYSURFACELEFT = 0x00000020;
        const ALLOW256 = 0x00000040;
        const VSYNC = 0x00000080;
        const _1BIT = 0x00000100;
        const _2BIT = 0x00000200;
        const ALPHA =  0x00000400;
    }
}

win32flags! {
    pub struct DDLOCK {
        const SURFACEMEMORYPTR = 0x00000000;
        const WAIT = 0x00000001;
        const EVENT = 0x00000002;
        const READONLY = 0x00000010;
        const WRITEONLY = 0x00000020;
        const NOSYSLOCK = 0x00000800;
        const NOOVERWRITE = 0x00001000;
        const DISCARDCONTENTS = 0x00002000;
        const OKTOSWAP = 0x00002000;
        const DONOTWAIT = 0x00004000;
        const HASVOLUMETEXTUREBOXRECT = 0x00008000;
        const NODIRTYUPDATE = 0x00010000;
    }
}

#[repr(C)]
#[derive(
    Clone,
    Debug,
    Default,
    zerocopy::FromBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
    zerocopy::IntoBytes,
)]
pub struct DDCOLORKEY {
    pub dwColorSpaceLowValue: u32,
    pub dwColorSpaceHighValue: u32,
}

#[repr(C)]
#[derive(
    Default, zerocopy::FromBytes, zerocopy::Immutable, zerocopy::KnownLayout, zerocopy::IntoBytes,
)]
pub struct DDSURFACEDESC {
    pub dwSize: u32,
    pub dwFlags: DDSD,
    pub dwHeight: u32,
    pub dwWidth: u32,

    pub lPitch_dwLinearSize: u32,
    pub dwBackBufferCount: u32,
    pub dwMipMapCount_dwZBufferBitDepth_dwRefreshRate: u32,
    pub dwAlphaBitDepth: u32,
    pub dwReserved: u32,
    pub lpSurface: u32,
    pub ddckCKDestOverlay: DDCOLORKEY,
    pub ddckCKDestBlt: DDCOLORKEY,
    pub ddckCKSrcOverlay: DDCOLORKEY,
    pub ddckCKSrcBlt: DDCOLORKEY,
    pub ddpfPixelFormat: DDPIXELFORMAT,
    pub ddsCaps: DDSCAPS,
}

/// Custom implementation of Debug that only displays the fields that have been marked present in the flags member.
impl std::fmt::Debug for DDSURFACEDESC {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut st = f.debug_struct("DDSURFACEDESC");
        st.field("dwSize", &self.dwSize);
        st.field("dwFlags", &self.dwFlags);
        if self.dwFlags.contains(DDSD::HEIGHT) {
            st.field("dwHeight", &self.dwHeight);
        }
        if self.dwFlags.contains(DDSD::WIDTH) {
            st.field("dwWidth", &self.dwWidth);
        }
        if self.dwFlags.contains(DDSD::PITCH) {
            st.field("lPitch", &self.lPitch_dwLinearSize);
        }
        if self.dwFlags.contains(DDSD::LINEARSIZE) {
            st.field("dwLinearSize", &self.lPitch_dwLinearSize);
        }
        if self.dwFlags.contains(DDSD::BACKBUFFERCOUNT) {
            st.field("dwBackBufferCount", &self.dwBackBufferCount);
        }
        if self.dwFlags.contains(DDSD::MIPMAPCOUNT) {
            st.field(
                "dwMipMapCount_dwZBufferBitDepth_dwRefreshRate",
                &self.dwMipMapCount_dwZBufferBitDepth_dwRefreshRate,
            );
        }
        if self.dwFlags.contains(DDSD::ALPHABITDEPTH) {
            st.field("dwAlphaBitDepth", &self.dwAlphaBitDepth);
        }
        if self.dwFlags.contains(DDSD::LPSURFACE) {
            st.field("lpSurface", &self.lpSurface);
        }
        if self.dwFlags.contains(DDSD::CKDESTOVERLAY) {
            st.field("ddckCKDestOverlay", &self.ddckCKDestOverlay);
        }
        if self.dwFlags.contains(DDSD::CKDESTBLT) {
            st.field("ddckCKDestBlt", &self.ddckCKDestBlt);
        }
        if self.dwFlags.contains(DDSD::CKSRCOVERLAY) {
            st.field("ddckCKSrcOverlay", &self.ddckCKSrcOverlay);
        }
        if self.dwFlags.contains(DDSD::CKSRCBLT) {
            st.field("ddckCKSrcBlt", &self.ddckCKSrcBlt);
        }
        if self.dwFlags.contains(DDSD::PIXELFORMAT) {
            st.field("ddpfPixelFormat", &self.ddpfPixelFormat);
        }
        if self.dwFlags.contains(DDSD::CAPS) {
            st.field("ddsCaps", &self.ddsCaps);
        }
        st.finish()
    }
}

impl DDSURFACEDESC {
    pub fn caps(&self) -> Option<&DDSCAPS> {
        if !self.dwFlags.contains(DDSD::CAPS) {
            return None;
        }
        Some(&self.ddsCaps)
    }
    pub fn back_buffer_count(&self) -> Option<u32> {
        if !self.dwFlags.contains(DDSD::BACKBUFFERCOUNT) {
            return None;
        }
        Some(self.dwBackBufferCount)
    }

    pub fn from_desc2(desc2: &DDSURFACEDESC2) -> DDSURFACEDESC {
        DDSURFACEDESC {
            dwSize: std::mem::size_of::<DDSURFACEDESC>() as u32,
            dwFlags: desc2.dwFlags,
            dwHeight: desc2.dwHeight,
            dwWidth: desc2.dwWidth,

            lPitch_dwLinearSize: desc2.lPitch_dwLinearSize,
            dwBackBufferCount: desc2.dwBackBufferCount_dwDepth,
            dwMipMapCount_dwZBufferBitDepth_dwRefreshRate: Default::default(),
            dwAlphaBitDepth: Default::default(),
            dwReserved: Default::default(),
            lpSurface: desc2.lpSurface,
            ddckCKDestOverlay: Default::default(),
            ddckCKDestBlt: Default::default(),
            ddckCKSrcOverlay: Default::default(),
            ddckCKSrcBlt: Default::default(),
            ddpfPixelFormat: desc2.ddpfPixelFormat.clone(),
            ddsCaps: desc2.ddsCaps.dwCaps,
        }
    }
}

#[repr(C)]
#[derive(
    Default, zerocopy::FromBytes, zerocopy::Immutable, zerocopy::KnownLayout, zerocopy::IntoBytes,
)]
pub struct DDSURFACEDESC2 {
    pub dwSize: u32,
    pub dwFlags: DDSD,
    pub dwHeight: u32,
    pub dwWidth: u32,

    pub lPitch_dwLinearSize: u32,
    pub dwBackBufferCount_dwDepth: u32,
    pub dwMipMapCount_dwRefreshRate_dwSrcVBHandle: u32,

    pub dwAlphaBitDepth: u32,
    pub dwReserved: u32,
    pub lpSurface: u32,
    pub ddckCKDestOverlay_dwEmptyFaceColor: DDCOLORKEY,
    pub ddckCKDestBlt: DDCOLORKEY,
    pub ddckCKSrcOverlay: DDCOLORKEY,
    pub ddckCKSrcBlt: DDCOLORKEY,

    /// Union with `dwFVF` (vertex-buffer descriptors set DDSD::FVF and read
    /// the first dword of this field as the FVF code).
    pub ddpfPixelFormat: DDPIXELFORMAT,
    pub ddsCaps: DDSCAPS2,
    pub dwTextureStage: u32,
}

/// Custom implementation of Debug that only displays the fields that have been marked present in the flags member.
impl std::fmt::Debug for DDSURFACEDESC2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut st = f.debug_struct("DDSURFACEDESC2");
        st.field("dwSize", &self.dwSize);
        st.field("dwFlags", &self.dwFlags);
        if self.dwFlags.contains(DDSD::HEIGHT) {
            st.field("dwHeight", &self.dwHeight);
        }
        if self.dwFlags.contains(DDSD::WIDTH) {
            st.field("dwWidth", &self.dwWidth);
        }
        if self.dwFlags.contains(DDSD::PITCH) {
            st.field("lPitch_dwLinearSize", &self.lPitch_dwLinearSize);
        }
        if self.dwFlags.contains(DDSD::BACKBUFFERCOUNT) {
            st.field("dwBackBufferCount_dwDepth", &self.dwBackBufferCount_dwDepth);
        }
        if self.dwFlags.contains(DDSD::MIPMAPCOUNT) {
            st.field(
                "dwMipMapCount_dwRefreshRate_dwSrcVBHandle",
                &self.dwMipMapCount_dwRefreshRate_dwSrcVBHandle,
            );
        }
        if self.dwFlags.contains(DDSD::ALPHABITDEPTH) {
            st.field("dwAlphaBitDepth", &self.dwAlphaBitDepth);
        }
        if self.dwFlags.contains(DDSD::LPSURFACE) {
            st.field("lpSurface", &self.lpSurface);
        }
        if self.dwFlags.contains(DDSD::CKDESTOVERLAY) {
            st.field(
                "ddckCKDestOverlay_dwEmptyFaceColor",
                &self.ddckCKDestOverlay_dwEmptyFaceColor,
            );
        }
        if self.dwFlags.contains(DDSD::CKDESTBLT) {
            st.field("ddckCKDestBlt", &self.ddckCKDestBlt);
        }
        if self.dwFlags.contains(DDSD::CKSRCOVERLAY) {
            st.field("ddckCKSrcOverlay", &self.ddckCKSrcOverlay);
        }
        if self.dwFlags.contains(DDSD::CKSRCBLT) {
            st.field("ddckCKSrcBlt", &self.ddckCKSrcBlt);
        }
        if self.dwFlags.contains(DDSD::PIXELFORMAT) {
            st.field("ddpfPixelFormat", &self.ddpfPixelFormat);
        }
        if self.dwFlags.contains(DDSD::FVF) {
            // dwFVF unions with ddpfPixelFormat's first dword.
            st.field("dwFVF", &self.ddpfPixelFormat.dwSize);
        }
        if self.dwFlags.contains(DDSD::CAPS) {
            st.field("ddsCaps", &self.ddsCaps);
        }
        if self.dwFlags.contains(DDSD::TEXTURESTAGE) {
            st.field("dwTextureStage", &self.dwTextureStage);
        }
        st.finish()
    }
}

impl DDSURFACEDESC2 {
    pub fn back_buffer_count(&self) -> Option<u32> {
        if !self.dwFlags.contains(DDSD::BACKBUFFERCOUNT) {
            return None;
        }
        Some(self.dwBackBufferCount_dwDepth)
    }

    pub fn caps(&self) -> Option<&DDSCAPS2> {
        if !self.dwFlags.contains(DDSD::CAPS) {
            return None;
        }
        Some(&self.ddsCaps)
    }

    pub fn from_desc(desc: &DDSURFACEDESC) -> DDSURFACEDESC2 {
        DDSURFACEDESC2 {
            dwSize: std::mem::size_of::<DDSURFACEDESC2>() as u32,
            dwFlags: desc.dwFlags,
            dwHeight: desc.dwHeight,
            dwWidth: desc.dwWidth,
            lPitch_dwLinearSize: desc.lPitch_dwLinearSize,
            dwBackBufferCount_dwDepth: desc.dwBackBufferCount,
            dwMipMapCount_dwRefreshRate_dwSrcVBHandle: Default::default(),
            dwAlphaBitDepth: Default::default(),
            dwReserved: Default::default(),
            lpSurface: desc.lpSurface,
            ddckCKDestOverlay_dwEmptyFaceColor: Default::default(),
            ddckCKDestBlt: Default::default(),
            ddckCKSrcOverlay: Default::default(),
            ddckCKSrcBlt: Default::default(),
            ddpfPixelFormat: Default::default(),
            ddsCaps: DDSCAPS2 {
                dwCaps: desc.ddsCaps,
                dwCaps2: Default::default(),
                dwCaps3: Default::default(),
                dwCaps4: Default::default(),
            },
            dwTextureStage: Default::default(),
        }
    }
}

#[repr(C)]
#[derive(
    Debug,
    Default,
    Clone,
    zerocopy::FromBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
    zerocopy::IntoBytes,
)]
pub struct DDPIXELFORMAT {
    pub dwSize: u32,
    pub dwFlags: u32,
    pub dwFourCC: u32,
    pub dwRGBBitCount: u32,
    pub dwRBitMask: u32,
    pub dwGBitMask: u32,
    pub dwBBitMask: u32,
    pub dwRGBAlphaBitMask: u32,
}

win32flags! {
    pub struct DDBLT {
        const ALPHADEST                = 0x00000001;
        const ALPHADESTCONSTOVERRIDE   = 0x00000002;
        const ALPHADESTNEG             = 0x00000004;
        const ALPHADESTSURFACEOVERRIDE = 0x00000008;
        const ALPHAEDGEBLEND           = 0x00000010;
        const ALPHASRC                 = 0x00000020;
        const ALPHASRCCONSTOVERRIDE    = 0x00000040;
        const ALPHASRCNEG              = 0x00000080;
        const ALPHASRCSURFACEOVERRIDE  = 0x00000100;
        const ASYNC                    = 0x00000200;
        const COLORFILL                = 0x00000400;
        const DDFX                     = 0x00000800;
        const DDROPS                   = 0x00001000;
        const KEYDEST                  = 0x00002000;
        const KEYDESTOVERRIDE          = 0x00004000;
        const KEYSRC                   = 0x00008000;
        const KEYSRCOVERRIDE           = 0x00010000;
        const ROP                      = 0x00020000;
        const ROTATIONANGLE            = 0x00040000;
        const ZBUFFER                  = 0x00080000;
        const ZBUFFERDESTCONSTOVERRIDE = 0x00100000;
        const ZBUFFERDESTOVERRIDE      = 0x00200000;
        const ZBUFFERSRCCONSTOVERRIDE  = 0x00400000;
        const ZBUFFERSRCOVERRIDE       = 0x00800000;
        const WAIT                     = 0x01000000;
        const DEPTHFILL                = 0x02000000;
        const DONOTWAIT                = 0x08000000;
  }
}

win32flags! {
    pub struct DDBLTFAST {
        // const NOCOLORKEY   = 0x00000000;
        const SRCCOLORKEY  = 0x00000001;
        const DESTCOLORKEY = 0x00000002;
        const WAIT         = 0x00000010;
        const DONOTWAIT    = 0x00000020;
  }
}

#[repr(C)]
#[derive(Debug)]
pub struct DDBLTFX {
    pub dwSize: u32,
    pub dwDDFX: DDBLTFXT,
    pub dwROP: u32,
    pub dwDDROP: u32,
    pub dwRotationAngle: u32,
    pub dwZBufferOpCode: u32,
    pub dwZBufferLow: u32,
    pub dwZBufferHigh: u32,
    pub dwZBufferBaseDest: u32,
    pub dwZDestConstBitDepth: u32,
    pub zDest: u32,
    pub dwZSrcConstBitDepth: u32,
    pub zSrc: u32,
    pub dwAlphaEdgeBlendBitDepth: u32,
    pub dwAlphaEdgeBlend: u32,
    pub dwReserved: u32,
    pub dwAlphaDestConstBitDepth: u32,
    pub alphaDest: u32,
    pub dwAlphaSrcConstBitDepth: u32,
    pub alphaSrc: u32,
    pub dwFillColor: u32,
    pub ddckDestColorkey: DDCOLORKEY,
    pub ddckSrcColorkey: DDCOLORKEY,
}

win32flags! {
    pub struct DDBLTFXT {
        const ARITHSTRETCHY   = 0x001;
        const MIRRORLEFTRIGHT = 0x002;
        const MIRRORUPDOWN    = 0x004;
        const NOTEARING       = 0x008;
        const ROTATE180       = 0x010;
        const ROTATE270       = 0x020;
        const ROTATE90        = 0x040;
        const ZBUFFERRANGE    = 0x080;
        const ZBUFFERBASEDEST = 0x100;
    }
}

#[repr(C)]
#[derive(
    Debug,
    Clone,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct PALETTEENTRY {
    pub peRed: u8,
    pub peGreen: u8,
    pub peBlue: u8,
    pub peFlags: u8,
}

#[repr(C, packed)]
#[derive(
    Debug,
    Clone,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct DDDEVICEIDENTIFIER {
    pub szDriver: [u8; 512],
    pub szDescription: [u8; 512],
    pub liDriverVersion: i64,
    pub dwVendorId: u32,
    pub dwDeviceId: u32,
    pub dwSubSysId: u32,
    pub dwRevision: u32,
    pub guidDeviceIdentifier: GUID,
    pub dwWHQLLevel: u32,
}

#[repr(C, packed)]
#[derive(
    Debug,
    Clone,
    Copy,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct DDCAPS_DX7 {
    pub dwSize: u32,
    pub dwCaps: u32,
    pub dwCaps2: u32,
    pub dwCKeyCaps: u32,
    pub dwFXCaps: u32,
    pub dwFXAlphaCaps: u32,
    pub dwPalCaps: u32,
    pub dwSVCaps: u32,
    pub dwAlphaBltConstBitDepths: u32,
    pub dwAlphaBltPixelBitDepths: u32,
    pub dwAlphaBltSurfaceBitDepths: u32,
    pub dwAlphaOverlayConstBitDepths: u32,
    pub dwAlphaOverlayPixelBitDepths: u32,
    pub dwAlphaOverlaySurfaceBitDepths: u32,
    pub dwZBufferBitDepths: u32,
    pub dwVidMemTotal: u32,
    pub dwVidMemFree: u32,
    pub dwMaxVisibleOverlays: u32,
    pub dwCurrVisibleOverlays: u32,
    pub dwNumFourCCCodes: u32,
    pub dwAlignBoundarySrc: u32,
    pub dwAlignSizeSrc: u32,
    pub dwAlignBoundaryDest: u32,
    pub dwAlignSizeDest: u32,
    pub dwAlignStrideAlign: u32,
    pub dwRops: [u32; 8],
    pub ddsOldCaps: DDSCAPS,
    pub dwMinOverlayStretch: u32,
    pub dwMaxOverlayStretch: u32,
    pub dwMinLiveVideoStretch: u32,
    pub dwMaxLiveVideoStretch: u32,
    pub dwMinHwCodecStretch: u32,
    pub dwMaxHwCodecStretch: u32,
    pub dwReserved1: u32,
    pub dwReserved2: u32,
    pub dwReserved3: u32,
    pub dwSVBCaps: u32,
    pub dwSVBCKeyCaps: u32,
    pub dwSVBFXCaps: u32,
    pub dwSVBRops: [u32; 8],
    pub dwVSBCaps: u32,
    pub dwVSBCKeyCaps: u32,
    pub dwVSBFXCaps: u32,
    pub dwVSBRops: [u32; 8],
    pub dwSSBCaps: u32,
    pub dwSSBCKeyCaps: u32,
    pub dwSSBFXCaps: u32,
    pub dwSSBRops: [u32; 8],
    pub dwMaxVideoPorts: u32,
    pub dwCurrVideoPorts: u32,
    pub dwSVBCaps2: u32,
    pub dwNLVBCaps: u32,
    pub dwNLVBCaps2: u32,
    pub dwNLVBCKeyCaps: u32,
    pub dwNLVBFXCaps: u32,
    pub dwNLVBRops: [u32; 8],
    pub ddsCaps: DDSCAPS2,
}

#[repr(C, packed)]
#[derive(
    Debug,
    Clone,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct DDDEVICEIDENTIFIER2 {
    pub szDriver: [u8; 512],
    pub szDescription: [u8; 512],
    pub liDriverVersion: i64,
    pub dwVendorId: u32,
    pub dwDeviceId: u32,
    pub dwSubSysId: u32,
    pub dwRevision: u32,
    pub guidDeviceIdentifier: GUID,
    pub dwWHQLLevel: u32,
    pub dwReserved1: u32,
    pub dwReserved2: u32,
    pub dwReserved3: u32,
    pub dwReserved4: u32,
}
