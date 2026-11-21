//! Модель данных, которую сэмплер отдаёт в UI.

/// Состояние процесса, как его показывает диспетчер задач.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ProcStatus {
    #[default]
    Running,
    Suspended,
    NotResponding,
}

impl ProcStatus {
    pub fn label(self) -> &'static str {
        match self {
            ProcStatus::Running => "Работает",
            ProcStatus::Suspended => "Приостановлен",
            ProcStatus::NotResponding => "Не отвечает",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum GroupKind {
    #[default]
    Apps,
    Background,
    Windows,
}

impl GroupKind {
    pub fn label(self) -> &'static str {
        match self {
            GroupKind::Apps => "Приложения",
            GroupKind::Background => "Фоновые процессы",
            GroupKind::Windows => "Процессы Windows",
        }
    }
}

use std::sync::Arc;

/// Один процесс в снимке. Все проценты — доля от всей системы (0..100).
#[derive(Clone, Debug, Default)]
pub struct ProcInfo {
    pub pid: u32,
    pub parent_pid: u32,
    pub create_time: i64,
    pub name: String,
    pub exe_path: Option<Arc<str>>,
    pub description: Option<Arc<str>>,
    pub user: Option<Arc<str>>,
    pub cmdline: Option<Arc<str>>,
    pub elevated: bool,
    pub session_id: u32,
    pub cpu: f32,
    pub cpu_time: i64,
    pub working_set: u64,
    pub private_bytes: u64,
    pub threads: u32,
    pub handles: u32,
    pub status: ProcStatus,
    pub window: Option<isize>,
    pub window_title: Option<String>,
    pub disk_read_bps: f64,
    pub disk_write_bps: f64,
    pub net_recv_bps: f64,
    pub net_send_bps: f64,
    pub gpu: f32,
    pub gpu_mem: u64,
    pub group: GroupKind,
}

impl ProcInfo {
    /// Имя для отображения: для системных процессов без пути показываем
    /// то, что отдала система, либо заменитель.
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            "[System]"
        } else {
            &self.name
        }
    }

    pub fn title(&self) -> &str {
        self.window_title
            .as_deref()
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| self.display_name())
    }

    /// Имя для таблицы: у приложений — понятное описание из exe
    /// (как «Google Chrome» вместо «chrome.exe»), у остальных — имя файла.
    pub fn task_name(&self) -> &str {
        if self.group == GroupKind::Apps {
            if let Some(desc) = self.description.as_deref() {
                let generic = desc.is_empty()
                    || desc.contains("Operating System")
                    || desc.contains("Microsoft® Windows®");
                if !generic {
                    return desc;
                }
            }
        }
        self.display_name()
    }

    pub fn is_highlighted_cpu(&self) -> bool {
        self.cpu >= 25.0
    }
}

#[derive(Clone, Debug, Default)]
pub struct HistPoint {
    pub cpu: f32,
    pub mem: f32,
    pub disk: f32,
    pub net_recv: f32,
    pub net_send: f32,
    pub gpu: f32,
}

#[derive(Clone, Debug, Default)]
pub struct SystemStats {
    pub cpu_total: f32,
    pub cpu_kernel: f32,
    pub cpu_speed_mhz: u32,
    pub cpu_base_mhz: u32,
    pub cpu_name: String,
    pub cores_physical: u32,
    pub cores_logical: u32,
    pub processes: u32,
    pub threads: u32,
    pub handles: u32,
    pub uptime_secs: u64,
    pub mem_total: u64,
    pub mem_avail: u64,
    pub mem_used: u64,
    pub mem_cached: u64,
    pub mem_free: u64,
    pub mem_modified: u64,
    pub mem_committed: u64,
    pub mem_commit_limit: u64,
    pub mem_paged_pool: u64,
    pub mem_nonpaged_pool: u64,
    pub mem_speed_mhz: u32,
    pub disk_active: f32,
    pub disk_read_bps: f64,
    pub disk_write_bps: f64,
    pub disk_response_ms: f64,
    pub disk_total: u64,
    pub disk_free: u64,
    pub disk_count: u32,
    pub net_recv_bps: f64,
    pub net_send_bps: f64,
    pub net_link_bps: u64,
    pub net_name: String,
    pub gpu: f32,
    pub gpu_dedicated: u64,
    pub gpu_shared: u64,
    pub gpu_name: String,
    pub admin: bool,
    pub elevated: bool,
}

#[derive(Clone, Debug, Default)]
pub struct DiskStats {
    /// «Диск 0».
    pub name: String,
    /// Модель оборудования: «Samsung SSD 860 EVO».
    pub model: String,
    /// Тома этого диска: «C: D:».
    pub volumes: String,
    pub active: f32,
    pub read_bps: f64,
    pub write_bps: f64,
    pub response_ms: f64,
    pub total: u64,
    pub free: u64,
}

#[derive(Clone, Debug, Default)]
pub struct NetStats {
    /// Понятное имя подключения: «Ethernet», «Wi-Fi».
    pub name: String,
    pub description: String,
    pub recv_bps: f64,
    pub send_bps: f64,
    pub link_bps: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Capabilities {
    pub etw_active: bool,
    pub etw_error: Option<String>,
    pub per_process_io: bool,
    pub gpu_available: bool,
}

/// Неизменяемый снимок состояния системы. UI читает его без блокировок.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub seq: u64,
    /// Фактический интервал между снимками, секунды.
    pub elapsed: f64,
    /// Сколько заняла сборка снимка, мс.
    pub collect_ms: f32,
    pub procs: Vec<ProcInfo>,
    pub system: SystemStats,
    /// Физические диски (как в диспетчере задач: «Диск 0», «Диск 1»).
    pub disks: Vec<DiskStats>,
    /// Сетевые адаптеры с ненулевым трафиком или активным статусом.
    pub nets: Vec<NetStats>,
    pub history: Vec<HistPoint>,
    pub core_history: Vec<Vec<f32>>,
    pub disk_history: Vec<Vec<f32>>,
    pub net_recv_history: Vec<Vec<f32>>,
    pub net_send_history: Vec<Vec<f32>>,
    pub capabilities: Capabilities,
}

impl Snapshot {
    pub fn proc_by_pid(&self, pid: u32) -> Option<&ProcInfo> {
        self.procs.iter().find(|p| p.pid == pid)
    }

    pub fn total_cpu(&self) -> f32 {
        self.system.cpu_total
    }
}

/// Форматирование объёма памяти в стиле диспетчера задач.
pub fn fmt_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let b = bytes as f64;
    if b >= TB {
        format!("{:.1} ТБ", b / TB)
    } else if b >= GB {
        format!("{:.1} ГБ", b / GB)
    } else if b >= 100.0 * MB {
        format!("{:.0} МБ", b / MB)
    } else if b >= MB {
        format!("{:.1} МБ", b / MB)
    } else if b >= KB {
        format!("{:.0} КБ", b / KB)
    } else {
        format!("{} Б", bytes)
    }
}

pub fn fmt_rate(bytes_per_sec: f64) -> String {
    if bytes_per_sec < 1.0 {
        return String::new();
    }
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    if bytes_per_sec >= GB {
        format!("{:.1} ГБ/с", bytes_per_sec / GB)
    } else if bytes_per_sec >= MB {
        format!("{:.1} МБ/с", bytes_per_sec / MB)
    } else if bytes_per_sec >= 10.0 * KB {
        format!("{:.0} КБ/с", bytes_per_sec / KB)
    } else {
        format!("{:.1} КБ/с", bytes_per_sec / KB)
    }
}

/// Процент в стиле Win10: целый, пока значение большое, с десятой — пока маленькое.
pub fn fmt_percent(v: f32) -> String {
    if v <= 0.0 {
        "0%".to_string()
    } else if v < 10.0 {
        format!("{:.1}%", v)
    } else {
        format!("{:.0}%", v)
    }
}

pub fn fmt_mhz(mhz: u32) -> String {
    if mhz >= 1000 {
        format!("{:.2} ГГц", mhz as f64 / 1000.0)
    } else {
        format!("{} МГц", mhz)
    }
}

/// Uptime в формате ЧЧ:ММ:СС:ДД.
pub fn fmt_uptime(secs: u64) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{d:02}:{h:02}:{m:02}:{s:02}")
}

/// Время создания процесса (FILETIME, 100 нс с 1601) в локальном времени.
pub fn fmt_filetime_local(ft: i64) -> String {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let filetime = FILETIME {
        dwLowDateTime: ft as u32,
        dwHighDateTime: (ft >> 32) as u32,
    };
    let mut utc = unsafe { std::mem::zeroed::<SYSTEMTIME>() };
    let mut local = unsafe { std::mem::zeroed::<SYSTEMTIME>() };
    unsafe {
        if FileTimeToSystemTime(&filetime, &mut utc) == 0 {
            return String::new();
        }
        if SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0 {
            return String::new();
        }
    }
    format!(
        "{:02}.{:02}.{} {:02}:{:02}:{:02}",
        local.wDay, local.wMonth, local.wYear, local.wHour, local.wMinute, local.wSecond
    )
}

/// Накопленное время ЦП (100 нс) в формате Ч:ММ:СС.
pub fn fmt_cpu_time(ft: i64) -> String {
    let secs = (ft.max(0) / 10_000_000) as u64;
    format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}
