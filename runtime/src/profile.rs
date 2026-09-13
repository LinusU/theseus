//! Counting how often each block runs, for finding the loops behind some
//! behaviour of a program.
//!
//! With THESEUS_PROFILE_BLOCKS=<path>, every block run is counted, and each
//! `snapshot` appends the counts since the previous one to that file, one
//! `<address> <count>` line per block that ran, under a `# <label>` line.

use std::{
    cell::RefCell,
    collections::HashMap,
    hash::{BuildHasherDefault, Hasher},
};

use crate::Context;

/// Block function pointers are already well spread out.
#[derive(Default)]
struct PointerHasher(u64);

impl Hasher for PointerHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 = (self.0 << 8) | b as u64;
        }
    }
    fn write_usize(&mut self, n: usize) {
        self.0 = (n as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

type Counts = HashMap<usize, u64, BuildHasherDefault<PointerHasher>>;

thread_local! {
    static COUNTS: RefCell<Counts> = RefCell::default();
}

fn path() -> Option<&'static str> {
    #[cfg(not(target_family = "wasm"))]
    {
        static PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        PATH.get_or_init(|| std::env::var("THESEUS_PROFILE_BLOCKS").ok().filter(|p| !p.is_empty()))
            .as_deref()
    }
    #[cfg(target_family = "wasm")]
    None
}

pub fn enabled() -> bool {
    path().is_some()
}

/// Count one run of the block whose function this is.
pub fn count(block: usize) {
    COUNTS.with_borrow_mut(|counts| *counts.entry(block).or_insert(0) += 1);
}

/// Append the counts since the last snapshot, labelled, and start over.
pub fn snapshot(ctx: &Context, label: &str) {
    let Some(path) = path() else {
        return;
    };
    let counts = COUNTS.with_borrow_mut(std::mem::take);
    let addresses: HashMap<usize, u32> = ctx
        .blocks
        .iter()
        .map(|&(addr, func)| (func as usize, addr))
        .collect();
    let mut rows: Vec<(u32, u64)> = counts
        .into_iter()
        .filter_map(|(func, n)| Some((*addresses.get(&func)?, n)))
        .collect();
    rows.sort_unstable();
    let mut out = format!("# {label}\n");
    for (addr, n) in rows {
        out.push_str(&format!("{addr:x} {n}\n"));
    }
    #[cfg(not(target_family = "wasm"))]
    {
        use std::io::Write;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path);
        if let Err(err) = file.and_then(|mut f| f.write_all(out.as_bytes())) {
            log::warn!("profile: {path}: {err}");
        }
    }
}
