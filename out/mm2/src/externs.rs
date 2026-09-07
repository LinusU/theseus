#![allow(dead_code)]
use runtime::*;

/// SafeDisc `callf 0:0x8000` far call. The original target is the start of
/// the (encrypted) SafeDisc loader; it is dead in the cracked executable, so
/// treat it as a no-op far call that returns to the caller.
pub fn safedisc_8000(ctx: &mut Context) -> Cont {
    ctx.retf32(0)
}
