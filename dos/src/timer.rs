//! Programmable Interrupt Timer.

use runtime::Context;

#[derive(Default)]
pub struct PIT {
    divisor: u16,
    /// Access mode from the last channel-0 control word: 1 = lobyte only,
    /// 2 = hibyte only, 3 = lobyte then hibyte (the default sequence).
    access_mode: u8,
    lobyte: Option<u8>,
    next_interrupt: Option<u64>,
}

/// Convert a ms tick count into the number of times the PIT ticks in that interval.
fn pit_ticks(time_ms: u32) -> u64 {
    const PIT_HZ: u64 = 1_193_182;
    time_ms as u64 * PIT_HZ / 1000
}

/// Convert a PIT divisor into the number of times the PIT ticks in that interval.
fn pit_period_ticks(divisor: u16) -> u64 {
    match divisor {
        // The PIT treats a programmed divisor of 0 as 65536.
        0 => 1 << 16,
        divisor => divisor as u64,
    }
}

impl PIT {
    /// Handle an `out` instruction that writes to a PIT port.
    pub fn out(&mut self, _ctx: &mut Context, port: u16, data: u8) {
        // https://wiki.osdev.org/Programmable_Interval_Timer
        match port {
            0x40 => match self.access_mode {
                1 => self.set_divisor(data as u16),
                2 => self.set_divisor((data as u16) << 8),
                // Channel 0's lobyte/hibyte sequence; the lobyte state is
                // shared with modes 1 and 2 that write a single byte.
                _ => match self.lobyte {
                    Some(lo) => {
                        self.lobyte = None;
                        self.set_divisor((data as u16) << 8 | (lo as u16));
                    }
                    None => self.lobyte = Some(data),
                },
            },
            // Channel 1 is the unused DRAM refresh counter and channel 2
            // gates the PC speaker, which this runtime does not emulate;
            // the writes are accepted and dropped.
            0x41 | 0x42 => log::info!("PIT channel {} data {data:#x} ignored", port - 0x40),
            0x43 => {
                let channel = data >> 6;
                let access_mode = (data >> 4) & 0b11;
                // The mode field is 3 bits where 6/7 alias to 2/3, so the
                // low two bits identify the effective counting mode.
                let operating_mode = (data >> 1) & 0b11;
                let bcd_mode = data & 0b1;
                if channel != 0 {
                    log::info!("PIT channel {channel} control {data:#x} ignored");
                    return;
                }
                if access_mode == 0 || bcd_mode != 0 {
                    // Counter-latch/read-back commands and BCD counting are
                    // not modeled; the latch sequence is left alone so a
                    // later control word starts a clean lo/hi write.
                    log::warn!("PIT control {data:#x} unsupported");
                    return;
                }
                if operating_mode != 0b11 {
                    log::warn!("PIT operating mode {operating_mode} treated as square wave");
                }
                self.access_mode = access_mode;
                self.lobyte = None;
            }
            _ => log::warn!("PIT out to unhandled port {port:#x}"),
        }
    }

    fn set_divisor(&mut self, divisor: u16) {
        self.divisor = divisor;
        self.next_interrupt = Some(pit_ticks(host::host().time()) + pit_period_ticks(self.divisor));
        log::info!("PIT divisor set to {divisor:#x}");
    }

    pub fn check_timer(&mut self, ctx: &mut Context, handler: (u16, u16)) {
        const MAX_TICKS_PER_CHECK: u32 = 8;
        let Some(mut next) = self.next_interrupt else {
            return;
        };

        let now = host::host().time();
        let now_ticks = pit_ticks(now);
        let mut fired = 0;
        while next <= now_ticks && fired < MAX_TICKS_PER_CHECK {
            self.call_timer(ctx, handler);
            next += pit_period_ticks(self.divisor);
            fired += 1;
        }
        if next <= now_ticks {
            // A host stall missed more ticks than we replay; drop the
            // backlog instead of running thousands of handlers at once.
            next = now_ticks + pit_period_ticks(self.divisor);
        }
        self.next_interrupt = Some(next);
    }

    fn call_timer(&mut self, ctx: &mut Context, handler: (u16, u16)) {
        let (seg, ofs) = handler;
        if seg == 0 {
            // No int 8 handler is installed; drop the tick instead of
            // running the null page.
            return;
        }
        log::info!("timer {seg:x}:{ofs:x}");

        // The interrupted context can be in a different segment than the
        // handler. The pushed frame reports the interrupted CS; the offset
        // is the handler's own because the true resume IP is not tracked
        // here — the loop below exits on the iret regardless of it.
        let esp = ctx.cpu.regs.esp;
        ctx.push16(ctx.cpu.flags.bits() as u16);
        ctx.push16(ctx.cpu.regs.cs);
        ctx.push16(ofs);

        let mut f = ctx.indirect16((seg, ofs).into());
        while ctx.cpu.regs.esp != esp {
            // don't check interrupts while running interrupt handler
            f = f.0(ctx);
        }
    }
}
