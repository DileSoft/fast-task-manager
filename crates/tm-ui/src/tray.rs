//! Значок в области уведомлений Windows.
//!
//! Создаём скрытое окно в отдельном потоке и регистрируем его в оболочке через
//! `Shell_NotifyIconW`. Двойной клик (или пункт «Открыть») показывает главное
//! окно, «Выход» закрывает приложение. События UI передаются через
//! `egui::Context` (он потокобезопасен), поэтому сам трей не зависит от цикла
//! сообщений eframe.

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;
use std::thread;

use eframe::egui::{self, ViewportCommand};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, LoadImageW, PostMessageW,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, TrackPopupMenu, TranslateMessage, HICON,
    IMAGE_ICON, LR_SHARED, MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON, SM_CYSMICON, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONDBLCLK, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP, WNDCLASSW,
};

const WM_TRAY: u32 = WM_APP + 1;
const CMD_SHOW: usize = 1;
const CMD_QUIT: usize = 2;
/// Идентификатор иконки в ресурсах exe (см. `build.rs`, `set_icon` → id 1).
const ICON_RESOURCE_ID: usize = 1;

static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
static UI_CTX: OnceLock<egui::Context> = OnceLock::new();

/// Запускает поток трея. Повторный вызов ничего не делает.
pub fn start(ctx: egui::Context) {
    if UI_CTX.set(ctx).is_err() {
        return;
    }
    thread::Builder::new()
        .name("tray".to_string())
        .spawn(|| {
            if let Err(error) = run() {
                eprintln!("трей: {error}");
            }
        })
        .ok();
}

/// Убирает значок из области уведомлений и останавливает поток трея.
pub fn shutdown() {
    let hwnd = TRAY_HWND.swap(0, Ordering::SeqCst);
    if hwnd != 0 {
        unsafe {
            PostMessageW(hwnd as HWND, WM_CLOSE, 0, 0);
        }
    }
}

fn show_window() {
    if let Some(ctx) = UI_CTX.get() {
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }
}

fn quit_app() {
    if let Some(ctx) = UI_CTX.get() {
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }
}

fn run() -> Result<(), String> {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class_name = wide("FastTaskManagerTrayWindow");
        let window_title = wide("");

        let mut class: WNDCLASSW = std::mem::zeroed();
        class.lpfnWndProc = Some(wnd_proc);
        class.hInstance = instance;
        class.lpszClassName = class_name.as_ptr();

        if RegisterClassW(&class) == 0 {
            return Err(format!("RegisterClassW: код {}", GetLastError()));
        }

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            window_title.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err(format!("CreateWindowExW: код {}", GetLastError()));
        }
        TRAY_HWND.store(hwnd as isize, Ordering::SeqCst);

        let cx = GetSystemMetrics(SM_CXSMICON);
        let cy = GetSystemMetrics(SM_CYSMICON);
        let mut icon: HICON = LoadImageW(
            instance,
            ICON_RESOURCE_ID as *const u16,
            IMAGE_ICON,
            cx,
            cy,
            LR_SHARED,
        ) as HICON;
        if icon.is_null() {
            icon = LoadImageW(
                instance,
                ICON_RESOURCE_ID as *const u16,
                IMAGE_ICON,
                0,
                0,
                LR_SHARED,
            ) as HICON;
        }

        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        data.hIcon = icon;
        let tip = "Диспетчер задач";
        let tip_len = tip.encode_utf16().count().min(data.szTip.len() - 1);
        for (slot, unit) in data.szTip.iter_mut().zip(tip.encode_utf16()) {
            *slot = unit;
        }
        // Гарантируем завершающий ноль, если строка короче буфера.
        data.szTip[tip_len] = 0;

        if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
            DestroyWindow(hwnd);
            return Err("Shell_NotifyIconW(NIM_ADD)".to_string());
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        remove_icon(hwnd);
        Ok(())
    }
}

/// Убирает значок из области уведомлений.
unsafe fn remove_icon(hwnd: HWND) {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAY => {
            let event = (lparam as u32) & 0xFFFF;
            match event {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => show_window(),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_CLOSE => {
            unsafe {
                remove_icon(hwnd);
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let menu = CreatePopupMenu();
        if menu.is_null() {
            return;
        }
        let open = wide("Открыть диспетчер задач");
        let quit = wide("Выход");
        AppendMenuW(menu, MF_STRING, CMD_SHOW, open.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, CMD_QUIT, quit.as_ptr());

        let mut point = POINT { x: 0, y: 0 };
        GetCursorPos(&mut point);
        // Обязательно перед TrackPopupMenu, иначе меню не закроется по клику
        // вне его (поведение оболочки Windows).
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            0,
            hwnd,
            std::ptr::null(),
        ) as usize;
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);

        match cmd {
            CMD_SHOW => show_window(),
            CMD_QUIT => quit_app(),
            _ => {}
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
