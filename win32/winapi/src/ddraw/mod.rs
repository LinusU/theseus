use std::{
    cell::{RefCell, RefMut},
    collections::HashMap,
    rc::Rc,
    sync::OnceLock,
};

use runtime::{ContFn, Context};

pub mod d3d7;
mod ddraw;
mod ddraw1;
mod ddraw7;
pub mod types;

pub use ddraw::*;
pub use ddraw1::*;
pub use ddraw7::*;
pub use types::DD;

use crate::{heap::Heap, kernel32};

/// # Safety
/// Must be called once before any DirectDraw interface methods are invoked.
pub unsafe fn init_vtables(ctx: &mut Context) {
    if unsafe { IDirectDraw::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let Some((addr, blocks)) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0000,
            &IDirectDraw::VTABLE_FUNCS,
        ) else {
            return;
        };
        unsafe { IDirectDraw::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDraw vtable allocated at {addr:#x}");
    }
    if unsafe { IDirectDrawSurface::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let Some((addr, blocks)) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0100,
            &IDirectDrawSurface::VTABLE_FUNCS,
        ) else {
            return;
        };
        unsafe { IDirectDrawSurface::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDrawSurface vtable allocated at {addr:#x}");
    }
    if unsafe { IDirectDrawPalette::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let Some((addr, blocks)) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0200,
            &IDirectDrawPalette::VTABLE_FUNCS,
        ) else {
            return;
        };
        unsafe { IDirectDrawPalette::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDrawPalette vtable allocated at {addr:#x}");
    }
    if unsafe { IDirectDraw7::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let Some((addr, blocks)) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0300,
            &IDirectDraw7::VTABLE_FUNCS,
        ) else {
            return;
        };
        unsafe { IDirectDraw7::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDraw7 vtable allocated at {addr:#x}");
    }
    if unsafe { IDirectDrawSurface7::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let Some((addr, blocks)) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0400,
            &IDirectDrawSurface7::VTABLE_FUNCS,
        ) else {
            return;
        };
        unsafe { IDirectDrawSurface7::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDrawSurface7 vtable allocated at {addr:#x}");
    }
    if unsafe { d3d7::IDirect3D7::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let Some((addr, blocks)) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0500,
            &d3d7::IDirect3D7::VTABLE_FUNCS,
        ) else {
            return;
        };
        unsafe { d3d7::IDirect3D7::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirect3D7 vtable allocated at {addr:#x}");
    }
    if unsafe { d3d7::IDirect3DDevice7::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let Some((addr, blocks)) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0600,
            &d3d7::IDirect3DDevice7::VTABLE_FUNCS,
        ) else {
            return;
        };
        unsafe { d3d7::IDirect3DDevice7::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirect3DDevice7 vtable allocated at {addr:#x}");
    }
    if unsafe { d3d7::IDirect3DVertexBuffer7::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let Some((addr, blocks)) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0700,
            &d3d7::IDirect3DVertexBuffer7::VTABLE_FUNCS,
        ) else {
            return;
        };
        unsafe { d3d7::IDirect3DVertexBuffer7::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirect3DVertexBuffer7 vtable allocated at {addr:#x}");
    }
}

fn init_vtable(
    ctx: &mut Context,
    heap: &mut Heap,
    base: u32,
    funcs: &[ContFn],
) -> Option<(u32, Vec<(u32, ContFn)>)> {
    let size = (funcs.len() * 4) as u32;
    let addr = heap.try_alloc(&mut ctx.memory, size)?;
    let mut blocks = Vec::with_capacity(funcs.len());
    for (i, &func) in funcs.iter().enumerate() {
        let fn_addr = base + i as u32;
        ctx.memory.write::<u32>(addr + i as u32 * 4, fn_addr);
        blocks.push((fn_addr, func));
    }
    Some((addr, blocks))
}

fn add_blocks(ctx: &mut Context, mut blocks: Vec<(u32, ContFn)>) {
    if blocks.is_empty() {
        return;
    }
    blocks.extend_from_slice(ctx.blocks);
    blocks.sort_by_key(|(addr, _)| *addr);
    ctx.blocks = Box::leak(blocks.into_boxed_slice());
}

pub const VTABLES: [(&str, &[&str]); 8] = [
    ("IDirectDraw", IDirectDraw::VTABLE_ENTRIES.as_slice()),
    (
        "IDirectDrawSurface",
        IDirectDrawSurface::VTABLE_ENTRIES.as_slice(),
    ),
    ("IDirectDraw7", IDirectDraw7::VTABLE_ENTRIES.as_slice()),
    (
        "IDirectDrawSurface7",
        IDirectDrawSurface7::VTABLE_ENTRIES.as_slice(),
    ),
    (
        "IDirectDrawPalette",
        IDirectDrawPalette::VTABLE_ENTRIES.as_slice(),
    ),
    ("IDirect3D7", d3d7::IDirect3D7::VTABLE_ENTRIES.as_slice()),
    (
        "IDirect3DDevice7",
        d3d7::IDirect3DDevice7::VTABLE_ENTRIES.as_slice(),
    ),
    (
        "IDirect3DVertexBuffer7",
        d3d7::IDirect3DVertexBuffer7::VTABLE_ENTRIES.as_slice(),
    ),
];

#[repr(C)]
#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    zerocopy::FromBytes,
    zerocopy::IntoBytes,
    zerocopy::Immutable,
    zerocopy::KnownLayout,
)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl GUID {
    pub const fn new(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> Self {
        Self {
            data1,
            data2,
            data3,
            data4,
        }
    }
}

impl std::fmt::Debug for GUID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-",
            self.data1,
            self.data2,
            self.data3,
            u16::from_le_bytes(self.data4[..2].try_into().unwrap())
        )?;
        for b in &self.data4[2..] {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct State {
    pub ddraw: RefCell<Option<DirectDraw>>,
    pub surf: RefCell<HashMap<u32, Rc<RefCell<Surface>>>>,
    pub palette: RefCell<HashMap<u32, Rc<RefCell<Palette>>>>,
    /// Surface GetDC on a non-32bpp surface hands out a scratch RGBA buffer
    /// that ReleaseDC converts back: DC handle -> scratch address.
    pub surface_dcs: RefCell<HashMap<u32, u32>>,
}

impl State {
    /// The `DirectDraw` object when `ptr` names it: `None` when no object
    /// was created or the COM `this` pointer isn't ours.
    pub fn get_ddraw(&self, ptr: u32) -> Option<RefMut<'_, DirectDraw>> {
        RefMut::filter_map(self.ddraw.borrow_mut(), |ddraw| {
            ddraw
                .as_mut()
                .filter(|ddraw| ddraw.addr == ptr || ddraw.aliases.contains(&ptr))
        })
        .ok()
    }
}

// TODO: reuse locking pattern from kernel32
// OnceLock rather than cell::OnceCell: get_or_init runs on host threads too
// (winmm/dsound callbacks), and cell::OnceCell::get_or_init panics with
// "reentrant init" when two threads race the initializer.
struct StaticState(OnceLock<State>);
unsafe impl Sync for StaticState {}

static STATE: StaticState = StaticState(OnceLock::new());

pub fn state() -> &'static State {
    STATE.0.get_or_init(State::default)
}
