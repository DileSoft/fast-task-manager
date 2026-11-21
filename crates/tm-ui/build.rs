//! Встраивание иконки и версии в exe (Windows). Если rc.exe недоступен,
//! сборка продолжается без ресурсов.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winres::WindowsResource::new();
    res.set_icon("assets/icon.ico");
    res.set("ProductName", "Fast Task Manager");
    res.set("FileDescription", "Быстрый диспетчер задач");
    res.set("CompanyName", "DileSoft");
    res.set("LegalCopyright", "MIT License, 2026 DileSoft");
    res.set("OriginalFilename", "tm.exe");
    if let Err(error) = res.compile() {
        println!("cargo:warning=ресурсы exe не встроены: {error}");
    }
}
