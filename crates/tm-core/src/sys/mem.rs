//! Системная память: GlobalMemoryStatusEx.
//!
//! GetPerformanceInfo сознательно не используется: он обходит все процессы
//! и на машинах с 500+ процессами стоит ~10 мс на вызов, что для тика раз
//! в секунду неприемлемо. Кэш, пулы и коммит берутся из PDH-счётчиков.

use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetDriveTypeW};
use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

use super::wide;

const DRIVE_FIXED: u32 = 3;

#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryInfo {
    pub total: u64,
    pub avail: u64,
    pub commit_total: u64,
    pub commit_limit: u64,
    pub cached: u64,
    pub paged_pool: u64,
    pub nonpaged_pool: u64,
}

pub fn collect() -> MemoryInfo {
    let mut info = MemoryInfo::default();
    let mut ms: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    ms.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut ms) } != 0 {
        info.total = ms.ullTotalPhys;
        info.avail = ms.ullAvailPhys;
        info.commit_total = ms.ullTotalPageFile.saturating_sub(ms.ullAvailPageFile);
        info.commit_limit = ms.ullTotalPageFile;
    }
    info
}

/// Ёмкость и свободное место одного тома («C:»).
pub fn volume_space(volume: &str) -> Option<(u64, u64)> {
    let letter = volume.trim_end_matches(':');
    if letter.len() != 1 {
        return None;
    }
    let root = format!("{letter}:\\");
    let wroot = wide(&root);
    if unsafe { GetDriveTypeW(wroot.as_ptr()) } != DRIVE_FIXED {
        return None;
    }
    let mut avail = 0u64;
    let mut total = 0u64;
    let mut free = 0u64;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wroot.as_ptr(),
            &mut avail as *mut u64,
            &mut total as *mut u64,
            &mut free as *mut u64,
        )
    };
    if ok != 0 && total > 0 {
        Some((total, free))
    } else {
        None
    }
}

/// Суммарная ёмкость и свободное место на фиксированных дисках.
/// Вызывать не чаще раза в несколько секунд.
pub fn disk_space() -> (u64, u64, u32) {
    let mut total = 0u64;
    let mut free = 0u64;
    let mut count = 0u32;
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        let wroot = wide(&root);
        if unsafe { GetDriveTypeW(wroot.as_ptr()) } != DRIVE_FIXED {
            continue;
        }
        let mut avail = 0u64;
        let mut total_bytes = 0u64;
        let mut free_bytes = 0u64;
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                wroot.as_ptr(),
                &mut avail as *mut u64,
                &mut total_bytes as *mut u64,
                &mut free_bytes as *mut u64,
            )
        };
        if ok != 0 && total_bytes > 0 {
            total += total_bytes;
            free += free_bytes;
            count += 1;
        }
    }
    (total, free, count)
}
