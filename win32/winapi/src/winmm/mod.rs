use std::sync::{Condvar, Mutex, MutexGuard};

static TIMER_COND: Condvar = Condvar::new();

use runtime::Context;

mod compat;
pub use compat::*;
mod time;
pub use time::*;
mod wave;
pub use wave::*;
mod misc;
pub use misc::*;
mod mmio;
pub use mmio::*;
mod joystick;
pub use joystick::*;
mod mixer;
pub use mixer::*;

#[derive(Default)]
pub struct State {
    timer: Option<Timer>,
    timer_thread: Option<std::thread::JoinHandle<()>>,
    wave: Option<wave::State>,
    mmio: Option<mmio::State>,
}

impl State {
    /// The mmio file table, created on first use (a `static` can't build the
    /// map up front).
    pub fn mmio(&mut self) -> &mut mmio::State {
        self.mmio.get_or_insert_with(Default::default)
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    timer: None,
    timer_thread: None,
    wave: None,
    mmio: None,
});

pub fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap()
}

fn winmm_main(ctx: &mut Context) {
    let mut lock = state();
    loop {
        let (callback, user_data, period) = match lock.timer.as_ref() {
            Some(t) => (t.callback, t.user_data, t.period),
            None => return,
        };

        let now = host::host().time();
        let delta = match &lock.timer {
            Some(t) if now < t.next => t.next - now,
            _ => 0,
        };

        if delta > 0 {
            let wait = std::time::Duration::from_millis(delta as u64);
            let (new_lock, timeout) = TIMER_COND.wait_timeout(lock, wait).unwrap();
            lock = new_lock;
            if !timeout.timed_out() {
                // timer was killed or reset; loop to recheck
                continue;
            }
        }

        let Some(timer) = lock.timer.as_mut() else {
            return;
        };
        timer.next = now + period;
        drop(lock);

        // LPTIMECALLBACK
        let func = ctx.indirect(callback);
        ctx.call32_x86(func, vec![1, 0, user_data, 0, 0]);

        lock = state();
    }
}
