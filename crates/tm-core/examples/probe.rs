//! Диагностический запуск ядра: `cargo run -p tm-core --example probe`.

use std::time::Duration;

fn main() {
    let sampler = tm_core::Sampler::start(1000);
    std::thread::sleep(Duration::from_millis(9000));
    let snap = sampler.latest();
    let s = &snap.system;

    println!("тик #{}, сборка {:.1} мс, интервал {:.2} с", snap.seq, snap.collect_ms, snap.elapsed);
    println!("процессов: {}, потоков: {}, handles: {}", s.processes, s.threads, s.handles);
    println!("CPU: {:.1}% (ядро {:.1}%), ядер: {}", s.cpu_total, s.cpu_kernel, s.cores_logical);
    println!("CPU: {} @ {} МГц, сейчас {} МГц", s.cpu_name, s.cpu_base_mhz, s.cpu_speed_mhz);
    println!(
        "память: занято {} из {} (доступно {}), кэш {}, коммит {} / {}",
        tm_core::fmt_bytes(s.mem_used),
        tm_core::fmt_bytes(s.mem_total),
        tm_core::fmt_bytes(s.mem_avail),
        tm_core::fmt_bytes(s.mem_cached),
        tm_core::fmt_bytes(s.mem_committed),
        tm_core::fmt_bytes(s.mem_commit_limit)
    );
    println!(
        "диск: активность {:.1}%, чтение {:.0} КБ/с, запись {:.0} КБ/с, отклик {:.2} мс",
        s.disk_active,
        s.disk_read_bps / 1024.0,
        s.disk_write_bps / 1024.0,
        s.disk_response_ms
    );
    println!(
        "сеть: {} / {}, вход {:.0} КБ/с, выход {:.0} КБ/с, канал {} Мбит/с",
        s.net_name,
        tm_core::fmt_bytes(s.net_link_bps / 8),
        s.net_recv_bps / 1024.0,
        s.net_send_bps / 1024.0,
        s.net_link_bps / 1_000_000
    );
    println!(
        "ГП: {} {:.0}%, выделено {}, ETW: {} ({:?}), админ: {}",
        s.gpu_name,
        s.gpu,
        tm_core::fmt_bytes(s.gpu_dedicated),
        snap.capabilities.etw_active,
        snap.capabilities.etw_error,
        s.admin
    );

    println!("\nТоп по CPU:");
    let mut procs: Vec<_> = snap.procs.iter().collect();
    procs.sort_by(|a, b| b.cpu.partial_cmp(&a.cpu).unwrap());
    for p in procs.iter().take(10) {
        println!(
            "  {:>6} {:<28} CPU {:>5}  MEM {:>10}  {:?}  {}",
            p.pid,
            tm_core::snapshot::ProcInfo::display_name(p),
            tm_core::fmt_percent(p.cpu),
            tm_core::fmt_bytes(p.working_set),
            p.group,
            p.description.as_deref().unwrap_or("")
        );
    }

    println!("\nТоп по памяти:");
    let mut procs: Vec<_> = snap.procs.iter().collect();
    procs.sort_by_key(|p| std::cmp::Reverse(p.working_set));
    for p in procs.iter().take(5) {
        println!(
            "  {:>6} {:<28} MEM {:>10}  {:?}",
            p.pid,
            p.name,
            tm_core::fmt_bytes(p.working_set),
            p.group
        );
    }

    println!("\nИстория: {} точек", snap.history.len());
    let with_path = snap.procs.iter().filter(|p| p.exe_path.is_some()).count();
    let with_user = snap.procs.iter().filter(|p| p.user.is_some()).count();
    let with_desc = snap.procs.iter().filter(|p| p.description.is_some()).count();
    println!(
        "идентификация: путей {with_path}, пользователей {with_user}, описаний {with_desc} из {}",
        snap.procs.len()
    );
    for p in snap.procs.iter().filter(|p| p.exe_path.is_some()).take(8) {
        println!("  {} -> {}", p.name, p.exe_path.as_deref().unwrap_or(""));
    }    if let Some(last) = snap.history.last() {
        println!("  последняя: {:?}", last);
    }
}
