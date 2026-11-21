//! Ядро диспетчера задач: сбор системных данных без UI.
//!
//! Никакого WMI. Процессы и CPU — один вызов NtQuerySystemInformation,
//! системные счётчики — PDH, сеть — GetIfTable2, диск/сеть по процессам — ETW.

pub mod etw;
pub mod sampler;
pub mod snapshot;
pub mod sys;

pub use sampler::{is_admin, Sampler};
pub use snapshot::{
    fmt_bytes, fmt_cpu_time, fmt_filetime_local, fmt_mhz, fmt_percent, fmt_rate, fmt_uptime,
    Capabilities, DiskStats, GroupKind, HistPoint, NetStats, ProcInfo, ProcStatus, Snapshot,
    SystemStats,
};
