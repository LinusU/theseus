//! Direct3D Immediate Mode as of DirectX 3/5: IDirect3D and the objects hung
//! off it, drawn through execute buffers.
//!
//! So far this is a probe, not a renderer: it accepts everything a game asks
//! of it so the game takes its Direct3D path, and logs what that path uses
//! (execute buffer opcodes, render states, vertex layouts, texture formats)
//! the first time each thing is seen. Nothing is drawn.

use std::collections::{BTreeSet, HashMap};

use runtime::Context;

use crate::{
    ddraw::{DD, GUID, state},
    heap::Heap,
    kernel32, stub,
};

pub const IID_IDirect3D: GUID = GUID((
    0x3bba0080,
    0x2421,
    0x11cf,
    [0xa3, 0x1a, 0x00, 0xaa, 0x00, 0xb9, 0x33, 0x56],
));
pub const IID_IDirect3DHALDevice: GUID = GUID((
    0x84e63de0,
    0x46aa,
    0x11cf,
    [0x81, 0x6f, 0x00, 0x00, 0xc0, 0x20, 0x15, 0x6e],
));
pub const IID_IDirect3DTexture: GUID = GUID((
    0x2cdcd9e0,
    0x25a0,
    0x11cf,
    [0xa3, 0x1a, 0x00, 0xaa, 0x00, 0xb9, 0x33, 0x56],
));

/// sizeof(D3DDEVICEDESC) in the DirectX 5 headers, which the game was built
/// with.
const DEVICEDESC_SIZE: u32 = 0xcc;

struct ExecuteBuffer {
    size: u32,
    data: Option<u32>,
    /// D3DEXECUTEDATA: dwVertexOffset, dwVertexCount, dwInstructionOffset,
    /// dwInstructionLength, dwHVertexOffset.
    execute_data: [u32; 5],
}

#[derive(Default)]
pub struct D3D {
    execute_buffers: HashMap<u32, ExecuteBuffer>,
    /// IDirect3DTexture pointer -> the surface it wraps.
    textures: HashMap<u32, u32>,
    next_matrix: u32,
    probe: Probe,
}

/// What the game's Direct3D path uses, gathered as it runs.
#[derive(Default)]
struct Probe {
    seen: BTreeSet<String>,
    scenes: u32,
    executes: u32,
    triangles: u32,
    vertices: u32,
}

impl Probe {
    /// Log `what` the first time it happens.
    fn note(&mut self, what: String) {
        if !self.seen.contains(&what) {
            log::info!("d3d: {what}");
            self.seen.insert(what);
        }
    }
}

fn note(what: String) {
    state().d3d.borrow_mut().probe.note(what);
}

fn alloc(ctx: &mut Context, size: u32) -> u32 {
    let addr = kernel32::lock().process_heap.alloc(&mut ctx.memory, size);
    ctx.memory[addr..][..size as usize].fill(0);
    addr
}

fn write_u32s(ctx: &mut Context, addr: u32, values: &[u32]) {
    for (i, &value) in values.iter().enumerate() {
        ctx.memory.write::<u32>(addr + i as u32 * 4, value);
    }
}

/// A D3DDEVICEDESC for a hardware device with the capabilities of a good 1998
/// accelerator.
fn write_hal_desc(ctx: &mut Context, addr: u32) {
    const D3DPRIMCAPS: [u32; 14] = [
        56,       // dwSize
        0x72,     // dwMiscCaps: MASKZ | CULLNONE | CULLCW | CULLCCW
        0x1b1,    // dwRasterCaps: DITHER | ZTEST | SUBPIXEL | FOGVERTEX | FOGTABLE
        0xff,     // dwZCmpCaps: all
        0x1fff,   // dwSrcBlendCaps: all
        0x1fff,   // dwDestBlendCaps: all
        0xff,     // dwAlphaCmpCaps: all
        0x8520a,  // dwShadeCaps: flat/gouraud color, specular, alpha, fog
        0xd,      // dwTextureCaps: PERSPECTIVE | ALPHA | TRANSPARENCY
        0x3f,     // dwTextureFilterCaps: nearest, linear, mipmapped
        0xcf,     // dwTextureBlendCaps: DECAL | MODULATE | DECALALPHA | MODULATEALPHA | COPY | ADD
        0x7,      // dwTextureAddressCaps: WRAP | MIRROR | CLAMP
        0, 0,     // dwStippleWidth, dwStippleHeight
    ];
    let mut desc = vec![
        DEVICEDESC_SIZE,
        0x7ff,        // dwFlags: every field below is valid
        2,            // dcmColorModel: D3DCOLOR_RGB
        0x1 | 0x10 | 0x40 | 0x100 | 0x200 | 0x400, // dwDevCaps
        8, 1,         // dtcTransformCaps: D3DTRANSFORMCAPS_CLIP
        1,            // bClipping
        16, 0x7, 1, 8, // dlcLightingCaps: point/spot/directional, RGB model, 8 lights
    ];
    desc.extend_from_slice(&D3DPRIMCAPS); // dpcLineCaps
    desc.extend_from_slice(&D3DPRIMCAPS); // dpcTriCaps
    desc.extend_from_slice(&[
        0x400,  // dwDeviceRenderBitDepth: DDBD_16
        0x400,  // dwDeviceZBufferBitDepth: DDBD_16
        0,      // dwMaxBufferSize: no limit
        0xffff, // dwMaxVertexCount
        1, 1, 256, 256, // texture size limits
        0, 0, 0, 0, // stipple sizes
    ]);
    assert_eq!(desc.len() as u32 * 4, DEVICEDESC_SIZE);
    write_u32s(ctx, addr, &desc);
}

fn render_state_name(state: u32) -> &'static str {
    match state {
        1 => "TEXTUREHANDLE",
        2 => "ANTIALIAS",
        3 => "TEXTUREADDRESS",
        4 => "TEXTUREPERSPECTIVE",
        5 => "WRAPU",
        6 => "WRAPV",
        7 => "ZENABLE",
        8 => "FILLMODE",
        9 => "SHADEMODE",
        10 => "LINEPATTERN",
        11 => "MONOENABLE",
        12 => "ROP2",
        13 => "PLANEMASK",
        14 => "ZWRITEENABLE",
        15 => "ALPHATESTENABLE",
        16 => "LASTPIXEL",
        17 => "TEXTUREMAG",
        18 => "TEXTUREMIN",
        19 => "SRCBLEND",
        20 => "DESTBLEND",
        21 => "TEXTUREMAPBLEND",
        22 => "CULLMODE",
        23 => "ZFUNC",
        24 => "ALPHAREF",
        25 => "ALPHAFUNC",
        26 => "DITHERENABLE",
        27 => "BLENDENABLE",
        28 => "FOGENABLE",
        29 => "SPECULARENABLE",
        30 => "ZVISIBLE",
        31 => "SUBPIXEL",
        32 => "SUBPIXELX",
        33 => "STIPPLEDALPHA",
        34 => "FOGCOLOR",
        35 => "FOGTABLEMODE",
        36 => "FOGTABLESTART",
        37 => "FOGTABLEEND",
        38 => "FOGTABLEDENSITY",
        39 => "STIPPLEENABLE",
        40 => "EDGEANTIALIAS",
        41 => "COLORKEYENABLE",
        43 => "BORDERCOLOR",
        44 => "TEXTUREADDRESSU",
        45 => "TEXTUREADDRESSV",
        46 => "MIPMAPLODBIAS",
        47 => "ZBIAS",
        48 => "RANGEFOGENABLE",
        49 => "ANISOTROPY",
        50 => "FLUSHBATCH",
        _ => "?",
    }
}

/// Walk an execute buffer's instruction stream, recording what it contains.
fn probe_execute(ctx: &mut Context, data: u32, exec: &[u32; 5]) {
    let [vertex_offset, vertex_count, instr_offset, instr_len, _hvertex_offset] = *exec;
    let mut d3d = state().d3d.borrow_mut();
    let probe = &mut d3d.probe;
    probe.executes += 1;
    probe.vertices += vertex_count;
    if vertex_offset != 0 {
        probe.note(format!("vertex offset {vertex_offset:#x}"));
    }

    let mut at = data + instr_offset;
    let end = at + instr_len;
    while at < end {
        let op = ctx.memory.read::<u8>(at);
        let size = ctx.memory.read::<u8>(at + 1) as u32;
        let count = ctx.memory.read::<u16>(at + 2) as u32;
        let payload = at + 4;
        match op {
            1 => probe.note("op POINT".into()),
            2 => probe.note("op LINE".into()),
            3 => {
                probe.note(format!("op TRIANGLE (size {size})"));
                probe.triangles += count;
                for i in 0..count {
                    let flags = ctx.memory.read::<u16>(payload + i * size + 6);
                    probe.note(format!("triangle flags {flags:#x}"));
                }
            }
            4 => probe.note("op MATRIXLOAD".into()),
            5 => probe.note("op MATRIXMULTIPLY".into()),
            6 | 7 | 8 => {
                let kind = ["STATETRANSFORM", "STATELIGHT", "STATERENDER"][op as usize - 6];
                for i in 0..count {
                    let ty = ctx.memory.read::<u32>(payload + i * size);
                    let value = ctx.memory.read::<u32>(payload + i * size + 4);
                    if op == 8 {
                        let name = render_state_name(ty);
                        if ty == 1 {
                            // Texture handles are pointers; the values say nothing.
                            probe.note(format!("{kind} {name}"));
                        } else {
                            probe.note(format!("{kind} {name}={value:#x}"));
                        }
                    } else {
                        probe.note(format!("{kind} {ty}"));
                    }
                }
            }
            9 => {
                for i in 0..count {
                    let rec = payload + i * size;
                    let flags = ctx.memory.read::<u32>(rec);
                    let start = ctx.memory.read::<u16>(rec + 4);
                    let dest = ctx.memory.read::<u16>(rec + 6);
                    let n = ctx.memory.read::<u32>(rec + 8);
                    let what = format!("op PROCESSVERTICES flags {flags:#x}");
                    if !probe.seen.contains(&what) {
                        // D3DTLVERTEX: sx sy sz rhw color specular tu tv.
                        let v = data + vertex_offset + start as u32 * 32;
                        let f = |o| ctx.memory.read::<f32>(v + o);
                        let c = |o| ctx.memory.read::<u32>(v + o);
                        log::info!(
                            "d3d: first vertex ({start}->{dest}, {n} of them): \
                             {:.2} {:.2} {:.4} {:.4} color {:#010x} spec {:#010x} uv {:.3} {:.3}",
                            f(0),
                            f(4),
                            f(8),
                            f(12),
                            c(16),
                            c(20),
                            f(24),
                            f(28)
                        );
                    }
                    probe.note(what);
                }
            }
            10 => probe.note("op TEXTURELOAD".into()),
            11 => {
                probe.note("op EXIT".into());
                break;
            }
            12 => probe.note("op BRANCHFORWARD".into()),
            13 => probe.note("op SPAN".into()),
            14 => probe.note("op SETSTATUS".into()),
            _ => {
                log::warn!("d3d: unknown execute buffer opcode {op}");
                break;
            }
        }
        at = payload + count * size;
    }
}

/// QueryInterface for objects that only answer to their own interface.
fn query_self(ctx: &mut Context, name: &str, this: u32, riid: u32, ppv: u32) -> DD {
    let iid = crate::Ptr::<GUID>::new(riid).read(&ctx.memory).unwrap();
    if iid == crate::ddraw::IID_IUnknown {
        ctx.memory.write::<u32>(ppv, this);
        return DD::OK;
    }
    log::warn!("{name}::QueryInterface({iid:?}): not supported");
    DD::E_NOINTERFACE
}

macro_rules! object_new {
    () => {
        pub static mut VTABLE: u32 = 0;

        pub fn new(ctx: &mut Context, heap: &mut Heap) -> u32 {
            let addr = heap.alloc(&mut ctx.memory, 4);
            ctx.memory.write(addr, unsafe { VTABLE });
            addr
        }
    };
}

fn new_object(ctx: &mut Context, new: fn(&mut Context, &mut Heap) -> u32) -> u32 {
    let mut kernel32 = kernel32::lock();
    new(ctx, &mut kernel32.process_heap)
}

pub mod IDirect3D {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 9] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "EnumDevices",
        "CreateLight",
        "CreateMaterial",
        "CreateViewport",
        "FindDevice",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3D", this, riid, ppvObject)
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
    pub fn Initialize(_ctx: &mut Context, _this: u32, _riid: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EnumDevices(ctx: &mut Context, _this: u32, lpEnumDevicesCallback: u32, lpUserArg: u32) -> DD {
        // One hardware device. The software rasterizers real DirectX also lists
        // would only give the game something worse to pick.
        let guid = alloc(ctx, 16);
        let GUID((d1, d2, d3, d4)) = IID_IDirect3DHALDevice;
        ctx.memory.write::<u32>(guid, d1);
        ctx.memory.write::<u16>(guid + 4, d2);
        ctx.memory.write::<u16>(guid + 6, d3);
        ctx.memory[guid + 8..][..8].copy_from_slice(&d4);
        let name = b"Direct3D HAL\0";
        let name_addr = alloc(ctx, name.len() as u32);
        ctx.memory[name_addr..][..name.len()].copy_from_slice(name);
        let hw = alloc(ctx, DEVICEDESC_SIZE);
        write_hal_desc(ctx, hw);
        let hel = alloc(ctx, DEVICEDESC_SIZE);
        ctx.memory.write::<u32>(hel, DEVICEDESC_SIZE);

        // LPD3DENUMDEVICESCALLBACK(lpGuid, lpDeviceDescription, lpDeviceName,
        //                          lpD3DHWDeviceDesc, lpD3DHELDeviceDesc, lpContext)
        let callback = ctx.indirect(lpEnumDevicesCallback);
        ctx.call32_x86(callback, vec![guid, name_addr, name_addr, hw, hel, lpUserArg]);

        let heap = &kernel32::lock().process_heap;
        for addr in [guid, name_addr, hw, hel] {
            heap.free(&mut ctx.memory, addr);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateLight(ctx: &mut Context, _this: u32, lplpDirect3DLight: u32, _pUnkOuter: u32) -> DD {
        let addr = new_object(ctx, IDirect3DLight::new);
        ctx.memory.write::<u32>(lplpDirect3DLight, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateMaterial(
        ctx: &mut Context,
        _this: u32,
        lplpDirect3DMaterial: u32,
        _pUnkOuter: u32,
    ) -> DD {
        let addr = new_object(ctx, IDirect3DMaterial::new);
        ctx.memory.write::<u32>(lplpDirect3DMaterial, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateViewport(
        ctx: &mut Context,
        _this: u32,
        lplpD3DViewport: u32,
        _pUnkOuter: u32,
    ) -> DD {
        let addr = new_object(ctx, IDirect3DViewport::new);
        ctx.memory.write::<u32>(lplpD3DViewport, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn FindDevice(_ctx: &mut Context, _this: u32, _lpD3DFDS: u32, _lpD3DFDR: u32) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    object_new!();
}

pub mod IDirect3DDevice {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 22] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "GetCaps",
        "SwapTextureHandles",
        "CreateExecuteBuffer",
        "GetStats",
        "Execute",
        "AddViewport",
        "DeleteViewport",
        "NextViewport",
        "Pick",
        "GetPickRecords",
        "EnumTextureFormats",
        "CreateMatrix",
        "SetMatrix",
        "GetMatrix",
        "DeleteMatrix",
        "BeginScene",
        "EndScene",
        "GetDirect3D",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DDevice", this, riid, ppvObject)
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
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpd3d: u32, _lpGUID: u32, _lpd3ddvdesc: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetCaps(ctx: &mut Context, _this: u32, lpD3DHWDevDesc: u32, lpD3DHELDevDesc: u32) -> DD {
        if lpD3DHWDevDesc != 0 {
            write_hal_desc(ctx, lpD3DHWDevDesc);
        }
        if lpD3DHELDevDesc != 0 {
            ctx.memory[lpD3DHELDevDesc + 4..][..DEVICEDESC_SIZE as usize - 4].fill(0);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SwapTextureHandles(_ctx: &mut Context, _this: u32, _lpD3DTex1: u32, _lpD3DTex2: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn EnumTextureFormats(
        ctx: &mut Context,
        _this: u32,
        lpd3dEnumTextureProc: u32,
        lpArg: u32,
    ) -> DD {
        // DDPF_* flags and masks: RGB565, ARGB1555, ARGB4444, 8- and 4-bit palettized.
        const FORMATS: &[(u32, u32, [u32; 4])] = &[
            (0x40, 16, [0xf800, 0x07e0, 0x001f, 0]),
            (0x41, 16, [0x7c00, 0x03e0, 0x001f, 0x8000]),
            (0x41, 16, [0x0f00, 0x00f0, 0x000f, 0xf000]),
            (0x60, 8, [0; 4]),
            (0x48, 4, [0; 4]),
        ];
        let size = std::mem::size_of::<crate::ddraw::types::DDSURFACEDESC>() as u32;
        let desc = alloc(ctx, size);
        for &(flags, bits, masks) in FORMATS {
            ctx.memory[desc..][..size as usize].fill(0);
            // DDSURFACEDESC: dwSize, dwFlags = DDSD_PIXELFORMAT | DDSD_CAPS, then
            // ddpfPixelFormat at 0x48 and ddsCaps (DDSCAPS_TEXTURE) at 0x68.
            write_u32s(ctx, desc, &[size, 0x1001]);
            write_u32s(ctx, desc + 0x48, &[32, flags, 0, bits]);
            write_u32s(ctx, desc + 0x58, &masks);
            ctx.memory.write::<u32>(desc + 0x68, 0x1000);
            let callback = ctx.indirect(lpd3dEnumTextureProc);
            ctx.call32_x86(callback, vec![desc, lpArg]);
            if ctx.cpu.regs.eax == 0 {
                break; // D3DENUMRET_CANCEL
            }
        }
        kernel32::lock().process_heap.free(&mut ctx.memory, desc);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn CreateExecuteBuffer(
        ctx: &mut Context,
        _this: u32,
        lpDesc: u32,
        lplpDirect3DExecuteBuffer: u32,
        _pUnkOuter: u32,
    ) -> DD {
        // D3DEXECUTEBUFFERDESC: dwSize, dwFlags, dwCaps, dwBufferSize, lpData.
        let flags = ctx.memory.read::<u32>(lpDesc + 4);
        let size = ctx.memory.read::<u32>(lpDesc + 12);
        let data = if flags & 4 != 0 {
            note("execute buffer with app-provided memory".into());
            Some(ctx.memory.read::<u32>(lpDesc + 16))
        } else {
            None
        };
        note(format!("execute buffer flags {flags:#x}"));
        let addr = new_object(ctx, IDirect3DExecuteBuffer::new);
        state().d3d.borrow_mut().execute_buffers.insert(
            addr,
            ExecuteBuffer {
                size,
                data,
                execute_data: [0; 5],
            },
        );
        ctx.memory.write::<u32>(lplpDirect3DExecuteBuffer, addr);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetStats(_ctx: &mut Context, _this: u32, _lpD3DStats: u32) -> DD {
        stub!(DD::OK)
    }

    #[win32_derive::dllexport]
    pub fn Execute(
        ctx: &mut Context,
        _this: u32,
        lpDirect3DExecuteBuffer: u32,
        _lpDirect3DViewport: u32,
        dwFlags: u32,
    ) -> DD {
        note(format!("Execute flags {dwFlags:#x}"));
        let (data, exec) = {
            let d3d = state().d3d.borrow();
            let buffer = &d3d.execute_buffers[&lpDirect3DExecuteBuffer];
            (buffer.data, buffer.execute_data)
        };
        if let Some(data) = data {
            probe_execute(ctx, data, &exec);
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn AddViewport(_ctx: &mut Context, _this: u32, _lpDirect3DViewport: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DeleteViewport(_ctx: &mut Context, _this: u32, _lpDirect3DViewport: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn NextViewport(
        _ctx: &mut Context,
        _this: u32,
        _lpDirect3DViewport: u32,
        _lplpAnotherViewport: u32,
        _dwFlags: u32,
    ) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    #[win32_derive::dllexport]
    pub fn Pick(
        _ctx: &mut Context,
        _this: u32,
        _lpDirect3DExecuteBuffer: u32,
        _lpDirect3DViewport: u32,
        _dwFlags: u32,
        _lpRect: u32,
    ) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    #[win32_derive::dllexport]
    pub fn GetPickRecords(_ctx: &mut Context, _this: u32, _lpCount: u32, _lpD3DPickRec: u32) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    #[win32_derive::dllexport]
    pub fn CreateMatrix(ctx: &mut Context, _this: u32, lpD3DMatHandle: u32) -> DD {
        note("CreateMatrix".into());
        let handle = {
            let mut d3d = state().d3d.borrow_mut();
            d3d.next_matrix += 1;
            d3d.next_matrix
        };
        ctx.memory.write::<u32>(lpD3DMatHandle, handle);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetMatrix(_ctx: &mut Context, _this: u32, _d3dMatHandle: u32, _lpD3DMatrix: u32) -> DD {
        note("SetMatrix".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetMatrix(_ctx: &mut Context, _this: u32, _D3DMatHandle: u32, _lpD3DMatrix: u32) -> DD {
        stub!(DD::ERR_GENERIC)
    }

    #[win32_derive::dllexport]
    pub fn DeleteMatrix(_ctx: &mut Context, _this: u32, _d3dMatHandle: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn BeginScene(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn EndScene(_ctx: &mut Context, _this: u32) -> DD {
        let mut d3d = state().d3d.borrow_mut();
        let probe = &mut d3d.probe;
        probe.scenes += 1;
        if probe.scenes % 300 == 0 {
            log::info!(
                "d3d: {} scenes: {} executes, {} triangles, {} vertices",
                probe.scenes,
                probe.executes,
                probe.triangles,
                probe.vertices
            );
        }
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetDirect3D(_ctx: &mut Context, _this: u32, _lplpD3D: u32) -> DD {
        todo!()
    }

    object_new!();
}

pub mod IDirect3DExecuteBuffer {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 10] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "Lock",
        "Unlock",
        "SetExecuteData",
        "GetExecuteData",
        "Validate",
        "Optimize",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DExecuteBuffer", this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(ctx: &mut Context, this: u32) -> u32 {
        let buffer = state().d3d.borrow_mut().execute_buffers.remove(&this);
        if let Some(ExecuteBuffer { data: Some(data), .. }) = buffer {
            kernel32::lock().process_heap.free(&mut ctx.memory, data);
        }
        0
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpDirect3DDevice: u32, _lpDesc: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Lock(ctx: &mut Context, this: u32, lpDesc: u32) -> DD {
        let (size, data) = {
            let d3d = state().d3d.borrow();
            let buffer = &d3d.execute_buffers[&this];
            (buffer.size, buffer.data)
        };
        let data = match data {
            Some(data) => data,
            None => {
                let data = alloc(ctx, size);
                state().d3d.borrow_mut().execute_buffers.get_mut(&this).unwrap().data = Some(data);
                data
            }
        };
        // D3DEXECUTEBUFFERDESC: dwFlags = D3DDEB_BUFSIZE | D3DDEB_LPDATA.
        write_u32s(ctx, lpDesc + 4, &[1 | 4, 0, size, data]);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unlock(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetExecuteData(ctx: &mut Context, this: u32, lpData: u32) -> DD {
        let mut exec = [0; 5];
        for (i, value) in exec.iter_mut().enumerate() {
            *value = ctx.memory.read::<u32>(lpData + 4 + i as u32 * 4);
        }
        state().d3d.borrow_mut().execute_buffers.get_mut(&this).unwrap().execute_data = exec;
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetExecuteData(_ctx: &mut Context, _this: u32, _lpData: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Validate(
        _ctx: &mut Context,
        _this: u32,
        _lpdwOffset: u32,
        _lpFunc: u32,
        _lpUserArg: u32,
        _dwReserved: u32,
    ) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Optimize(_ctx: &mut Context, _this: u32, _dwDummy: u32) -> DD {
        DD::OK
    }

    object_new!();
}

pub mod IDirect3DViewport {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 16] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "GetViewport",
        "SetViewport",
        "TransformVertices",
        "LightElements",
        "SetBackground",
        "GetBackground",
        "SetBackgroundDepth",
        "GetBackgroundDepth",
        "Clear",
        "AddLight",
        "DeleteLight",
        "NextLight",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DViewport", this, riid, ppvObject)
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
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpDirect3D: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetViewport(_ctx: &mut Context, _this: u32, _lpData: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetViewport(ctx: &mut Context, _this: u32, lpData: u32) -> DD {
        // D3DVIEWPORT: dwSize, dwX, dwY, dwWidth, dwHeight, then floats dvScaleX,
        // dvScaleY, dvMaxX, dvMaxY, dvMinZ, dvMaxZ.
        let d = |i: u32| ctx.memory.read::<u32>(lpData + i * 4);
        let f = |i: u32| ctx.memory.read::<f32>(lpData + i * 4);
        note(format!(
            "viewport {}x{} at {},{} scale {} {} max {} {} z {}..{}",
            d(3),
            d(4),
            d(1),
            d(2),
            f(5),
            f(6),
            f(7),
            f(8),
            f(9),
            f(10)
        ));
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn TransformVertices(
        _ctx: &mut Context,
        _this: u32,
        _dwVertexCount: u32,
        _lpData: u32,
        _dwFlags: u32,
        _lpOffscreen: u32,
    ) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn LightElements(_ctx: &mut Context, _this: u32, _dwElementCount: u32, _lpData: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetBackground(_ctx: &mut Context, _this: u32, _hMat: u32) -> DD {
        note("SetBackground".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetBackground(_ctx: &mut Context, _this: u32, _lphMat: u32, _lpValid: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn SetBackgroundDepth(_ctx: &mut Context, _this: u32, _lpDDSurface: u32) -> DD {
        note("SetBackgroundDepth".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetBackgroundDepth(_ctx: &mut Context, _this: u32, _lplpDDSurface: u32, _lpValid: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn Clear(_ctx: &mut Context, _this: u32, dwCount: u32, _lpRects: u32, dwFlags: u32) -> DD {
        note(format!("viewport Clear flags {dwFlags:#x} ({dwCount} rects)"));
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn AddLight(_ctx: &mut Context, _this: u32, _lpDirect3DLight: u32) -> DD {
        note("AddLight".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn DeleteLight(_ctx: &mut Context, _this: u32, _lpDirect3DLight: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn NextLight(
        _ctx: &mut Context,
        _this: u32,
        _lpDirect3DLight: u32,
        _lplpDirect3DLight: u32,
        _dwFlags: u32,
    ) -> DD {
        todo!()
    }

    object_new!();
}

pub mod IDirect3DMaterial {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 9] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "SetMaterial",
        "GetMaterial",
        "GetHandle",
        "Reserve",
        "Unreserve",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DMaterial", this, riid, ppvObject)
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
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpDirect3D: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetMaterial(_ctx: &mut Context, _this: u32, _lpMat: u32) -> DD {
        note("SetMaterial".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetMaterial(_ctx: &mut Context, _this: u32, _lpMat: u32) -> DD {
        todo!()
    }

    #[win32_derive::dllexport]
    pub fn GetHandle(ctx: &mut Context, this: u32, _lpDirect3DDevice: u32, lpHandle: u32) -> DD {
        ctx.memory.write::<u32>(lpHandle, this);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Reserve(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unreserve(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    object_new!();
}

pub mod IDirect3DLight {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 6] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "SetLight",
        "GetLight",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DLight", this, riid, ppvObject)
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
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpDirect3D: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn SetLight(_ctx: &mut Context, _this: u32, _lpLight: u32) -> DD {
        note("SetLight".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetLight(_ctx: &mut Context, _this: u32, _lpLight: u32) -> DD {
        todo!()
    }

    object_new!();
}

pub mod IDirect3DTexture {
    use super::*;

    pub const VTABLE_ENTRIES: [&str; 8] = [
        "QueryInterface",
        "AddRef",
        "Release",
        "Initialize",
        "GetHandle",
        "PaletteChanged",
        "Load",
        "Unload",
    ];

    #[win32_derive::dllexport]
    pub fn QueryInterface(ctx: &mut Context, this: u32, riid: u32, ppvObject: u32) -> DD {
        query_self(ctx, "IDirect3DTexture", this, riid, ppvObject)
    }

    #[win32_derive::dllexport]
    pub fn AddRef(_ctx: &mut Context, _this: u32) -> u32 {
        1
    }

    #[win32_derive::dllexport]
    pub fn Release(_ctx: &mut Context, this: u32) -> u32 {
        state().d3d.borrow_mut().textures.remove(&this);
        0
    }

    #[win32_derive::dllexport]
    pub fn Initialize(_ctx: &mut Context, _this: u32, _lpD3DDevice: u32, _lpDDSurface: u32) -> DD {
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn GetHandle(ctx: &mut Context, this: u32, _lpDirect3DDevice: u32, lpHandle: u32) -> DD {
        // The texture's own pointer serves as its handle.
        ctx.memory.write::<u32>(lpHandle, this);
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn PaletteChanged(_ctx: &mut Context, _this: u32, _dwStart: u32, _dwCount: u32) -> DD {
        note("texture PaletteChanged".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Load(_ctx: &mut Context, _this: u32, _lpD3DTexture: u32) -> DD {
        note("texture Load".into());
        DD::OK
    }

    #[win32_derive::dllexport]
    pub fn Unload(_ctx: &mut Context, _this: u32) -> DD {
        DD::OK
    }

    object_new!();
}

/// IDirectDrawSurface::QueryInterface for the Direct3D interfaces a surface
/// can hand out: a device (rendering into the surface) or a texture.
pub fn surface_query_interface(ctx: &mut Context, surface: u32, iid: &GUID, ppv: u32) -> Option<DD> {
    let addr = if *iid == IID_IDirect3DHALDevice {
        let (width, height, bpp) = {
            let surfaces = state().surf.borrow();
            let s = surfaces.get(&surface)?.borrow();
            (s.width, s.height, s.bytes_per_pixel * 8)
        };
        note(format!("device on a {width}x{height}x{bpp} surface"));
        new_object(ctx, IDirect3DDevice::new)
    } else if *iid == IID_IDirect3DTexture {
        let addr = new_object(ctx, IDirect3DTexture::new);
        state().d3d.borrow_mut().textures.insert(addr, surface);
        addr
    } else {
        return None;
    };
    ctx.memory.write::<u32>(ppv, addr);
    Some(DD::OK)
}

/// IDirectDraw::QueryInterface(IID_IDirect3D).
pub fn ddraw_query_interface(ctx: &mut Context, ppv: u32) -> DD {
    let addr = new_object(ctx, IDirect3D::new);
    ctx.memory.write::<u32>(ppv, addr);
    DD::OK
}
