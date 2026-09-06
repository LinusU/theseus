use std::{cell::Cell, collections::HashMap, sync::Mutex};

use runtime::Mappings;

use crate::{
    HANDLE, Handles,
    heap::Heap,
    kernel32::{self, CommandLine, DLLs, Object},
    locked_state::LockedState,
};

pub struct LoadedModule {
    pub image_base: u32,
    pub resources: std::ops::Range<u32>,
}

pub struct State {
    pub mappings: Mappings,
    pub heaps: HashMap<u32, Heap>,
    pub process_heap: Heap,
    pub image_base: u32,
    pub resources: std::ops::Range<u32>,
    pub loaded_modules: HashMap<u32, LoadedModule>,
    pub command_line: CommandLine,
    pub environ: Cell<u32>,
    /// Process environment variables, in insertion order. Names compare
    /// case-insensitively, matching Windows semantics.
    pub env: Vec<(String, String)>,
    /// STD_INPUT_HANDLE/STD_OUTPUT_HANDLE/STD_ERROR_HANDLE as overridden by
    /// SetStdHandle; GetStdHandle reports these values.
    pub std_handles: [u32; 3],
    pub next_thread_id: u32,
    /// Bitmask of allocated TLS slots; TLS_MINIMUM_AVAILABLE is 64.
    pub tls_allocated: u64,
    pub unhandled_exception_filter: u32,
    pub console_ctrl_handlers: Vec<u32>,
    pub thread_priorities: HashMap<HANDLE, i32>,
    pub process_priority_class: u32,
    pub dlls: Box<dyn kernel32::DLLs>,
    pub objects: Handles<Object>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn build_state(image_base: u32, resources: std::ops::Range<u32>) -> State {
    let mut dlls = Box::new(kernel32::Exports::default());
    dlls.register_module("kernel32");
    State {
        image_base,
        resources,
        loaded_modules: HashMap::new(),
        heaps: HashMap::new(),
        mappings: Default::default(),
        process_heap: Default::default(),
        command_line: Default::default(),
        environ: Default::default(),
        env: Vec::new(),
        std_handles: [
            crate::kernel32::file::STDIN_HFILE,
            crate::kernel32::file::STDOUT_HFILE,
            crate::kernel32::file::STDERR_HFILE,
        ],
        next_thread_id: 2,
        tls_allocated: 0,
        unhandled_exception_filter: 0,
        console_ctrl_handlers: Vec::new(),
        thread_priorities: HashMap::new(),
        process_priority_class: 0x20,
        dlls,
        objects: Handles::new(0x1000),
    }
}

pub fn init_state(image_base: u32, resources: std::ops::Range<u32>) {
    *STATE.lock().unwrap() = Some(build_state(image_base, resources));
}

/// Initialize the shared state only when it is still empty, so parallel tests
/// cannot reset each other's setup mid-assertion.
#[cfg(test)]
pub fn ensure_test_state() {
    let mut state = STATE.lock().unwrap();
    if state.is_none() {
        *state = Some(build_state(0x400000, 0..0));
    }
}

pub type Lock = LockedState<State>;
pub fn lock() -> Lock {
    LockedState::from(&STATE)
}

#[cfg(test)]
mod tests {
    use super::{ensure_test_state, lock};

    #[test]
    fn kernel32_is_loaded_at_process_initialization() {
        ensure_test_state();
        assert!(lock().dlls.module_handle("KERNEL32.DLL").is_some());
    }
}
