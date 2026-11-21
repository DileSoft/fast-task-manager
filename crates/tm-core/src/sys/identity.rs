//! Идентификация процесса: путь, описание, владелец, повышение прав.
//! Все вызовы дорогие — вызываются только при первом появлении процесса.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, LookupAccountSidW, TokenElevation, TokenUser, SID_NAME_USE,
    TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::wide;

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;

#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(
        process_handle: HANDLE,
        info_class: u32,
        info: *mut c_void,
        info_length: u32,
        return_length: *mut u32,
    ) -> i32;
}

/// Командная строка процесса через `ProcessCommandLineInformation`
/// (Windows 8.1+). Работает без чтения памяти другого процесса.
pub fn command_line(pid: u32) -> Option<String> {
    if pid <= 4 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }

    let mut needed = 0u32;
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    let size = (needed as usize + 128).max(4096);
    let mut buf = vec![0u8; size];
    let mut ret = 0u32;
    let status2 = unsafe {
        NtQueryInformationProcess(
            handle,
            PROCESS_COMMAND_LINE_INFORMATION,
            buf.as_mut_ptr() as *mut c_void,
            size as u32,
            &mut ret,
        )
    };
    unsafe {
        CloseHandle(handle);
    }
    if status2 < 0 {
        return None;
    }
    let _ = status;

    let unicode = unsafe { &*(buf.as_ptr() as *const UnicodeString) };
    let len_bytes = unicode.length as usize;
    if len_bytes == 0 || unicode.buffer.is_null() {
        return None;
    }
    let base = buf.as_ptr() as usize;
    let ptr = unicode.buffer as usize;
    if ptr < base || ptr.saturating_add(len_bytes) > base + size {
        return None;
    }
    let offset = ptr - base;
    let bytes = &buf[offset..offset + len_bytes];
    let wide: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let text = String::from_utf16_lossy(&wide).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Identity {
    pub exe_path: Option<String>,
    pub description: Option<String>,
    pub user: Option<String>,
    pub elevated: bool,
}

pub fn resolve(pid: u32) -> Identity {
    let mut id = Identity::default();
    if pid == 0 || pid == 4 {
        // System Idle Process / System — доступ закрыт, как и в оригинале.
        id.user = Some("SYSTEM".to_string());
        return id;
    }

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return id;
    }

    // Путь.
    let mut buf = vec![0u16; 32768];
    let mut len = buf.len() as u32;
    if unsafe { QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) }
        != 0
    {
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        if !path.is_empty() {
            id.description = file_description(&path);
            id.exe_path = Some(path);
        }
    }

    // Пользователь и повышение прав.
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe { OpenProcessToken(handle, TOKEN_QUERY, &mut token) } != 0 {
        id.user = token_user(token);
        id.elevated = token_elevated(token);
        unsafe {
            CloseHandle(token);
        }
    }

    unsafe {
        CloseHandle(handle);
    }
    id
}

fn file_description(path: &str) -> Option<String> {
    let wpath = wide(path);
    let mut dummy = 0u32;
    let size = unsafe { GetFileVersionInfoSizeW(wpath.as_ptr(), &mut dummy) };
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    if unsafe { GetFileVersionInfoW(wpath.as_ptr(), 0, size, buf.as_mut_ptr() as *mut c_void) } == 0
    {
        return None;
    }

    let mut desc = query_version_string(&buf, "FileDescription");
    if desc.as_deref().is_none_or(|s| s.trim().is_empty()) {
        desc = query_version_string(&buf, "ProductName");
    }
    desc.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn query_version_string(buf: &[u8], key: &str) -> Option<String> {
    // Таблица переводов даёт нужный язык; иначе — стандартный 0409.
    let mut translations: Vec<(u16, u16)> = Vec::new();
    let mut ptr: *mut c_void = std::ptr::null_mut();
    let mut len = 0u32;
    let sub = wide(r"\VarFileInfo\Translation");
    if unsafe {
        VerQueryValueW(
            buf.as_ptr() as *const c_void,
            sub.as_ptr(),
            &mut ptr,
            &mut len,
        )
    } != 0
        && len >= 4
    {
        let p = ptr as *const u16;
        for i in 0..(len as usize / 4) {
            translations.push((unsafe { *p.add(i * 2) }, unsafe { *p.add(i * 2 + 1) }));
        }
    }
    if translations.is_empty() {
        translations.push((0x0409, 0x04b0));
        translations.push((0x0419, 0x04b0));
    }
    for (lang, cp) in translations {
        let sub = wide(&format!(r"\StringFileInfo\{lang:04x}{cp:04x}\{key}"));
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let ok = unsafe {
            VerQueryValueW(
                buf.as_ptr() as *const c_void,
                sub.as_ptr(),
                &mut ptr,
                &mut len,
            )
        };
        if ok != 0 && len > 1 && !ptr.is_null() {
            let s = unsafe {
                String::from_utf16_lossy(std::slice::from_raw_parts(
                    ptr as *const u16,
                    len as usize,
                ))
            };
            let s = s.trim_matches('\0').trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

fn token_user(token: HANDLE) -> Option<String> {
    let mut size = 0u32;
    unsafe {
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut size);
    }
    if size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize];
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut c_void,
            size,
            &mut size,
        )
    } == 0
    {
        return None;
    }
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
    let sid = user.User.Sid;
    if sid.is_null() {
        return None;
    }

    let mut name = [0u16; 256];
    let mut name_len = name.len() as u32;
    let mut domain = [0u16; 256];
    let mut domain_len = domain.len() as u32;
    let mut sid_type: SID_NAME_USE = 0;
    let ok = unsafe {
        LookupAccountSidW(
            std::ptr::null(),
            sid,
            name.as_mut_ptr(),
            &mut name_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut sid_type,
        )
    };
    if ok == 0 {
        return None;
    }
    let name = String::from_utf16_lossy(&name[..name_len as usize]);
    let domain = String::from_utf16_lossy(&domain[..domain_len as usize]);
    Some(if domain.is_empty() {
        name
    } else {
        format!("{domain}\\{name}")
    })
}

fn token_elevated(token: HANDLE) -> bool {
    let mut elev = TOKEN_ELEVATION::default();
    let mut size = 0u32;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            &mut elev as *mut _ as *mut c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
    };
    ok != 0 && elev.TokenIsElevated != 0
}
