use runtime::Context;

use crate::{
    kernel32,
    winmm::{TIMER_COND, state, winmm_main},
};

/// How a timer expiry is delivered: TIME_CALLBACK_FUNCTION calls a guest
/// function, while TIME_CALLBACK_EVENT_SET/PULSE signal an event object
/// (`lpTimeProc` is then an event handle rather than a function).
#[derive(Debug, Copy, Clone)]
pub enum Notify {
    Function(u32),
    Event { handle: u32, pulse: bool },
}

#[derive(Clone)]
pub struct Timer {
    pub id: u32,
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
    if !fuEvent.valid || lpTimeProc < 0x1000 {
        return 0;
    }

    let notify = match fuEvent.event_pulse {
        None => Notify::Function(lpTimeProc),
        Some(pulse) => Notify::Event {
            handle: lpTimeProc,
            pulse,
        },
    };

    let (id, need_spawn) = {
        let mut lock = state();
        let id = lock.next_id;
        lock.next_id = lock.next_id.wrapping_add(1);
        if lock.next_id == 0 {
            lock.next_id = 1;
        }
        lock.timers.insert(
            id,
            Timer {
                id,
                period: uDelay,
                // Saturate rather than wrap: the worker compares
                // `next <= now` directly, so a wrapped `next` would fire
                // immediately instead of far in the future.
                next: host::host().time().saturating_add(uDelay),
                periodic: fuEvent.periodic,
                notify,
                user_data: dwUser,
            },
        );
        let need_spawn = !lock.thread_running;
        if need_spawn {
            lock.thread_running = true;
        }
        // Wake any sleeping worker so it recomputes the next due time.
        TIMER_COND.notify_one();
        (id, need_spawn)
    };

    if need_spawn
        && kernel32::lock()
            .create_thread(ctx, "winmm".into(), |ctx| {
                winmm_main(ctx);
            })
            .is_none()
    {
        let mut lock = state();
        lock.timers.remove(&id);
        if lock.timers.is_empty() {
            lock.thread_running = false;
        }
        return 0;
    }

    id
}

#[win32_derive::dllexport]
pub fn timeKillEvent(_ctx: &mut Context, uTimerID: u32) -> u32 {
    const TIMERR_NOERROR: u32 = 0;
    const MMSYSERR_INVALHANDLE: u32 = 5;

    let removed = state().timers.remove(&uTimerID).is_some();
    if removed {
        TIMER_COND.notify_one();
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

    /// The timer slot is global, so tests that register one serialize.
    static TIMER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        let _guard = TIMER_LOCK.lock().unwrap();
        let mut ctx = context();
        assert_eq!(timeKillEvent(&mut ctx, 1), 5); // none registered

        state().timers.insert(
            1,
            Timer {
                id: 1,
                period: 10,
                next: 0,
                periodic: true,
                notify: Notify::Function(0),
                user_data: 0,
            },
        );
        assert_eq!(timeKillEvent(&mut ctx, 2), 5); // wrong id
        assert_eq!(timeKillEvent(&mut ctx, 1), 0);
        assert!(state().timers.is_empty());
    }

    #[test]
    fn time_set_event_allows_multiple_timers_with_distinct_ids() {
        let _guard = TIMER_LOCK.lock().unwrap();
        let mut ctx = context();
        // Fake a running worker so the test does not actually spawn a thread.
        state().thread_running = true;

        let id1 = timeSetEvent(&mut ctx, 10, 0, 0x1234, 0, TIME::from_abi(0));
        assert_eq!(id1, 1);

        let id2 = timeSetEvent(&mut ctx, 20, 0, 0x5678, 0, TIME::from_abi(0));
        assert_eq!(id2, 2);
        assert_eq!(state().timers.len(), 2);

        assert_eq!(timeKillEvent(&mut ctx, id1), 0);
        assert_eq!(timeKillEvent(&mut ctx, id2), 0);
        assert!(state().timers.is_empty());

        state().thread_running = false;
    }

    #[test]
    fn time_set_event_rejects_unknown_callback_kinds() {
        assert!(!TIME::from_abi(0x30).valid);
        assert!(TIME::from_abi(0x11).valid); // periodic | EVENT_SET
        assert!(TIME::from_abi(0x21).valid); // periodic | EVENT_PULSE
    }

    #[test]
    fn time_set_event_rejects_null_and_low_pointers() {
        let _guard = TIMER_LOCK.lock().unwrap();
        let mut ctx = context();
        state().thread_running = true;

        assert_eq!(timeSetEvent(&mut ctx, 10, 0, 0, 0, TIME::from_abi(0)), 0);
        assert_eq!(
            timeSetEvent(&mut ctx, 10, 0, 0x500, 0, TIME::from_abi(0)),
            0
        );
        assert_eq!(
            timeSetEvent(&mut ctx, 10, 0, 0x500, 0, TIME::from_abi(0x11)),
            0
        );
        assert!(state().timers.is_empty());

        state().thread_running = false;
    }
}
