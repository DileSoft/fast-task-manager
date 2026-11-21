//! Тонкие обёртки над Win32/NT API. Всё, что дороже кэша, вызывается снаружи.

pub mod actions;
pub mod disk;
pub mod identity;
pub mod mem;
pub mod net;
pub mod nt;
pub mod pdh;
pub mod registry;
pub mod window;

/// Строка в UTF-16 с завершающим нулём.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Копирует wide-строку из буфера фиксированной длины, обрезая по нулю.
pub fn from_wide_buf(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Копирует wide-строку по указателю.
///
/// # Safety
///
/// `ptr` должен указывать на `len` валидных элементов `u16`.
pub unsafe fn from_wide_ptr(ptr: *const u16, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf16_lossy(slice)
}

pub fn last_error() -> u32 {
    unsafe { windows_sys::Win32::Foundation::GetLastError() }
}
