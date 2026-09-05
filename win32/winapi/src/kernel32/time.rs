use runtime::Context;

use crate::Ptr;

#[repr(C)]
#[derive(Debug, Default, zerocopy::IntoBytes, zerocopy::Immutable)]
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

fn current_system_time() -> SYSTEMTIME {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = now.as_secs() as i64;
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
        wMilliseconds: (now.subsec_millis()) as u16,
    }
}

#[win32_derive::dllexport]
pub fn GetLocalTime(ctx: &mut Context, lpSystemTime: Ptr<SYSTEMTIME>) {
    let _ = lpSystemTime.write(&mut ctx.memory, current_system_time());
}

#[win32_derive::dllexport]
pub fn GetSystemTime(ctx: &mut Context, lpSystemTime: Ptr<SYSTEMTIME>) {
    let _ = lpSystemTime.write(&mut ctx.memory, current_system_time());
}

#[win32_derive::dllexport]
pub fn GetTickCount(_ctx: &mut Context) -> u32 {
    host::host().time()
}

#[win32_derive::dllexport]
pub fn QueryPerformanceCounter(ctx: &mut Context, lpPerformanceCount: crate::Ptr<u64>) -> bool {
    lpPerformanceCount
        .write(&mut ctx.memory, host::host().time() as u64)
        .is_some()
}

#[win32_derive::dllexport]
pub fn QueryPerformanceFrequency(ctx: &mut Context, lpFrequency: crate::Ptr<u64>) -> bool {
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
    ctx.memory[lpTimeZoneInformation.addr..][..172].fill(0);
    0 // TIME_ZONE_ID_UNKNOWN
}

#[win32_derive::dllexport]
pub fn FileTimeToLocalFileTime(
    ctx: &mut Context,
    lpFileTime: crate::Ptr<u64>,
    lpLocalFileTime: crate::Ptr<u64>,
) -> bool {
    let time = lpFileTime.read(&ctx.memory).unwrap_or(0);
    lpLocalFileTime.write(&mut ctx.memory, time).is_some()
}

#[win32_derive::dllexport]
pub fn FileTimeToSystemTime(
    ctx: &mut Context,
    _lpFileTime: crate::Ptr<u64>,
    lpSystemTime: crate::Ptr<u8>,
) -> bool {
    // SYSTEMTIME is 16 bytes; the game only shows these values incidentally.
    ctx.memory[lpSystemTime.addr..][..16].fill(0);
    true
}

#[cfg(test)]
mod tests {
    use super::SYSTEMTIME;

    #[test]
    fn system_time_matches_win32_abi() {
        assert_eq!(std::mem::size_of::<SYSTEMTIME>(), 16);
    }
}
