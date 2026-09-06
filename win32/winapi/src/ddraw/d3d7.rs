//! Direct3D 7 interfaces hung off `IDirectDraw7::QueryInterface`.
//!
//! There is no rasterizer in the emulated host: the device tracks all the
//! state the API can express (transforms, viewports, materials, lights,
//! render states, texture bindings, clip state) so Get/Set pairs round-trip
//! correctly, while the drawing entry points are acknowledged without
//! producing pixels.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use runtime::*;

use super::types::{DD, DDPIXELFORMAT, DDSCAPS};
use crate::{
    RECT,
    ddraw::{GUID, state},
    heap::Heap,
    kernel32,
};

fn log_vertex_start(ctx: &mut Context, v: u32, vcount: u32, fvf: u32) {
    if !log::log_enabled!(log::Level::Debug) || v == 0 || vcount == 0 || fvf == 0 {
        return;
    }
    let size = vertex_size(fvf);
    let max = (size * vcount.min(2)).min(64) as usize;
    let max = max.min(ctx.memory.bytes.len().saturating_sub(v as usize));
    let mut bytes = Vec::with_capacity(max);
    for i in 0..max as u32 {
        bytes.push(ctx.memory.read::<u8>(v + i));
    }
    log::debug!("  vertex data: {:02x?}", bytes);
}

pub const IID_IDirect3D7: GUID = GUID::new(
    0xf5049e77,
    0x4861,
    0x11d2,
    [0xa4, 0x07, 0x00, 0xa0, 0xc9, 0x06, 0x29, 0xa8],
);
const IID_IDIRECT3DVERTEXBUFFER7: GUID = GUID::new(
    0xf5049e7d,
    0x4861,
    0x11d2,
    [0xa4, 0x07, 0x00, 0xa0, 0xc9, 0x06, 0x29, 0xa8],
);
const IID_IDIRECT3DDEVICE7: GUID = GUID::new(
    0xf5049e7e,
    0x4861,
    0x11d2,
    [0xa4, 0x07, 0x00, 0xa0, 0xc9, 0x06, 0x29, 0xa8],
);

// The device GUIDs a Direct3D 7 driver of this era enumerates.
const IID_IDIRECT3DTNLHALDEVICE: GUID = GUID::new(
    0xf5049e78,
    0x4861,
    0x11d2,
    [0xa4, 0x07, 0x00, 0xa0, 0xc9, 0x06, 0x29, 0xa8],
);
const IID_IDIRECT3DHALDEVICE: GUID = GUID::new(
    0x84e63de0,
    0x46aa,
    0x11cf,
    [0x81, 0x6f, 0x00, 0x00, 0xc0, 0x20, 0x15, 0x6e],
);
const IID_IDIRECT3DRGBDEVICE: GUID = GUID::new(
    0xa4665c60,
    0xa267,
    0x11cf,
    [0x81, 0x6f, 0x00, 0x00, 0xc0, 0x20, 0x15, 0x6e],
);

const IID_IUNKNOWN: GUID = GUID::new(0, 0, 0, [0; 8]);

#[repr(C)]
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct D3DPRIMCAPS {
    pub dwSize: u32,
    pub dwMiscCaps: u32,
    pub dwRasterCaps: u32,
    pub dwZCmpCaps: u32,
    pub dwSrcBlendCaps: u32,
    pub dwDestBlendCaps: u32,
    pub dwAlphaCmpCaps: u32,
    pub dwShadeCaps: u32,
    pub dwTextureCaps: u32,
    pub dwTextureFilterCaps: u32,
    pub dwTextureBlendCaps: u32,
    pub dwTextureAddressCaps: u32,
    pub dwStippleWidth: u32,
    pub dwStippleHeight: u32,
}

#[repr(C)]
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct D3DDEVICEDESC7 {
    pub dwDevCaps: u32,
    pub dpcLineCaps: D3DPRIMCAPS,
    pub dpcTriCaps: D3DPRIMCAPS,
    pub dwDeviceRenderBitDepth: u32,
    pub dwDeviceZBufferBitDepth: u32,
    pub dwMinTextureWidth: u32,
    pub dwMinTextureHeight: u32,
    pub dwMaxTextureWidth: u32,
    pub dwMaxTextureHeight: u32,
    pub dwMaxTextureRepeat: u32,
    pub dwMaxTextureAspectRatio: u32,
    pub dwMaxAnisotropy: u32,
    pub dvGuardBandLeft: f32,
    pub dvGuardBandRight: f32,
    pub dvGuardBandTop: f32,
    pub dvGuardBandBottom: f32,
    pub dvExtentsAdjust: f32,
    pub dwStencilCaps: u32,
    pub dwFVFCaps: u32,
    pub dwTextureOpCaps: u32,
    pub wMaxTextureBlendStages: u16,
    pub wMaxSimultaneousTextures: u16,
    pub dwMaxActiveLights: u32,
    pub dvMaxVertexW: f32,
    pub deviceGUID: GUID,
    pub wMaxUserClipPlanes: u16,
    pub wMaxVertexBlendMatrices: u16,
    pub dwVertexProcessingCaps: u32,
    pub dwReserved1: u32,
    pub dwReserved2: u32,
    pub dwReserved3: u32,
    pub dwReserved4: u32,
}

#[repr(C)]
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct D3DVERTEXBUFFERDESC {
    pub dwSize: u32,
    pub dwCaps: u32,
    pub dwFVF: u32,
    pub dwNumVertices: u32,
}

#[repr(C)]
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct D3DVIEWPORT7 {
    pub dwX: u32,
    pub dwY: u32,
    pub dwWidth: u32,
    pub dwHeight: u32,
    pub dvMinZ: f32,
    pub dvMaxZ: f32,
}

#[repr(C)]
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct D3DMATERIAL7 {
    pub diffuse: [f32; 4],
    pub ambient: [f32; 4],
    pub specular: [f32; 4],
    pub emissive: [f32; 4],
    pub power: f32,
}

#[repr(C)]
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct D3DCLIPSTATUS {
    pub dwFlags: u32,
    pub dwStatus: u32,
    pub minx: f32,
    pub maxx: f32,
    pub miny: f32,
    pub maxy: f32,
    pub minz: f32,
    pub maxz: f32,
}

/// sizeof(D3DLIGHT7); the contents are opaque to the no-rasterizer model.
const D3DLIGHT7_SIZE: usize = 104;

/// A high-end-for-2000 device description, matching the generous capability
/// reporting the DirectDraw `GetCaps` path already uses.
fn device_desc7(guid: GUID) -> D3DDEVICEDESC7 {
    let prim = D3DPRIMCAPS {
        dwSize: std::mem::size_of::<D3DPRIMCAPS>() as u32,
        dwMiscCaps: 0x003F_CFFF,
        dwRasterCaps: 0x0F7F_F1F1,
        dwZCmpCaps: 0xFF,
        dwSrcBlendCaps: 0x3FFF,
        dwDestBlendCaps: 0x3FFF,
        dwAlphaCmpCaps: 0xFF,
        dwShadeCaps: 0x000F_0FCF,
        dwTextureCaps: 0x03FD_3F77,
        dwTextureFilterCaps: 0x0707_FFFF,
        dwTextureBlendCaps: 0xFF,
        dwTextureAddressCaps: 0x3F,
        dwStippleWidth: 32,
        dwStippleHeight: 32,
    };
    D3DDEVICEDESC7 {
        dwDevCaps: 0x001F_DFFF,
        dpcLineCaps: prim,
        dpcTriCaps: prim,
        dwDeviceRenderBitDepth: 0x200 | 0x800, // DDBD_16 | DDBD_32
        dwDeviceZBufferBitDepth: 0x200 | 0x400 | 0x800,
        dwMinTextureWidth: 1,
        dwMinTextureHeight: 1,
        dwMaxTextureWidth: 2048,
        dwMaxTextureHeight: 2048,
        dwMaxTextureRepeat: 2048,
        dwMaxTextureAspectRatio: 2048,
        dwMaxAnisotropy: 2,
        dvGuardBandLeft: 0.0,
        dvGuardBandRight: 0.0,
        dvGuardBandTop: 0.0,
        dvGuardBandBottom: 0.0,
        dvExtentsAdjust: 0.0,
        dwStencilCaps: 0xFF,
        dwFVFCaps: 0x0008_0008, // DONOTSTRIPELEMENTS | 8 texture coord sets
        dwTextureOpCaps: 0x03FF_FFFC,
        wMaxTextureBlendStages: 8,
        wMaxSimultaneousTextures: 4,
        dwMaxActiveLights: u32::MAX,
        dvMaxVertexW: 1.0e10,
        deviceGUID: guid,
        wMaxUserClipPlanes: 6,
        wMaxVertexBlendMatrices: 4,
        dwVertexProcessingCaps: 0xFF,
        dwReserved1: 0,
        dwReserved2: 0,
        dwReserved3: 0,
        dwReserved4: 0,
    }
}

/// Texture pixel formats the emulated device enumerates, most common first.
fn texture_formats() -> Vec<DDPIXELFORMAT> {
    let rgb = |count: u32, r: u32, g: u32, b: u32, a: u32, extra: u32| DDPIXELFORMAT {
        dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
        dwFlags: 0x40 | extra, // DDPF_RGB
        dwFourCC: 0,
        dwRGBBitCount: count,
        dwRBitMask: r,
        dwGBitMask: g,
        dwBBitMask: b,
        dwRGBAlphaBitMask: a,
    };
    vec![
        rgb(16, 0xF800, 0x07E0, 0x001F, 0, 0),      // R5G6B5
        rgb(16, 0x7C00, 0x03E0, 0x001F, 0, 0),      // X1R5G5B5
        rgb(16, 0x0F00, 0x00F0, 0x000F, 0xF000, 0), // A4R4G4B4
        rgb(16, 0x7C00, 0x03E0, 0x001F, 0x8000, 0), // A1R5G5B5
        // 32-bit surfaces store RGBA bytes, so R is the low byte here.
        rgb(32, 0x00FF, 0xFF00, 0xFF0000, 0xFF000000, 0), // A8R8G8B8
        rgb(32, 0x00FF, 0xFF00, 0xFF0000, 0, 0),          // X8R8G8B8
        rgb(8, 0, 0, 0, 0, 0x20),                         // palettized
    ]
}

fn zbuffer_formats() -> Vec<DDPIXELFORMAT> {
    // For depth formats the unions read as dwZBufferBitDepth, then
    // dwStencilBitDepth, dwZBitMask, dwStencilBitMask.
    let z = |depth: u32, stencil: u32, zmask: u32, smask: u32| DDPIXELFORMAT {
        dwSize: std::mem::size_of::<DDPIXELFORMAT>() as u32,
        dwFlags: 0x400, // DDPF_ZBUFFER
        dwFourCC: 0,
        dwRGBBitCount: depth,
        dwRBitMask: stencil,
        dwGBitMask: zmask,
        dwBBitMask: smask,
        dwRGBAlphaBitMask: 0,
    };
    vec![
        z(16, 0, 0x0000_FFFF, 0),
        z(32, 0, 0xFFFF_FFFF, 0),
        z(32, 8, 0xFFFF_FF00, 0x0000_00FF), // D24S8
    ]
}

/// Byte stride of a flexible-vertex-format vertex.
fn vertex_size(fvf: u32) -> u32 {
    let mut size = match fvf & 0x400E {
        0x002 => 12,                                       // D3DFVF_XYZ
        0x004 => 16,                                       // D3DFVF_XYZRHW
        0x4002 => 16,                                      // D3DFVF_XYZW
        n @ (0x006..=0x00e) => 16 + ((n - 0x006) / 2) * 4, // D3DFVF_XYZB1..5
        _ => 12,
    };
    if fvf & 0x1000 != 0 || fvf & 0x8000 != 0 {
        size += 4; // D3DFVF_LASTBETA_UBYTE4 / LASTBETA_D3DCOLOR
    }
    if fvf & 0x010 != 0 {
        size += 4 * 3; // D3DFVF_NORMAL
    }
    if fvf & 0x020 != 0 {
        size += 4; // D3DFVF_PSIZE
    }
    if fvf & 0x040 != 0 {
        size += 4; // D3DFVF_DIFFUSE
    }
    if fvf & 0x080 != 0 {
        size += 4; // D3DFVF_SPECULAR
    }
    for i in 0..((fvf & 0x0F00) >> 8) {
        // D3DFVF_TEXCOORDSIZE fields are two bits each starting at bit 16;
        // the encoded value maps to {2, 3, 4, 1} floats.
        let floats = [2, 3, 4, 1][((fvf >> (16 + i * 2)) & 3) as usize];
        size += floats * 4;
    }
    size
}

pub struct Device {
    pub addr: u32,
    pub d3d: u32,
    pub render_target: u32,
    transforms: HashMap<u32, [f32; 16]>,
    viewport: Option<D3DVIEWPORT7>,
    material: Option<D3DMATERIAL7>,
    lights: HashMap<u32, [u8; D3DLIGHT7_SIZE]>,
    lights_enabled: HashMap<u32, bool>,
    render_states: HashMap<u32, u32>,
    texture_stage_states: HashMap<(u32, u32), u32>,
    textures: HashMap<u32, u32>,
    clip_status: Option<D3DCLIPSTATUS>,
    clip_planes: HashMap<u32, [f32; 4]>,
    in_scene: bool,
    /// Pixels rasterized since the last full-target clear.
    drew_since_clear: bool,
}

impl Device {
    fn new(addr: u32, d3d: u32, render_target: u32) -> Self {
        Device {
            addr,
            d3d,
            render_target,
            transforms: HashMap::new(),
            viewport: None,
            material: None,
            lights: HashMap::new(),
            lights_enabled: HashMap::new(),
            render_states: HashMap::new(),
            texture_stage_states: HashMap::new(),
            textures: HashMap::new(),
            clip_status: None,
            clip_planes: HashMap::new(),
            in_scene: false,
            drew_since_clear: false,
        }
    }
}

pub struct VertexBuffer {
    pub addr: u32,
    pub desc: D3DVERTEXBUFFERDESC,
    pub data: u32,
    pub data_len: u32,
}

#[derive(Default)]
pub struct D3DState {
    pub devices: RefCell<HashMap<u32, Device>>,
    pub vertex_buffers: RefCell<HashMap<u32, VertexBuffer>>,
    /// Texture surface addresses already reported as never-written, so the
    /// rasterizer diagnostic logs each one only once.
    unwritten_textures: RefCell<std::collections::HashSet<u32>>,
    /// Texture surface addresses already dumped via THESEUS_TEX_DUMP.
    dumped_textures: RefCell<std::collections::HashSet<u32>>,
    state_blocks: RefCell<std::collections::HashSet<u32>>,
    next_state_block: std::cell::Cell<u32>,
}

impl D3DState {
    fn alloc_state_block(&self) -> u32 {
        let id = self.next_state_block.get().max(1);
        self.next_state_block.set(id + 1);
        self.state_blocks.borrow_mut().insert(id);
        id
    }
}

struct StaticState(OnceCell<D3DState>);
unsafe impl Sync for StaticState {}
static D3D_STATE: StaticState = StaticState(OnceCell::new());

pub fn d3d_state() -> &'static D3DState {
    D3D_STATE.0.get_or_init(D3DState::default)
}

fn read_matrix(memory: &Memory, addr: u32) -> [f32; 16] {
    memory.read::<[f32; 16]>(addr)
}

fn write_matrix(memory: &mut Memory, addr: u32, m: [f32; 16]) {
    memory.write::<[f32; 16]>(addr, m);
}

fn mat_mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for row in 0..4 {
        for col in 0..4 {
            out[row * 4 + col] = (0..4).map(|k| a[row * 4 + k] * b[k * 4 + col]).sum();
        }
    }
    out
}

pub mod IDirect3D7 {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 8] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "EnumDevices",
        "CreateDevice",
        "CreateVertexBuffer",
        "EnumZBufferFormats",
        "EvictManagedTextures",
    ];

    pub const VTABLE_FUNCS: [runtime::ContFn; 8] = [
        QueryInterface_stdcall,
        AddRef_stdcall,
        Release_stdcall,
        EnumDevices_stdcall,
        CreateDevice_stdcall,
        CreateVertexBuffer_stdcall,
        EnumZBufferFormats_stdcall,
        EvictManagedTextures_stdcall,
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppv: u32) -> DD {
        if ppv == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(iid) = crate::Ptr::<GUID>::new(riid).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let out = if iid == IID_IUNKNOWN || iid == IID_IDirect3D7 {
            this
        } else {
            0
        };
        if crate::Ptr::<u32>::new(ppv)
            .write(&mut ctx.memory, out)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        if out == 0 {
            return DD::E_NOINTERFACE;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        0
    }

    #[win32_derive::dllexport]
    pub fn EnumDevices(
        ctx: &mut Context,
        _this: u32,
        lpEnumDevicesCallback: u32,
        lpUserArg: u32,
    ) -> DD {
        if lpEnumDevicesCallback == 0 {
            return DD::ERR_INVALIDPARAMS;
        }

        const DEVICES: &[(GUID, &str, &str)] = &[
            (
                IID_IDIRECT3DTNLHALDEVICE,
                "Direct3D T&L HAL\0",
                "Direct3D T&L HAL\0",
            ),
            (IID_IDIRECT3DHALDEVICE, "Direct3D HAL\0", "Direct3D HAL\0"),
            (IID_IDIRECT3DRGBDEVICE, "RGB Emulation\0", "RGB Emulation\0"),
        ];

        for &(guid, desc, name) in DEVICES {
            let desc_addr = crate::ddraw::alloc_string(ctx, desc);
            let name_addr = crate::ddraw::alloc_string(ctx, name);
            let device = device_desc7(guid);
            let dd_addr = kernel32::lock().process_heap.alloc(
                &mut ctx.memory,
                std::mem::size_of::<D3DDEVICEDESC7>() as u32,
            );
            ctx.memory.write(dd_addr, device);

            let callback = ctx.indirect(lpEnumDevicesCallback);
            ctx.call32_x86(callback, vec![desc_addr, name_addr, dd_addr, lpUserArg]);
            let ret = ctx.cpu.regs.eax;

            let kernel32 = kernel32::lock();
            kernel32.process_heap.free(&mut ctx.memory, desc_addr);
            kernel32.process_heap.free(&mut ctx.memory, name_addr);
            kernel32.process_heap.free(&mut ctx.memory, dd_addr);
            drop(kernel32);

            if ret == 0 {
                return DD::OK; // D3DENUMRET_CANCEL
            }
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateDevice(
        ctx: &mut Context,
        this: u32,
        _riid: u32,
        lpDDS: u32,
        lplpD3DDevice: u32,
    ) -> DD {
        if !crate::ddraw::ddraw::guest_range(ctx, lplpD3DDevice, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        ctx.memory.write::<u32>(lplpD3DDevice, 0);
        if !state().surf.borrow().contains_key(&lpDDS) {
            return DD::ERR_INVALIDPARAMS;
        }
        let mut kernel32 = kernel32::lock();
        let addr = IDirect3DDevice7::new(ctx, &mut kernel32.process_heap);
        drop(kernel32);
        d3d_state()
            .devices
            .borrow_mut()
            .insert(addr, Device::new(addr, this, lpDDS));
        ctx.memory.write::<u32>(lplpD3DDevice, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateVertexBuffer(
        ctx: &mut Context,
        _this: u32,
        lpD3DVertBufferDesc: u32,
        lplpD3DVertBuffer: u32,
        _dwFlags: u32,
    ) -> DD {
        if !crate::ddraw::ddraw::guest_range(ctx, lplpD3DVertBuffer, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        ctx.memory.write::<u32>(lplpD3DVertBuffer, 0);
        let Some(desc) =
            crate::Ptr::<D3DVERTEXBUFFERDESC>::new(lpD3DVertBufferDesc).read(&ctx.memory)
        else {
            return DD::ERR_INVALIDPARAMS;
        };
        if desc.dwSize != std::mem::size_of::<D3DVERTEXBUFFERDESC>() as u32 {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(data_len) = desc.dwNumVertices.checked_mul(vertex_size(desc.dwFVF)) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut kernel32 = kernel32::lock();
        let data = kernel32
            .process_heap
            .alloc(&mut ctx.memory, data_len.max(4));
        let Some(buf) = ctx
            .memory
            .bytes
            .get_mut(data as usize..(data + data_len) as usize)
        else {
            return DD::ERR_GENERIC;
        };
        buf.fill(0);
        let addr = IDirect3DVertexBuffer7::new(ctx, &mut kernel32.process_heap);
        drop(kernel32);
        d3d_state().vertex_buffers.borrow_mut().insert(
            addr,
            VertexBuffer {
                addr,
                desc,
                data,
                data_len,
            },
        );
        ctx.memory.write::<u32>(lplpD3DVertBuffer, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EnumZBufferFormats(
        ctx: &mut Context,
        _this: u32,
        _riidDevice: u32,
        lpEnumCallback: u32,
        lpContext: u32,
    ) -> DD {
        if lpEnumCallback == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        for fmt in zbuffer_formats() {
            let addr = kernel32::lock()
                .process_heap
                .alloc(&mut ctx.memory, std::mem::size_of::<DDPIXELFORMAT>() as u32);
            ctx.memory.write(addr, fmt);
            let callback = ctx.indirect(lpEnumCallback);
            ctx.call32_x86(callback, vec![addr, lpContext]);
            let ret = ctx.cpu.regs.eax;
            kernel32::lock().process_heap.free(&mut ctx.memory, addr);
            if ret == 0 {
                return DD::OK;
            }
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EvictManagedTextures(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }
}

pub mod IDirect3DDevice7 {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 49] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "GetCaps",
        "EnumTextureFormats",
        "BeginScene",
        "EndScene",
        "GetDirect3D",
        "SetRenderTarget",
        "GetRenderTarget",
        "Clear",
        "SetTransform",
        "GetTransform",
        "SetViewport",
        "MultiplyTransform",
        "GetViewport",
        "SetMaterial",
        "GetMaterial",
        "SetLight",
        "GetLight",
        "SetRenderState",
        "GetRenderState",
        "BeginStateBlock",
        "EndStateBlock",
        "PreLoad",
        "DrawPrimitive",
        "DrawIndexedPrimitive",
        "SetClipStatus",
        "GetClipStatus",
        "DrawPrimitiveStrided",
        "DrawIndexedPrimitiveStrided",
        "DrawPrimitiveVB",
        "DrawIndexedPrimitiveVB",
        "ComputeSphereVisibility",
        "GetTexture",
        "SetTexture",
        "GetTextureStageState",
        "SetTextureStageState",
        "ValidateDevice",
        "ApplyStateBlock",
        "CaptureStateBlock",
        "DeleteStateBlock",
        "CreateStateBlock",
        "Load",
        "LightEnable",
        "GetLightEnable",
        "SetClipPlane",
        "GetClipPlane",
        "GetInfo",
    ];

    pub const VTABLE_FUNCS: [runtime::ContFn; 49] = [
        QueryInterface_stdcall,
        AddRef_stdcall,
        Release_stdcall,
        GetCaps_stdcall,
        EnumTextureFormats_stdcall,
        BeginScene_stdcall,
        EndScene_stdcall,
        GetDirect3D_stdcall,
        SetRenderTarget_stdcall,
        GetRenderTarget_stdcall,
        Clear_stdcall,
        SetTransform_stdcall,
        GetTransform_stdcall,
        SetViewport_stdcall,
        MultiplyTransform_stdcall,
        GetViewport_stdcall,
        SetMaterial_stdcall,
        GetMaterial_stdcall,
        SetLight_stdcall,
        GetLight_stdcall,
        SetRenderState_stdcall,
        GetRenderState_stdcall,
        BeginStateBlock_stdcall,
        EndStateBlock_stdcall,
        PreLoad_stdcall,
        DrawPrimitive_stdcall,
        DrawIndexedPrimitive_stdcall,
        SetClipStatus_stdcall,
        GetClipStatus_stdcall,
        DrawPrimitiveStrided_stdcall,
        DrawIndexedPrimitiveStrided_stdcall,
        DrawPrimitiveVB_stdcall,
        DrawIndexedPrimitiveVB_stdcall,
        ComputeSphereVisibility_stdcall,
        GetTexture_stdcall,
        SetTexture_stdcall,
        GetTextureStageState_stdcall,
        SetTextureStageState_stdcall,
        ValidateDevice_stdcall,
        ApplyStateBlock_stdcall,
        CaptureStateBlock_stdcall,
        DeleteStateBlock_stdcall,
        CreateStateBlock_stdcall,
        Load_stdcall,
        LightEnable_stdcall,
        GetLightEnable_stdcall,
        SetClipPlane_stdcall,
        GetClipPlane_stdcall,
        GetInfo_stdcall,
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppv: u32) -> DD {
        if ppv == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(iid) = crate::Ptr::<GUID>::new(riid).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let out = if iid == IID_IUNKNOWN || iid == IID_IDIRECT3DDEVICE7 {
            this
        } else {
            0
        };
        if crate::Ptr::<u32>::new(ppv)
            .write(&mut ctx.memory, out)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        if out == 0 {
            return DD::E_NOINTERFACE;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        0
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, this: u32, lpD3DDevDesc: u32) -> DD {
        if lpD3DDevDesc == 0 || !d3d_state().devices.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        ctx.memory
            .write(lpD3DDevDesc, device_desc7(IID_IDIRECT3DTNLHALDEVICE));
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EnumTextureFormats(
        ctx: &mut Context,
        this: u32,
        lpd3dEnumPixelProc: u32,
        lpArg: u32,
    ) -> DD {
        if lpd3dEnumPixelProc == 0 || !d3d_state().devices.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        for fmt in texture_formats() {
            let addr = kernel32::lock()
                .process_heap
                .alloc(&mut ctx.memory, std::mem::size_of::<DDPIXELFORMAT>() as u32);
            ctx.memory.write(addr, fmt);
            let callback = ctx.indirect(lpd3dEnumPixelProc);
            ctx.call32_x86(callback, vec![addr, lpArg]);
            let ret = ctx.cpu.regs.eax;
            kernel32::lock().process_heap.free(&mut ctx.memory, addr);
            if ret == 0 {
                return DD::OK;
            }
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn BeginScene(_ctx: &mut Context, this: u32) -> DD {
        log::debug!("BeginScene dev={this:#x}");
        if let Some(device) = d3d_state().devices.borrow_mut().get_mut(&this) {
            device.in_scene = true;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EndScene(_ctx: &mut Context, this: u32) -> DD {
        log::debug!("EndScene dev={this:#x}");
        if let Some(device) = d3d_state().devices.borrow_mut().get_mut(&this) {
            device.in_scene = false;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetDirect3D(ctx: &mut Context, this: u32, lplpD3D: u32) -> DD {
        if lplpD3D == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        ctx.memory.write::<u32>(lplpD3D, device.d3d);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetRenderTarget(
        _ctx: &mut Context,
        this: u32,
        lpNewRenderTarget: u32,
        _dwFlags: u32,
    ) -> DD {
        if !state().surf.borrow().contains_key(&lpNewRenderTarget) {
            return DD::ERR_INVALIDPARAMS;
        }
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        log::debug!("SetRenderTarget: dev={this:#x} rt={lpNewRenderTarget:#x}");
        device.render_target = lpNewRenderTarget;
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetRenderTarget(ctx: &mut Context, this: u32, lplpRenderTarget: u32) -> DD {
        if lplpRenderTarget == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        ctx.memory
            .write::<u32>(lplpRenderTarget, device.render_target);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Clear(
        ctx: &mut Context,
        this: u32,
        dwCount: u32,
        lpRects: u32,
        dwFlags: u32,
        dwColor: u32,
        dvZ: u32,
        _dwStencil: u32,
    ) -> DD {
        const D3DCLEAR_TARGET: u32 = 0x00000001;
        const D3DCLEAR_ZBUFFER: u32 = 0x00000002;
        if dwFlags & (D3DCLEAR_TARGET | D3DCLEAR_ZBUFFER) == 0 {
            return DD::OK;
        }
        let (surface_addr, skip_target) = {
            let devices = d3d_state().devices.borrow();
            let Some(device) = devices.get(&this) else {
                return DD::ERR_INVALIDPARAMS;
            };
            // MM2 issues a second full-target Clear mid-scene after the world
            // pass; on real hardware that would erase the frame, so the only
            // consistent reading is that this clear is meant to refresh just
            // the depth buffer for the HUD pass. Skip the color fill once
            // geometry has landed since the last clear.
            let skip = device.in_scene
                && device.drew_since_clear
                && std::env::var("THESEUS_NO_CLEAR_SKIP").is_err();
            (device.render_target, skip)
        };
        log::debug!(
            "Clear: dev={this:#x} rt={surface_addr:#x} flags={dwFlags:#x} count={dwCount} rects={lpRects:#x} color={dwColor:#x} z={:#x}",
            dvZ
        );
        let surf = {
            let surfs = state().surf.borrow();
            let Some(surf) = surfs.get(&surface_addr) else {
                return DD::ERR_INVALIDPARAMS;
            };
            surf.clone()
        };
        // The rect list restricts the clear; none means the whole surface.
        // D3DRECT is {x1,y1,x2,y2} i32s, 16 bytes each.
        let rects: Vec<(i32, i32, i32, i32)> = if dwCount != 0 && lpRects != 0 {
            (0..dwCount)
                .map(|i| {
                    let base = lpRects + i * 16;
                    (
                        ctx.memory.read::<i32>(base),
                        ctx.memory.read::<i32>(base + 4),
                        ctx.memory.read::<i32>(base + 8),
                        ctx.memory.read::<i32>(base + 12),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        // Rows covered by the clear rects; without a list this is the whole
        // surface. Calls `write` per scanline run to fill.
        let fill = |ctx: &mut Context,
                    surf: &std::rc::Rc<RefCell<crate::ddraw::Surface>>,
                    write: &dyn Fn(&mut Context, u32, usize)| {
            let mut surf = surf.borrow_mut();
            let addr = surf.lock(&mut ctx.memory);
            let bpp = surf.bytes_per_pixel;
            let stride = surf.width * bpp;
            if rects.is_empty() {
                write(ctx, addr, (surf.height * stride) as usize);
                return;
            }
            for &(x1, y1, x2, y2) in &rects {
                let left = x1.max(0).min(surf.width as i32) as u32;
                let right = x2.max(0).min(surf.width as i32) as u32;
                let top = y1.max(0).min(surf.height as i32) as u32;
                let bottom = y2.max(0).min(surf.height as i32) as u32;
                for y in top..bottom {
                    write(
                        ctx,
                        addr + y * stride + left * bpp,
                        (right.saturating_sub(left) * bpp) as usize,
                    );
                }
            }
        };
        if dwFlags & D3DCLEAR_ZBUFFER != 0 {
            // The z-buffer is the DDSCAPS_ZBUFFER surface attached to the
            // render target; store dvZ packed to its 16-bit depth.
            let zbuf = {
                let rt = surf.borrow();
                rt.attachments
                    .iter()
                    .find(|s| s.borrow().caps.dwCaps.contains(DDSCAPS::ZBUFFER))
                    .cloned()
            };
            if let Some(zbuf) = zbuf {
                let z = z_to_u16(f32::from_bits(dvZ));
                {
                    let zsurf = zbuf.borrow();
                    log::debug!(
                        "Clear zbuf: surf={:#x} {}x{} bpp={} pixels={:#x} z={z:#x} rt={}x{}",
                        zsurf.addr,
                        zsurf.width,
                        zsurf.height,
                        zsurf.bytes_per_pixel,
                        zsurf.pixels.unwrap_or(0),
                        surf.borrow().width,
                        surf.borrow().height,
                    );
                }
                fill(ctx, &zbuf, &|ctx, at, len| {
                    let bytes = z.to_le_bytes();
                    for chunk in ctx.memory[at..][..len].chunks_exact_mut(2) {
                        chunk.copy_from_slice(&bytes);
                    }
                });
            } else {
                log::debug!("Clear zbuf: no z-buffer attached to {surface_addr:#x}");
            }
        }
        if dwFlags & D3DCLEAR_TARGET == 0 || skip_target {
            return DD::OK;
        }
        if let Some(device) = d3d_state().devices.borrow_mut().get_mut(&this) {
            device.drew_since_clear = false;
        }
        let bpp = surf.borrow().bytes_per_pixel;
        match bpp {
            4 => {
                fill(ctx, &surf, &|ctx, at, len| {
                    let bytes = dwColor.to_le_bytes();
                    for chunk in ctx.memory[at..][..len].chunks_exact_mut(4) {
                        chunk.copy_from_slice(&bytes);
                    }
                });
            }
            2 => {
                let r = ((dwColor >> 16) & 0xFF) as u16;
                let g = ((dwColor >> 8) & 0xFF) as u16;
                let b = (dwColor & 0xFF) as u16;
                let pixel: u16 = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
                let bytes = pixel.to_le_bytes();
                fill(ctx, &surf, &|ctx, at, len| {
                    for chunk in ctx.memory[at..][..len].chunks_exact_mut(2) {
                        chunk.copy_from_slice(&bytes);
                    }
                });
            }
            _ => {
                fill(ctx, &surf, &|ctx, at, len| {
                    ctx.memory[at..][..len].fill(0);
                });
            }
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetTransform(ctx: &mut Context, this: u32, dwState: u32, lpMatrix: u32) -> DD {
        if !crate::ddraw::ddraw::guest_range(ctx, lpMatrix, 64) {
            return DD::ERR_INVALIDPARAMS;
        }
        let matrix = read_matrix(&ctx.memory, lpMatrix);
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        device.transforms.insert(dwState, matrix);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetTransform(ctx: &mut Context, this: u32, dwState: u32, lpMatrix: u32) -> DD {
        if !crate::ddraw::ddraw::guest_range(ctx, lpMatrix, 64) {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut matrix = [0.0f32; 16];
        for i in 0..4 {
            matrix[i * 4 + i] = 1.0; // unset transforms read back as identity
        }
        if let Some(m) = device.transforms.get(&dwState) {
            matrix = *m;
        }
        write_matrix(&mut ctx.memory, lpMatrix, matrix);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetViewport(ctx: &mut Context, this: u32, lpViewport: u32) -> DD {
        let Some(viewport) = crate::Ptr::<D3DVIEWPORT7>::new(lpViewport).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        device.viewport = Some(viewport);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn MultiplyTransform(ctx: &mut Context, this: u32, dwState: u32, lpMatrix: u32) -> DD {
        if !crate::ddraw::ddraw::guest_range(ctx, lpMatrix, 64) {
            return DD::ERR_INVALIDPARAMS;
        }
        let rhs = read_matrix(&ctx.memory, lpMatrix);
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut cur = [0.0f32; 16];
        for i in 0..4 {
            cur[i * 4 + i] = 1.0;
        }
        if let Some(m) = device.transforms.get(&dwState) {
            cur = *m;
        }
        device.transforms.insert(dwState, mat_mul(cur, rhs));
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetViewport(ctx: &mut Context, this: u32, lpViewport: u32) -> DD {
        if !crate::ddraw::ddraw::guest_range(
            ctx,
            lpViewport,
            std::mem::size_of::<D3DVIEWPORT7>() as u32,
        ) {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let viewport = device.viewport.unwrap_or(D3DVIEWPORT7 {
            dwX: 0,
            dwY: 0,
            dwWidth: 640,
            dwHeight: 480,
            dvMinZ: 0.0,
            dvMaxZ: 1.0,
        });
        ctx.memory.write(lpViewport, viewport);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetMaterial(ctx: &mut Context, this: u32, lpMaterial: u32) -> DD {
        let Some(material) = crate::Ptr::<D3DMATERIAL7>::new(lpMaterial).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        device.material = Some(material);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetMaterial(ctx: &mut Context, this: u32, lpMaterial: u32) -> DD {
        if lpMaterial == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let material = device.material.unwrap_or_default();
        if crate::Ptr::<D3DMATERIAL7>::new(lpMaterial)
            .write(&mut ctx.memory, material)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetLight(ctx: &mut Context, this: u32, dwLightIndex: u32, lpLight: u32) -> DD {
        if lpLight == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        if !crate::ddraw::ddraw::guest_range(ctx, lpLight, D3DLIGHT7_SIZE as u32) {
            return DD::ERR_INVALIDPARAMS;
        }
        let mut light = [0u8; D3DLIGHT7_SIZE];
        light.copy_from_slice(&ctx.memory[lpLight..][..D3DLIGHT7_SIZE]);
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        device.lights.insert(dwLightIndex, light);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetLight(ctx: &mut Context, this: u32, dwLightIndex: u32, lpLight: u32) -> DD {
        if lpLight == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let Some(light) = device.lights.get(&dwLightIndex) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if !crate::ddraw::ddraw::guest_range(ctx, lpLight, D3DLIGHT7_SIZE as u32) {
            return DD::ERR_INVALIDPARAMS;
        }
        ctx.memory[lpLight..][..D3DLIGHT7_SIZE].copy_from_slice(light);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetRenderState(_ctx: &mut Context, this: u32, dwState: u32, dwValue: u32) -> DD {
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        device.render_states.insert(dwState, dwValue);
        if !HANDLED_RENDER_STATES.contains(&dwState) {
            warn_unhandled_render_state(dwState, dwValue);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetRenderState(ctx: &mut Context, this: u32, dwState: u32, lpdwRenderState: u32) -> DD {
        if lpdwRenderState == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        ctx.memory.write::<u32>(
            lpdwRenderState,
            *device.render_states.get(&dwState).unwrap_or(&0),
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn BeginStateBlock(_ctx: &mut Context, this: u32) -> DD {
        if !d3d_state().devices.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EndStateBlock(ctx: &mut Context, this: u32, lpdwBlockHandle: u32) -> DD {
        if lpdwBlockHandle == 0 || !d3d_state().devices.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        ctx.memory
            .write::<u32>(lpdwBlockHandle, d3d_state().alloc_state_block());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn PreLoad(_ctx: &mut Context, _this: u32, _lpddsTexture: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DrawPrimitive(
        ctx: &mut Context,
        this: u32,
        dptPrimitiveType: u32,
        dwVertexTypeDesc: u32,
        lpvVertices: u32,
        dwVertexCount: u32,
        _dwFlags: u32,
    ) -> DD {
        log::debug!(
            "DrawPrimitive: prim={} fvf={:#x} vaddr={:#x} vcount={}",
            dptPrimitiveType,
            dwVertexTypeDesc,
            lpvVertices,
            dwVertexCount
        );
        log_vertex_start(ctx, lpvVertices, dwVertexCount, dwVertexTypeDesc);
        rasterize(
            ctx,
            this,
            dptPrimitiveType,
            dwVertexTypeDesc,
            lpvVertices,
            dwVertexCount,
            0,
            0,
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DrawIndexedPrimitive(
        ctx: &mut Context,
        this: u32,
        dptPrimitiveType: u32,
        dwVertexTypeDesc: u32,
        lpvVertices: u32,
        dwVertexCount: u32,
        lpwIndices: u32,
        dwIndexCount: u32,
        _dwFlags: u32,
    ) -> DD {
        log::debug!(
            "DrawIndexedPrimitive: prim={} fvf={:#x} vaddr={:#x} vcount={} icount={}",
            dptPrimitiveType,
            dwVertexTypeDesc,
            lpvVertices,
            dwVertexCount,
            dwIndexCount
        );
        log_vertex_start(ctx, lpvVertices, dwVertexCount, dwVertexTypeDesc);
        rasterize(
            ctx,
            this,
            dptPrimitiveType,
            dwVertexTypeDesc,
            lpvVertices,
            dwVertexCount,
            lpwIndices,
            dwIndexCount,
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetClipStatus(ctx: &mut Context, this: u32, lpD3DClipStatus: u32) -> DD {
        let Some(status) = crate::Ptr::<D3DCLIPSTATUS>::new(lpD3DClipStatus).read(&ctx.memory)
        else {
            return DD::ERR_INVALIDPARAMS;
        };
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        device.clip_status = Some(status);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetClipStatus(ctx: &mut Context, this: u32, lpD3DClipStatus: u32) -> DD {
        if lpD3DClipStatus == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        ctx.memory
            .write(lpD3DClipStatus, device.clip_status.unwrap_or_default());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DrawPrimitiveStrided(
        _ctx: &mut Context,
        _this: u32,
        dptPrimitiveType: u32,
        dwVertexTypeDesc: u32,
        lpVertexArray: u32,
        dwVertexCount: u32,
        _dwFlags: u32,
    ) -> DD {
        log::debug!(
            "DrawPrimitiveStrided: prim={} fvf={:#x} arr={:#x} vcount={}",
            dptPrimitiveType,
            dwVertexTypeDesc,
            lpVertexArray,
            dwVertexCount
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DrawIndexedPrimitiveStrided(
        _ctx: &mut Context,
        _this: u32,
        dptPrimitiveType: u32,
        dwVertexTypeDesc: u32,
        lpVertexArray: u32,
        dwVertexCount: u32,
        _lpwIndices: u32,
        dwIndexCount: u32,
        _dwFlags: u32,
    ) -> DD {
        log::debug!(
            "DrawIndexedPrimitiveStrided: prim={} fvf={:#x} arr={:#x} vcount={} icount={}",
            dptPrimitiveType,
            dwVertexTypeDesc,
            lpVertexArray,
            dwVertexCount,
            dwIndexCount
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DrawPrimitiveVB(
        ctx: &mut Context,
        _this: u32,
        dptPrimitiveType: u32,
        lpd3dVertexBuffer: u32,
        dwStartVertex: u32,
        dwNumVertices: u32,
        _dwFlags: u32,
    ) -> DD {
        let (addr, fvf) = d3d_state()
            .vertex_buffers
            .borrow()
            .get(&lpd3dVertexBuffer)
            .map(|vb| (vb.data, vb.desc.dwFVF))
            .unwrap_or((0, 0));
        log::debug!(
            "DrawPrimitiveVB: prim={} vb={:#x} start={} fvf={:#x} vcount={} data={:#x}",
            dptPrimitiveType,
            lpd3dVertexBuffer,
            dwStartVertex,
            fvf,
            dwNumVertices,
            addr
        );
        log_vertex_start(
            ctx,
            addr + dwStartVertex * vertex_size(fvf),
            dwNumVertices,
            fvf,
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DrawIndexedPrimitiveVB(
        ctx: &mut Context,
        _this: u32,
        dptPrimitiveType: u32,
        lpd3dVertexBuffer: u32,
        dwStartVertex: u32,
        dwNumVertices: u32,
        _lpwIndices: u32,
        dwIndexCount: u32,
        _dwFlags: u32,
    ) -> DD {
        let (addr, fvf) = d3d_state()
            .vertex_buffers
            .borrow()
            .get(&lpd3dVertexBuffer)
            .map(|vb| (vb.data, vb.desc.dwFVF))
            .unwrap_or((0, 0));
        log::debug!(
            "DrawIndexedPrimitiveVB: prim={} vb={:#x} start={} fvf={:#x} vcount={} icount={} data={:#x}",
            dptPrimitiveType,
            lpd3dVertexBuffer,
            dwStartVertex,
            fvf,
            dwNumVertices,
            dwIndexCount,
            addr
        );
        log_vertex_start(
            ctx,
            addr + dwStartVertex * vertex_size(fvf),
            dwNumVertices,
            fvf,
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn ComputeSphereVisibility(
        ctx: &mut Context,
        _this: u32,
        _lpCenters: u32,
        _lpRadii: u32,
        dwNumSpheres: u32,
        _dwFlags: u32,
        lpdwReturnValues: u32,
    ) -> DD {
        if !crate::ddraw::ddraw::guest_range(ctx, lpdwReturnValues, dwNumSpheres.saturating_mul(4))
        {
            return DD::ERR_INVALIDPARAMS;
        }
        for i in 0..dwNumSpheres {
            // Zero means "fully visible" in the clip-code sense, which is the
            // honest answer when no frustum planes are tracked.
            ctx.memory.write::<u32>(lpdwReturnValues + i * 4, 0);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetTexture(ctx: &mut Context, this: u32, dwStage: u32, lplpTexture: u32) -> DD {
        if lplpTexture == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let texture = *device.textures.get(&dwStage).unwrap_or(&0);
        if crate::Ptr::<u32>::new(lplpTexture)
            .write(&mut ctx.memory, texture)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetTexture(_ctx: &mut Context, this: u32, dwStage: u32, lpTexture: u32) -> DD {
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if lpTexture == 0 {
            device.textures.remove(&dwStage);
        } else {
            if !state().surf.borrow().contains_key(&lpTexture) {
                return DD::ERR_INVALIDPARAMS;
            }
            device.textures.insert(dwStage, lpTexture);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetTextureStageState(
        ctx: &mut Context,
        this: u32,
        dwStage: u32,
        dwState: u32,
        lpdwValue: u32,
    ) -> DD {
        if lpdwValue == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        ctx.memory.write::<u32>(
            lpdwValue,
            *device
                .texture_stage_states
                .get(&(dwStage, dwState))
                .unwrap_or(&0),
        );
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetTextureStageState(
        _ctx: &mut Context,
        this: u32,
        dwStage: u32,
        dwState: u32,
        dwValue: u32,
    ) -> DD {
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        device
            .texture_stage_states
            .insert((dwStage, dwState), dwValue);
        if dwStage != 0 || !HANDLED_TEXTURE_STAGE_STATES.contains(&dwState) {
            warn_unhandled_texture_stage_state(dwStage, dwState, dwValue);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn ValidateDevice(ctx: &mut Context, this: u32, lpdwPasses: u32) -> DD {
        if lpdwPasses == 0 || !d3d_state().devices.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        // The modelled device imposes no texture-stage conflicts; one pass.
        if crate::Ptr::<u32>::new(lpdwPasses)
            .write(&mut ctx.memory, 1)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn ApplyStateBlock(_ctx: &mut Context, this: u32, dwBlockHandle: u32) -> DD {
        if !d3d_state().devices.borrow().contains_key(&this)
            || !d3d_state().state_blocks.borrow().contains(&dwBlockHandle)
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CaptureStateBlock(_ctx: &mut Context, this: u32, dwBlockHandle: u32) -> DD {
        if !d3d_state().devices.borrow().contains_key(&this)
            || !d3d_state().state_blocks.borrow().contains(&dwBlockHandle)
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DeleteStateBlock(_ctx: &mut Context, this: u32, dwBlockHandle: u32) -> DD {
        if !d3d_state().devices.borrow().contains_key(&this)
            || !d3d_state().state_blocks.borrow_mut().remove(&dwBlockHandle)
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateStateBlock(
        ctx: &mut Context,
        this: u32,
        _d3dsbType: u32,
        lpdwBlockHandle: u32,
    ) -> DD {
        if lpdwBlockHandle == 0 || !d3d_state().devices.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        ctx.memory
            .write::<u32>(lpdwBlockHandle, d3d_state().alloc_state_block());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Load(
        ctx: &mut Context,
        _this: u32,
        lpDestTex: u32,
        lpDestPoint: u32,
        lpSrcTex: u32,
        lprcSrcRect: u32,
        _dwFlags: u32,
    ) -> DD {
        // Both textures are IDirectDrawSurface7 interface addresses. Loading a
        // level onto itself is a documented no-op.
        if lpDestTex == lpSrcTex {
            return DD::OK;
        }
        let (dst_rc, src_rc) = {
            let surfaces = state().surf.borrow();
            (
                surfaces.get(&lpDestTex).cloned(),
                surfaces.get(&lpSrcTex).cloned(),
            )
        };
        let (Some(dst_rc), Some(src_rc)) = (dst_rc, src_rc) else {
            return DD::ERR_INVALIDPARAMS;
        };

        // lpDestPoint is a POINT; null is the origin. A null rect means the
        // whole source level.
        let (mut dx, mut dy) = if lpDestPoint != 0 {
            if !crate::ddraw::ddraw::guest_range(ctx, lpDestPoint, 8) {
                return DD::ERR_INVALIDPARAMS;
            }
            (
                ctx.memory.read::<i32>(lpDestPoint),
                ctx.memory.read::<i32>(lpDestPoint + 4),
            )
        } else {
            (0, 0)
        };
        let mut src_rect = crate::ddraw::ddraw::read_rect(ctx, lprcSrcRect);

        // Load cascades through every mip level the two chains have in
        // common, halving the rect and point at each step.
        let mut dst_level = dst_rc;
        let mut src_level = src_rc;
        loop {
            let (w, h) = match &src_rect {
                Some(r) => (r.right - r.left, r.bottom - r.top),
                None => {
                    let src = src_level.borrow();
                    (src.width as i32, src.height as i32)
                }
            };
            let dst_rect = RECT {
                left: dx,
                top: dy,
                right: dx + w,
                bottom: dy + h,
            };
            // Copy the addresses out first: `blit_copy` borrows both surfaces
            // mutably, which would conflict with a `Ref` held across the call.
            let dst_addr = dst_level.borrow().addr;
            let src_addr = src_level.borrow().addr;
            crate::ddraw::ddraw::blit_copy(ctx, dst_addr, Some(dst_rect), src_addr, src_rect, None);

            let next = {
                let dst = dst_level.borrow();
                let src = src_level.borrow();
                match (dst.attachments.first(), src.attachments.first()) {
                    (Some(d), Some(s)) => (d.clone(), s.clone()),
                    _ => break,
                }
            };
            dst_level = next.0;
            src_level = next.1;
            src_rect = src_rect.map(|r| RECT {
                left: r.left / 2,
                top: r.top / 2,
                right: r.right / 2,
                bottom: r.bottom / 2,
            });
            dx /= 2;
            dy /= 2;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn LightEnable(_ctx: &mut Context, this: u32, dwLightIndex: u32, bEnable: u32) -> DD {
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        device.lights_enabled.insert(dwLightIndex, bEnable != 0);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetLightEnable(ctx: &mut Context, this: u32, dwLightIndex: u32, pbEnable: u32) -> DD {
        if pbEnable == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let enabled = *device.lights_enabled.get(&dwLightIndex).unwrap_or(&false) as u32;
        if crate::Ptr::<u32>::new(pbEnable)
            .write(&mut ctx.memory, enabled)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetClipPlane(ctx: &mut Context, this: u32, dwIndex: u32, pPlaneEquation: u32) -> DD {
        if !crate::ddraw::ddraw::guest_range(ctx, pPlaneEquation, 16) {
            return DD::ERR_INVALIDPARAMS;
        }
        let plane = ctx.memory.read::<[f32; 4]>(pPlaneEquation);
        let mut devices = d3d_state().devices.borrow_mut();
        let Some(device) = devices.get_mut(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        device.clip_planes.insert(dwIndex, plane);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetClipPlane(ctx: &mut Context, this: u32, dwIndex: u32, pPlaneEquation: u32) -> DD {
        if pPlaneEquation == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let Some(plane) = device.clip_planes.get(&dwIndex) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if !crate::ddraw::ddraw::guest_range(ctx, pPlaneEquation, 16) {
            return DD::ERR_INVALIDPARAMS;
        }
        ctx.memory.write::<[f32; 4]>(pPlaneEquation, *plane);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetInfo(
        _ctx: &mut Context,
        _this: u32,
        _dwDevInfoID: u32,
        _pDevInfoStruct: u32,
        _dwSize: u32,
    ) -> DD {
        // The device-info query is a developer/driver detail; there is no
        // debug info to report.
        DD::ERR_GENERIC
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }
}

pub mod IDirect3DVertexBuffer7 {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 9] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Lock",
        "Unlock",
        "ProcessVertices",
        "ProcessVerticesStrided",
        "GetVertexBufferDesc",
        "Optimize",
    ];

    pub const VTABLE_FUNCS: [runtime::ContFn; 9] = [
        QueryInterface_stdcall,
        AddRef_stdcall,
        Release_stdcall,
        Lock_stdcall,
        Unlock_stdcall,
        ProcessVertices_stdcall,
        ProcessVerticesStrided_stdcall,
        GetVertexBufferDesc_stdcall,
        Optimize_stdcall,
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppv: u32) -> DD {
        if ppv == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let Some(iid) = crate::Ptr::<GUID>::new(riid).read(&ctx.memory) else {
            return DD::ERR_INVALIDPARAMS;
        };
        let out = if iid == IID_IUNKNOWN || iid == IID_IDIRECT3DVERTEXBUFFER7 {
            this
        } else {
            0
        };
        if crate::Ptr::<u32>::new(ppv)
            .write(&mut ctx.memory, out)
            .is_none()
        {
            return DD::ERR_INVALIDPARAMS;
        }
        if out == 0 {
            return DD::E_NOINTERFACE;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, _this: u32) -> u32 {
        0
    }

    #[win32_derive::dllexport]
    pub fn Lock(ctx: &mut Context, this: u32, _dwFlags: u32, lplpData: u32, lpdwSize: u32) -> DD {
        let buffers = d3d_state().vertex_buffers.borrow();
        let Some(vb) = buffers.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        if lplpData != 0 && !crate::ddraw::ddraw::guest_range(ctx, lplpData, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        if lpdwSize != 0 && !crate::ddraw::ddraw::guest_range(ctx, lpdwSize, 4) {
            return DD::ERR_INVALIDPARAMS;
        }
        if lplpData != 0 {
            ctx.memory.write::<u32>(lplpData, vb.data);
        }
        if lpdwSize != 0 {
            ctx.memory.write::<u32>(lpdwSize, vb.data_len);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unlock(_ctx: &mut Context, this: u32) -> DD {
        if !d3d_state().vertex_buffers.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn ProcessVertices(
        _ctx: &mut Context,
        this: u32,
        _dwVertexOp: u32,
        _dwDestIndex: u32,
        _dwCount: u32,
        _lpSrcBuffer: u32,
        _dwSrcIndex: u32,
        _lpD3DDevice: u32,
        _dwFlags: u32,
    ) -> DD {
        if !d3d_state().vertex_buffers.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn ProcessVerticesStrided(
        _ctx: &mut Context,
        this: u32,
        _lpVertexArray: u32,
        _dwDestIndex: u32,
        _dwCount: u32,
        _lpD3DDevice: u32,
        _dwFlags: u32,
    ) -> DD {
        if !d3d_state().vertex_buffers.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetVertexBufferDesc(ctx: &mut Context, this: u32, lpVBD: u32) -> DD {
        if lpVBD == 0 {
            return DD::ERR_INVALIDPARAMS;
        }
        let buffers = d3d_state().vertex_buffers.borrow();
        let Some(vb) = buffers.get(&this) else {
            return DD::ERR_INVALIDPARAMS;
        };
        ctx.memory.write(lpVBD, vb.desc);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Optimize(_ctx: &mut Context, this: u32, _lpD3DDevice: u32, _dwFlags: u32) -> DD {
        if !d3d_state().vertex_buffers.borrow().contains_key(&this) {
            return DD::ERR_INVALIDPARAMS;
        }
        DD::OK
    }

    pub static mut VTABLE: u32 = 0;

    pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
        let addr = heap.alloc(&mut ctx.memory, 4);
        ctx.memory.write(addr, unsafe { VTABLE });
        addr
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct RhwVertex {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
    diffuse: u32,
    specular: u32,
    u: f32,
    v: f32,
    u_w: f32,
    v_w: f32,
}

fn read_vertex(mem: &Memory, addr: u32) -> RhwVertex {
    let x = mem.read::<f32>(addr);
    let y = mem.read::<f32>(addr + 4);
    let z = mem.read::<f32>(addr + 8);
    let w = mem.read::<f32>(addr + 12);
    let u = mem.read::<f32>(addr + 24);
    let v = mem.read::<f32>(addr + 28);
    let (u_w, v_w) = if w != 0.0 { (u * w, v * w) } else { (u, v) };
    RhwVertex {
        x,
        y,
        z,
        w,
        diffuse: mem.read::<u32>(addr + 16),
        specular: mem.read::<u32>(addr + 20),
        u,
        v,
        u_w,
        v_w,
    }
}

// D3DRENDERSTATETYPE values the rasterizer acts on.
const D3DRENDERSTATE_ZENABLE: u32 = 7;
const D3DRENDERSTATE_ZWRITEENABLE: u32 = 14;
const D3DRENDERSTATE_ALPHATESTENABLE: u32 = 15;
const D3DRENDERSTATE_SRCBLEND: u32 = 19;
const D3DRENDERSTATE_DESTBLEND: u32 = 20;
const D3DRENDERSTATE_CULLMODE: u32 = 22;
const D3DRENDERSTATE_ZFUNC: u32 = 23;
const D3DRENDERSTATE_ALPHAREF: u32 = 24;
const D3DRENDERSTATE_ALPHAFUNC: u32 = 25;
const D3DRENDERSTATE_SHADEMODE: u32 = 9;
const D3DRENDERSTATE_ALPHABLENDENABLE: u32 = 27;
const D3DRENDERSTATE_FOGCOLOR: u32 = 34;
const D3DRENDERSTATE_FOGSTART: u32 = 36;
const D3DRENDERSTATE_FOGEND: u32 = 37;
const D3DRENDERSTATE_FOGVERTEXMODE: u32 = 140;
const D3DCULL_CW: u32 = 2;
const D3DCULL_CCW: u32 = 3;
const D3DSHADE_FLAT: u32 = 1;
const D3DFOG_LINEAR: u32 = 3;

const HANDLED_RENDER_STATES: &[u32] = &[
    D3DRENDERSTATE_ZENABLE,
    D3DRENDERSTATE_ZWRITEENABLE,
    D3DRENDERSTATE_ALPHATESTENABLE,
    D3DRENDERSTATE_SRCBLEND,
    D3DRENDERSTATE_DESTBLEND,
    D3DRENDERSTATE_CULLMODE,
    D3DRENDERSTATE_ZFUNC,
    D3DRENDERSTATE_ALPHAREF,
    D3DRENDERSTATE_ALPHAFUNC,
    D3DRENDERSTATE_SHADEMODE,
    D3DRENDERSTATE_ALPHABLENDENABLE,
    D3DRENDERSTATE_FOGCOLOR,
    D3DRENDERSTATE_FOGSTART,
    D3DRENDERSTATE_FOGEND,
    D3DRENDERSTATE_FOGVERTEXMODE,
    // Legacy states that the modern SetTextureStageState filter/address path
    // has replaced; their default values match what the rasterizer already does.
    4,   // D3DRENDERSTATE_TEXTUREPERSPECTIVE
    137, // D3DRENDERSTATE_TEXTUREMAG
    // Ambient feeds the D3D lighting pipeline, which never runs for the
    // pre-transformed, pre-lit D3DFVF_XYZRHW geometry this rasterizer draws.
    139, // D3DRENDERSTATE_AMBIENT
];

// States the rasterizer consults (filter/mip) or whose D3D7 defaults match the
// rasterizer's MODULATE/TEXTURE/DIFFUSE/Wrap behavior.
const HANDLED_TEXTURE_STAGE_STATES: &[u32] = &[
    1,  // D3DTSS_COLOROP
    2,  // D3DTSS_COLORARG1
    3,  // D3DTSS_COLORARG2
    4,  // D3DTSS_ALPHAOP
    5,  // D3DTSS_ALPHAARG1
    6,  // D3DTSS_ALPHAARG2
    13, // D3DTSS_ADDRESSU
    14, // D3DTSS_ADDRESSV
    16, // D3DTSS_MAGFILTER
    17, // D3DTSS_MINFILTER
    18, // D3DTSS_MIPFILTER
];

static WARNED_RENDER_STATES: LazyLock<Mutex<HashSet<u32>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static WARNED_TEXTURE_STAGE_STATES: LazyLock<Mutex<HashSet<(u32, u32)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn warn_unhandled_render_state(dw_state: u32, dw_value: u32) {
    let mut warned = WARNED_RENDER_STATES.lock().unwrap();
    if warned.insert(dw_state) {
        log::warn!("unhandled D3DRENDERSTATE({dw_state}) = {dw_value}");
    }
}

fn warn_unhandled_texture_stage_state(dw_stage: u32, dw_state: u32, dw_value: u32) {
    let mut warned = WARNED_TEXTURE_STAGE_STATES.lock().unwrap();
    if warned.insert((dw_stage, dw_state)) {
        log::warn!("unhandled D3DTSS stage={dw_stage} state={dw_state} = {dw_value}");
    }
}

/// Pack a [0,1] depth into a 16-bit z-buffer word.
fn z_to_u16(z: f32) -> u16 {
    (z.clamp(0.0, 1.0) * 65535.0) as u16
}

fn z_passes(zfunc: u32, new: u16, cur: u16) -> bool {
    match zfunc {
        1 => false,      // D3DCMP_NEVER
        2 => new < cur,  // D3DCMP_LESS
        3 => new == cur, // D3DCMP_EQUAL
        4 => new <= cur, // D3DCMP_LESSEQUAL
        5 => new > cur,  // D3DCMP_GREATER
        6 => new != cur, // D3DCMP_NOTEQUAL
        7 => new >= cur, // D3DCMP_GREATEREQUAL
        _ => true,       // D3DCMP_ALWAYS
    }
}

fn argb_to_565(c: u32) -> u16 {
    let r = ((c >> 16) & 0xff) as u16;
    let g = ((c >> 8) & 0xff) as u16;
    let b = (c & 0xff) as u16;
    ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
}

/// D3DBLEND factor applied per channel; s/d are the source and destination
/// channel values in 0..1, sa/da the source and destination alpha in 0..1.
fn blend_factor(blend: u32, s: f32, d: f32, sa: f32, da: f32) -> f32 {
    match blend {
        1 => 0.0,               // D3DBLEND_ZERO
        2 => 1.0,               // D3DBLEND_ONE
        3 => s,                 // D3DBLEND_SRCCOLOR
        4 => 1.0 - s,           // D3DBLEND_INVSRCCOLOR
        5 => sa,                // D3DBLEND_SRCALPHA
        6 => 1.0 - sa,          // D3DBLEND_INVSRCALPHA
        7 => da,                // D3DBLEND_DESTALPHA
        8 => 1.0 - da,          // D3DBLEND_INVDESTALPHA
        9 => d,                 // D3DBLEND_DESTCOLOR
        10 => 1.0 - d,          // D3DBLEND_INVDESTCOLOR
        11 => sa.min(1.0 - sa), // D3DBLEND_SRCALPHASAT
        _ => 1.0,
    }
}

/// Blend `src` onto `dst`, both RGB565. `sa` is the interpolated source
/// alpha in 0..255; a 565 render target carries no alpha so da is 1.
fn blend_565(dst: u16, src: u16, sa: f32, sblend: u32, dblend: u32) -> u16 {
    let sa = sa / 255.0;
    let da = 1.0;
    let mut out = 0u16;
    let mut shift = 0;
    for (bits, max) in [(5u32, 31.0f32), (6, 63.0), (5, 31.0)] {
        let s = ((src >> shift) & ((1 << bits) - 1)) as f32 / max;
        let d = ((dst >> shift) & ((1 << bits) - 1)) as f32 / max;
        let v = s * blend_factor(sblend, s, d, sa, da) + d * blend_factor(dblend, s, d, sa, da);
        out |= ((v.clamp(0.0, 1.0) * max) as u16) << shift;
        shift += bits;
    }
    out
}

// Texture pixel formats the rasterizer decodes, classified from the
// surface's DDPIXELFORMAT channel masks.
const TEXFMT_RGB565: u8 = 0;
const TEXFMT_A1R5G5B5: u8 = 1;
const TEXFMT_A4R4G4B4: u8 = 2;

/// Decode one raw texture word to 8-bit (r, g, b, a).
fn decode_texel(p: u16, fmt: u8) -> (u8, u8, u8, u8) {
    match fmt {
        // A1R5G5B5: 1-bit alpha, 5/5/5 color.
        TEXFMT_A1R5G5B5 => (
            ((((p >> 10) & 0x1f) << 3) | ((p >> 10) & 0x1f) >> 2) as u8,
            ((((p >> 5) & 0x1f) << 3) | ((p >> 5) & 0x1f) >> 2) as u8,
            (((p & 0x1f) << 3) | (p & 0x1f) >> 2) as u8,
            if p & 0x8000 != 0 { 255 } else { 0 },
        ),
        // A4R4G4B4: 4-bit channels expanded to 8.
        TEXFMT_A4R4G4B4 => (
            (((p >> 8) & 0xf) * 17) as u8,
            (((p >> 4) & 0xf) * 17) as u8,
            ((p & 0xf) * 17) as u8,
            (((p >> 12) & 0xf) * 17) as u8,
        ),
        // RGB565 (and any unrecognized mask, treated as opaque 565).
        _ => (
            ((((p >> 11) & 0x1f) << 3) | ((p >> 11) & 0x1f) >> 2) as u8,
            ((((p >> 5) & 0x3f) << 2) | ((p >> 5) & 0x3f) >> 4) as u8,
            (((p & 0x1f) << 3) | (p & 0x1f) >> 2) as u8,
            255,
        ),
    }
}

fn pack_565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
}

/// Blend a 565 color with a 565 fog color by factor `f` (1.0 = no fog).
fn fog_565(color: u16, fog: u16, f: f32) -> u16 {
    let r = ((((color >> 11) & 0x1f) << 3) | ((color >> 11) & 0x1f) >> 2) as f32;
    let g = ((((color >> 5) & 0x3f) << 2) | ((color >> 5) & 0x3f) >> 4) as f32;
    let b = (((color & 0x1f) << 3) | (color & 0x1f) >> 2) as f32;
    let fr = ((((fog >> 11) & 0x1f) << 3) | ((fog >> 11) & 0x1f) >> 2) as f32;
    let fg = ((((fog >> 5) & 0x3f) << 2) | ((fog >> 5) & 0x3f) >> 4) as f32;
    let fb = (((fog & 0x1f) << 3) | (fog & 0x1f) >> 2) as f32;
    pack_565(
        (r * f + fr * (1.0 - f)) as u8,
        (g * f + fg * (1.0 - f)) as u8,
        (b * f + fb * (1.0 - f)) as u8,
    )
}

/// Sample one texel (nearest), returning (RGB565 color, 0-255 alpha).
fn sample_texel(
    mem: &Memory,
    addr: u32,
    width: u32,
    height: u32,
    u: f32,
    v: f32,
    fmt: u8,
) -> Option<(u16, u8)> {
    if width == 0 || height == 0 {
        return None;
    }
    let u = u.fract();
    let u = if u < 0.0 { u + 1.0 } else { u };
    let v = v.fract();
    let v = if v < 0.0 { v + 1.0 } else { v };
    let x = (u * (width - 1) as f32) as u32 % width;
    let y = (v * (height - 1) as f32) as u32 % height;
    let p = mem.read::<u16>(addr + (y * width + x) * 2);
    let (r, g, b, a) = decode_texel(p, fmt);
    Some((pack_565(r, g, b), a))
}

/// Bilinear sample (D3DTFG_LINEAR), returning (RGB565 color, 0-255 alpha).
/// Texel centers sit at half-integer coordinates, so the footprint is
/// offset by 0.5 and addresses wrap like the nearest sampler.
fn sample_texel_linear(
    mem: &Memory,
    addr: u32,
    width: u32,
    height: u32,
    u: f32,
    v: f32,
    fmt: u8,
) -> Option<(u16, u8)> {
    if width == 0 || height == 0 {
        return None;
    }
    let u = u.fract();
    let u = if u < 0.0 { u + 1.0 } else { u };
    let v = v.fract();
    let v = if v < 0.0 { v + 1.0 } else { v };
    let fx = u * width as f32 - 0.5;
    let fy = v * height as f32 - 0.5;
    let x0 = fx.floor() as i32;
    let y0 = fy.floor() as i32;
    let dx = fx - x0 as f32;
    let dy = fy - y0 as f32;
    let tap = |x: i32, y: i32| {
        let x = x.rem_euclid(width as i32) as u32;
        let y = y.rem_euclid(height as i32) as u32;
        let p = mem.read::<u16>(addr + (y * width + x) * 2);
        decode_texel(p, fmt)
    };
    let c00 = tap(x0, y0);
    let c10 = tap(x0 + 1, y0);
    let c01 = tap(x0, y0 + 1);
    let c11 = tap(x0 + 1, y0 + 1);
    // Lerp each channel: top row c00->c10, bottom c01->c11, then across.
    let mix = |i: usize| {
        let g = |c: (u8, u8, u8, u8)| match i {
            0 => c.0,
            1 => c.1,
            2 => c.2,
            _ => c.3,
        };
        let a = g(c00) as f32 + (g(c10) as f32 - g(c00) as f32) * dx;
        let b = g(c01) as f32 + (g(c11) as f32 - g(c01) as f32) * dx;
        (a + (b - a) * dy) as u8
    };
    let (r, g, b, a) = (mix(0), mix(1), mix(2), mix(3));
    Some((pack_565(r, g, b), a))
}

fn rasterize(
    ctx: &mut Context,
    this: u32,
    dptPrimitiveType: u32,
    dwVertexTypeDesc: u32,
    lpvVertices: u32,
    dwVertexCount: u32,
    lpwIndices: u32,
    dwIndexCount: u32,
) {
    // Only the FVF and primitive types the race loop actually uses.
    if !(4..=6).contains(&dptPrimitiveType) || dwVertexTypeDesc != 0x1c4 {
        log::debug!(
            "rasterize: skip prim={} fvf={:#x} verts={}",
            dptPrimitiveType,
            dwVertexTypeDesc,
            dwVertexCount
        );
        return;
    }

    #[allow(clippy::too_many_arguments)]
    struct Targets {
        rt_surface: u32,
        rt_addr: u32,
        rt_width: u32,
        rt_height: u32,
        rt_bpp: u32,
        tex_addr: u32,
        tex_width: u32,
        tex_height: u32,
        tex_bpp: u32,
        tex_fmt: u8,
        mag_filter: u32,
        min_filter: u32,
        // (pixels, width, height) for each filled sub-level past the base.
        tex_mips: Vec<(u32, u32, u32)>,
        zbuf_addr: u32,
        cull: u32,
        zenable: u32,
        zwrite: u32,
        zfunc: u32,
        alpha_blend: u32,
        src_blend: u32,
        dst_blend: u32,
        alpha_test: u32,
        alpha_func: u32,
        alpha_ref: u32,
        flat_shade: bool,
        fog_color: u16,
        fog_start: f32,
        fog_end: f32,
        fog_linear: bool,
    }

    let t = 'targets: {
        let devices = d3d_state().devices.borrow();
        let Some(device) = devices.get(&this) else {
            return;
        };
        let rt_surface_key = device.render_target;
        let tex = *device.textures.get(&0).unwrap_or(&0);
        // D3D7 defaults: cull CCW; z test on, writes on, less-or-equal.
        let cull = *device
            .render_states
            .get(&D3DRENDERSTATE_CULLMODE)
            .unwrap_or(&D3DCULL_CCW);
        let zenable = *device
            .render_states
            .get(&D3DRENDERSTATE_ZENABLE)
            .unwrap_or(&1);
        let zwrite = *device
            .render_states
            .get(&D3DRENDERSTATE_ZWRITEENABLE)
            .unwrap_or(&1);
        let zfunc = *device
            .render_states
            .get(&D3DRENDERSTATE_ZFUNC)
            .unwrap_or(&4);
        // D3D7 defaults: no blending, src=ONE dst=ZERO.
        let alpha_blend = *device
            .render_states
            .get(&D3DRENDERSTATE_ALPHABLENDENABLE)
            .unwrap_or(&0);
        let src_blend = *device
            .render_states
            .get(&D3DRENDERSTATE_SRCBLEND)
            .unwrap_or(&2);
        let dst_blend = *device
            .render_states
            .get(&D3DRENDERSTATE_DESTBLEND)
            .unwrap_or(&1);
        // Alpha test defaults: disabled, ALWAYS(8), ref 0.
        let alpha_test = *device
            .render_states
            .get(&D3DRENDERSTATE_ALPHATESTENABLE)
            .unwrap_or(&0);
        let alpha_func = *device
            .render_states
            .get(&D3DRENDERSTATE_ALPHAFUNC)
            .unwrap_or(&8);
        let alpha_ref = *device
            .render_states
            .get(&D3DRENDERSTATE_ALPHAREF)
            .unwrap_or(&0);
        let shade_mode = *device
            .render_states
            .get(&D3DRENDERSTATE_SHADEMODE)
            .unwrap_or(&2);
        let flat_shade = shade_mode == D3DSHADE_FLAT;

        // Linear fog.  FOGVERTEXMODE=3 selects linear; FOGSTART/FOGEND are
        // stored as u32 bit patterns of an f32.
        let fog_color = *device
            .render_states
            .get(&D3DRENDERSTATE_FOGCOLOR)
            .unwrap_or(&0);
        let fog_start = f32::from_bits(
            *device
                .render_states
                .get(&D3DRENDERSTATE_FOGSTART)
                .unwrap_or(&0),
        );
        let fog_end = f32::from_bits(
            *device
                .render_states
                .get(&D3DRENDERSTATE_FOGEND)
                .unwrap_or(&0),
        );
        let fog_vertex_mode = *device
            .render_states
            .get(&D3DRENDERSTATE_FOGVERTEXMODE)
            .unwrap_or(&0);
        let fog_linear = fog_vertex_mode == D3DFOG_LINEAR && fog_start < fog_end;

        let surfs = state().surf.borrow();
        let rt_surf = surfs.get(&rt_surface_key).cloned();
        let tex_surf = surfs.get(&tex).cloned();
        drop(surfs);

        // Classify the texture's channel masks so alpha-bearing formats
        // decode properly instead of being read as opaque 565.
        let tex_fmt = tex_surf
            .as_ref()
            .map(|s| {
                let pf = &s.borrow().pixel_format;
                if pf.dwRGBAlphaBitMask == 0x8000 && pf.dwRBitMask == 0x7c00 {
                    TEXFMT_A1R5G5B5
                } else if pf.dwRGBAlphaBitMask == 0xf000 && pf.dwRBitMask == 0x0f00 {
                    TEXFMT_A4R4G4B4
                } else {
                    TEXFMT_RGB565
                }
            })
            .unwrap_or(TEXFMT_RGB565);

        let Some(rt_surf) = rt_surf else {
            log::debug!(
                "rasterize: skip prim={} - no render target surface {rt_surface_key:#x}",
                dptPrimitiveType
            );
            return;
        };
        let rt = rt_surf.borrow();
        let (rt_w, rt_h, rt_bpp) = (rt.width, rt.height, rt.bytes_per_pixel);
        // The z-buffer rides along as an attached DDSCAPS_ZBUFFER surface.
        let zbuf_surf = rt
            .attachments
            .iter()
            .find(|s| s.borrow().caps.dwCaps.contains(DDSCAPS::ZBUFFER))
            .cloned();

        let (tex_w, tex_h, tex_bpp, tex_addr) = if let Some(ref s) = tex_surf {
            let s_borrow = s.borrow();
            let (w, h, bpp) = (s_borrow.width, s_borrow.height, s_borrow.bytes_per_pixel);
            if s_borrow.pixels.is_none() && d3d_state().unwritten_textures.borrow_mut().insert(tex)
            {
                log::debug!("rasterize: texture {tex:#x} ({w}x{h}) was never locked or blitted");
            }
            let pixels_addr = s_borrow.pixels;
            drop(s_borrow);
            // THESEUS_TEX_DUMP=<dir> writes each bound texture's pixel data
            // once as a PPM, for diagnosing what the rasterizer samples.
            if let (Some(addr), Ok(dir)) = (pixels_addr, std::env::var("THESEUS_TEX_DUMP"))
                && bpp == 2
                && d3d_state().dumped_textures.borrow_mut().insert(tex)
            {
                let path = format!("{dir}/tex_{tex:08x}.ppm");
                let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
                for i in 0..(w * h) {
                    let p = ctx.memory.read::<u16>(addr + i * 2);
                    out.push((((p >> 11) & 0x1f) << 3) as u8);
                    out.push((((p >> 5) & 0x3f) << 2) as u8);
                    out.push(((p & 0x1f) << 3) as u8);
                }
                let _ = std::fs::write(&path, out);
            }
            let addr = s.borrow_mut().lock(&mut ctx.memory);
            (w, h, bpp, addr)
        } else {
            (0, 0, 0, 0)
        };

        // Mip sub-levels that the game actually filled (pixels present).
        // D3DTSS_MIPFILTER = 18; an explicit D3DTFP_NONE(1) disables mips.
        let tex_mips = if let Some(tex) = &tex_surf
            && *device.texture_stage_states.get(&(0, 18)).unwrap_or(&0) != 1
        {
            let mut levels = Vec::new();
            for att in &tex.borrow().attachments {
                let l = att.borrow();
                if !l.caps.dwCaps.contains(DDSCAPS::MIPMAP) {
                    continue;
                }
                match l.pixels {
                    Some(p) => levels.push((p, l.width, l.height)),
                    None => break, // a hole in the chain ends it
                }
            }
            levels
        } else {
            Vec::new()
        };

        // Texture-stage filters: D3DTSS_MAGFILTER=16, MINFILTER=17 —
        // D3DTFG_POINT(1) by default.
        let tss = |s: u32| *device.texture_stage_states.get(&(0, s)).unwrap_or(&1);
        let mag_filter = tss(16);
        let min_filter = tss(17);

        // Lock the render target now so its pixel address is known.
        drop(rt);
        let rt_addr = rt_surf.borrow_mut().lock(&mut ctx.memory);
        let zbuf_addr = zbuf_surf
            .map(|z| z.borrow_mut().lock(&mut ctx.memory))
            .unwrap_or(0);

        break 'targets Targets {
            rt_surface: rt_surface_key,
            rt_addr,
            rt_width: rt_w,
            rt_height: rt_h,
            rt_bpp,
            tex_addr,
            tex_width: tex_w,
            tex_height: tex_h,
            tex_bpp,
            tex_fmt,
            mag_filter,
            min_filter,
            tex_mips,
            zbuf_addr,
            cull,
            zenable,
            zwrite,
            zfunc,
            alpha_blend,
            src_blend,
            dst_blend,
            alpha_test,
            alpha_func,
            alpha_ref,
            flat_shade,
            fog_color: argb_to_565(fog_color),
            fog_start,
            fog_end,
            fog_linear,
        };
    };

    if t.rt_bpp != 2 || (t.tex_addr != 0 && t.tex_bpp != 2) {
        log::debug!(
            "rasterize: skip prim={} verts={} rt_bpp={} tex_bpp={} tex={:#x}",
            dptPrimitiveType,
            dwVertexCount,
            t.rt_bpp,
            t.tex_bpp,
            t.tex_addr
        );
        return;
    }
    if t.rt_addr == 0 || dwVertexCount < 3 {
        log::debug!(
            "rasterize: skip prim={} verts={} rt_addr={:#x}",
            dptPrimitiveType,
            dwVertexCount,
            t.rt_addr
        );
        return;
    }

    let vsize = vertex_size(dwVertexTypeDesc);
    let stride = t.rt_width * t.rt_bpp;
    // THESEUS_PROBE=x,y logs every draw that writes that render-target pixel.
    let probe_addr = std::env::var("THESEUS_PROBE").ok().and_then(|s| {
        let (x, y) = s.split_once(',')?;
        Some(t.rt_addr + y.parse::<u32>().ok()? * stride + x.parse::<u32>().ok()? * 2)
    });
    // Z-testing is only meaningful when a z-buffer is actually attached.
    let zbuf_addr = if t.zenable != 0 && std::env::var("THESEUS_NO_ZTEST").is_err() {
        t.zbuf_addr
    } else {
        0
    };
    let zstride = t.rt_width * 2;

    // Expand the draw into an index stream (or implicit vertex ordinals),
    // then decompose it into independent triangles per D3DPRIMITIVETYPE.
    let verts: Vec<u32> = if lpwIndices != 0 && dwIndexCount >= 3 {
        (0..dwIndexCount)
            .map(|i| ctx.memory.read::<u16>(lpwIndices + i * 2) as u32)
            .collect()
    } else {
        (0..dwVertexCount).collect()
    };
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    let mut pixels_written = 0u32;
    let mut last_color = 0u16;
    match dptPrimitiveType {
        // D3DPT_TRIANGLELIST: each consecutive triple is one triangle.
        4 => {
            for tri in verts.chunks_exact(3) {
                triangles.push([tri[0], tri[1], tri[2]]);
            }
        }
        // D3DPT_TRIANGLESTRIP: alternating winding so every emitted
        // triangle has the same orientation.
        5 => {
            for i in 2..verts.len() {
                if i % 2 == 0 {
                    triangles.push([verts[i - 2], verts[i - 1], verts[i]]);
                } else {
                    triangles.push([verts[i - 1], verts[i - 2], verts[i]]);
                }
            }
        }
        // D3DPT_TRIANGLEFAN: every triangle shares the first vertex.
        6 => {
            for i in 2..verts.len() {
                triangles.push([verts[0], verts[i - 1], verts[i]]);
            }
        }
        _ => {}
    }

    let tri_count = triangles.len();
    for tri in triangles {
        let a = read_vertex(&ctx.memory, lpvVertices + tri[0] * vsize);
        let b = read_vertex(&ctx.memory, lpvVertices + tri[1] * vsize);
        let c = read_vertex(&ctx.memory, lpvVertices + tri[2] * vsize);

        // Triangle area in screen space; in the y-down convention a positive
        // signed area is the clockwise (front) face.
        let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
        if area == 0.0 {
            continue;
        }
        match t.cull {
            D3DCULL_CW if area > 0.0 => continue,
            D3DCULL_CCW if area < 0.0 => continue,
            _ => {}
        }

        // With mips available, λ = log2(max texel-space UV derivative) picks
        // the level per pixel, evaluated below in the pixel loop.
        let base_level = (t.tex_addr, t.tex_width, t.tex_height);

        // Compute the 2D bounding box, clamped to the render target.
        let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as i32;
        let max_x = a.x.max(b.x).max(c.x).ceil() as i32;
        let min_x = min_x.min(t.rt_width as i32).max(0);
        let max_x = max_x.min(t.rt_width as i32);

        let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as i32;
        let max_y = a.y.max(b.y).max(c.y).ceil() as i32;
        let min_y = min_y.min(t.rt_height as i32).max(0);
        let max_y = max_y.min(t.rt_height as i32);

        // The covered pixels on each scanline form a contiguous span, so
        // bound it by the edge intercepts and walk the barycentric edge
        // functions incrementally instead of testing the whole bounding box.
        // w0(x,y) = (b.x-x)(c.y-y) - (b.y-y)(c.x-x)
        //         = (b.y-c.y)*x + (c.x-b.x)*y + (b.x*c.y - c.x*b.y)
        // and likewise for w1 along edge c->a; w2 = area - w0 - w1.
        let dw0dx = b.y - c.y;
        let dw0dy = c.x - b.x;
        let dw1dx = c.y - a.y;
        let dw1dy = a.x - c.x;
        let w0_const = b.x * c.y - c.x * b.y;
        let w1_const = c.x * a.y - a.x * c.y;

        for py in min_y..max_y {
            let py_f = py as f32 + 0.5;

            // Scanline bounds from the triangle/edge crossings.
            let mut span_lo = f32::INFINITY;
            let mut span_hi = f32::NEG_INFINITY;
            for [x1, y1, x2, y2] in [
                [a.x, a.y, b.x, b.y],
                [b.x, b.y, c.x, c.y],
                [c.x, c.y, a.x, a.y],
            ] {
                if (y1 <= py_f) == (y2 <= py_f) {
                    continue;
                }
                let xi = x1 + (py_f - y1) * (x2 - x1) / (y2 - y1);
                span_lo = span_lo.min(xi);
                span_hi = span_hi.max(xi);
            }
            // A one-pixel margin keeps the inclusive edge test
            // authoritative at the span boundaries.
            let xs = ((span_lo - 0.5).floor() as i32 - 1).max(min_x);
            let xe = ((span_hi - 0.5).ceil() as i32 + 1).min(max_x);
            if xs >= xe {
                continue;
            }

            // Edge functions at the first pixel center, stepped across the
            // span by their constant x-derivatives.
            let mut w0 = dw0dx * (xs as f32 + 0.5) + dw0dy * py_f + w0_const;
            let mut w1 = dw1dx * (xs as f32 + 0.5) + dw1dy * py_f + w1_const;
            let mut entered = false;
            for px in xs..xe {
                // Barycentric weights using sub-triangle areas.
                let w2 = area - w0 - w1;
                let alpha = w0 / area;
                let beta = w1 / area;
                let gamma = w2 / area;
                let inside = w0 * area >= 0.0 && w1 * area >= 0.0 && w2 * area >= 0.0;
                w0 += dw0dx;
                w1 += dw1dx;
                if !inside {
                    // Coverage on a scanline is contiguous; nothing further
                    // right can re-enter the triangle.
                    if entered {
                        break;
                    }
                    continue;
                }
                entered = true;

                // Perspective-correct texture coordinate interpolation.
                let w_sum = alpha * a.w + beta * b.w + gamma * c.w;
                let persp = t.tex_addr != 0 && w_sum != 0.0;
                let u = if persp {
                    (alpha * a.u_w + beta * b.u_w + gamma * c.u_w) / w_sum
                } else {
                    alpha * a.u + beta * b.u + gamma * c.u
                };
                let v = if persp {
                    (alpha * a.v_w + beta * b.v_w + gamma * c.v_w) / w_sum
                } else {
                    alpha * a.v + beta * b.v + gamma * c.v
                };

                // Source color and alpha. The default MODULATE stage
                // multiplies the texture by the vertex diffuse; 565 textures
                // have no texel alpha so theirs is the diffuse alpha alone.
                // D3DSHADE_FLAT uses the first vertex's diffuse for the whole
                // triangle, GOURAUD interpolates it.
                let diffuse = if t.flat_shade {
                    a.diffuse
                } else {
                    let rr = (alpha * ((a.diffuse >> 16) & 0xff) as f32
                        + beta * ((b.diffuse >> 16) & 0xff) as f32
                        + gamma * ((c.diffuse >> 16) & 0xff) as f32)
                        as u32;
                    let gg = (alpha * ((a.diffuse >> 8) & 0xff) as f32
                        + beta * ((b.diffuse >> 8) & 0xff) as f32
                        + gamma * ((c.diffuse >> 8) & 0xff) as f32)
                        as u32;
                    let bb = (alpha * ((a.diffuse) & 0xff) as f32
                        + beta * ((b.diffuse) & 0xff) as f32
                        + gamma * ((c.diffuse) & 0xff) as f32) as u32;
                    let aa = (alpha * ((a.diffuse >> 24) & 0xff) as f32
                        + beta * ((b.diffuse >> 24) & 0xff) as f32
                        + gamma * ((c.diffuse >> 24) & 0xff) as f32)
                        as u32;
                    (aa.min(255) << 24) | (rr.min(255) << 16) | (gg.min(255) << 8) | bb.min(255)
                };
                let diff_a = ((diffuse >> 24) & 0xff) as f32;
                let (mut color, sa) = if t.tex_addr != 0 {
                    // rho is the texel-space UV derivative magnitude, needed
                    // both to pick the mip level and to choose between the
                    // magnification and minification filters.
                    let filtered = t.min_filter == 2 || t.mag_filter == 2; // D3DTFG_LINEAR
                    let rho = if !t.tex_mips.is_empty() || filtered {
                        // d(uv)/dx and d(uv)/dy: re-evaluate the barycentrics
                        // one pixel right and one down (w0/w1 were already
                        // stepped in x, so step them back for the y probe).
                        let alpha2 = w0 / area;
                        let beta2 = w1 / area;
                        let alpha3 = (w0 - dw0dx + dw0dy) / area;
                        let beta3 = (w1 - dw1dx + dw1dy) / area;
                        let uv_at = |al: f32, be: f32| {
                            let ga = 1.0 - al - be;
                            if persp {
                                let w = al * a.w + be * b.w + ga * c.w;
                                (
                                    (al * a.u_w + be * b.u_w + ga * c.u_w) / w,
                                    (al * a.v_w + be * b.v_w + ga * c.v_w) / w,
                                )
                            } else {
                                (
                                    al * a.u + be * b.u + ga * c.u,
                                    al * a.v + be * b.v + ga * c.v,
                                )
                            }
                        };
                        let (u2, v2) = uv_at(alpha2, beta2);
                        let (u3, v3) = uv_at(alpha3, beta3);
                        ((u2 - u) * t.tex_width as f32)
                            .abs()
                            .max(((v2 - v) * t.tex_height as f32).abs())
                            .max(((u3 - u) * t.tex_width as f32).abs())
                            .max(((v3 - v) * t.tex_height as f32).abs())
                    } else {
                        0.0
                    };
                    let (taddr, tw, th) = if rho > 0.0 && !t.tex_mips.is_empty() {
                        let lvl = rho.max(1e-9).log2().round().max(0.0) as usize;
                        if lvl == 0 {
                            base_level
                        } else {
                            t.tex_mips
                                .get(lvl - 1)
                                .copied()
                                .or_else(|| t.tex_mips.last().copied())
                                .unwrap_or(base_level)
                        }
                    } else {
                        base_level
                    };
                    // rho > 1 is minifying (MINFILTER), <= 1 magnifying.
                    let linear = if rho > 1.0 {
                        t.min_filter == 2
                    } else {
                        t.mag_filter == 2
                    };
                    let (c, ta) = if linear {
                        sample_texel_linear(&ctx.memory, taddr, tw, th, u, v, t.tex_fmt)
                    } else {
                        sample_texel(&ctx.memory, taddr, tw, th, u, v, t.tex_fmt)
                    }
                    .unwrap_or((argb_to_565(0xff_00_00_00), 255));
                    (c, ta as f32 * diff_a / 255.0)
                } else {
                    let r = ((diffuse >> 16) & 0xff).min(255);
                    let g = ((diffuse >> 8) & 0xff).min(255);
                    let b = ((diffuse) & 0xff).min(255);
                    (
                        ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3),
                        diff_a,
                    )
                };

                // Linear fog: view-space depth is 1/w (rhw).  Objects inside
                // FOGSTART keep full color, objects past FOGEND become fog.
                if t.fog_linear && w_sum != 0.0 {
                    let dist = 1.0 / w_sum;
                    let f = ((t.fog_end - dist) / (t.fog_end - t.fog_start)).clamp(0.0, 1.0);
                    color = fog_565(color, t.fog_color, f);
                }

                // An alpha-tested-out fragment leaves color and depth alone.
                if t.alpha_test != 0 && !z_passes(t.alpha_func, sa as u16, t.alpha_ref as u16) {
                    continue;
                }

                // Depth is linear in screen space for pre-transformed verts.
                if zbuf_addr != 0 {
                    let z = z_to_u16(alpha * a.z + beta * b.z + gamma * c.z);
                    let zaddr = zbuf_addr + py as u32 * zstride + px as u32 * 2;
                    if !z_passes(t.zfunc, z, ctx.memory.read::<u16>(zaddr)) {
                        continue;
                    }
                    if t.zwrite != 0 {
                        ctx.memory.write::<u16>(zaddr, z);
                    }
                }

                let pixel_addr = t.rt_addr + py as u32 * stride + px as u32 * 2;
                let color = if t.alpha_blend != 0 {
                    let dst = ctx.memory.read::<u16>(pixel_addr);
                    blend_565(dst, color, sa, t.src_blend, t.dst_blend)
                } else {
                    color
                };
                ctx.memory.write::<u16>(pixel_addr, color);
                pixels_written += 1;
                last_color = color;
                if probe_addr == Some(pixel_addr) {
                    log::warn!(
                        "rasterize probe: prim={dptPrimitiveType} fvf={dwVertexTypeDesc:#x} tri={tri:?} rt={:#x} tex={:#x} fmt={} {}x{} mips={} color={color:#06x} sa={} zen={} zw={} zf={} atest={} afunc={} aref={} blend={} sb={} db={} a=({},{},z={} d={:#x} uv={},{}) b=({},{},z={} d={:#x} uv={},{}) c=({},{},z={} d={:#x} uv={},{})",
                        t.rt_addr,
                        t.tex_addr,
                        t.tex_fmt,
                        t.tex_width,
                        t.tex_height,
                        t.tex_mips.len(),
                        sa as u32,
                        t.zenable,
                        t.zwrite,
                        t.zfunc,
                        t.alpha_test,
                        t.alpha_func,
                        t.alpha_ref,
                        t.alpha_blend,
                        t.src_blend,
                        t.dst_blend,
                        a.x,
                        a.y,
                        a.z,
                        a.diffuse,
                        a.u,
                        a.v,
                        b.x,
                        b.y,
                        b.z,
                        b.diffuse,
                        b.u,
                        b.v,
                        c.x,
                        c.y,
                        c.z,
                        c.diffuse,
                        c.u,
                        c.v,
                    );
                    // Dump each distinct texture feeding the probed draws.
                    if t.tex_addr != 0 {
                        static DUMPED: std::sync::Mutex<Option<std::collections::HashSet<u32>>> =
                            std::sync::Mutex::new(None);
                        if DUMPED
                            .lock()
                            .unwrap()
                            .get_or_insert_with(Default::default)
                            .insert(t.tex_addr)
                        {
                            let (ta, tw, th) = base_level;
                            let mut out = format!("P6\n{tw} {th}\n255\n").into_bytes();
                            for i in 0..(tw * th) {
                                let p = ctx
                                    .memory
                                    .bytes
                                    .get((ta + i * 2) as usize..(ta + (i + 1) * 2) as usize)
                                    .and_then(|b| <[u8; 2]>::try_from(b).ok())
                                    .map(u16::from_le_bytes)
                                    .unwrap_or(0);
                                out.push((((p >> 11) & 0x1f) << 3) as u8);
                                out.push((((p >> 5) & 0x3f) << 2) as u8);
                                out.push(((p & 0x1f) << 3) as u8);
                            }
                            let path = format!("/tmp/probe_tex_{ta:08x}.ppm");
                            let _ = std::fs::write(&path, out);
                            log::warn!("probe tex dump: addr={ta:#x} {tw}x{th} -> {path}");
                        }
                    }
                }
            }
        }
    }
    log::debug!(
        "rasterize: prim={} verts={} tris={} wrote {} px rt_surf={:#x} rt={:#x} {}x{} tex={:#x} color={:#06x}",
        dptPrimitiveType,
        dwVertexCount,
        tri_count,
        pixels_written,
        t.rt_surface,
        t.rt_addr,
        t.rt_width,
        t.rt_height,
        t.tex_addr,
        last_color
    );
    if pixels_written != 0
        && let Some(device) = d3d_state().devices.borrow_mut().get_mut(&this)
    {
        device.drew_since_clear = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_desc7_has_the_win32_layout_size() {
        assert_eq!(std::mem::size_of::<D3DDEVICEDESC7>(), 236);
        assert_eq!(std::mem::size_of::<D3DPRIMCAPS>(), 56);
        assert_eq!(std::mem::size_of::<D3DVERTEXBUFFERDESC>(), 16);
        assert_eq!(std::mem::size_of::<D3DVIEWPORT7>(), 24);
        assert_eq!(std::mem::size_of::<D3DMATERIAL7>(), 68);
        assert_eq!(std::mem::size_of::<D3DCLIPSTATUS>(), 32);
    }

    #[test]
    fn vertex_size_computes_fvf_strides() {
        // D3DFVF_XYZ | D3DFVF_NORMAL | D3DFVF_DIFFUSE | D3DFVF_TEX1
        assert_eq!(vertex_size(0x002 | 0x010 | 0x040 | 0x100), 12 + 12 + 4 + 8);
        // D3DFVF_XYZRHW | D3DFVF_DIFFUSE | D3DFVF_TEX2
        assert_eq!(vertex_size(0x004 | 0x040 | 0x200), 16 + 4 + 16);
        // D3DFVF_XYZB3 (3 blend weights)
        assert_eq!(vertex_size(0x00a), 12 + 3 * 4);
    }
}
