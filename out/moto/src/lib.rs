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
    if !pixel_snap() {
        winapi::ddraw::set_vertex_hook(subpixel_vertices);
    }
    winapi::start(&mut ctx, exe);
}

/// MOTO_PIXEL_SNAP=1: draw vertices at whole pixels of the game's 640x480,
/// as the game does itself.
fn pixel_snap() -> bool {
    #[cfg(not(target_family = "wasm"))]
    return std::env::var("MOTO_PIXEL_SNAP").is_ok_and(|v| !v.is_empty() && v != "0");
    #[cfg(target_family = "wasm")]
    false
}

/// Put Direct3D vertices back where they were before the game rounded them
/// to whole pixels.
///
/// The game projects everything into its array of transformed vertices
/// (see `grow_vertex_array`): camera-space x and y already scaled by the
/// focal length at +0 and +4, 1/z at +0x18, and the screen position at
/// +0x10/+0x14 as integers of its 640x480 (`fistp` at 0x49b46b/0x49b474 and
/// 0x49b7bc/0x49b7c4). Its clipping, culling and sorting all work on those
/// integers, and so do the D3DTLVERTEXs it sends, which at 4K moves
/// vertices in steps of 4.5 pixels: scenery jitters as the camera moves.
///
/// A D3DTLVERTEX's rhw is that same 1/z, bit for bit, so its entry can be
/// found and the position computed again without rounding. That is only
/// used when the entry's integers are the vertex's own and the position is
/// less than a pixel from them, so vertices placed some other way, and 2D
/// ones sharing a 1/z, are left as they are. In the attract demo that
/// moves about two thirds of all vertices.
fn subpixel_vertices(
    memory: &runtime::Memory,
    vertices: &mut [[u8; winapi::ddraw::TLVERTEX_SIZE]],
) {
    // Vertices projected by 0x49b2c0 are placed around the screen center
    // (0x60d3c4, 0x624f88).
    const CENTER_X: u32 = 0x60d3c4;
    const CENTER_Y: u32 = 0x624f88;
    const COUNT: u32 = 0x68b014;
    const VERTEX_SIZE: u32 = 28;

    thread_local! {
        static BY_RHW: std::cell::RefCell<std::collections::HashMap<u32, u32>> = Default::default();
    }
    // The array's address, as grow_vertex_array stored it.
    let array = memory.read::<u32>(0x497ce3);
    let count = memory.read::<u32>(COUNT).min(VERTICES);
    let center_x = memory.read::<i32>(CENTER_X) as f64;
    let center_y = memory.read::<i32>(CENTER_Y) as f64;

    BY_RHW.with_borrow_mut(|by_rhw| {
        by_rhw.clear();
        for i in 0..count {
            let entry = array + i * VERTEX_SIZE;
            by_rhw.insert(memory.read::<u32>(entry + 0x18), entry);
        }
        for vertex in vertices {
            let field = |o: usize| u32::from_le_bytes(vertex[o..o + 4].try_into().unwrap());
            let Some(&entry) = by_rhw.get(&field(12)) else {
                continue;
            };
            let (sx, sy) = (f32::from_bits(field(0)), f32::from_bits(field(4)));
            if memory.read::<i32>(entry + 0x10) as f32 != sx
                || memory.read::<i32>(entry + 0x14) as f32 != sy
            {
                continue;
            }
            let inv_z = memory.read::<f32>(entry + 0x18) as f64;
            let x = center_x + memory.read::<f32>(entry) as f64 * inv_z;
            let y = center_y - memory.read::<f32>(entry + 4) as f64 * inv_z;
            // Projection rounds (at most half a pixel off), but the vertices
            // that near-plane clipping makes (0x497d60) are truncated by
            // _ftol (less than a pixel off).
            if (x - sx as f64).abs() >= 1.0 || (y - sy as f64).abs() >= 1.0 {
                continue;
            }
            vertex[0..4].copy_from_slice(&(x as f32).to_le_bytes());
            vertex[4..8].copy_from_slice(&(y as f32).to_le_bytes());
        }
    });
}

/// MOTO_DRAW_DISTANCE: how many more track sections to draw ahead than the
/// game does itself, with the ground to match (default 80; 0 for the
/// original).
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

/// Draw `extra` more track sections ahead than the game does, and the
/// ground to match.
///
/// Each frame the game lists what to draw around the camera (0x40ce60, a
/// method of the track at 0x52c108) from a window size W that the track's
/// constructor sets to 10 (0x40c5b4): W blocks of ground ahead (large
/// meshes, several sections long each, 0x40d03b) and 1 behind, then 4*W+5
/// sections ahead, with their scenery objects, and 4+4 behind (split more
/// evenly when looking across the track). That is why the ground and the
/// scenery appeared a chunk at a time.
///
/// W is scaled up here so the ground and the sections reach as far as each
/// other, and the displacements of the `lea`s computing the section counts
/// (+4/+5) make up the rest of `extra`; all of them are translated as memory
/// reads (see translate.sh). The section list lives in a 256-entry array
/// (0x5dabd8) and must stay shorter than the shortest track (435 sections,
/// Track06), so `extra` is capped.
fn widen_section_window(memory: &mut runtime::Memory, extra: u32) {
    let extra = extra.min(MAX_EXTRA_SECTIONS);
    let window = (40 + extra) / 4;
    let rest = 45 + extra - (4 * window + 5);
    // The track constructor's mov dword ptr [esi + 0x24], 10.
    memory.write::<u32>(0x40c5b7, window);
    // Last index of the sections ahead: lea ecx, [4*eax + 4].
    memory.write::<u32>(0x40d1ad, 4 + rest);
    // Number of sections ahead: lea edx, [4*ecx + 5].
    memory.write::<u32>(0x40d1cf, 5 + rest);
    // Last index of the sections behind: lea ecx, [4*eax + 4].
    memory.write::<u32>(0x40d277, 4 + rest);
    // Number of sections in all: lea edx, [ecx + 4*eax + 4] (a byte).
    memory.write::<u8>(0x40d2a1, (4 + rest) as u8);
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

/// Room in the array of transformed vertices, once `grow_vertex_array` has
/// moved it.
const VERTICES: u32 = 64000;

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
