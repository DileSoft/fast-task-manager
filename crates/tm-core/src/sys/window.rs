//! Окна верхнего уровня: определение «приложений», заголовки, «не отвечает».

use std::collections::HashMap;

use windows_sys::Win32::Foundation::{HWND, LPARAM, TRUE};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsHungAppWindow, IsWindowVisible, PostMessageW, GWL_EXSTYLE,
    GW_OWNER, WM_CLOSE, WS_EX_TOOLWINDOW,
};

#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub hung: bool,
}

struct EnumCtx {
    map: HashMap<u32, WindowInfo>,
}

pub fn visible_windows() -> HashMap<u32, WindowInfo> {
    let mut ctx = EnumCtx {
        map: HashMap::with_capacity(256),
    };
    unsafe {
        EnumWindows(Some(enum_cb), &mut ctx as *mut EnumCtx as LPARAM);
    }
    ctx.map
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
    let ctx = &mut *(lparam as *mut EnumCtx);
    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }
    if !GetWindow(hwnd, GW_OWNER).is_null() {
        return TRUE;
    }
    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW != 0 {
        return TRUE;
    }
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return TRUE;
    }
    let mut buf = vec![0u16; len as usize + 1];
    let copied = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if copied <= 0 {
        return TRUE;
    }
    let title = String::from_utf16_lossy(&buf[..copied as usize]);
    if title.trim().is_empty() {
        return TRUE;
    }

    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 {
        return TRUE;
    }

    let entry = WindowInfo {
        hwnd: hwnd as isize,
        title,
        hung: IsHungAppWindow(hwnd) != 0,
    };
    // Предпочитаем окно, которое не висит.
    match ctx.map.get(&pid) {
        Some(existing) if !existing.hung => {}
        _ => {
            ctx.map.insert(pid, entry);
        }
    }
    TRUE
}

/// Мягко закрывает окно приложения, как «Снять задачу» для приложений.
pub fn close_window(hwnd: isize) -> bool {
    unsafe { PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0) != 0 }
}

/// Поднимает окно процесса на передний план.
pub fn focus_window(hwnd: isize) -> bool {
    use windows_sys::Win32::System::Threading::{
        AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };

    let hwnd = hwnd as HWND;
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        } else {
            ShowWindow(hwnd, SW_SHOW);
        }

        if SetForegroundWindow(hwnd) != 0 {
            BringWindowToTop(hwnd);
            return true;
        }

        // Обход ограничения Windows на смену переднего плана: временно
        // присоединяем очередь ввода к потоку окна-цели.
        let mut pid = 0u32;
        let tid = GetWindowThreadProcessId(hwnd, &mut pid);
        let current = GetCurrentThreadId();
        if tid != 0
            && tid != current
            && pid != 0
            && pid != GetCurrentProcessId()
            && AttachThreadInput(current, tid, 1) != 0
        {
            let ok = SetForegroundWindow(hwnd);
            BringWindowToTop(hwnd);
            AttachThreadInput(current, tid, 0);
            return ok != 0;
        }
        false
    }
}
