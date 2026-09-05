use std::sync::{Mutex, MutexGuard};

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

#[derive(Default)]
pub struct State {
    timer: Option<Timer>,
    wave: Option<wave::State>,
    mmio: Option<mmio::State>,
    midi: Option<midi::State>,
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
    wave: None,
    mmio: None,
    midi: None,
});

pub fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap()
}

fn winmm_main(ctx: &mut Context) {
    loop {
        let mut lock = state();
        let Some(timer) = lock.timer.as_mut() else {
            return;
        };

        let now = host::host().time();
        if now < timer.next {
            let delta = timer.next - now;
            std::thread::sleep(std::time::Duration::from_millis(delta as u64));
        }

        let notify = timer.notify;
        let periodic = timer.periodic;
        let user_data = timer.user_data;
        if periodic {
            timer.next = now + timer.period;
        } else {
            lock.timer = None;
        }
        drop(lock);

        match notify {
            Notify::Function(callback) => {
                let func = ctx.indirect(callback);
                let timer_id = 1;
                // LPTIMECALLBACK
                ctx.call32_x86(func, vec![timer_id, 0, user_data, 0, 0]);
            }
            Notify::Event { handle, pulse } => {
                kernel32::signal_event(HANDLE::from_abi(handle), pulse);
            }
        }

        if !periodic {
            return;
        }
    }
}
