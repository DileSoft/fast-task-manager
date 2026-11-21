//! Чтение значений из реестра (без WMI).

use windows_sys::Win32::System::Registry::{
    RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
};

use super::wide;

pub fn read_string(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            root as _,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as *mut _,
            &mut size,
        )
    };
    if rc != 0 {
        return None;
    }
    let chars = (size as usize / 2).min(buf.len());
    let end = buf[..chars].iter().position(|&c| c == 0).unwrap_or(chars);
    Some(String::from_utf16_lossy(&buf[..end]))
}

pub fn read_u32(root: HKEY, subkey: &str, value: &str) -> Option<u32> {
    let subkey = wide(subkey);
    let value = wide(value);
    let mut data = 0u32;
    let mut size = 4u32;
    let rc = unsafe {
        RegGetValueW(
            root as _,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    if rc != 0 {
        None
    } else {
        Some(data)
    }
}

/// Название процессора из реестра.
pub fn cpu_name() -> String {
    read_string(
        HKEY_LOCAL_MACHINE,
        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        "ProcessorNameString",
    )
    .map(|s| s.trim().to_string())
    .unwrap_or_else(|| "Процессор".to_string())
}

/// Номинальная частота процессора, МГц.
pub fn cpu_base_mhz() -> u32 {
    read_u32(
        HKEY_LOCAL_MACHINE,
        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
        "~MHz",
    )
    .unwrap_or(0)
}

/// Имя видеокарты из драйверного ключа.
pub fn gpu_name() -> Option<String> {
    let base = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";
    for i in 0..8 {
        let sub = format!(r"{base}\{i:04}");
        if let Some(name) = read_string(HKEY_LOCAL_MACHINE, &sub, "DriverDesc") {
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Windows-тема приложений: true — светлая.
pub fn apps_use_light_theme() -> bool {
    read_u32(
        HKEY_CURRENT_USER,
        r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        "AppsUseLightTheme",
    )
    .unwrap_or(0)
        != 0
}
