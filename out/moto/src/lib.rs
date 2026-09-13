#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

mod generated;

#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn main() {
    let exe = &generated::EXEDATA;
    let mut ctx = winapi::load(exe);
    widen_section_window(&mut ctx.memory, extra_sections());
    winapi::start(&mut ctx, exe);
}

/// MOTO_DRAW_DISTANCE: how many more track sections to draw, ahead and
/// behind, than the game does itself (default 80; 0 for the original).
fn extra_sections() -> u32 {
    #[cfg(not(target_family = "wasm"))]
    if let Ok(value) = std::env::var("MOTO_DRAW_DISTANCE") {
        if let Ok(n) = value.parse::<u32>() {
            return n;
        }
    }
    80
}

/// Draw `extra` more track sections each way than the game does.
///
/// Each frame the game lists the sections around the camera to draw, with
/// their scenery (0x40ce60): with W = 10 from the track, 4*W+5 sections
/// ahead and 4+4 behind when looking down the track, split more evenly when
/// looking across it. That is why scenery appears a chunk at a time. The
/// four `lea`s computing those counts have their displacements (+4/+5)
/// translated as memory reads (see translate.sh), so adding to them here
/// lengthens both runs. The list lives in a 256-entry array (0x5dabd8) and
/// must stay shorter than the shortest track (435 sections, Track06), so
/// `extra` is capped at 100.
fn widen_section_window(memory: &mut runtime::Memory, extra: u32) {
    let extra = extra.min(100);
    // Last index of the sections ahead: lea ecx, [4*eax + 4].
    memory.write::<u32>(0x40d1ad, 4 + extra);
    // Number of sections ahead: lea edx, [4*ecx + 5].
    memory.write::<u32>(0x40d1cf, 5 + extra);
    // Last index of the sections behind: lea ecx, [4*eax + 4].
    memory.write::<u32>(0x40d277, 4 + extra);
    // Number of sections in all: lea edx, [ecx + 4*eax + 4] (a byte).
    memory.write::<u8>(0x40d2a1, (4 + extra) as u8);
}
