//! Фоновый сэмплер: раз в тик собирает неизменяемый снимок и атомарно
//! публикует его. UI никогда не ждёт сэмплер.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;

use crate::etw::{self, Etw};
use crate::snapshot::*;
use crate::sys::{identity, mem, net, nt, pdh, registry, window};

const HISTORY_LEN: usize = 600;
const IDENTITY_BUDGET_PER_TICK: usize = 48;

#[derive(Clone, Copy, Debug)]
struct ProcTimes {
    create_time: i64,
    kernel: i64,
    user: i64,
}

#[derive(Clone, Debug, Default)]
struct CachedIdentity {
    exe: Option<Arc<str>>,
    desc: Option<Arc<str>>,
    user: Option<Arc<str>>,
    elevated: bool,
}

impl CachedIdentity {
    fn from_raw(raw: identity::Identity) -> Self {
        Self {
            exe: raw.exe_path.map(|s| Arc::from(s.as_str())),
            desc: raw.description.map(|s| Arc::from(s.as_str())),
            user: raw.user.map(|s| Arc::from(s.as_str())),
            elevated: raw.elevated,
        }
    }
}

/// Индексы PDH-счётчиков. `None` — счётчик недоступен на этой машине.
struct Counters {
    q: pdh::Query,
    gpu: pdh::Query,
    mem_avail_mb: Option<usize>,
    mem_committed: Option<usize>,
    mem_commit_limit: Option<usize>,
    standby_normal: Option<usize>,
    standby_reserve: Option<usize>,
    standby_core: Option<usize>,
    modified: Option<usize>,
    free_zero: Option<usize>,
    cache_bytes: Option<usize>,
    pool_paged: Option<usize>,
    pool_nonpaged: Option<usize>,
    cpu_perf: Option<usize>,
    disk_idle: Option<usize>,
    disk_read: Option<usize>,
    disk_write: Option<usize>,
    disk_latency: Option<usize>,
    gpu_engine: Option<usize>,
    gpu_dedicated: Option<usize>,
    gpu_shared: Option<usize>,
}

impl Counters {
    fn new() -> Option<Self> {
        let mut q = pdh::Query::new()?;
        let mut gpu = pdh::Query::new()?;
        let mem_avail_mb = q.add(r"\Memory\Available MBytes");
        let mem_committed = q.add(r"\Memory\Committed Bytes");
        let mem_commit_limit = q.add(r"\Memory\Commit Limit");
        let standby_normal = q.add(r"\Memory\Standby Cache Normal Priority Bytes");
        let standby_reserve = q.add(r"\Memory\Standby Cache Reserve Bytes");
        let standby_core = q.add(r"\Memory\Standby Cache Core Bytes");
        let modified = q.add(r"\Memory\Modified Page List Bytes");
        let free_zero = q.add(r"\Memory\Free & Zero Page List Bytes");
        let cache_bytes = q.add(r"\Memory\Cache Bytes");
        let pool_paged = q.add(r"\Memory\Pool Paged Bytes");
        let pool_nonpaged = q.add(r"\Memory\Pool Nonpaged Bytes");
        let cpu_perf = q.add(r"\Processor Information(_Total)\% Processor Performance");
        let disk_idle = q.add(r"\PhysicalDisk(_Total)\% Idle Time");
        let disk_read = q.add(r"\PhysicalDisk(_Total)\Disk Read Bytes/sec");
        let disk_write = q.add(r"\PhysicalDisk(_Total)\Disk Write Bytes/sec");
        let disk_latency = q.add(r"\PhysicalDisk(_Total)\Avg. Disk sec/Transfer");
        let gpu_engine = gpu.add(r"\GPU Engine(*)\Utilization Percentage");
        let gpu_dedicated = gpu.add(r"\GPU Adapter Memory(*)\Dedicated Usage");
        let gpu_shared = gpu.add(r"\GPU Adapter Memory(*)\Shared Usage");
        Some(Self {
            q,
            gpu,
            mem_avail_mb,
            mem_committed,
            mem_commit_limit,
            standby_normal,
            standby_reserve,
            standby_core,
            modified,
            free_zero,
            cache_bytes,
            pool_paged,
            pool_nonpaged,
            cpu_perf,
            disk_idle,
            disk_read,
            disk_write,
            disk_latency,
            gpu_engine,
            gpu_dedicated,
            gpu_shared,
        })
    }
}

struct Collector {
    proc_query: nt::ProcessQuery,
    cpu_query: nt::CpuQuery,
    prev_times: HashMap<u32, ProcTimes>,
    prev_cores: Vec<nt::CoreTimes>,
    prev_net: HashMap<String, (u64, u64)>,
    prev_io: HashMap<u32, etw::IoCounters>,
    last_tick: Instant,
    identities: HashMap<(u32, i64), CachedIdentity>,
    live_keys: HashSet<(u32, i64)>,
    counters: Option<Counters>,
    history: Vec<HistPoint>,
    core_history: Vec<Vec<f32>>,
    seq: u64,
    collect_ema: f32,
    logical_cpus: usize,
    cpu_name: String,
    cpu_base_mhz: u32,
    gpu_name: String,
    etw: etw::EtwHandle,
    raw_buf: Vec<nt::RawProc>,
    net_buf: Vec<net::AdapterSample>,
    core_buf: Vec<nt::CoreTimes>,
    gpu_engine_cache: Vec<(String, f32)>,
    gpu_dedicated_cache: u64,
    gpu_shared_cache: u64,
    disk_space_cache: (u64, u64, u32),
    disk_space_tick: u64,
}

impl Collector {
    fn new(etw: etw::EtwHandle) -> Self {
        let logical = nt::logical_cpu_count();
        Self {
            proc_query: nt::ProcessQuery::new(),
            cpu_query: nt::CpuQuery::new(),
            prev_times: HashMap::with_capacity(512),
            prev_cores: Vec::new(),
            prev_net: HashMap::new(),
            prev_io: HashMap::new(),
            last_tick: Instant::now(),
            identities: HashMap::with_capacity(512),
            live_keys: HashSet::with_capacity(512),
            counters: Counters::new(),
            history: Vec::with_capacity(HISTORY_LEN),
            core_history: Vec::new(),
            seq: 0,
            collect_ema: 0.0,
            logical_cpus: logical,
            cpu_name: registry::cpu_name(),
            cpu_base_mhz: registry::cpu_base_mhz(),
            gpu_name: registry::gpu_name().unwrap_or_else(|| "Графический процессор".to_string()),
            etw,
            raw_buf: Vec::with_capacity(512),
            net_buf: Vec::new(),
            core_buf: Vec::with_capacity(logical),
            gpu_engine_cache: Vec::new(),
            gpu_dedicated_cache: 0,
            gpu_shared_cache: 0,
            disk_space_cache: (0, 0, 0),
            disk_space_tick: 0,
        }
    }

    fn collect(&mut self) -> Snapshot {
        let started = Instant::now();
        let profile = std::env::var_os("TM_PROFILE").is_some();
        let mut mark = started;
        let mut lap = |name: &str| {
            if profile {
                let now = Instant::now();
                eprintln!("  {name}: {:.1} мс", (now - mark).as_secs_f64() * 1000.0);
                mark = now;
            }
        };
        let elapsed = (started - self.last_tick).as_secs_f64().max(0.001);
        self.last_tick = started;
        self.seq += 1;

        let etw_active = self.etw.active.load(Ordering::Relaxed);

        // 1. Процессы (один системный вызов).
        self.raw_buf.clear();
        let procs_ok = self
            .proc_query
            .query(&mut self.raw_buf, etw_active)
            .is_ok();
        lap("процессы");

        // 2. Карта потоков для ETW-диска.
        if etw_active {
            let mut threads = HashMap::with_capacity(self.raw_buf.len() * 6);
            for p in &self.raw_buf {
                for t in &p.thread_ids {
                    threads.insert(*t, p.pid);
                }
            }
            self.etw.sink.threads.store(Arc::new(threads));
        }
        lap("потоки");

        // 3. Загрузка ядер.
        self.core_buf.clear();
        let _ = self.cpu_query.query(&mut self.core_buf);
        let mut core_pct = Vec::with_capacity(self.core_buf.len());
        let mut first_cpu_sample = false;
        if self.prev_cores.len() != self.core_buf.len() {
            self.prev_cores = self.core_buf.clone();
            first_cpu_sample = true;
        }
        let mut kernel_delta = 0i64;
        let mut total_delta = 0i64;
        for (i, cur) in self.core_buf.iter().enumerate() {
            let prev = self.prev_cores[i];
            let idle = (cur.idle - prev.idle).max(0);
            let kernel = (cur.kernel - prev.kernel).max(0);
            let user = (cur.user - prev.user).max(0);
            let total = (kernel + user).max(1);
            let pct = ((total - idle) as f32 / total as f32 * 100.0).clamp(0.0, 100.0);
            core_pct.push(if first_cpu_sample { 0.0 } else { pct });
            kernel_delta += kernel - idle;
            total_delta += total;
        }
        self.prev_cores.clone_from(&self.core_buf);
        let cpu_total = if core_pct.is_empty() {
            0.0
        } else {
            core_pct.iter().sum::<f32>() / core_pct.len() as f32
        };
        lap("ядра");
        let cpu_kernel = if total_delta > 0 {
            (kernel_delta as f32 / total_delta as f32 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        // 4. Системная сеть.
        self.net_buf.clear();
        self.net_buf.extend(net::query());
        let mut recv_delta = 0u64;
        let mut send_delta = 0u64;
        let mut link_bps = 0u64;
        let mut net_name = String::new();
        for a in &self.net_buf {
            if net_name.is_empty() {
                net_name.clone_from(&a.name);
            }
            link_bps = link_bps.max(a.link_bps);
            if let Some((pi, po)) = self.prev_net.get(&a.name) {
                recv_delta += a.in_octets.saturating_sub(*pi);
                send_delta += a.out_octets.saturating_sub(*po);
            }
        }
        self.prev_net = self
            .net_buf
            .iter()
            .map(|a| (a.name.clone(), (a.in_octets, a.out_octets)))
            .collect();
        lap("сеть");
        let net_recv_bps = recv_delta as f64 / elapsed;
        let net_send_bps = send_delta as f64 / elapsed;

        // 5. PDH-счётчики.
        let t_mem = Instant::now();
        let mi = mem::collect();
        if profile {
            eprintln!("    mem.collect: {:.1} мс", t_mem.elapsed().as_secs_f64() * 1000.0);
        }
        let mem_total = mi.total;
        let mut mem_avail = mi.avail;
        let mem_used;
        let mem_cached;
        let mem_free;
        let mem_modified;
        let mut mem_committed = mi.commit_total;
        let mut mem_commit_limit = mi.commit_limit;
        let mut mem_paged = 0u64;
        let mut mem_nonpaged = 0u64;
        let mut disk_active = 0.0f32;
        let mut disk_read_bps = 0.0f64;
        let mut disk_write_bps = 0.0f64;
        let mut disk_latency_ms = 0.0f64;
        let mut gpu = 0.0f32;
        let mut gpu_dedicated = 0u64;
        let mut gpu_shared = 0u64;
        let mut gpu_available = false;
        let mut cpu_perf = 100.0f64;
        let mut per_pid_gpu: HashMap<u32, f32> = HashMap::new();

        if let Some(c) = self.counters.as_mut() {
            let t0 = Instant::now();
            c.q.collect();
            if profile {
                eprintln!("    pdh.collect: {:.1} мс", t0.elapsed().as_secs_f64() * 1000.0);
            }
            let t1 = Instant::now();

            cpu_perf = c.q.value(c.cpu_perf).unwrap_or(100.0);
            if let Some(v) = c.q.value(c.mem_avail_mb) {
                mem_avail = (v.max(0.0) * 1024.0 * 1024.0) as u64;
            }
            if let Some(v) = c.q.value(c.mem_committed) {
                mem_committed = v.max(0.0) as u64;
            }
            if let Some(v) = c.q.value(c.mem_commit_limit) {
                mem_commit_limit = v.max(0.0) as u64;
            }
            if let Some(v) = c.q.value(c.pool_paged) {
                mem_paged = v.max(0.0) as u64;
            }
            if let Some(v) = c.q.value(c.pool_nonpaged) {
                mem_nonpaged = v.max(0.0) as u64;
            }

            let standby = c.q.value(c.standby_normal).unwrap_or(0.0)
                + c.q.value(c.standby_reserve).unwrap_or(0.0)
                + c.q.value(c.standby_core).unwrap_or(0.0);
            let modified = c.q.value(c.modified).unwrap_or(0.0);
            let free_zero = c.q.value(c.free_zero);
            let cache = c.q.value(c.cache_bytes).unwrap_or(0.0);

            if std::env::var_os("TM_SIMPLE_MEM").is_none() && (standby > 0.0 || modified > 0.0) {
                mem_free = free_zero.unwrap_or(mi.avail as f64).max(0.0) as u64;
                mem_modified = modified.max(0.0) as u64;
                mem_cached = (standby + cache).max(0.0) as u64;
                mem_used = mem_total.saturating_sub(mem_free + mem_modified + mem_cached);
            } else {
                mem_used = mem_total.saturating_sub(mi.avail);
                mem_cached = mi.cached;
                mem_free = mi.avail;
                mem_modified = 0;
            }

            if let Some(idle) = c.q.value(c.disk_idle) {
                disk_active = (100.0 - idle as f32).clamp(0.0, 100.0);
            }
            disk_read_bps = c.q.value(c.disk_read).unwrap_or(0.0).max(0.0);
            disk_write_bps = c.q.value(c.disk_write).unwrap_or(0.0).max(0.0);
            disk_latency_ms = c.q.value(c.disk_latency).unwrap_or(0.0).max(0.0) * 1000.0;

            // GPU-счётчики тяжёлые (сотни экземпляров) — раз в 3 тика.
            if profile {
                eprintln!("    pdh.values: {:.1} мс", t1.elapsed().as_secs_f64() * 1000.0);
            }
            let t2 = Instant::now();
            let refresh_gpu = self.seq % 3 == 1 || self.gpu_engine_cache.is_empty();
            if refresh_gpu {
                c.gpu.collect();
                self.gpu_engine_cache = c
                    .gpu
                    .array(c.gpu_engine)
                    .into_iter()
                    .map(|(name, v)| (name, v as f32))
                    .collect();
                self.gpu_dedicated_cache = c
                    .gpu
                    .array(c.gpu_dedicated)
                    .into_iter()
                    .map(|(_, v)| v.max(0.0) as u64)
                    .sum();
                self.gpu_shared_cache = c
                    .gpu
                    .array(c.gpu_shared)
                    .into_iter()
                    .map(|(_, v)| v.max(0.0) as u64)
                    .sum();
            }
            gpu_available = !self.gpu_engine_cache.is_empty();
            for (name, value) in &self.gpu_engine_cache {
                let v = value.clamp(0.0, 100.0);
                gpu = gpu.max(v);
                if let Some(pid) = pdh::instance_pid(name) {
                    let e = per_pid_gpu.entry(pid).or_insert(0.0);
                    *e += v;
                }
            }
            for v in per_pid_gpu.values_mut() {
                *v = v.clamp(0.0, 100.0);
            }
            gpu_dedicated = self.gpu_dedicated_cache;
            gpu_shared = self.gpu_shared_cache;
            if profile {
                eprintln!("    pdh.gpu: {:.1} мс", t2.elapsed().as_secs_f64() * 1000.0);
            }
        } else {
            mem_used = mem_total.saturating_sub(mem_avail);
            mem_cached = mi.cached;
            mem_free = mem_avail;
            mem_modified = 0;
        }
        lap("pdh");

        // 6. Список процессов.
        let windows = window::visible_windows();
        lap("окна");
        let mut procs = Vec::with_capacity(self.raw_buf.len());
        let mut next_times = HashMap::with_capacity(self.raw_buf.len());
        let mut next_io = HashMap::with_capacity(self.raw_buf.len());
        let mut total_threads = 0u32;
        let mut total_handles = 0u32;
        let mut identity_budget = if self.seq <= 1 {
            160
        } else {
            IDENTITY_BUDGET_PER_TICK
        };
        let mut ident_ms = 0.0f64;
        let mut ident_count = 0usize;
        self.live_keys.clear();
        let io_snapshot: Option<HashMap<u32, etw::IoCounters>> = if etw_active {
            Some(
                self.etw
                    .sink
                    .io
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone(),
            )
        } else {
            None
        };

        if procs_ok {
            for raw in self.raw_buf.iter() {
                let key = (raw.pid, raw.create_time);
                self.live_keys.insert(key);

                let ident = match self.identities.get(&key) {
                    Some(cached) => cached.clone(),
                    None => {
                        let is_app = windows.contains_key(&raw.pid);
                        if identity_budget > 0 || is_app {
                            if identity_budget > 0 {
                                identity_budget -= 1;
                            }
                            let t_ident = Instant::now();
                            let resolved =
                                CachedIdentity::from_raw(identity::resolve(raw.pid));
                            ident_ms += t_ident.elapsed().as_secs_f64() * 1000.0;
                            ident_count += 1;
                            self.identities.insert(key, resolved.clone());
                            resolved
                        } else {
                            // Не кэшируем пустую запись: дорезолвим на следующем тике.
                            CachedIdentity::default()
                        }
                    }
                };

                let prev = self.prev_times.get(&raw.pid).filter(|p| p.create_time == raw.create_time);
                let cpu = match prev {
                    Some(p) => {
                        let delta = (raw.kernel_time - p.kernel) + (raw.user_time - p.user);
                        (delta as f64 / (elapsed * self.logical_cpus as f64 * 10_000_000.0) * 100.0)
                            .clamp(0.0, 100.0) as f32
                    }
                    None => 0.0,
                };
                next_times.insert(
                    raw.pid,
                    ProcTimes {
                        create_time: raw.create_time,
                        kernel: raw.kernel_time,
                        user: raw.user_time,
                    },
                );

                let (disk_r, disk_w, net_r, net_s) = match io_snapshot.as_ref() {
                    Some(map) => {
                        let cur = map.get(&raw.pid).copied().unwrap_or_default();
                        let prev = self.prev_io.get(&raw.pid).copied().unwrap_or_default();
                        next_io.insert(raw.pid, cur);
                        (
                            cur.read.saturating_sub(prev.read) as f64 / elapsed,
                            cur.write.saturating_sub(prev.write) as f64 / elapsed,
                            cur.recv.saturating_sub(prev.recv) as f64 / elapsed,
                            cur.send.saturating_sub(prev.send) as f64 / elapsed,
                        )
                    }
                    None => (0.0, 0.0, 0.0, 0.0),
                };

                let win = windows.get(&raw.pid);
                let status = if raw.suspended {
                    ProcStatus::Suspended
                } else if win.map(|w| w.hung).unwrap_or(false) {
                    ProcStatus::NotResponding
                } else {
                    ProcStatus::Running
                };
                let group = if win.is_some() {
                    GroupKind::Apps
                } else if raw.session_id == 0 || is_windows_process(&raw.name) {
                    GroupKind::Windows
                } else {
                    GroupKind::Background
                };

                let working_set = if raw.working_set_private > 0 {
                    raw.working_set_private
                } else {
                    raw.working_set
                };
                let name = if raw.pid == 0 && raw.name.is_empty() {
                    "System Idle Process".to_string()
                } else {
                    raw.name.clone()
                };

                total_threads += raw.threads;
                total_handles += raw.handles;

                procs.push(ProcInfo {
                    pid: raw.pid,
                    parent_pid: raw.parent_pid,
                    create_time: raw.create_time,
                    name,
                    exe_path: ident.exe,
                    description: ident.desc,
                    user: ident.user,
                    elevated: ident.elevated,
                    session_id: raw.session_id,
                    cpu,
                    cpu_time: raw.kernel_time + raw.user_time,
                    working_set,
                    private_bytes: raw.private_bytes,
                    threads: raw.threads,
                    handles: raw.handles,
                    status,
                    window: win.map(|w| w.hwnd),
                    window_title: win.map(|w| w.title.clone()),
                    disk_read_bps: disk_r,
                    disk_write_bps: disk_w,
                    net_recv_bps: net_r,
                    net_send_bps: net_s,
                    gpu: per_pid_gpu.get(&raw.pid).copied().unwrap_or(0.0),
                    gpu_mem: 0,
                    group,
                });
            }
        }
        drop(io_snapshot);
        self.prev_times = next_times;
        self.prev_io = next_io;
        self.identities.retain(|k, _| self.live_keys.contains(k));
        if profile && ident_count > 0 {
            eprintln!(
                "    идентификация: {ident_count} шт, {ident_ms:.1} мс ({:.2} мс/шт)",
                ident_ms / ident_count as f64
            );
        }
        lap("строки");
        if std::env::var_os("TM_DEBUG_IDENT").is_some() {
            let with_path = self.identities.values().filter(|v| v.exe.is_some()).count();
            eprintln!(
                "ident tick {}: cache={} live={} with_path={} budget_left={}",
                self.seq,
                self.identities.len(),
                self.live_keys.len(),
                with_path,
                identity_budget
            );
        }

        // 7. История для вкладки «Производительность».
        let mem_total_for_graph = mem_total.max(1);
        let mem_pct = (mem_used as f64 / mem_total_for_graph as f64 * 100.0) as f32;
        self.history.push(HistPoint {
            cpu: cpu_total,
            mem: mem_pct.clamp(0.0, 100.0),
            disk: disk_active,
            net_recv: net_recv_bps as f32,
            net_send: net_send_bps as f32,
            gpu,
        });
        if self.history.len() > HISTORY_LEN {
            let excess = self.history.len() - HISTORY_LEN;
            self.history.drain(0..excess);
        }
        if self.core_history.len() != core_pct.len() {
            self.core_history.resize(core_pct.len(), Vec::new());
        }
        for (i, v) in core_pct.iter().enumerate() {
            let hist = &mut self.core_history[i];
            hist.push(*v);
            if hist.len() > HISTORY_LEN {
                let excess = hist.len() - HISTORY_LEN;
                hist.drain(0..excess);
            }
        }

        // 8. Системные показатели.
        let uptime_secs = unsafe {
            windows_sys::Win32::System::SystemInformation::GetTickCount64() / 1000
        };
        if self.disk_space_tick == 0 || self.seq - self.disk_space_tick >= 10 {
            self.disk_space_cache = mem::disk_space();
            self.disk_space_tick = self.seq;
        }
        let (disk_total, disk_free, disk_count) = self.disk_space_cache;
        let cpu_speed = if self.cpu_base_mhz > 0 {
            ((self.cpu_base_mhz as f64) * cpu_perf / 100.0) as u32
        } else {
            0
        };
        let capabilities = Capabilities {
            etw_active,
            etw_error: self.etw.status.lock().ok().and_then(|s| s.clone()),
            per_process_io: etw_active,
            gpu_available,
        };

        let actual_ms = started.elapsed().as_secs_f32() * 1000.0;
        self.collect_ema = if self.collect_ema == 0.0 {
            actual_ms
        } else {
            self.collect_ema * 0.8 + actual_ms * 0.2
        };

        Snapshot {
            seq: self.seq,
            elapsed,
            collect_ms: self.collect_ema,
            procs,
            system: SystemStats {
                cpu_total,
                cpu_kernel,
                cpu_speed_mhz: cpu_speed,
                cpu_base_mhz: self.cpu_base_mhz,
                cpu_name: self.cpu_name.clone(),
                cores_physical: self.logical_cpus as u32,
                cores_logical: self.logical_cpus as u32,
                processes: self.raw_buf.len() as u32,
                threads: total_threads,
                handles: total_handles,
                uptime_secs,
                mem_total,
                mem_avail,
                mem_used,
                mem_cached,
                mem_free,
                mem_modified,
                mem_committed,
                mem_commit_limit,
                mem_paged_pool: mem_paged,
                mem_nonpaged_pool: mem_nonpaged,
                mem_speed_mhz: 0,
                disk_active,
                disk_read_bps,
                disk_write_bps,
                disk_response_ms: disk_latency_ms,
                disk_total,
                disk_free,
                disk_count,
                net_recv_bps,
                net_send_bps,
                net_link_bps: link_bps,
                net_name: if net_name.is_empty() {
                    "Сеть".to_string()
                } else {
                    net_name
                },
                gpu,
                gpu_dedicated,
                gpu_shared,
                gpu_name: self.gpu_name.clone(),
                admin: is_admin(),
                elevated: is_admin(),
            },
            history: self.history.clone(),
            core_history: self.core_history.clone(),
            capabilities,
        }
    }
}

fn is_windows_process(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "system"
            | "registry"
            | "smss.exe"
            | "csrss.exe"
            | "wininit.exe"
            | "winlogon.exe"
            | "services.exe"
            | "lsass.exe"
            | "svchost.exe"
            | "fontdrvhost.exe"
            | "dwm.exe"
            | "spoolsv.exe"
            | "taskhostw.exe"
            | "sihost.exe"
            | "ctfmon.exe"
            | "runtimebroker.exe"
            | "searchindexer.exe"
            | "securityhealthservice.exe"
            | "audiodg.exe"
            | "conhost.exe"
            | "wudfhost.exe"
            | "sppsvc.exe"
    )
}

static ADMIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn is_admin() -> bool {
    *ADMIN.get_or_init(|| {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        unsafe {
            let mut token = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elev = TOKEN_ELEVATION::default();
            let mut size = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                &mut elev as *mut _ as *mut _,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            );
            CloseHandle(token);
            ok != 0 && elev.TokenIsElevated != 0
        }
    })
}

pub enum Cmd {
    SetInterval(u64),
    Pause(bool),
    Refresh,
    Stop,
}

pub struct Sampler {
    snapshot: Arc<ArcSwap<Snapshot>>,
    tx: Mutex<Sender<Cmd>>,
    join: Mutex<Option<JoinHandle<()>>>,
    etw: Option<Etw>,
}

impl Sampler {
    pub fn start(interval_ms: u64) -> Sampler {
        let snapshot = Arc::new(ArcSwap::from_pointee(Snapshot::default()));
        let (tx, rx) = mpsc::channel();

        let etw = Etw::start();
        let mut collector = Collector::new(etw.handle.clone());

        let shared = snapshot.clone();
        let join = std::thread::Builder::new()
            .name("tm-sampler".into())
            .spawn(move || {
                let mut interval = Duration::from_millis(interval_ms.max(100));
                let mut paused = false;
                let mut next = Instant::now();
                loop {
                    let timeout = if paused {
                        Duration::from_millis(200)
                    } else {
                        next.saturating_duration_since(Instant::now())
                    };
                    match rx.recv_timeout(timeout) {
                        Ok(Cmd::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                        Ok(Cmd::SetInterval(ms)) => {
                            interval = Duration::from_millis(ms.max(100));
                            next = Instant::now() + interval;
                        }
                        Ok(Cmd::Pause(p)) => {
                            paused = p;
                            if !paused {
                                next = Instant::now();
                            }
                        }
                        Ok(Cmd::Refresh) => {
                            paused = false;
                            next = Instant::now();
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            if paused {
                                continue;
                            }
                            let snap = collector.collect();
                            shared.store(Arc::new(snap));
                            next = Instant::now() + interval;
                        }
                    }
                }
            })
            .ok();

        Sampler {
            snapshot,
            tx: Mutex::new(tx),
            join: Mutex::new(join),
            etw: Some(etw),
        }
    }

    pub fn latest(&self) -> Arc<Snapshot> {
        self.snapshot.load_full()
    }

    pub fn set_interval(&self, ms: u64) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(Cmd::SetInterval(ms));
        }
    }

    pub fn set_paused(&self, paused: bool) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(Cmd::Pause(paused));
        }
    }

    pub fn refresh_now(&self) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(Cmd::Refresh);
        }
    }

    pub fn etw_error(&self) -> Option<String> {
        self.etw.as_ref().and_then(|e| e.error())
    }

    pub fn etw_active(&self) -> bool {
        self.etw.as_ref().map(|e| e.active()).unwrap_or(false)
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(Cmd::Stop);
        }
        if let Ok(mut join) = self.join.lock() {
            if let Some(j) = join.take() {
                let _ = j.join();
            }
        }
        // ETW останавливаем после сэмплера, чтобы не потерять буферы.
        self.etw.take();
    }
}
