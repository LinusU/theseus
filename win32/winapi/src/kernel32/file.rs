use std::io::{Read, Seek, SeekFrom, Write};

use runtime::Context;

use crate::{
    Ptr,
    kernel32::{Object, lock},
};

pub type HANDLE = u32;

#[derive(Debug, PartialEq, Eq, win32_derive::ABIEnum)]
pub enum CreationDisposition {
    CREATE_NEW = 1,
    CREATE_ALWAYS = 2,
    OPEN_EXISTING = 3,
    OPEN_ALWAYS = 4,
    TRUNCATE_EXISTING = 5,
}

#[derive(Debug, PartialEq, Eq, win32_derive::ABIEnum)]
pub enum MoveMethod {
    FILE_BEGIN = 0,
    FILE_CURRENT = 1,
    FILE_END = 2,
}

const STDIN_HFILE: HANDLE = 0xF11E_0001;
const STDOUT_HFILE: HANDLE = 0xF11E_0002;
const STDERR_HFILE: HANDLE = 0xF11E_0003;

const INVALID_SET_FILE_POINTER: u32 = 0xFFFF_FFFF;

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

/// The host directory that maps to the root of the C: drive: the process's
/// initial current directory. Captured on first use, before any chdir.
fn initial_cwd() -> &'static std::path::Path {
    static CWD: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    CWD.get_or_init(|| host::fs::current_dir().unwrap_or_else(|_| ".".into()))
}

/// Resolve a Windows-style path against the host filesystem, ignoring case
/// (game data files and the paths that reference them often disagree on it).
pub fn resolve_path(path: &str) -> std::path::PathBuf {
    let path = path.replace('\\', "/");
    // Strip any drive prefix; the initial cwd acts as the root of C:.
    let path = if path.len() >= 2 && path.as_bytes()[1] == b':' {
        &path[2..]
    } else {
        &path[..]
    };
    let mut result = if path.starts_with('/') {
        initial_cwd().to_path_buf()
    } else {
        // Absolute base so that ".." components resolve properly.
        host::fs::current_dir().unwrap_or_else(|_| ".".into())
    };
    'component: for comp in path.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            result.pop();
            continue;
        }
        let direct = result.join(comp);
        if host::fs::exists(&direct) {
            result = direct;
            continue;
        }
        if let Ok(entries) = host::fs::read_dir(&result) {
            for entry in entries {
                if entry.name.eq_ignore_ascii_case(comp) {
                    result = result.join(&entry.name);
                    continue 'component;
                }
            }
        }
        result = direct; // may not exist, e.g. a file about to be created
    }
    result
}

#[win32_derive::dllexport]
pub fn GetStdHandle(_ctx: &mut Context, nStdHandle: u32) -> u32 {
    match nStdHandle as i32 {
        -10 => STDIN_HFILE,
        -11 => STDOUT_HFILE,
        -12 => STDERR_HFILE,
        _ => {
            log::error!("GetStdHandle: invalid handle");
            0
        }
    }
}

#[win32_derive::dllexport]
pub fn SetStdHandle(_ctx: &mut Context, _nStdHandle: u32, _hHandle: u32) -> u32 {
    crate::stub!(1)
}

#[win32_derive::dllexport]
pub fn CreateFileA(
    ctx: &mut Context,
    lpFileName: Ptr<u8>,
    dwDesiredAccess: u32,
    _dwShareMode: u32,
    _lpSecurityAttributes: Ptr<()>,
    dwCreationDisposition: CreationDisposition,
    _dwFlagsAndAttributes: u32,
    _hTemplateFile: u32,
) -> crate::HANDLE {
    let name = ctx.memory.read_str(lpFileName.addr).to_owned();
    let path = resolve_path(&name);
    let write = dwDesiredAccess & GENERIC_WRITE != 0;
    let read = dwDesiredAccess & GENERIC_READ != 0;
    let mut opts = host::fs::OpenOptions::new();
    opts.read(read || !write);
    if write {
        opts.write(true);
    }
    match dwCreationDisposition {
        CreationDisposition::CREATE_NEW => {
            opts.create_new(true);
        }
        CreationDisposition::CREATE_ALWAYS => {
            opts.create(true).truncate(true);
        }
        CreationDisposition::OPEN_EXISTING => {}
        CreationDisposition::OPEN_ALWAYS => {
            opts.create(true);
        }
        CreationDisposition::TRUNCATE_EXISTING => {
            opts.truncate(true);
        }
    }
    match opts.open(&path) {
        Ok(file) => lock().objects.add(Object::File(file)),
        Err(err) => {
            log::warn!("CreateFileA({name:?} => {path:?}): {err}");
            crate::HANDLE::invalid()
        }
    }
}

#[win32_derive::dllexport]
pub fn ReadFile(
    ctx: &mut Context,
    hFile: crate::HANDLE,
    lpBuffer: Ptr<u8>,
    nNumberOfBytesToRead: u32,
    lpNumberOfBytesRead: Ptr<u32>,
    lpOverlapped: Ptr<()>,
) -> bool {
    assert_eq!(lpOverlapped.addr, 0);
    let mut kernel32 = lock();
    let Some(Object::File(file)) = kernel32.objects.get_mut(hFile) else {
        log::warn!("ReadFile({hFile:?}): unknown handle");
        return false;
    };
    let buf = &mut ctx.memory[lpBuffer.addr..][..nNumberOfBytesToRead as usize];
    let mut total = 0;
    while total < buf.len() {
        match file.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(err) => {
                log::warn!("ReadFile({hFile:?}): {err}");
                return false;
            }
        }
    }
    drop(kernel32);
    if lpNumberOfBytesRead.addr != 0 {
        lpNumberOfBytesRead
            .write(&mut ctx.memory, total as u32)
            .unwrap();
    }
    true
}

#[win32_derive::dllexport]
pub fn WriteFile(
    ctx: &mut Context,
    hFile: u32,
    lpBuffer: Ptr<u8>,
    nNumberOfBytesToWrite: u32,
    lpNumberOfBytesWritten: Ptr<u32>,
    lpOverlapped: Ptr<()>,
) -> u32 {
    assert_eq!(lpOverlapped.addr, 0);
    if hFile == STDOUT_HFILE || hFile == STDERR_HFILE {
        let buf = &ctx.memory[lpBuffer.addr..][..nNumberOfBytesToWrite as usize];
        host::host().console_write(buf);
        if lpNumberOfBytesWritten.addr != 0 {
            lpNumberOfBytesWritten
                .write(&mut ctx.memory, nNumberOfBytesToWrite)
                .unwrap();
        }
        return 1;
    }

    let mut kernel32 = lock();
    let Some(Object::File(file)) = kernel32.objects.get_mut(crate::HANDLE::from_raw(hFile)) else {
        log::warn!("WriteFile({hFile:x}): unknown handle");
        return 0;
    };
    let buf = &ctx.memory[lpBuffer.addr..][..nNumberOfBytesToWrite as usize];
    match file.write_all(buf) {
        Ok(()) => {
            drop(kernel32);
            if lpNumberOfBytesWritten.addr != 0 {
                lpNumberOfBytesWritten
                    .write(&mut ctx.memory, nNumberOfBytesToWrite)
                    .unwrap();
            }
            1
        }
        Err(err) => {
            log::warn!("WriteFile({hFile:x}): {err}");
            0
        }
    }
}

#[win32_derive::dllexport]
pub fn SetFilePointer(
    ctx: &mut Context,
    hFile: crate::HANDLE,
    lDistanceToMove: i32,
    lpDistanceToMoveHigh: Ptr<i32>,
    dwMoveMethod: MoveMethod,
) -> u32 {
    let distance = if lpDistanceToMoveHigh.addr != 0 {
        let high = lpDistanceToMoveHigh.read(&ctx.memory).unwrap();
        ((high as i64) << 32) | (lDistanceToMove as u32 as i64)
    } else {
        lDistanceToMove as i64
    };
    let from = match dwMoveMethod {
        MoveMethod::FILE_BEGIN => SeekFrom::Start(distance as u64),
        MoveMethod::FILE_CURRENT => SeekFrom::Current(distance),
        MoveMethod::FILE_END => SeekFrom::End(distance),
    };
    let mut kernel32 = lock();
    let Some(Object::File(file)) = kernel32.objects.get_mut(hFile) else {
        log::warn!("SetFilePointer({hFile:?}): unknown handle");
        return INVALID_SET_FILE_POINTER;
    };
    match file.seek(from) {
        Ok(pos) => {
            drop(kernel32);
            if lpDistanceToMoveHigh.addr != 0 {
                lpDistanceToMoveHigh
                    .write(&mut ctx.memory, (pos >> 32) as i32)
                    .unwrap();
            }
            pos as u32
        }
        Err(err) => {
            log::warn!("SetFilePointer({hFile:?}): {err}");
            INVALID_SET_FILE_POINTER
        }
    }
}

#[win32_derive::dllexport]
pub fn SetEndOfFile(_ctx: &mut Context, hFile: crate::HANDLE) -> bool {
    let mut kernel32 = lock();
    let Some(Object::File(file)) = kernel32.objects.get_mut(hFile) else {
        log::warn!("SetEndOfFile({hFile:?}): unknown handle");
        return false;
    };
    let Ok(pos) = file.stream_position() else {
        return false;
    };
    file.set_len(pos).is_ok()
}

#[win32_derive::dllexport]
pub fn FlushFileBuffers(_ctx: &mut Context, hFile: crate::HANDLE) -> bool {
    let mut kernel32 = lock();
    let Some(Object::File(file)) = kernel32.objects.get_mut(hFile) else {
        return false;
    };
    file.flush().is_ok()
}

#[win32_derive::dllexport]
pub fn CloseHandle(_ctx: &mut Context, hObject: crate::HANDLE) -> bool {
    // Also called for handles we don't track (stdio etc.); succeed regardless.
    lock().objects.remove(hObject);
    true
}

#[win32_derive::dllexport]
pub fn DeleteFileA(ctx: &mut Context, lpFileName: Ptr<u8>) -> bool {
    let name = ctx.memory.read_str(lpFileName.addr).to_owned();
    host::fs::remove_file(&resolve_path(&name)).is_ok()
}

#[win32_derive::dllexport]
pub fn GetFileType(_ctx: &mut Context, hFile: u32) -> u32 /* FILE_TYPE */ {
    let FILE_TYPE_DISK = 0x1;
    let FILE_TYPE_CHAR = 0x2;
    let FILE_TYPE_UNKNOWN = 0x8;
    match hFile {
        STDIN_HFILE | STDOUT_HFILE | STDERR_HFILE => return FILE_TYPE_CHAR,
        _ => {}
    }
    if let Some(Object::File(_)) = lock().objects.get(crate::HANDLE::from_raw(hFile)) {
        return FILE_TYPE_DISK;
    }

    log::error!("GetFileType({hFile:?}) unknown handle");
    FILE_TYPE_UNKNOWN
}

#[win32_derive::dllexport]
pub fn SetHandleCount(_ctx: &mut Context, uNumber: u32) -> u32 {
    // "For Windows Win32 systems, this API has no effect."
    uNumber
}

#[win32_derive::dllexport]
pub fn GetCurrentDirectoryA(ctx: &mut Context, nBufferLength: u32, lpBuffer: Ptr<u8>) -> u32 {
    let cur = host::fs::current_dir().unwrap_or_default();
    let rel = cur
        .strip_prefix(initial_cwd())
        .unwrap_or(std::path::Path::new(""))
        .to_string_lossy()
        .replace('/', "\\");
    let path = if rel.is_empty() {
        "C:\\".to_string()
    } else {
        format!("C:\\{rel}")
    };
    let bytes = path.as_bytes();
    if (nBufferLength as usize) < bytes.len() + 1 {
        return bytes.len() as u32 + 1;
    }
    ctx.memory[lpBuffer.addr..][..bytes.len()].copy_from_slice(bytes);
    ctx.memory
        .write::<u8>(lpBuffer.addr + bytes.len() as u32, 0);
    bytes.len() as u32
}

#[win32_derive::dllexport]
pub fn SetCurrentDirectoryA(ctx: &mut Context, lpPathName: Ptr<u8>) -> bool {
    let name = ctx.memory.read_str(lpPathName.addr).to_owned();
    let path = resolve_path(&name);
    match host::fs::set_current_dir(&path) {
        Ok(()) => true,
        Err(err) => {
            log::warn!("SetCurrentDirectoryA({name:?} => {path:?}): {err}");
            false
        }
    }
}

/// State of an in-progress FindFirstFile/FindNextFile iteration.
pub struct FindHandle {
    pub entries: Vec<FindEntry>,
    pub index: usize,
}

pub struct FindEntry {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

#[repr(C)]
#[derive(zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct WIN32_FIND_DATAA {
    dwFileAttributes: u32,
    ftCreationTime: [u32; 2],
    ftLastAccessTime: [u32; 2],
    ftLastWriteTime: [u32; 2],
    nFileSizeHigh: u32,
    nFileSizeLow: u32,
    dwReserved0: u32,
    dwReserved1: u32,
    cFileName: [u8; 260],
    cAlternateFileName: [u8; 14],
    _pad: [u8; 2],
}

fn find_data(entry: &FindEntry) -> WIN32_FIND_DATAA {
    let mut cFileName = [0u8; 260];
    let name = entry.name.as_bytes();
    let len = name.len().min(259);
    cFileName[..len].copy_from_slice(&name[..len]);
    WIN32_FIND_DATAA {
        dwFileAttributes: if entry.is_dir {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_NORMAL
        },
        ftCreationTime: [0; 2],
        ftLastAccessTime: [0; 2],
        ftLastWriteTime: [0; 2],
        nFileSizeHigh: (entry.size >> 32) as u32,
        nFileSizeLow: entry.size as u32,
        dwReserved0: 0,
        dwReserved1: 0,
        cFileName,
        cAlternateFileName: [0; 14],
        _pad: [0; 2],
    }
}

/// Match a DOS wildcard pattern (* and ?) against a name, ignoring case.
/// Iterative rather than recursive: on a mismatch we only ever back up to the
/// most recent `*`, which keeps a pattern like `*a*a*a*b` linear instead of
/// exponential in the length of the name.
fn wildcard_match(pattern: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n) = (0, 0);
    // Where to resume from if the current guess for how much `*` ate fails.
    let mut star: Option<(usize, usize)> = None;
    loop {
        match (pattern.get(p), name.get(n)) {
            (Some(b'*'), _) => {
                p += 1;
                star = Some((p, n));
            }
            (Some(b'?'), Some(_)) => {
                p += 1;
                n += 1;
            }
            (Some(&pc), Some(&nc)) if pc.eq_ignore_ascii_case(&nc) => {
                p += 1;
                n += 1;
            }
            (None, None) => return true,
            _ => match star {
                // Let the last `*` eat one more character and try again.
                Some((star_p, star_n)) if star_n < name.len() => {
                    p = star_p;
                    n = star_n + 1;
                    star = Some((star_p, n));
                }
                _ => return false,
            },
        }
    }
}

#[win32_derive::dllexport]
pub fn FindFirstFileA(
    ctx: &mut Context,
    lpFileName: Ptr<u8>,
    lpFindFileData: Ptr<WIN32_FIND_DATAA>,
) -> crate::HANDLE {
    let pattern = ctx.memory.read_str(lpFileName.addr).to_owned();
    let pattern = pattern.replace('\\', "/");
    let (dir, file_pattern) = match pattern.rfind('/') {
        Some(pos) => (&pattern[..pos], &pattern[pos + 1..]),
        None => (".", &pattern[..]),
    };
    let dir = resolve_path(dir);
    let mut entries = Vec::new();
    if let Ok(dir_entries) = host::fs::read_dir(&dir) {
        for entry in dir_entries {
            if !wildcard_match(file_pattern.as_bytes(), entry.name.as_bytes()) {
                continue;
            }
            entries.push(FindEntry {
                name: entry.name,
                size: entry.len,
                is_dir: entry.is_dir,
            });
        }
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    if entries.is_empty() {
        return crate::HANDLE::invalid();
    }
    lpFindFileData
        .write(&mut ctx.memory, find_data(&entries[0]))
        .unwrap();
    lock()
        .objects
        .add(Object::FindHandle(FindHandle { entries, index: 1 }))
}

#[win32_derive::dllexport]
pub fn FindNextFileA(
    ctx: &mut Context,
    hFindFile: crate::HANDLE,
    lpFindFileData: Ptr<WIN32_FIND_DATAA>,
) -> bool {
    let mut kernel32 = lock();
    let Some(Object::FindHandle(find)) = kernel32.objects.get_mut(hFindFile) else {
        log::warn!("FindNextFileA({hFindFile:?}): unknown handle");
        return false;
    };
    let Some(entry) = find.entries.get(find.index) else {
        return false;
    };
    let data = find_data(entry);
    find.index += 1;
    drop(kernel32);
    lpFindFileData.write(&mut ctx.memory, data).unwrap();
    true
}

#[win32_derive::dllexport]
pub fn FindClose(_ctx: &mut Context, hFindFile: crate::HANDLE) -> bool {
    lock().objects.remove(hFindFile).is_some()
}

#[cfg(test)]
mod tests {
    use super::wildcard_match;

    #[test]
    fn wildcards() {
        let cases: &[(&str, &str, bool)] = &[
            ("*", "anything", true),
            ("*.txt", "notes.txt", true),
            ("*.txt", "notes.txtx", false),
            ("*.*", "a.b", true),
            ("*.*", "noext", false),
            ("a?c", "abc", true),
            ("a?c", "ac", false),
            ("ABC", "abc", true), // case insensitive
            ("a*b*c", "axxbyyc", true),
            ("a*b*c", "axxbyy", false),
            ("**", "x", true),
            ("", "", true),
            ("", "x", false),
            ("x", "", false),
            ("*x", "x", true),
            // Pathological for a backtracking matcher.
            (
                "*a*a*a*a*a*a*a*b",
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                false,
            ),
        ];
        for &(pattern, name, want) in cases {
            let got = wildcard_match(pattern.as_bytes(), name.as_bytes());
            assert_eq!(got, want, "{pattern:?} vs {name:?}");
        }
    }
}

#[win32_derive::dllexport]
pub fn GetFileSize(ctx: &mut Context, hFile: crate::HANDLE, lpFileSizeHigh: Ptr<u32>) -> u32 {
    const INVALID_FILE_SIZE: u32 = 0xffff_ffff;
    let mut kernel32 = lock();
    let Some(Object::File(file)) = kernel32.objects.get_mut(hFile) else {
        log::warn!("GetFileSize({hFile:?}): unknown handle");
        return INVALID_FILE_SIZE;
    };
    let Ok(pos) = file.stream_position() else {
        return INVALID_FILE_SIZE;
    };
    let Ok(len) = file.seek(SeekFrom::End(0)) else {
        return INVALID_FILE_SIZE;
    };
    let _ = file.seek(SeekFrom::Start(pos));
    drop(kernel32);
    if lpFileSizeHigh.addr != 0 {
        lpFileSizeHigh.write(&mut ctx.memory, (len >> 32) as u32);
    }
    len as u32
}

#[win32_derive::dllexport]
pub fn GetFileTime(
    ctx: &mut Context,
    _hFile: crate::HANDLE,
    lpCreationTime: Ptr<u64>,
    lpLastAccessTime: Ptr<u64>,
    lpLastWriteTime: Ptr<u64>,
) -> bool {
    // FILETIME of 0 is Jan 1 1601, which programs treat as "unknown".
    for time in [lpCreationTime, lpLastAccessTime, lpLastWriteTime] {
        if time.addr != 0 {
            time.write(&mut ctx.memory, 0);
        }
    }
    true
}

#[win32_derive::dllexport]
pub fn GetFullPathNameA(
    ctx: &mut Context,
    lpFileName: Ptr<u8>,
    nBufferLength: u32,
    lpBuffer: Ptr<u8>,
    lpFilePart: Ptr<u32>,
) -> u32 {
    let name = ctx.memory.read_str(lpFileName.addr).to_string();
    let full = if name.len() >= 2 && name.as_bytes()[1] == b':' {
        name
    } else if name.starts_with('\\') {
        format!("C:{name}")
    } else {
        // Relative to the current directory, which GetCurrentDirectoryA reports
        // under C:. It needs a buffer in x86 memory; borrow one from the heap.
        let tmp = lock().process_heap.alloc(&mut ctx.memory, 260);
        GetCurrentDirectoryA(ctx, 260, Ptr::new(tmp));
        let cur_str = ctx.memory.read_str(tmp).to_string();
        lock().process_heap.free(&mut ctx.memory, tmp);
        if cur_str.ends_with('\\') {
            format!("{cur_str}{name}")
        } else {
            format!("{cur_str}\\{name}")
        }
    };
    if (nBufferLength as usize) < full.len() + 1 {
        return full.len() as u32 + 1;
    }
    let buf_addr = lpBuffer.addr;
    crate::kernel32::write_cstr(ctx, lpBuffer, nBufferLength, full.as_bytes());
    if lpFilePart.addr != 0 {
        let last = full.rfind('\\').map_or(0, |i| i + 1);
        lpFilePart.write(&mut ctx.memory, buf_addr + last as u32);
    }
    full.len() as u32
}

#[win32_derive::dllexport]
pub fn LockFile(
    _ctx: &mut Context,
    _hFile: crate::HANDLE,
    _dwFileOffsetLow: u32,
    _dwFileOffsetHigh: u32,
    _nNumberOfBytesToLockLow: u32,
    _nNumberOfBytesToLockHigh: u32,
) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn UnlockFile(
    _ctx: &mut Context,
    _hFile: crate::HANDLE,
    _dwFileOffsetLow: u32,
    _dwFileOffsetHigh: u32,
    _nNumberOfBytesToUnlockLow: u32,
    _nNumberOfBytesToUnlockHigh: u32,
) -> bool {
    true
}

/// The drive letter we present as a CD-ROM. Every drive maps onto the same
/// host directory (see `resolve_path`), so a CD-based game finds its data
/// there regardless of which drive it thinks it is reading.
const CDROM_DRIVE: u8 = b'D';

fn is_cdrom_path(path: &str) -> bool {
    path.as_bytes()
        .first()
        .is_some_and(|c| c.to_ascii_uppercase() == CDROM_DRIVE)
}

#[win32_derive::dllexport]
pub fn GetLogicalDrives(_ctx: &mut Context) -> u32 {
    (1 << (b'C' - b'A')) | (1 << (CDROM_DRIVE - b'A'))
}

#[win32_derive::dllexport]
pub fn GetDriveTypeA(ctx: &mut Context, lpRootPathName: Ptr<u8>) -> u32 {
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_CDROM: u32 = 5;
    if lpRootPathName.addr == 0 {
        return DRIVE_FIXED;
    }
    let path = ctx.memory.read_str(lpRootPathName.addr);
    if is_cdrom_path(&path) {
        DRIVE_CDROM
    } else {
        DRIVE_FIXED
    }
}

/// Volume label reported for the CD-ROM drive. Games check this to verify
/// their CD is inserted; set THESEUS_CD_LABEL to the label the game expects.
fn cdrom_label() -> String {
    #[cfg(not(target_family = "wasm"))]
    if let Ok(label) = std::env::var("THESEUS_CD_LABEL") {
        return label;
    }
    "CDROM".to_string()
}

#[win32_derive::dllexport]
pub fn GetVolumeInformationA(
    ctx: &mut Context,
    lpRootPathName: Ptr<u8>,
    lpVolumeNameBuffer: Ptr<u8>,
    nVolumeNameSize: u32,
    lpVolumeSerialNumber: Ptr<u32>,
    lpMaximumComponentLength: Ptr<u32>,
    lpFileSystemFlags: Ptr<u32>,
    lpFileSystemNameBuffer: Ptr<u8>,
    nFileSystemNameSize: u32,
) -> bool {
    let cdrom = lpRootPathName.addr != 0 && is_cdrom_path(&ctx.memory.read_str(lpRootPathName.addr));
    let (label, fs) = if cdrom {
        (cdrom_label(), "CDFS")
    } else {
        ("THESEUS".to_string(), "FAT")
    };
    crate::kernel32::write_cstr(ctx, lpVolumeNameBuffer, nVolumeNameSize, label.as_bytes());
    crate::kernel32::write_cstr(ctx, lpFileSystemNameBuffer, nFileSystemNameSize, fs.as_bytes());
    if lpVolumeSerialNumber.addr != 0 {
        lpVolumeSerialNumber.write(&mut ctx.memory, 0x1234_5678);
    }
    if lpMaximumComponentLength.addr != 0 {
        lpMaximumComponentLength.write(&mut ctx.memory, 255);
    }
    if lpFileSystemFlags.addr != 0 {
        lpFileSystemFlags.write(&mut ctx.memory, 0);
    }
    true
}
