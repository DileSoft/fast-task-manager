//! Обёртка над PDH: системные счётчики CPU/диска/памяти/ГП.
//!
//! Используется `PdhAddEnglishCounterW`, поэтому имена счётчиков не зависят
//! от языка Windows.

use std::ffi::c_void;

use windows_sys::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE,
    PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

use super::wide;

pub struct Counter {
    h: PDH_HCOUNTER,
}

pub struct Query {
    h: PDH_HQUERY,
    counters: Vec<Counter>,
}

// PDH-хендлы используются только в одном потоке (сэмплере).
unsafe impl Send for Query {}

impl Query {
    pub fn new() -> Option<Self> {
        let mut h: PDH_HQUERY = std::ptr::null_mut();
        let rc = unsafe { PdhOpenQueryW(std::ptr::null(), 0, &mut h) };
        if rc != 0 || h.is_null() {
            return None;
        }
        Some(Self {
            h,
            counters: Vec::new(),
        })
    }

    /// Добавляет счётчик по английскому пути; `None`, если счётчик недоступен.
    pub fn add(&mut self, path: &str) -> Option<usize> {
        let w = wide(path);
        let mut h: PDH_HCOUNTER = std::ptr::null_mut();
        let rc = unsafe { PdhAddEnglishCounterW(self.h, w.as_ptr(), 0, &mut h) };
        if rc != 0 || h.is_null() {
            return None;
        }
        self.counters.push(Counter { h });
        Some(self.counters.len() - 1)
    }

    pub fn collect(&self) -> bool {
        unsafe { PdhCollectQueryData(self.h) == 0 }
    }

    pub fn value(&self, idx: Option<usize>) -> Option<f64> {
        let idx = idx?;
        let c = self.counters.get(idx)?;
        let mut val = PDH_FMT_COUNTERVALUE::default();
        let rc = unsafe {
            PdhGetFormattedCounterValue(c.h, PDH_FMT_DOUBLE, std::ptr::null_mut(), &mut val)
        };
        if rc != 0 {
            return None;
        }
        if val.CStatus != 0 && val.CStatus != 1 {
            return None;
        }
        Some(unsafe { val.Anonymous.doubleValue })
    }

    /// Значения счётчика с подстановкой `*` в имени экземпляра.
    pub fn array(&self, idx: Option<usize>) -> Vec<(String, f64)> {
        let Some(idx) = idx else {
            return Vec::new();
        };
        let Some(c) = self.counters.get(idx) else {
            return Vec::new();
        };

        let mut size = 0u32;
        let mut count = 0u32;
        let rc = unsafe {
            PdhGetFormattedCounterArrayW(
                c.h,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                std::ptr::null_mut(),
            )
        };
        if rc != PDH_MORE_DATA || size == 0 {
            return Vec::new();
        }

        // Буфер выровнен под PDH_FMT_COUNTERVALUE_ITEM_W.
        let words = (size as usize + 7) / 8 + 1;
        let mut buf = vec![0u64; words];
        let mut size2 = (words * 8) as u32;
        let mut count2 = count;
        let rc = unsafe {
            PdhGetFormattedCounterArrayW(
                c.h,
                PDH_FMT_DOUBLE,
                &mut size2,
                &mut count2,
                buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W,
            )
        };
        if rc != 0 {
            return Vec::new();
        }

        let item_size = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
        let base = buf.as_ptr() as *const u8;
        let mut out = Vec::with_capacity(count2 as usize);
        for i in 0..count2 as usize {
            let item = unsafe { &*(base.add(i * item_size) as *const PDH_FMT_COUNTERVALUE_ITEM_W) };
            if item.FmtValue.CStatus != 0 && item.FmtValue.CStatus != 1 {
                continue;
            }
            let name = unsafe { pwstr_to_string(item.szName) };
            let value = unsafe { item.FmtValue.Anonymous.doubleValue };
            out.push((name, value));
        }
        out
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        if !self.h.is_null() {
            unsafe {
                PdhCloseQuery(self.h);
            }
        }
    }
}

unsafe fn pwstr_to_string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
        if len > 512 {
            break;
        }
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

/// Утилита: разбирает имя экземпляра вида
/// `pid_1234_luid_0x..._phys_0_eng_0_engtype_3D` и возвращает PID.
pub fn instance_pid(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("pid_")?;
    let end = rest.find('_').unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Второй компонент имени экземпляра, например `engtype_3D`.
pub fn instance_engine_type(name: &str) -> &str {
    name.rsplit("engtype_").next().unwrap_or("")
}

#[allow(dead_code)]
fn _assert_void(_: *mut c_void) {}
