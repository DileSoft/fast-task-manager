//! Системная сеть: все активные адаптеры через GetIfTable2.
//!
//! Отбрасываем только петлевые/туннельные/выключенные интерфейсы. Понятное
//! имя берём из `Alias` («Ethernet», «Wi-Fi»), а служебное описание — из
//! `Description`; фильтрация виртуальных дублей выполняется в сэмплере.

use windows_sys::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

use super::from_wide_buf;

const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
const IF_TYPE_TUNNEL: u32 = 131;
const IF_OPER_STATUS_UP: i32 = 1;

#[derive(Clone, Debug)]
pub struct AdapterSample {
    /// Понятное имя подключения.
    pub name: String,
    /// Описание драйвера/физического адаптера.
    pub description: String,
    pub in_octets: u64,
    pub out_octets: u64,
    pub link_bps: u64,
}

pub fn query() -> Vec<AdapterSample> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    let rc = unsafe { GetIfTable2(&mut table) };
    if rc != 0 || table.is_null() {
        return Vec::new();
    }

    let mut out = Vec::new();
    unsafe {
        let num = (*table).NumEntries as usize;
        let rows = std::ptr::addr_of!((*table).Table)
            as *const windows_sys::Win32::NetworkManagement::IpHelper::MIB_IF_ROW2;
        for i in 0..num {
            let row = &*rows.add(i);
            if row.Type == IF_TYPE_SOFTWARE_LOOPBACK || row.Type == IF_TYPE_TUNNEL {
                continue;
            }
            if row.OperStatus as i32 != IF_OPER_STATUS_UP {
                continue;
            }
            let description = from_wide_buf(&row.Description);
            let alias = from_wide_buf(&row.Alias);
            let name = if alias.is_empty() {
                description.clone()
            } else {
                alias
            };
            out.push(AdapterSample {
                name,
                description,
                in_octets: row.InOctets,
                out_octets: row.OutOctets,
                link_bps: row.ReceiveLinkSpeed.max(row.TransmitLinkSpeed),
            });
        }
        FreeMibTable(table as *const _);
    }
    out
}
