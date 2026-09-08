//! Critical sections and interlocked operations.
//!
//! Theseus runs the program's threads one at a time (see `host::SingleThreader`),
//! so a critical section can never be contended and the operations below don't
//! need to touch the CRITICAL_SECTION the program hands us.

use runtime::Context;

use crate::Ptr;

#[win32_derive::dllexport]
pub fn InitializeCriticalSection(_ctx: &mut Context, _lpCriticalSection: Ptr<()>) {}

#[win32_derive::dllexport]
pub fn EnterCriticalSection(_ctx: &mut Context, _lpCriticalSection: Ptr<()>) {}

#[win32_derive::dllexport]
pub fn LeaveCriticalSection(_ctx: &mut Context, _lpCriticalSection: Ptr<()>) {}

#[win32_derive::dllexport]
pub fn DeleteCriticalSection(_ctx: &mut Context, _lpCriticalSection: Ptr<()>) {}

#[win32_derive::dllexport]
pub fn InterlockedIncrement(ctx: &mut Context, Addend: Ptr<i32>) -> i32 {
    let val = Addend.read(&ctx.memory).unwrap().wrapping_add(1);
    Addend.write(&mut ctx.memory, val);
    val
}

#[win32_derive::dllexport]
pub fn InterlockedDecrement(ctx: &mut Context, Addend: Ptr<i32>) -> i32 {
    let val = Addend.read(&ctx.memory).unwrap().wrapping_sub(1);
    Addend.write(&mut ctx.memory, val);
    val
}
