//! Действия над процессами: снятие задачи, дерево, расположение файла, UAC.

use std::ffi::c_void;

use windows_sys::Win32::Foundation::{CloseHandle, HWND};
use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::{wide, window};

fn win_error(code: u32) -> String {
    match code {
        5 => "Отказано в доступе. Запустите программу от имени администратора.".to_string(),
        87 => "Процесс уже завершён.".to_string(),
        _ => format!("Ошибка Windows ({code})"),
    }
}

/// Завершает процесс. Если есть окно — сначала пытается закрыть его мягко.
pub fn end_task(pid: u32, hwnd: Option<isize>) -> Result<(), String> {
    if pid <= 4 {
        return Err("Критический системный процесс завершить нельзя.".to_string());
    }
    if let Some(hwnd) = hwnd {
        if window::close_window(hwnd) {
            return Ok(());
        }
    }
    terminate(pid)
}

pub fn terminate(pid: u32) -> Result<(), String> {
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle.is_null() {
        return Err(win_error(super::last_error()));
    }
    let ok = unsafe { TerminateProcess(handle, 1) };
    let err = if ok == 0 {
        Some(win_error(super::last_error()))
    } else {
        None
    };
    unsafe {
        CloseHandle(handle);
    }
    match err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Завершает процесс и всех его потомков (по снимку `parent_pid`).
pub fn end_task_tree(procs: &[crate::snapshot::ProcInfo], pid: u32) -> Result<usize, String> {
    let mut targets = vec![pid];
    let mut i = 0;
    while i < targets.len() {
        let parent = targets[i];
        for p in procs {
            if p.parent_pid == parent && !targets.contains(&p.pid) {
                targets.push(p.pid);
            }
        }
        i += 1;
    }

    let mut done = 0;
    let mut last_err = None;
    for target in targets.into_iter().rev() {
        if target <= 4 {
            continue;
        }
        match terminate(target) {
            Ok(()) => done += 1,
            Err(e) => last_err = Some(e),
        }
    }
    if done == 0 {
        Err(last_err.unwrap_or_else(|| "Не удалось завершить процессы".to_string()))
    } else {
        Ok(done)
    }
}

/// Открывает проводник с выделенным файлом.
pub fn open_file_location(path: &str) -> Result<(), String> {
    let exe = wide("explorer.exe");
    let params = wide(&format!("/select,\"{path}\""));
    let rc = unsafe {
        ShellExecuteW(
            std::ptr::null_mut::<c_void>() as HWND,
            wide("open").as_ptr(),
            exe.as_ptr(),
            params.as_ptr(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if rc <= 32 {
        Err("Не удалось открыть проводник".to_string())
    } else {
        Ok(())
    }
}

/// Перезапускает программу с правами администратора (UAC).
pub fn restart_elevated() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_w = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let rc = unsafe {
        ShellExecuteW(
            std::ptr::null_mut::<c_void>() as HWND,
            verb.as_ptr(),
            exe_w.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    } as isize;
    if rc <= 32 {
        Err("Запрос на повышение прав отклонён или недоступен".to_string())
    } else {
        Ok(())
    }
}
