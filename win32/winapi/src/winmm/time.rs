use runtime::Context;

use crate::{
    kernel32, stub,
    winmm::{state, winmm_main},
};

pub struct Timer {
    pub period: u32,
    pub next: u32,
    pub callback: u32,
    pub user_data: u32,
}

#[win32_derive::dllexport]
pub fn timeGetTime(_ctx: &mut Context) -> u32 {
    host::host().time()
}

#[derive(Debug)]
pub struct TIME {
    periodic: bool,
    #[allow(unused)]
    event: (), // todo
}

impl crate::dllexport::FromABIParam for TIME {
    fn from_abi(val: u32) -> Self {
        // kind of a bitfield, kind of an enum
        let periodic = (val & 0xF) != 0;
        assert_eq!(periodic, true);
        let event = match val & 0xF0 {
            0x00 => (),      // FUNCTION
            0x10 => todo!(), // EVENT_SET
            0x20 => todo!(), // EVENT_PULSE
            _ => unimplemented!(),
        };
        TIME { periodic, event }
    }
}

#[win32_derive::dllexport]
pub fn timeSetEvent(
    ctx: &mut Context,
    uDelay: u32,
    _uResolution: u32,
    lpTimeProc: u32,
    dwUser: u32,
    fuEvent: TIME,
) -> u32 {
    assert_eq!(fuEvent.periodic, true);

    let mut state = state();
    assert!(state.timer.is_none());
    state.timer = Some(Timer {
        period: uDelay,
        next: host::host().time() + uDelay,
        callback: lpTimeProc,
        user_data: dwUser,
    });
    kernel32::lock().create_thread(ctx, "winmm".into(), |ctx| {
        winmm_main(ctx);
    });

    stub!(1)
}

#[win32_derive::dllexport]
pub fn timeKillEvent(_ctx: &mut Context, uTimerID: u32) -> u32 {
    const TIMERR_NOERROR: u32 = 0;
    const MMSYSERR_INVALHANDLE: u32 = 5;

    // The emulated timer model supports a single periodic event with id 1.
    // Clearing it makes the winmm thread exit its loop at the next wake.
    let mut state = state();
    if uTimerID == 1 && state.timer.take().is_some() {
        TIMERR_NOERROR
    } else {
        MMSYSERR_INVALHANDLE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime::{BlockCache, CPU, Context, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    #[test]
    fn time_kill_event_clears_only_the_registered_timer() {
        let mut ctx = context();
        assert_eq!(timeKillEvent(&mut ctx, 1), 5); // none registered

        state().timer = Some(Timer {
            period: 10,
            next: 0,
            callback: 0,
            user_data: 0,
        });
        assert_eq!(timeKillEvent(&mut ctx, 2), 5); // wrong id
        assert_eq!(timeKillEvent(&mut ctx, 1), 0);
        assert!(state().timer.is_none());
    }
}
