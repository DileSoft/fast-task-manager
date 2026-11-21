//! Проверка доступа к процессам: `cargo run -p tm-core --example ident`.

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

fn main() {
    let mut query = tm_core::sys::nt::ProcessQuery::new();
    let mut procs = Vec::new();
    query.query(&mut procs, false).unwrap();
    println!("всего процессов: {}", procs.len());

    let mut opened = 0;
    let mut named = 0;
    let mut denied_open = 0;
    let mut denied_name = 0;
    let mut shown = 0;
    for p in &procs {
        if p.pid == 0 {
            continue;
        }
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, p.pid);
            if h.is_null() {
                denied_open += 1;
                if shown < 5 {
                    println!("{} {}: OpenProcess err={}", p.pid, p.name, GetLastError());
                    shown += 1;
                }
                continue;
            }
            opened += 1;
            let mut buf = vec![0u16; 1024];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) == 0 {
                denied_name += 1;
                if shown < 5 {
                    println!(
                        "{} {}: QueryFullProcessImageName err={}",
                        p.pid,
                        p.name,
                        GetLastError()
                    );
                    shown += 1;
                }
            } else {
                named += 1;
                if shown < 3 {
                    println!(
                        "{} {}",
                        p.pid,
                        String::from_utf16_lossy(&buf[..len as usize])
                    );
                }
            }
            CloseHandle(h);
        }
    }
    println!(
        "открыто {opened}, путей {named}, отказ OpenProcess {denied_open}, отказ имени {denied_name}"
    );
}
