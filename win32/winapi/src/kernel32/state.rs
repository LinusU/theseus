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
    pub next_thread_id: u32,
    pub next_tls_index: u32,
    pub unhandled_exception_filter: u32,
    pub console_ctrl_handlers: Vec<u32>,
    pub thread_priorities: HashMap<HANDLE, i32>,
    pub process_priority_class: u32,
    pub dlls: Box<dyn kernel32::DLLs>,
    pub objects: Handles<Object>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

pub fn init_state(image_base: u32, resources: std::ops::Range<u32>) {
    let mut state = STATE.lock().unwrap();
    let mut dlls = Box::new(kernel32::Exports::default());
    dlls.register_module("kernel32");
    *state = Some(State {
        image_base,
        resources,
        loaded_modules: HashMap::new(),
        heaps: HashMap::new(),
        mappings: Default::default(),
        process_heap: Default::default(),
        command_line: Default::default(),
        environ: Default::default(),
        env: Vec::new(),
        next_thread_id: 2,
        next_tls_index: 0,
        unhandled_exception_filter: 0,
        console_ctrl_handlers: Vec::new(),
        thread_priorities: HashMap::new(),
        process_priority_class: 0x20,
        dlls,
        objects: Handles::new(0x1000),
    });
}

pub type Lock = LockedState<State>;
pub fn lock() -> Lock {
    LockedState::from(&STATE)
}

#[cfg(test)]
mod tests {
    use super::{init_state, lock};

    #[test]
    fn kernel32_is_loaded_at_process_initialization() {
        init_state(0x400000, 0..0);
        assert!(lock().dlls.module_handle("KERNEL32.DLL").is_some());
    }
}
