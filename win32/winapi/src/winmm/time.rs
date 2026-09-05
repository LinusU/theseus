use runtime::Context;

use crate::{
    kernel32,
    winmm::{state, winmm_main},
};

/// How a timer expiry is delivered: TIME_CALLBACK_FUNCTION calls a guest
/// function, while TIME_CALLBACK_EVENT_SET/PULSE signal an event object
/// (`lpTimeProc` is then an event handle rather than a function).
#[derive(Debug, Copy, Clone)]
pub enum Notify {
    Function(u32),
    Event { handle: u32, pulse: bool },
}

pub struct Timer {
    pub period: u32,
    pub next: u32,
    pub periodic: bool,
    pub notify: Notify,
    pub user_data: u32,
}

#[win32_derive::dllexport]
pub fn timeGetTime(_ctx: &mut Context) -> u32 {
    host::host().time()
}

#[derive(Debug)]
pub struct TIME {
    periodic: bool,
    /// Some(pulse) when the callback parameter is an event handle to
    /// signal (EVENT_SET/EVENT_PULSE) rather than a function.
    event_pulse: Option<bool>,
    /// False when `fuEvent` contains bits the emulated model does not handle.
    valid: bool,
}

impl crate::dllexport::FromABIParam for TIME {
    fn from_abi(val: u32) -> Self {
        // kind of a bitfield, kind of an enum
        let periodic = (val & 0xF) != 0;
        let (valid, event_pulse) = match val & 0xF0 {
            0x00 => (true, None),        // FUNCTION
            0x10 => (true, Some(false)), // EVENT_SET
            0x20 => (true, Some(true)),  // EVENT_PULSE
            _ => (false, None),
        };
        TIME {
            periodic,
            event_pulse,
            valid,
        }
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
    if !fuEvent.valid {
        return 0;
    }

    let notify = match fuEvent.event_pulse {
        None => Notify::Function(lpTimeProc),
        Some(pulse) => Notify::Event {
            handle: lpTimeProc,
            pulse,
        },
    };

    let mut state = state();
    assert!(state.timer.is_none());
    state.timer = Some(Timer {
        period: uDelay,
        next: host::host().time() + uDelay,
        periodic: fuEvent.periodic,
        notify,
        user_data: dwUser,
    });
    kernel32::lock().create_thread(ctx, "winmm".into(), |ctx| {
        winmm_main(ctx);
    });

    1
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
    use crate::dllexport::FromABIParam;
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
            periodic: true,
            notify: Notify::Function(0),
            user_data: 0,
        });
        assert_eq!(timeKillEvent(&mut ctx, 2), 5); // wrong id
        assert_eq!(timeKillEvent(&mut ctx, 1), 0);
        assert!(state().timer.is_none());
    }

    #[test]
    fn time_set_event_rejects_unknown_callback_kinds() {
        assert!(!TIME::from_abi(0x30).valid);
        assert!(TIME::from_abi(0x11).valid); // periodic | EVENT_SET
        assert!(TIME::from_abi(0x21).valid); // periodic | EVENT_PULSE
    }
}
