use std::{
    cell::{OnceCell, RefCell, RefMut},
    collections::HashMap,
    rc::Rc,
};

use runtime::{ContFn, Context};

mod ddraw;
mod ddraw1;
mod ddraw7;
pub mod types;

pub use ddraw::*;
pub use ddraw1::*;
pub use ddraw7::*;
pub use types::DD;

use crate::{heap::Heap, kernel32};

pub unsafe fn init_vtables(ctx: &mut Context) {
    if unsafe { IDirectDraw::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let (addr, blocks) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0000,
            &IDirectDraw::VTABLE_FUNCS,
        );
        unsafe { IDirectDraw::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDraw vtable allocated at {addr:#x}");
    }
    if unsafe { IDirectDrawSurface::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let (addr, blocks) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0100,
            &IDirectDrawSurface::VTABLE_FUNCS,
        );
        unsafe { IDirectDrawSurface::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDrawSurface vtable allocated at {addr:#x}");
    }
    if unsafe { IDirectDrawPalette::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let (addr, blocks) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0200,
            &IDirectDrawPalette::VTABLE_FUNCS,
        );
        unsafe { IDirectDrawPalette::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDrawPalette vtable allocated at {addr:#x}");
    }
    if unsafe { IDirectDraw7::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let (addr, blocks) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0300,
            &IDirectDraw7::VTABLE_FUNCS,
        );
        unsafe { IDirectDraw7::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDraw7 vtable allocated at {addr:#x}");
    }
    if unsafe { IDirectDrawSurface7::VTABLE } == 0 {
        let mut kernel32 = kernel32::lock();
        let (addr, blocks) = init_vtable(
            ctx,
            &mut kernel32.process_heap,
            0xfafe0400,
            &IDirectDrawSurface7::VTABLE_FUNCS,
        );
        unsafe { IDirectDrawSurface7::VTABLE = addr };
        drop(kernel32);
        add_blocks(ctx, blocks);
        log::debug!("IDirectDrawSurface7 vtable allocated at {addr:#x}");
    }
}

fn init_vtable(
    ctx: &mut Context,
    heap: &mut Heap,
    base: u32,
    funcs: &[ContFn],
) -> (u32, Vec<(u32, ContFn)>) {
    let size = (funcs.len() * 4) as u32;
    let addr = heap.alloc(&mut ctx.memory, size);
    let mut blocks = Vec::with_capacity(funcs.len());
    for (i, &func) in funcs.iter().enumerate() {
        let fn_addr = base + i as u32;
        ctx.memory.write::<u32>(addr + i as u32 * 4, fn_addr);
        blocks.push((fn_addr, func));
    }
    (addr, blocks)
}

fn add_blocks(ctx: &mut Context, mut blocks: Vec<(u32, ContFn)>) {
    if blocks.is_empty() {
        return;
    }
    blocks.extend_from_slice(ctx.blocks);
    blocks.sort_by_key(|(addr, _)| *addr);
    ctx.blocks = Box::leak(blocks.into_boxed_slice());
}

pub const VTABLES: [(&'static str, &[&str]); 5] = [
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
];

#[repr(C)]
#[derive(Clone, Copy, PartialEq, zerocopy::FromBytes)]
pub struct GUID(pub (u32, u16, u16, [u8; 8]));

impl std::fmt::Debug for GUID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:04x}-",
            self.0.0,
            self.0.1,
            self.0.2,
            u16::from_le_bytes(self.0.3[..2].try_into().unwrap())
        )?;
        for b in &self.0.3[2..] {
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
}

impl State {
    pub fn get_ddraw(&self, ptr: u32) -> RefMut<'_, DirectDraw> {
        let ddraw = RefMut::map(self.ddraw.borrow_mut(), |ddraw| ddraw.as_mut().unwrap());
        assert!(ptr == ddraw.addr);
        ddraw
    }
}

// TODO: reuse locking pattern from kernel32
struct StaticState(OnceCell<State>);
unsafe impl Sync for StaticState {}

static STATE: StaticState = StaticState(OnceCell::new());

pub fn state() -> &'static State {
    STATE.0.get_or_init(|| Default::default())
}
