//! Проверка ETW-сессии: сеть и диск по процессам.
//! Запускать с правами администратора.
//! `cargo run --release -p tm-core --example etwprobe`

use std::io::{Read, Write};
use std::net::TcpStream;

fn main() {
    let mut query = tm_core::sys::nt::ProcessQuery::new();
    let mut procs = Vec::new();
    let _ = query.query(&mut procs, false);
    let names: std::collections::HashMap<u32, String> =
        procs.iter().map(|p| (p.pid, p.name.clone())).collect();
    let me = std::process::id();

    let etw = tm_core::etw::Etw::start();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    println!("активно={} ошибка={:?}", etw.active(), etw.error());

    // Сетевая активность в этом процессе.
    let net_thread = std::thread::spawn(|| {
        if let Ok(mut stream) = TcpStream::connect("example.com:80") {
            let _ = stream
                .write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n");
            let mut buf = Vec::new();
            let _ = stream.read_to_end(&mut buf);
            println!("получено по сети: {} байт", buf.len());
        }
    });

    // Дисковая активность в этом процессе.
    let disk_thread = std::thread::spawn(|| {
        let path = std::env::temp_dir().join("tm-etw-probe.bin");
        if let Ok(mut f) = std::fs::File::create(&path) {
            let chunk = vec![0x5Au8; 1024 * 1024];
            for _ in 0..64 {
                let _ = f.write_all(&chunk);
            }
            let _ = f.sync_all();
        }
        let _ = std::fs::remove_file(&path);
    });

    let _ = net_thread.join();
    let _ = disk_thread.join();
    std::thread::sleep(std::time::Duration::from_millis(2500));

    let io = etw.handle.sink.io.lock().unwrap().clone();
    println!("процессов в карте ETW: {}", io.len());
    let mut rows: Vec<_> = io.iter().collect();
    rows.sort_by_key(|(_, c)| std::cmp::Reverse(c.recv + c.send + c.read + c.write));
    for (pid, c) in rows.iter().take(12) {
        println!(
            "  pid {pid:>6} {:<24} recv={:>10} send={:>8} read={:>11} write={:>11}{}",
            names.get(pid).cloned().unwrap_or_default(),
            c.recv,
            c.send,
            c.read,
            c.write,
            if **pid == me {
                "  <- этот процесс"
            } else {
                ""
            }
        );
    }
    if let Some(c) = io.get(&me) {
        println!(
            "этот процесс: recv={} send={} read={} write={}",
            c.recv, c.send, c.read, c.write
        );
    } else {
        println!("ВНИМАНИЕ: этот процесс не попал в карту ETW");
    }
}
