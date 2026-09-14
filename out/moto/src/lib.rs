#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

mod generated;

#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn main() {
    let exe = &generated::EXEDATA;
    let mut ctx = winapi::load(exe);
    let extra = extra_sections();
    widen_section_window(&mut ctx.memory, extra);
    lengthen_view(&mut ctx.memory, extra);
    grow_vertex_array(&mut ctx.memory);
    if !low_poly_bikes() {
        full_detail_bikes(&mut ctx.memory);
    }
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

const MAX_EXTRA_SECTIONS: u32 = 100;

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
    let extra = extra.min(MAX_EXTRA_SECTIONS);
    // Last index of the sections ahead: lea ecx, [4*eax + 4].
    memory.write::<u32>(0x40d1ad, 4 + extra);
    // Number of sections ahead: lea edx, [4*ecx + 5].
    memory.write::<u32>(0x40d1cf, 5 + extra);
    // Last index of the sections behind: lea ecx, [4*eax + 4].
    memory.write::<u32>(0x40d277, 4 + extra);
    // Number of sections in all: lea edx, [ecx + 4*eax + 4] (a byte).
    memory.write::<u8>(0x40d2a1, (4 + extra) as u8);
}

/// Move the far plane out as far as `extra` more sections reach.
///
/// Scenery objects are drawn for every listed section (see
/// `widen_section_window`), but the ground is cut off by the view frustum,
/// whose far plane is the "LenBPlane" distance the game's settings give each
/// detail level (a 36-byte row per level at 0x51d1b8, filled in from its
/// configuration after startup): 12000 for levels 0-2, 14000 and 18000
/// above. The three places that read it have their displacements translated
/// as memory reads (see translate.sh) and are pointed at a table of longer
/// distances here instead, in the unused end of .rdata. The depth-sorting
/// buckets (8000 of them, 0x655d38) cover 4 units each; objects beyond that
/// all share the last one, so they get coarser in step.
fn lengthen_view(memory: &mut runtime::Memory, extra: u32) {
    const LEN_B_PLANE: [f32; 5] = [12000.0, 12000.0, 12000.0, 14000.0, 18000.0];
    const TABLE: u32 = 0x4dad30;
    let scale = (45 + extra.min(MAX_EXTRA_SECTIONS)) as f32 / 45.0;
    for (level, len) in LEN_B_PLANE.iter().enumerate() {
        memory.write::<u32>(TABLE + 36 * level as u32, (len * scale).to_bits());
    }
    // mov ecx, [eax + 8*eax + 0x51d1bc] (twice) and fld [eax + 8*eax + 0x51d1bc].
    for disp in [0x46e734, 0x46e90e, 0x48014a] {
        memory.write::<u32>(disp, TABLE);
    }
    // Depth to bucket: fmul dword ptr [0x4d1f9c], read by the two inserts.
    memory.write::<u32>(0x4d1f9c, (0.25 / scale).to_bits());
}

/// MOTO_LOW_POLY_BIKES=1: draw distant bikes with fewer polygons, as the
/// game does itself.
fn low_poly_bikes() -> bool {
    #[cfg(not(target_family = "wasm"))]
    return std::env::var("MOTO_LOW_POLY_BIKES").is_ok_and(|v| !v.is_empty() && v != "0");
    #[cfg(target_family = "wasm")]
    false
}

/// Draw every bike with its most detailed model, however far away.
///
/// Each frame 0x45f160 picks a rider's model by distance from the camera
/// (the index at +0x638 of the rider, which the drawing at 0x461440 uses to
/// pick a list of parts): 0 up to 1000, 2 up to 2500, 3 up to 3000 and 4
/// beyond. The three `mov`s storing 2, 3 and 4 have their immediates
/// translated as memory reads (see translate.sh), so storing 0 here makes
/// them all pick the first.
fn full_detail_bikes(memory: &mut runtime::Memory) {
    for imm in [0x45f1f9, 0x45f21c, 0x45f27a] {
        memory.write::<u32>(imm, 0);
    }
}

/// Move the array of transformed vertices somewhere with room for 8 times
/// as many.
///
/// Everything drawn is transformed into one array per frame (0x6d20d8,
/// 28 bytes a vertex, the count in 0x68b014), made at startup with room for
/// 8000 (0x497ce0) and filled with no check. The demo's start line uses
/// about 2700; full-detail bikes alone take it to about 6900, and with the
/// longer view on top it overflows into the globals after it and the game
/// crashes. Its 21 uses have their address constants translated as memory
/// reads (see translate.sh) and are pointed at a larger array here.
fn grow_vertex_array(memory: &mut runtime::Memory) {
    const OLD: u32 = 0x6d20d8;
    const VERTEX_SIZE: u32 = 28;
    const VERTICES: u32 = 64000;
    let new = winapi::kernel32::lock()
        .mappings
        .alloc("moto vertices".into(), VERTICES * VERTEX_SIZE);
    // The loop constructing them: mov edi, OLD; mov esi, 7999.
    memory.write::<u32>(0x497ce3, new);
    memory.write::<u32>(0x497ce8, VERTICES - 1);
    // lea reg, [4*reg + OLD].
    for lea in [
        0x498005, 0x498032, 0x498410, 0x49843d, 0x498813, 0x49883e, 0x498ac9, 0x498af6, 0x498ec4,
        0x498ef1, 0x49916c, 0x499199, 0x49b2d7, 0x49b4ca,
    ] {
        memory.write::<u32>(lea + 3, new);
    }
    // lea reg, [eax + OLD] and [eax + OLD + 0x10].
    for (lea, field) in [
        (0x49c7f2, 0),
        (0x49c800, 0x10),
        (0x49cc43, 0),
        (0x49cc51, 0x10),
    ] {
        assert_eq!(memory.read::<u32>(lea + 2), OLD + field);
        memory.write::<u32>(lea + 2, new + field);
    }
}
