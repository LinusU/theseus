//! Windows hooks (SetWindowsHookEx).
//!
//! MFC installs a WH_CBT hook and relies on its HCBT_CREATEWND notification to
//! attach its CWnd objects to the windows being created, then subclasses them
//! from inside the hook. Without that notification no MFC message routing
//! happens at all, so CreateWindowEx calls into the hooks here.

use runtime::Context;

use crate::{
    kernel32,
    user32::{HWND, state},
};

pub struct Hook {
    /// WH_* hook type.
    pub id: i32,
    /// x86 address of the HOOKPROC.
    pub proc_addr: u32,
}

pub const WH_CBT: i32 = 5;
const HCBT_CREATEWND: u32 = 3;

/// CREATESTRUCTA, as passed to the CBT hook and in WM_(NC)CREATE.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct CREATESTRUCTA {
    pub lpCreateParams: u32,
    pub hInstance: u32,
    pub hMenu: u32,
    pub hwndParent: u32,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub style: u32,
    pub lpszName: u32,
    pub lpszClass: u32,
    pub dwExStyle: u32,
}

#[repr(C)]
#[derive(zerocopy::IntoBytes, zerocopy::Immutable)]
struct CBT_CREATEWNDA {
    lpcs: u32,
    hwndInsertAfter: u32,
}

/// Deliver HCBT_CREATEWND for a newly created window to every CBT hook.
pub fn cbt_create_wnd(ctx: &mut Context, hwnd: HWND, cs: &CREATESTRUCTA) {
    let hooks: Vec<u32> = state()
        .hooks
        .borrow()
        .iter()
        .filter(|hook| hook.id == WH_CBT)
        .map(|hook| hook.proc_addr)
        .collect();
    if hooks.is_empty() {
        return;
    }

    let cs_size = std::mem::size_of::<CREATESTRUCTA>() as u32;
    let cbt_size = std::mem::size_of::<CBT_CREATEWNDA>() as u32;
    let buf = kernel32::lock()
        .process_heap
        .alloc(&mut ctx.memory, cs_size + cbt_size);
    ctx.memory.write(buf, *cs);
    ctx.memory.write(
        buf + cs_size,
        CBT_CREATEWNDA {
            lpcs: buf,
            hwndInsertAfter: 0,
        },
    );
    for proc_addr in hooks {
        let hook = ctx.indirect(proc_addr);
        // HOOKPROC(nCode, wParam, lParam); nonzero return would cancel creation,
        // which we don't support.
        ctx.call32_x86(hook, vec![HCBT_CREATEWND, hwnd.to_raw(), buf + cs_size]);
    }
    kernel32::lock().process_heap.free(&mut ctx.memory, buf);
}

pub type HHOOK = u32;

#[win32_derive::dllexport]
pub fn SetWindowsHookExA(
    _ctx: &mut Context,
    idHook: i32,
    lpfn: u32,
    _hmod: u32,
    _dwThreadId: u32,
) -> HHOOK {
    if idHook != WH_CBT {
        log::warn!("SetWindowsHookExA({idHook}): hook type never fires");
    }
    let mut hooks = state().hooks.borrow_mut();
    hooks.push(Hook {
        id: idHook,
        proc_addr: lpfn,
    });
    hooks.len() as HHOOK
}

#[win32_derive::dllexport]
pub fn UnhookWindowsHookEx(_ctx: &mut Context, hhk: HHOOK) -> bool {
    // Hook handles are indices; disable rather than remove so others stay valid.
    let mut hooks = state().hooks.borrow_mut();
    match hooks.get_mut(hhk.wrapping_sub(1) as usize) {
        Some(hook) => {
            hook.id = -1;
            true
        }
        None => false,
    }
}

#[win32_derive::dllexport]
pub fn CallNextHookEx(
    _ctx: &mut Context,
    _hhk: HHOOK,
    _nCode: i32,
    _wParam: u32,
    _lParam: u32,
) -> u32 {
    0
}
