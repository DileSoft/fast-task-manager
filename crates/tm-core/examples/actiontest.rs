//! Проверка действий: запускает дочерний процесс и снимает его.
//! `cargo run -p tm-core --example actiontest`

use std::os::windows::process::CommandExt;

fn main() {
    let mut child = std::process::Command::new("cmd")
        .args(["/c", "ping -n 60 127.0.0.1 > nul"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn()
        .expect("не удалось запустить потомка");
    let pid = child.id();
    std::thread::sleep(std::time::Duration::from_millis(300));
    println!("потомок pid={pid}, живой={:?}", child.try_wait());

    match tm_core::sys::actions::terminate(pid) {
        Ok(()) => println!("terminate: ok"),
        Err(e) => println!("terminate: ошибка {e}"),
    }
    std::thread::sleep(std::time::Duration::from_millis(400));
    println!("после завершения try_wait={:?}", child.try_wait());

    // Проверка открытия проводника не запускается автоматически,
    // чтобы тест не открывал окна.
    println!("проверка завершена");
}
