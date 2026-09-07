use std::collections::BTreeMap;
use std::sync::{Condvar, Mutex, MutexGuard};

use runtime::Context;

use crate::{FromABIParam, HANDLE, kernel32};

mod time;
pub use time::*;
mod wave;
pub use wave::*;
mod mixer;
pub use mixer::*;
mod misc;
pub use misc::*;
mod mmio;
pub use mmio::*;
mod midi;
pub use midi::*;

pub struct State {
    /// Active timers keyed by their WinMM id.
    pub timers: BTreeMap<u32, Timer>,
    /// Ids start at 1 and 0 is reserved as the failure value.
    pub next_id: u32,
    /// Whether the single `winmm_main` worker thread is running.
    pub thread_running: bool,
    pub wave: Option<wave::State>,
    pub mmio: Option<mmio::State>,
    pub midi: Option<midi::State>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            timers: BTreeMap::new(),
            next_id: 1,
            thread_running: false,
            wave: None,
            mmio: None,
            midi: None,
        }
    }
}

impl State {
    /// The mmio file table, created on first use (a `static` can't build the
    /// map up front).
    pub fn mmio(&mut self) -> &mut mmio::State {
        self.mmio.get_or_insert_with(Default::default)
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    timers: BTreeMap::new(),
    next_id: 1,
    thread_running: false,
    wave: None,
    mmio: None,
    midi: None,
});
pub(crate) static TIMER_COND: Condvar = Condvar::new();

pub fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn winmm_main(ctx: &mut Context) {
    loop {
        let mut lock = state();
        if lock.timers.is_empty() {
            lock.thread_running = false;
            break;
        }

        let now = host::host().time();
        let Some(next) = lock.timers.values().map(|t| t.next).min() else {
            lock.thread_running = false;
            break;
        };

        if now < next {
            let delta = std::time::Duration::from_millis((next - now) as u64);
            let (l, _) = TIMER_COND
                .wait_timeout(lock, delta)
                .unwrap_or_else(|e| e.into_inner());
            lock = l;
            continue;
        }

        let due_ids: Vec<u32> = lock
            .timers
            .values()
            .filter(|t| t.next <= now)
            .map(|t| t.id)
            .collect();

        let mut timers = Vec::with_capacity(due_ids.len());
        for id in due_ids {
            let Some(timer) = lock.timers.get(&id) else {
                continue;
            };
            let timer = timer.clone();
            if !timer.periodic {
                lock.timers.remove(&id);
            } else if let Some(t) = lock.timers.get_mut(&id) {
                // Floor the period at 1ms: Windows clamps to the timer
                // resolution, and an unclamped 0 here would reschedule the
                // timer due-every-iteration and spin this thread.
                t.next = now.saturating_add(t.period.max(1));
            }
            timers.push(timer);
        }

        drop(lock);

        for timer in timers {
            match timer.notify {
                Notify::Function(callback) => {
                    let func = ctx.indirect(callback);
                    // LPTIMECALLBACK
                    ctx.call32_x86(func, vec![timer.id, 0, timer.user_data, 0, 0]);
                }
                Notify::Event { handle, pulse } => {
                    kernel32::signal_event(HANDLE::from_abi(handle), pulse);
                }
            }
        }
    }
}
