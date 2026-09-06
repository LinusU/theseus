use runtime::Context;

use crate::Ptr;

#[repr(C)]
#[derive(Debug, Default, zerocopy::FromBytes, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}

/// The civil-from-days conversion (Howard Hinnant's algorithm) shared by
/// GetSystemTime/GetLocalTime and FileTimeToSystemTime.
fn civil_from_unix(seconds: i64, millis: u16) -> SYSTEMTIME {
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }).div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096).div_euclid(365);
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_part = (5 * doy + 2).div_euclid(153);
    let day = doy - (153 * month_part + 2).div_euclid(5) + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    SYSTEMTIME {
        wYear: year as u16,
        wMonth: month as u16,
        wDayOfWeek: (days + 4).rem_euclid(7) as u16,
        wDay: day as u16,
        wHour: (day_seconds / 3_600) as u16,
        wMinute: (day_seconds / 60 % 60) as u16,
        wSecond: (day_seconds % 60) as u16,
        wMilliseconds: millis,
    }
}

fn current_system_time() -> SYSTEMTIME {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    civil_from_unix(now.as_secs() as i64, now.subsec_millis() as u16)
}

fn get_time(ctx: &mut Context, lpSystemTime: Ptr<SYSTEMTIME>) {
    if !crate::ddraw::guest_range(
        ctx,
        lpSystemTime.addr,
        std::mem::size_of::<SYSTEMTIME>() as u32,
    ) {
        return;
    }
    let _ = lpSystemTime.write(&mut ctx.memory, current_system_time());
}

#[win32_derive::dllexport]
pub fn GetLocalTime(ctx: &mut Context, lpSystemTime: Ptr<SYSTEMTIME>) {
    get_time(ctx, lpSystemTime);
}

#[win32_derive::dllexport]
pub fn GetSystemTime(ctx: &mut Context, lpSystemTime: Ptr<SYSTEMTIME>) {
    get_time(ctx, lpSystemTime);
}

#[win32_derive::dllexport]
pub fn GetTickCount(_ctx: &mut Context) -> u32 {
    host::host().time()
}

#[win32_derive::dllexport]
pub fn QueryPerformanceCounter(ctx: &mut Context, lpPerformanceCount: crate::Ptr<u64>) -> bool {
    if !crate::ddraw::guest_range(
        ctx,
        lpPerformanceCount.addr,
        std::mem::size_of::<u64>() as u32,
    ) {
        return false;
    }
    lpPerformanceCount
        .write(&mut ctx.memory, host::host().time() as u64)
        .is_some()
}

#[win32_derive::dllexport]
pub fn QueryPerformanceFrequency(ctx: &mut Context, lpFrequency: crate::Ptr<u64>) -> bool {
    if !crate::ddraw::guest_range(ctx, lpFrequency.addr, std::mem::size_of::<u64>() as u32) {
        return false;
    }
    lpFrequency.write(&mut ctx.memory, 1_000).is_some()
}

#[win32_derive::dllexport]
pub fn Sleep(_ctx: &mut Context, dwMilliseconds: u32) {
    std::thread::sleep(std::time::Duration::from_millis(dwMilliseconds as u64));
}

#[win32_derive::dllexport]
pub fn GetTimeZoneInformation(ctx: &mut Context, lpTimeZoneInformation: crate::Ptr<u8>) -> u32 /* TIME_ZONE_ID */
{
    // TIME_ZONE_INFORMATION is 172 bytes; report UTC by zeroing it.
    let Some(buf) = ctx
        .memory
        .bytes
        .get_mut(lpTimeZoneInformation.addr as usize..)
        .and_then(|buf| buf.get_mut(..172))
    else {
        return u32::MAX; // TIME_ZONE_ID_INVALID
    };
    buf.fill(0);
    0 // TIME_ZONE_ID_UNKNOWN
}

#[win32_derive::dllexport]
pub fn FileTimeToLocalFileTime(
    ctx: &mut Context,
    lpFileTime: crate::Ptr<u64>,
    lpLocalFileTime: crate::Ptr<u64>,
) -> bool {
    let Some(time) = lpFileTime.read(&ctx.memory) else {
        return false;
    };
    lpLocalFileTime.write(&mut ctx.memory, time).is_some()
}

#[win32_derive::dllexport]
pub fn FileTimeToSystemTime(
    ctx: &mut Context,
    lpFileTime: crate::Ptr<u64>,
    lpSystemTime: crate::Ptr<SYSTEMTIME>,
) -> bool {
    let Some(filetime) = lpFileTime.read(&ctx.memory) else {
        return false;
    };
    // A FILETIME counts 100ns ticks since 1601-01-01, which is
    // 11644473600 seconds before the unix epoch.
    let seconds = (filetime / 10_000_000) as i64 - 11_644_473_600;
    let millis = ((filetime / 10_000) % 1_000) as u16;
    lpSystemTime
        .write(&mut ctx.memory, civil_from_unix(seconds, millis))
        .is_some()
}

#[cfg(test)]
mod tests {
    use super::{
        FileTimeToLocalFileTime, FileTimeToSystemTime, GetLocalTime, GetSystemTime,
        QueryPerformanceCounter, QueryPerformanceFrequency, SYSTEMTIME,
    };
    use crate::Ptr;
    use runtime::{BlockCache, CPU, Context, Memory};

    fn context() -> Context {
        Context {
            cpu: CPU::default(),
            thread_handle: 0,
            thread_id: 0,
            memory: Memory::leak_new(0x4000),
            blocks: &[],
            cache: BlockCache::default(),
            recent: [Context::return_from_x86; 4],
        }
    }

    #[test]
    fn system_time_matches_win32_abi() {
        assert_eq!(std::mem::size_of::<SYSTEMTIME>(), 16);
    }

    #[test]
    fn get_local_and_system_time_reject_bad_output_pointers() {
        let mut ctx = context();
        // Low and far out-of-range output pointers are rejected without panic.
        GetLocalTime(&mut ctx, Ptr::new(0x500));
        GetSystemTime(&mut ctx, Ptr::new(0xffff_fff0));
        // A valid pointer writes a plausible year (wYear at offset 0).
        GetLocalTime(&mut ctx, Ptr::new(0x1000));
        let year = ctx.memory.read::<u16>(0x1000);
        assert!((2000..3000).contains(&year));
        GetSystemTime(&mut ctx, Ptr::new(0x2000));
        let year = ctx.memory.read::<u16>(0x2000);
        assert!((2000..3000).contains(&year));
    }

    #[test]
    fn query_performance_pointers_reject_bad_output_pointers() {
        let mut ctx = context();
        // Low and far out-of-range output pointers return false without panic.
        assert!(!QueryPerformanceCounter(&mut ctx, Ptr::new(0x500)));
        assert!(!QueryPerformanceFrequency(&mut ctx, Ptr::new(0xffff_fff0)));
        // Valid pointers write the expected values.
        assert!(QueryPerformanceCounter(&mut ctx, Ptr::new(0x1000)));
        assert!(ctx.memory.read::<u64>(0x1000) > 0);
        assert!(QueryPerformanceFrequency(&mut ctx, Ptr::new(0x2000)));
        assert_eq!(ctx.memory.read::<u64>(0x2000), 1_000);
    }

    #[test]
    fn filetime_to_system_time_converts_known_dates() {
        let mut ctx = context();
        // 2000-01-01 00:00:00 UTC, a Saturday.
        ctx.memory.write::<u64>(0x1000, 125_911_584_000_000_000);
        assert!(FileTimeToSystemTime(
            &mut ctx,
            Ptr::new(0x1000),
            Ptr::new(0x2000)
        ));
        let st = ctx.memory.read::<SYSTEMTIME>(0x2000);
        assert_eq!(
            (st.wYear, st.wMonth, st.wDay, st.wDayOfWeek),
            (2000, 1, 1, 6)
        );

        // FILETIME 0 is 1601-01-01, a Monday.
        ctx.memory.write::<u64>(0x1000, 0);
        assert!(FileTimeToSystemTime(
            &mut ctx,
            Ptr::new(0x1000),
            Ptr::new(0x2000)
        ));
        let st = ctx.memory.read::<SYSTEMTIME>(0x2000);
        assert_eq!(
            (st.wYear, st.wMonth, st.wDay, st.wDayOfWeek),
            (1601, 1, 1, 1)
        );

        // An unreadable input pointer fails without writing.
        assert!(!FileTimeToSystemTime(
            &mut ctx,
            Ptr::new(0xffff_ff00),
            Ptr::new(0x2000)
        ));
    }

    #[test]
    fn filetime_to_local_file_time_copies_and_rejects_bad_pointers() {
        let mut ctx = context();
        ctx.memory.write::<u64>(0x1000, 125_911_584_000_000_000);

        assert!(FileTimeToLocalFileTime(
            &mut ctx,
            Ptr::new(0x1000),
            Ptr::new(0x2000)
        ));
        assert_eq!(ctx.memory.read::<u64>(0x2000), 125_911_584_000_000_000);

        // Identity model: local == UTC, so zero stays zero.
        ctx.memory.write::<u64>(0x1000, 0);
        assert!(FileTimeToLocalFileTime(
            &mut ctx,
            Ptr::new(0x1000),
            Ptr::new(0x2000)
        ));
        assert_eq!(ctx.memory.read::<u64>(0x2000), 0);

        // Bad input or output pointers fail without writing.
        assert!(!FileTimeToLocalFileTime(
            &mut ctx,
            Ptr::new(0xffff_ff00),
            Ptr::new(0x2000)
        ));
        assert!(!FileTimeToLocalFileTime(
            &mut ctx,
            Ptr::new(0x1000),
            Ptr::new(0xffff_ff00)
        ));
    }
}
