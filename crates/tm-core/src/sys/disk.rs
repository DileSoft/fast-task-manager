//! Модели физических дисков через `IOCTL_STORAGE_QUERY_PROPERTY`.
//!
//! Дескриптор устройства отдаёт строку `ProductId` («Samsung SSD 860 EVO»,
//! «WDC WD10EZEX-08WN4A0»). Открываем `\\.\PhysicalDriveN` без прав доступа —
//! запрос свойств хранения доступен обычному пользователю. WMI не нужен.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Ioctl::{
    StorageDeviceProperty, IOCTL_STORAGE_QUERY_PROPERTY, STORAGE_DEVICE_DESCRIPTOR,
    STORAGE_PROPERTY_QUERY,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

use super::wide;

/// Модель физического диска по его номеру (`0`, `1`, …).
pub fn model(number: u32) -> Option<String> {
    let path = wide(&format!(r"\\.\PhysicalDrive{number}"));
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }

    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: 0,
        AdditionalParameters: [0],
    };
    let mut buf = [0u8; 1024];
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            &query as *const _ as *const c_void,
            std::mem::size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            buf.as_mut_ptr() as *mut c_void,
            buf.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    unsafe {
        CloseHandle(handle);
    }
    if ok == 0 || returned < std::mem::size_of::<STORAGE_DEVICE_DESCRIPTOR>() as u32 {
        return None;
    }

    let desc = unsafe { &*(buf.as_ptr() as *const STORAGE_DEVICE_DESCRIPTOR) };
    let offset = desc.ProductIdOffset as usize;
    if offset == 0 || offset >= buf.len() {
        return None;
    }
    let tail = &buf[offset..];
    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
    let text = String::from_utf8_lossy(&tail[..end]).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}
