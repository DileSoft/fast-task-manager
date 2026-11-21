//! Перечисление процессов и процессорного времени через
//! `NtQuerySystemInformation` — один системный вызов на тик.

use ntapi::ntexapi::{
    NtQuerySystemInformation, SystemProcessInformation, SystemProcessorPerformanceInformation,
    SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION, SYSTEM_PROCESS_INFORMATION,
    SYSTEM_THREAD_INFORMATION,
};

use super::from_wide_ptr;

const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004u32 as i32;

/// Причина ожидания `Suspended` (KWAIT_REASON) и состояние `Waiting` (KTHREAD_STATE).
const KTHREAD_STATE_WAITING: i32 = 5;
const KWAIT_REASON_SUSPENDED: i32 = 5;

#[derive(Clone, Debug, Default)]
pub struct RawProc {
    pub pid: u32,
    pub parent_pid: u32,
    pub create_time: i64,
    pub kernel_time: i64,
    pub user_time: i64,
    pub working_set: u64,
    pub working_set_private: u64,
    pub private_bytes: u64,
    pub threads: u32,
    pub handles: u32,
    pub session_id: u32,
    pub name: String,
    pub suspended: bool,
    pub thread_ids: Vec<u32>,
}

/// Переиспользуемый буфер для системного вызова: размер стабилизируется
/// после первых тиков и дальше память не выделяется.
pub struct ProcessQuery {
    buf: Vec<u64>,
}

impl Default for ProcessQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessQuery {
    pub fn new() -> Self {
        Self {
            buf: vec![0u64; 256 * 1024 / 8],
        }
    }

    /// Заполняет `out` свежим списком процессов. `with_threads` включает
    /// идентификаторы потоков (нужны только для ETW-маппинга).
    pub fn query(
        &mut self,
        out: &mut Vec<RawProc>,
        with_threads: bool,
    ) -> Result<(), &'static str> {
        let mut required: u32 = 0;
        let mut attempt = 0;
        loop {
            let bytes = self.buf.len() * 8;
            let status = unsafe {
                NtQuerySystemInformation(
                    SystemProcessInformation,
                    self.buf.as_mut_ptr() as *mut _,
                    bytes as u32,
                    &mut required,
                )
            };
            if status == STATUS_INFO_LENGTH_MISMATCH {
                let need = (required.max(bytes as u32) as usize + 256 * 1024) & !7usize;
                self.buf.resize(need / 8, 0);
                attempt += 1;
                if attempt > 4 {
                    return Err("Не удалось получить список процессов");
                }
                continue;
            }
            if status < 0 {
                return Err("NtQuerySystemInformation вернул ошибку");
            }
            break;
        }

        out.clear();
        // Хвостовой SYSTEM_THREAD_INFORMATION[1] учитывается в размере структуры.
        let thread_size = std::mem::size_of::<SYSTEM_THREAD_INFORMATION>();
        let base_size = std::mem::size_of::<SYSTEM_PROCESS_INFORMATION>() - thread_size;

        let mut ptr = self.buf.as_ptr() as *const u8;
        loop {
            let info = unsafe { &*(ptr as *const SYSTEM_PROCESS_INFORMATION) };

            let name = unsafe {
                from_wide_ptr(info.ImageName.Buffer, (info.ImageName.Length as usize) / 2)
            };

            let mut suspended = info.NumberOfThreads > 0;
            let mut thread_ids = Vec::new();
            if with_threads {
                thread_ids.reserve(info.NumberOfThreads as usize);
            }
            let threads_ptr = unsafe { ptr.add(base_size) } as *const SYSTEM_THREAD_INFORMATION;
            for i in 0..info.NumberOfThreads as usize {
                let t = unsafe { &*threads_ptr.add(i) };
                let state = t.ThreadState as i32;
                let reason = t.WaitReason as i32;
                if !(state == KTHREAD_STATE_WAITING && reason == KWAIT_REASON_SUSPENDED) {
                    suspended = false;
                }
                if with_threads {
                    thread_ids.push(t.ClientId.UniqueThread as usize as u32);
                }
            }

            out.push(RawProc {
                pid: info.UniqueProcessId as usize as u32,
                parent_pid: info.InheritedFromUniqueProcessId as usize as u32,
                create_time: unsafe { *info.CreateTime.QuadPart() },
                kernel_time: unsafe { *info.KernelTime.QuadPart() },
                user_time: unsafe { *info.UserTime.QuadPart() },
                working_set: info.WorkingSetSize as u64,
                working_set_private: unsafe { *info.WorkingSetPrivateSize.QuadPart() as u64 },
                private_bytes: info.PrivatePageCount as u64,
                threads: info.NumberOfThreads,
                handles: info.HandleCount,
                session_id: info.SessionId,
                name,
                suspended,
                thread_ids,
            });

            if info.NextEntryOffset == 0 {
                break;
            }
            ptr = unsafe { ptr.add(info.NextEntryOffset as usize) };
        }

        Ok(())
    }
}

/// Дельты процессорного времени по каждому логическому ядру.
#[derive(Clone, Copy, Debug, Default)]
pub struct CoreTimes {
    pub idle: i64,
    pub kernel: i64,
    pub user: i64,
}

pub struct CpuQuery {
    buf: Vec<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>,
}

impl Default for CpuQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuQuery {
    pub fn new() -> Self {
        let n = logical_cpu_count().max(1);
        Self {
            buf: vec![unsafe { std::mem::zeroed() }; n],
        }
    }

    pub fn query(&mut self, out: &mut Vec<CoreTimes>) -> Result<(), &'static str> {
        let mut required = 0u32;
        let status = unsafe {
            NtQuerySystemInformation(
                SystemProcessorPerformanceInformation,
                self.buf.as_mut_ptr() as *mut _,
                (self.buf.len() * std::mem::size_of::<SYSTEM_PROCESSOR_PERFORMANCE_INFORMATION>())
                    as u32,
                &mut required,
            )
        };
        if status < 0 {
            return Err("Не удалось получить загрузку процессора");
        }
        out.clear();
        out.extend(self.buf.iter().map(|c| CoreTimes {
            idle: unsafe { *c.IdleTime.QuadPart() },
            kernel: unsafe { *c.KernelTime.QuadPart() },
            user: unsafe { *c.UserTime.QuadPart() },
        }));
        Ok(())
    }
}

pub fn logical_cpu_count() -> usize {
    let n = unsafe { windows_sys::Win32::System::Threading::GetActiveProcessorCount(0xFFFF) };
    if n == 0 {
        1
    } else {
        n as usize
    }
}
