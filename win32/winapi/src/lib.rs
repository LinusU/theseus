#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

pub mod advapi32;
pub mod bitmap_format;
pub mod ddraw;
pub mod dinput;
mod dllexport;
pub mod dplayx;
pub mod dsound;
pub mod ebueula;
pub mod gdi32;
mod handle;
mod heap;
pub mod imm32;
pub mod kernel32;
mod locked_state;
pub mod msacm32;
pub mod msvcrt;
pub mod ole32;
mod point;
mod ptr;
mod rect;
pub mod shell32;
pub mod trace;
pub mod user32;
pub mod winmm;

/// Functions a program may resolve at runtime with LoadLibrary/GetProcAddress
/// instead of importing statically. The translator reserves a callable address
/// for each of these, so a call through the returned pointer lands somewhere.
///
/// Statically imported functions need no entry here; only add a name when a
/// program is seen looking it up by hand.
pub const DYNAMIC_EXPORTS: &[(&str, &[&str])] = &[
    ("kernel32", &["IsProcessorFeaturePresent"]),
    // The Microsoft C runtime loads user32 on demand to report fatal errors.
    (
        "user32",
        &["MessageBoxA", "GetActiveWindow", "GetLastActivePopup"],
    ),
];

/// Dynamic exports the WinAPI layer can provide even when an older generated
/// snapshot did not reserve and register a synthetic address for them. Each
/// entry is appended to the runtime block table so `GetProcAddress` can return
/// a callable pointer.
pub const BUILTIN_EXPORTS: &[(&str, &str, ContFn)] = &[
    (
        "kernel32",
        "IsProcessorFeaturePresent",
        kernel32::IsProcessorFeaturePresent_stdcall,
    ),
    ("EBUEULA", "EBUEula", ebueula::EBUEula_stdcall),
    ("DDRAW", "DirectDrawCreate", ddraw::DirectDrawCreate_stdcall),
    (
        "DDRAW",
        "DirectDrawCreateEx",
        ddraw::DirectDrawCreateEx_stdcall,
    ),
    (
        "DDRAW",
        "DirectDrawEnumerateA",
        ddraw::DirectDrawEnumerateA_stdcall,
    ),
    (
        "DDRAW",
        "DirectDrawEnumerateExA",
        ddraw::DirectDrawEnumerateExA_stdcall,
    ),
    (
        "blade",
        "DirectDrawCreate",
        ddraw::DirectDrawCreate_stdcall,
    ),
    (
        "blade",
        "DirectDrawCreateEx",
        ddraw::DirectDrawCreateEx_stdcall,
    ),
    (
        "blade",
        "DirectDrawCreateClipper",
        ddraw::DirectDrawCreateClipper_stdcall,
    ),
    (
        "blade",
        "DirectDrawEnumerateA",
        ddraw::DirectDrawEnumerateA_stdcall,
    ),
    (
        "blade",
        "DirectDrawEnumerateW",
        ddraw::DirectDrawEnumerateW_stdcall,
    ),
    (
        "blade",
        "DirectDrawEnumerateExA",
        ddraw::DirectDrawEnumerateExA_stdcall,
    ),
    (
        "blade",
        "DirectDrawEnumerateExW",
        ddraw::DirectDrawEnumerateExW_stdcall,
    ),
    ("blade", "GetDXVB", ddraw::GetDXVB_stdcall),
];

pub use dllexport::{ABIReturn, FromABIParam};
pub use handle::{HANDLE, Handles};
pub use point::POINT;
pub use ptr::Ptr;
pub use rect::RECT;

macro_rules! stub {
    ($arg:expr) => {{
        log::warn!("stub: using {:?}", $arg);
        $arg
    }};
}
use runtime::{CPU, ContFn, Context, EXEData, Mappings, Memory, Regs};
pub(crate) use stub;

pub fn load(exe: &EXEData) -> Context {
    host::init();
    crate::trace::init(&host::trace_spec());

    // Room for the program's image, its heaps and the flat pool games of this
    // era carve out for themselves.
    let memory_size = 256 << 20;
    let mut memory = Memory::leak_new(memory_size);

    kernel32::init_state(exe.image_base, exe.resources.clone());

    let mut regs = Regs::default();
    let mut mappings = Mappings::default();
    (exe.init)(&mut regs, &mut memory, &mut mappings);

    let mut lock = kernel32::lock();
    // The mappings the program declared have to reach the state, or every
    // later allocation hands out addresses the program is already using.
    lock.mappings = mappings;

    // Older generated snapshots may not reserve addresses for dynamic exports
    // added after they were translated. Append them to the block table and
    // register them with the DLL state so GetProcAddress can resolve them.
    let mut blocks: Vec<(u32, ContFn)> = exe.blocks.iter().copied().collect();
    for (next_addr, (dll, name, func)) in (0xfafd_0000..).zip(BUILTIN_EXPORTS) {
        blocks.push((next_addr, *func));
        lock.dlls.register_export(dll, name, next_addr);
    }
    blocks.sort_by_key(|(addr, _)| *addr);
    let blocks: &'static [(u32, ContFn)] = Box::leak(blocks.into_boxed_slice());

    let mut ctx = Context {
        cpu: CPU::default(),
        thread_handle: lock.objects.add(kernel32::Object::Thread).to_raw(),
        thread_id: 1,
        memory,
        blocks,
        cache: Default::default(),
        recent: [Context::return_from_x86; 4],
    };
    ctx.cpu.regs = regs;
    lock.init_process(&mut ctx);
    ctx
}

pub fn start(ctx: &mut Context, exe: &EXEData) {
    assert!(!ctx.cpu.real_mode);
    ctx.call32_x86(exe.entry_point, vec![]);
    // TODO: per Windows, we need to join any spawned threads here.
}

pub fn run(exe: &EXEData) {
    let mut ctx = load(exe);
    start(&mut ctx, exe);
}
