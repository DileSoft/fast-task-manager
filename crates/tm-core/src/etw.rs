//! ETW-сессия реального времени для сетевой и дисковой активности по процессам.
//!
//! Требует прав администратора — как и в сторонних диспетчерах задач.
//! Провайдеры:
//!   * Microsoft-Windows-Kernel-Network — события 10/11 (TCP v4 send/recv),
//!     26/27 (TCP v6), 42/43 (UDP v4), 58/59 (UDP v6). PID лежит в первых
//!     4 байтах payload, размер — в следующих 4.
//!   * Microsoft-Windows-Kernel-Disk — события 10/11 (read/write).
//!     Размер передачи — по смещению 8, IssuingThreadId — по смещению 40;
//!     PID получаем через карту потоков из снимка NtQuerySystemInformation.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use arc_swap::ArcSwap;
use windows_sys::core::GUID;
use windows_sys::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW,
    EVENT_RECORD, CONTROLTRACE_HANDLE, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE,
};

use crate::sys::wide;

const NETWORK_PROVIDER: GUID = GUID {
    data1: 0x7dd4_2a49,
    data2: 0x5329,
    data3: 0x4832,
    data4: [0x8d, 0xfd, 0x43, 0xd9, 0x79, 0x15, 0x3a, 0x88],
};

const DISK_PROVIDER: GUID = GUID {
    data1: 0xc7bd_e69a,
    data2: 0xe1e0,
    data3: 0x4177,
    data4: [0xb6, 0xef, 0x28, 0x3a, 0xd1, 0x52, 0x52, 0x71],
};

const SESSION_NAME: &str = "TaskManager-Etw-01";
const WNODE_FLAG_TRACED_GUID: u32 = 0x0002_0000;
const EVENT_TRACE_REAL_TIME_MODE: u32 = 0x0000_0100;
const EVENT_CONTROL_CODE_ENABLE_PROVIDER: u32 = 1;
const ERROR_ALREADY_EXISTS: u32 = 183;

#[derive(Clone, Copy, Debug, Default)]
pub struct IoCounters {
    pub read: u64,
    pub write: u64,
    pub recv: u64,
    pub send: u64,
}

impl IoCounters {
    fn merge(&mut self, other: &IoCounters) {
        self.read += other.read;
        self.write += other.write;
        self.recv += other.recv;
        self.send += other.send;
    }
}

/// Общее состояние, куда ETW-поток складывает накопленные байты,
/// а сэмплер раз в секунду забирает дельты.
pub struct Sink {
    pub io: Mutex<HashMap<u32, IoCounters>>,
    /// Поток -> процесс, обновляется сэмплером из системного снимка.
    pub threads: ArcSwap<HashMap<u32, u32>>,
}

impl Default for Sink {
    fn default() -> Self {
        Self {
            io: Mutex::new(HashMap::with_capacity(256)),
            threads: ArcSwap::from_pointee(HashMap::new()),
        }
    }
}

thread_local! {
    static PENDING: RefCell<HashMap<u32, IoCounters>> = RefCell::new(HashMap::new());
    static LAST_FLUSH: Cell<i64> = const { Cell::new(0) };
    static EVENT_COUNT: Cell<u64> = const { Cell::new(0) };
}

fn flush_pending(sink: &Sink) {
    PENDING.with(|p| {
        let mut pending = p.borrow_mut();
        if pending.is_empty() {
            return;
        }
        if let Ok(mut shared) = sink.io.lock() {
            for (pid, c) in pending.iter() {
                shared.entry(*pid).or_default().merge(c);
            }
        }
        pending.clear();
    });
}

fn accumulate(sink: &Sink, pid: u32, add: IoCounters, timestamp: i64) {
    PENDING.with(|p| {
        let mut pending = p.borrow_mut();
        let entry = pending.entry(pid).or_default();
        entry.read += add.read;
        entry.write += add.write;
        entry.recv += add.recv;
        entry.send += add.send;
    });
    // Сбрасываем в общую карту примерно раз в 0.5 с (timestamp в 100 нс)
    // либо каждые 4096 событий, если метки времени не приходят.
    let last = LAST_FLUSH.with(|f| f.get());
    let count = EVENT_COUNT.with(|c| {
        let v = c.get() + 1;
        c.set(v);
        v
    });
    if timestamp - last > 5_000_000 || count % 4096 == 0 {
        LAST_FLUSH.with(|f| f.set(timestamp));
        flush_pending(sink);
    }
}

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    if record.is_null() {
        return;
    }
    let record = &*record;
    let header = &record.EventHeader;
    let id = header.EventDescriptor.Id;
    let payload = record.UserData as *const u8;
    let len = record.UserDataLength as usize;

    // Контекст — указатель на Sink, живёт в потоке сессии.
    let sink = &*(record.UserContext as *const Sink);

    if guid_eq(&header.ProviderId, &NETWORK_PROVIDER) {
        if len < 8 || payload.is_null() {
            return;
        }
        let pid = u32::from_le_bytes([*payload, *payload.add(1), *payload.add(2), *payload.add(3)]);
        let size = u32::from_le_bytes([
            *payload.add(4),
            *payload.add(5),
            *payload.add(6),
            *payload.add(7),
        ]) as u64;
        if pid == 0 || size == 0 || size > 64 * 1024 * 1024 {
            return;
        }
        let mut add = IoCounters::default();
        match id {
            10 | 26 | 42 | 58 => add.send = size,
            11 | 27 | 43 | 59 => add.recv = size,
            _ => return,
        }
        accumulate(sink, pid, add, header.TimeStamp);
        return;
    }

    if guid_eq(&header.ProviderId, &DISK_PROVIDER) {
        if len < 12 || payload.is_null() {
            return;
        }
        let transfer = u32::from_le_bytes([
            *payload.add(8),
            *payload.add(9),
            *payload.add(10),
            *payload.add(11),
        ]) as u64;
        if transfer == 0 || transfer > 64 * 1024 * 1024 {
            return;
        }
        let thread_id = if len >= 44 {
            Some(u32::from_le_bytes([
                *payload.add(40),
                *payload.add(41),
                *payload.add(42),
                *payload.add(43),
            ]))
        } else {
            None
        };
        let map = sink.threads.load();
        let pid = thread_id
            .and_then(|t| map.get(&t).copied())
            .or_else(|| {
                // Фолбэк: заголовок события иногда содержит нужный процесс.
                if header.ProcessId > 4 {
                    Some(header.ProcessId)
                } else {
                    None
                }
            });
        let Some(pid) = pid else { return };
        let mut add = IoCounters::default();
        match id {
            10 => add.read = transfer,
            11 => add.write = transfer,
            _ => return,
        }
        accumulate(sink, pid, add, header.TimeStamp);
    }
}

fn guid_eq(a: &GUID, b: &GUID) -> bool {
    a.data1 == b.data1 && a.data2 == b.data2 && a.data3 == b.data3 && a.data4 == b.data4
}

/// Строит буфер свойств сессии: структура + имя сессии.
fn session_properties(name: &str) -> Vec<u64> {
    let props_size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
    let name_w = wide(name);
    let total = props_size + name_w.len() * 2;
    let words = (total + 7) / 8 + 1;
    let mut buf = vec![0u64; words];
    unsafe {
        let props = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
        (*props).Wnode.BufferSize = total as u32;
        (*props).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        (*props).Wnode.ClientContext = 1; // QPC
        (*props).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
        (*props).BufferSize = 64;
        (*props).MinimumBuffers = 32;
        (*props).MaximumBuffers = 128;
        (*props).FlushTimer = 1;
        (*props).LogFileNameOffset = 0;
        (*props).LoggerNameOffset = props_size as u32;

        let dst = (buf.as_mut_ptr() as *mut u8).add(props_size) as *mut u16;
        std::ptr::copy_nonoverlapping(name_w.as_ptr(), dst, name_w.len());
    }
    buf
}

fn props_mut(buf: &mut [u64]) -> *mut EVENT_TRACE_PROPERTIES {
    buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES
}

pub struct Etw {
    pub handle: EtwHandle,
    trace_handle: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

/// Разделяемое состояние сессии: сэмплеру нужны только эти поля.
#[derive(Clone)]
pub struct EtwHandle {
    pub sink: Arc<Sink>,
    pub status: Arc<Mutex<Option<String>>>,
    pub active: Arc<AtomicBool>,
}

impl Etw {
    /// Запускает сессию в фоне. Ошибки доступны через `error()`.
    pub fn start() -> Etw {
        let sink = Arc::new(Sink::default());
        let status = Arc::new(Mutex::new(None));
        let active = Arc::new(AtomicBool::new(false));
        let trace_handle = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));

        let sink2 = sink.clone();
        let status2 = status.clone();
        let active2 = active.clone();
        let handle2 = trace_handle.clone();
        let stop2 = stop.clone();

        let join = std::thread::Builder::new()
            .name("tm-etw".into())
            .spawn(move || run_session(sink2, status2, active2, handle2, stop2))
            .ok();

        Etw {
            handle: EtwHandle {
                sink,
                status,
                active,
            },
            trace_handle,
            stop,
            join,
        }
    }

    pub fn active(&self) -> bool {
        self.handle.active.load(Ordering::Relaxed)
    }

    pub fn error(&self) -> Option<String> {
        self.handle.status.lock().ok().and_then(|s| s.clone())
    }
}

impl Drop for Etw {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);

        // Сначала останавливаем сессию, затем закрываем обработчик —
        // это гарантированно будит ProcessTrace.
        let mut props = session_properties(SESSION_NAME);
        unsafe {
            ControlTraceW(
                CONTROLTRACE_HANDLE { Value: 0 },
                wide(SESSION_NAME).as_ptr(),
                props_mut(&mut props),
                EVENT_TRACE_CONTROL_STOP,
            );
        }
        let h = self.trace_handle.load(Ordering::SeqCst);
        if h != 0 {
            unsafe {
                CloseTrace(PROCESSTRACE_HANDLE { Value: h });
            }
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn set_error(status: &Mutex<Option<String>>, msg: impl Into<String>) {
    if let Ok(mut s) = status.lock() {
        *s = Some(msg.into());
    }
}

fn run_session(
    sink: Arc<Sink>,
    status: Arc<Mutex<Option<String>>>,
    active: Arc<AtomicBool>,
    trace_handle: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
) {
    let name = wide(SESSION_NAME);

    // Убираем сессию, оставшуюся от предыдущего запуска.
    let mut stale = session_properties(SESSION_NAME);
    unsafe {
        ControlTraceW(
            CONTROLTRACE_HANDLE { Value: 0 },
            name.as_ptr(),
            props_mut(&mut stale),
            EVENT_TRACE_CONTROL_STOP,
        );
    }

    let mut props = session_properties(SESSION_NAME);
    let mut session_handle = CONTROLTRACE_HANDLE { Value: 0 };
    let rc = unsafe { StartTraceW(&mut session_handle, name.as_ptr(), props_mut(&mut props)) };
    drop(props);
    if rc == ERROR_ALREADY_EXISTS {
        set_error(
            &status,
            "Сессия ETW уже занята другим экземпляром программы. Закройте его.".to_string(),
        );
        return;
    }
    if rc != 0 {
        set_error(
            &status,
            format!(
                "Не удалось запустить ETW (код {rc}). Нужны права администратора."
            ),
        );
        return;
    }

    let net_rc = unsafe {
        EnableTraceEx2(
            session_handle,
            &NETWORK_PROVIDER,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER,
            5,          // verbose
            0x30,       // IPV4 | IPV6
            0,
            0,
            std::ptr::null(),
        )
    };
    let disk_rc = unsafe {
        EnableTraceEx2(
            session_handle,
            &DISK_PROVIDER,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER,
            5,
            0,
            0,
            0,
            std::ptr::null(),
        )
    };

    if net_rc != 0 && disk_rc != 0 {
        let mut p = session_properties(SESSION_NAME);
        unsafe {
            ControlTraceW(
                session_handle,
                name.as_ptr(),
                props_mut(&mut p),
                EVENT_TRACE_CONTROL_STOP,
            );
        }
        set_error(
            &status,
            format!(
                "ETW недоступен (сеть: {}, диск: {}). Требуются права администратора.",
                error_text(net_rc),
                error_text(disk_rc)
            ),
        );
        return;
    }

    if net_rc != 0 {
        set_error(
            &status,
            format!("Сетевые события ETW недоступны: {}", error_text(net_rc)),
        );
    }

    let sink_ptr = Arc::as_ptr(&sink) as *mut c_void;
    let mut logfile: EVENT_TRACE_LOGFILEW = unsafe { std::mem::zeroed() };
    logfile.LoggerName = name.as_ptr() as *mut u16;
    logfile.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
    logfile.Anonymous2.EventRecordCallback = Some(on_event);
    logfile.Context = sink_ptr;

    let trace = unsafe { OpenTraceW(&mut logfile) };
    if trace.Value == u64::MAX || trace.Value == 0 {
        let mut p = session_properties(SESSION_NAME);
        unsafe {
            ControlTraceW(
                session_handle,
                name.as_ptr(),
                props_mut(&mut p),
                EVENT_TRACE_CONTROL_STOP,
            );
        }
        set_error(&status, "Не удалось открыть ETW-сессию".to_string());
        return;
    }
    trace_handle.store(trace.Value, Ordering::SeqCst);
    active.store(true, Ordering::SeqCst);

    unsafe {
        ProcessTrace(&trace, 1, std::ptr::null(), std::ptr::null());
    }
    flush_pending(&sink);
    active.store(false, Ordering::SeqCst);

    unsafe {
        CloseTrace(trace);
    }
    trace_handle.store(0, Ordering::SeqCst);

    let mut p = session_properties(SESSION_NAME);
    unsafe {
        ControlTraceW(
            session_handle,
            name.as_ptr(),
            props_mut(&mut p),
            EVENT_TRACE_CONTROL_STOP,
        );
    }
    let _ = stop.load(Ordering::SeqCst);
}

fn error_text(rc: u32) -> String {
    match rc {
        5 => "отказано в доступе".to_string(),
        87 => "неверный параметр".to_string(),
        2 => "провайдер не найден".to_string(),
        _ => format!("код {rc}"),
    }
}
