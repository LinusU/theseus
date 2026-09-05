use runtime::Context;

use crate::stub;

// Exception filter return values used in `__except` expressions.
const EXCEPTION_CONTINUE_SEARCH: i32 = 0;

#[win32_derive::dllexport]
pub fn _XcptFilter(_ctx: &mut Context, _xcptnum: u32, _pxcptinfoptrs: u32) -> i32 {
    // No per-signal disposition is modeled; continue searching.
    EXCEPTION_CONTINUE_SEARCH
}

#[win32_derive::dllexport]
pub fn __getmainargs(_ctx: &mut Context) -> i32 {
    0
}

#[win32_derive::dllexport]
pub fn __p__commode(_ctx: &mut Context) -> u32 {
    stub!(0)
}

#[win32_derive::dllexport]
pub fn __p__fmode(_ctx: &mut Context) -> u32 {
    stub!(0)
}

#[win32_derive::dllexport]
pub fn __set_app_type(_ctx: &mut Context, _at: i32) {}

#[win32_derive::dllexport]
pub fn __setusermatherr(_ctx: &mut Context, _pf: u32) {
    // No user math-error handler is modeled; the default remains in effect.
}

// data:
// _acmdln
// _adjust_fdiv

#[win32_derive::dllexport]
pub fn _controlfp(_ctx: &mut Context) -> u32 {
    stub!(0)
}

// Exception disposition returned by language-specific SEH handlers.
const EXCEPTION_CONTINUE_SEARCH_DISPOSITION: i32 = 1;

#[win32_derive::dllexport]
pub fn _except_handler3(
    _ctx: &mut Context,
    _exception_record: u32,
    _registration_frame: u32,
    _context: u32,
    _dispatcher: u32,
) -> i32 {
    // No SEH scope table is modeled; continue searching for a handler.
    EXCEPTION_CONTINUE_SEARCH_DISPOSITION
}

#[win32_derive::dllexport]
pub fn _exit(_ctx: &mut Context, status: i32) {
    std::process::exit(status);
}

#[win32_derive::dllexport]
pub fn _initterm(_ctx: &mut Context) {}

#[win32_derive::dllexport]
pub fn exit(_ctx: &mut Context, status: i32) {
    // No CRT atexit-handler model exists; exit forwards to the host directly.
    std::process::exit(status);
}

// MSDN: "Calling rand before any call to srand generates the same sequence as calling srand with seed passed as 1."
static mut RAND_STATE: u32 = 1;

#[win32_derive::dllexport]
pub fn rand(_ctx: &mut Context) -> u32 {
    const RAND_MAX: u32 = 0xFFFF;
    unsafe {
        RAND_STATE = ((RAND_STATE.wrapping_mul(134775813)).wrapping_add(1)) % (1 << 31);
        RAND_STATE % RAND_MAX
    }
}

#[win32_derive::dllexport]
pub fn srand(_ctx: &mut Context) {}
