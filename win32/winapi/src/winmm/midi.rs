use runtime::Context;

use super::state;

const MMSYSERR_NOERROR: u32 = 0;

#[derive(Default)]
pub struct State {
    volumes: std::collections::HashMap<u32, u32>,
}

#[win32_derive::dllexport]
pub fn midiOutSetVolume(_ctx: &mut Context, hmo: u32, dwVolume: u32) -> u32 {
    state()
        .midi
        .get_or_insert_with(Default::default)
        .volumes
        .insert(hmo, dwVolume);
    MMSYSERR_NOERROR
}
