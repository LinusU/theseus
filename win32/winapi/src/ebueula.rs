use runtime::Context;

#[win32_derive::dllexport]
pub fn EBUEula(_ctx: &mut Context, _subkey: u32, _buffer: u32, _unused: u32, _flags: u32) -> u32 {
    // Midtown Madness 2 loads EBUEULA.DLL and calls the single exported
    // EBUEula function during startup. The game treats any non-zero return
    // value as acceptance of the EULA, so return a positive value to let it
    // continue. The supplied arguments are a registry subkey, a buffer,
    // and two flag words; no actual registry or UI work is modeled.
    log::debug!("EBUEula: reporting EULA accepted");
    1
}
