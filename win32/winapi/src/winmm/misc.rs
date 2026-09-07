use runtime::Context;

use crate::kernel32::HMODULE;

#[win32_derive::dllexport]
pub fn joyGetNumDevs(_ctx: &mut Context) -> u32 {
    0
}

#[win32_derive::dllexport]
pub fn PlaySoundA(
    _ctx: &mut Context,
    _pszSound: u32, /* CSTR */
    _hmod: HMODULE,
    _fdwSound: u32, /* SND_FLAGS */
) -> bool {
    // No audio backend is modeled; report failure rather than panic.
    false
}

#[win32_derive::dllexport]
pub fn PlaySoundW(
    _ctx: &mut Context,
    _pszSound: u32, /* WSTR */
    _hmod: HMODULE,
    _fdwSound: u32, /* SND_FLAGS */
) -> bool {
    // No audio backend is modeled; report failure rather than panic.
    false
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
    fn play_sound_a_and_w_report_no_audio_backend() {
        let mut ctx = context();
        assert!(!PlaySoundA(&mut ctx, 0, 0, 0));
        assert!(!PlaySoundW(&mut ctx, 0, 0, 0));
    }
}
