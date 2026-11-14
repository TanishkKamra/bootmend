//! Drive health: NVMe health log via IOCTL, ATA SMART tables from WMI, and Windows'
//! own reliability counters, merged into one record per physical disk.

use crate::snapshot::{DriveHealth, SmartAttr};
use crate::util::J;
use anyhow::{anyhow, Result};
use serde_json::Value;

const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
const STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY: u32 = 50;
const PROTOCOL_TYPE_NVME: u32 = 3;
const NVME_DATA_TYPE_LOG_PAGE: u32 = 2;
const NVME_LOG_PAGE_HEALTH_INFO: u32 = 2;
const HEADER: usize = 8; // PropertyId + QueryType, or Version + Size on the way back
const PROTO: usize = 40; // STORAGE_PROTOCOL_SPECIFIC_DATA

#[derive(Debug, Clone, PartialEq)]
pub struct NvmeHealth {
    pub critical_warning: u8,
    pub temp_c: Option<f64>,
    pub spare_pct: u8,
    pub spare_threshold: u8,
    pub used_pct: u8,
    pub tb_read: f64,
    pub tb_written: f64,
    pub power_cycles: u128,
    pub power_on_hours: u128,
    pub unsafe_shutdowns: u128,
    pub media_errors: u128,
}

fn u128le(b: &[u8]) -> u128 {
    b.iter().rev().fold(0u128, |acc, x| (acc << 8) | *x as u128)
}

/// Parse the 512-byte NVMe "SMART / Health Information" log page (NVMe base spec, log id 02h).
pub fn parse_nvme_health(b: &[u8]) -> Option<NvmeHealth> {
    if b.len() < 192 {
        return None;
    }
    let kelvin = u16::from_le_bytes([b[1], b[2]]);
    // Data units are thousands of 512-byte blocks.
    let tb = |r: &[u8]| u128le(r) as f64 * 512_000.0 / 1e12;
    Some(NvmeHealth {
        critical_warning: b[0],
        temp_c: (kelvin > 200).then_some(kelvin as f64 - 273.0),
        spare_pct: b[3],
        spare_threshold: b[4],
        used_pct: b[5],
        tb_read: tb(&b[32..48]),
        tb_written: tb(&b[48..64]),
        power_cycles: u128le(&b[112..128]),
        power_on_hours: u128le(&b[128..144]),
        unsafe_shutdowns: u128le(&b[144..160]),
        media_errors: u128le(&b[160..176]),
    })
}

struct Handle(windows::Win32::Foundation::HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        // Nothing useful to do if closing fails; the handle dies with the process anyway.
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.0) };
    }
}

/// Ask the NVMe driver for the health log of `\\.\PhysicalDriveN`.
pub fn read_nvme_health(disk: i64) -> Result<NvmeHealth> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
    use windows::Win32::System::IO::DeviceIoControl;

    let path = HSTRING::from(format!(r"\\.\PhysicalDrive{disk}"));
    let open = |access: u32| unsafe { CreateFileW(&path, access, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, FILE_FLAGS_AND_ATTRIBUTES(0), None) };
    // Read/write access needs administrator; zero access works for many drivers without it.
    let h = Handle(open(GENERIC_READ.0 | GENERIC_WRITE.0).or_else(|_| open(0))?);

    let mut buf = vec![0u8; HEADER + PROTO + 512];
    let put = |b: &mut [u8], at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    put(&mut buf, 0, STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY);
    put(&mut buf, 4, 0); // PropertyStandardQuery
    put(&mut buf, HEADER, PROTOCOL_TYPE_NVME);
    put(&mut buf, HEADER + 4, NVME_DATA_TYPE_LOG_PAGE);
    put(&mut buf, HEADER + 8, NVME_LOG_PAGE_HEALTH_INFO);
    put(&mut buf, HEADER + 16, PROTO as u32); // ProtocolDataOffset
    put(&mut buf, HEADER + 20, 512); // ProtocolDataLength

    let mut returned = 0u32;
    let len = buf.len() as u32;
    let p = buf.as_mut_ptr() as *mut std::ffi::c_void;
    unsafe { DeviceIoControl(h.0, IOCTL_STORAGE_QUERY_PROPERTY, Some(p), len, Some(p), len, Some(&mut returned), None)? };

    let rd = |at: usize| u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]]) as usize;
    let (off, n) = (rd(HEADER + 16), rd(HEADER + 20));
    let start = HEADER + off;
    if n < 192 || start + n > buf.len() {
        return Err(anyhow!("the drive returned an unexpected health log"));
    }
    parse_nvme_health(&buf[start..start + n]).ok_or_else(|| anyhow!("health log too short"))
}

pub fn attr_name(id: u8) -> &'static str {
    match id {
        1 => "Read error rate",
        5 => "Reallocated sectors",
        9 => "Power-on hours",
        10 => "Spin retry count",
        12 => "Power cycles",
        169 => "Remaining life",
        171 => "Program fail count",
        172 => "Erase fail count",
        173 => "Wear leveling count",
        174 => "Unexpected power loss",
        177 => "Wear leveling count",
        181 => "Program fail count",
        182 => "Erase fail count",
        183 => "Runtime bad blocks",
        184 => "End-to-end errors",
        187 => "Reported uncorrectable errors",
        188 => "Command timeouts",
        190 | 194 => "Temperature",
        192 => "Power-off retracts",
        196 => "Reallocation events",
        197 => "Pending sectors",
        198 => "Uncorrectable sectors",
        199 => "Cable (CRC) errors",
        202 => "Remaining life",
        231 => "Remaining life",
        233 => "Media wearout",
        241 => "Total writes",
        242 => "Total reads",
        _ => "Vendor specific",
    }
}

/// Parse the 512-byte ATA SMART data table (30 entries of 12 bytes after a 2-byte revision).
pub fn parse_ata(data: &[u8]) -> Vec<SmartAttr> {
    (0..30)
        .filter_map(|i| {
            let o = 2 + i * 12;
            let e = data.get(o..o + 12)?;
            (e[0] != 0).then(|| SmartAttr { id: e[0], name: attr_name(e[0]).into(), value: e[3], worst: e[4], raw: u128le(&e[5..11]) as u64 })
        })
        .collect()
}

fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase()
}

/// WMI instance names look like `SCSI\Disk&Ven_&Prod_ST1000DM010-2EP1\4&...`; match on the model prefix.
pub fn instance_matches(instance: &str, model: &str) -> bool {
    let i = norm(instance);
    // The device path may hold the vendor ("WDC") in a separate field, so also try the model without it.
    let product = model.split_once(' ').map(|(_, rest)| rest).unwrap_or(model);
    [model, product].iter().any(|m| {
        let key: String = norm(m).chars().take(14).collect();
        key.len() >= 6 && i.contains(&key)
    })
}

pub fn drive_health(storage: &Value) -> Vec<DriveHealth> {
    let ata = storage.a("ataSmart");
    let predict = storage.a("failurePredict");
    storage
        .a("disks")
        .into_iter()
        .map(|d| {
            let model = d.s("Model").unwrap_or_else(|| "Unknown drive".into());
            let bus = d.s("Bus").unwrap_or_default();
            let media = d.s("Media").unwrap_or_default();
            let kind = match (bus.as_str(), media.as_str()) {
                ("NVMe", _) => "NVMe SSD".to_string(),
                (b, "SSD") => format!("{b} SSD"),
                (b, "HDD") => format!("{b} hard disk"),
                (b, _) => format!("{b} drive"),
            };
            let mut h = DriveHealth {
                disk: d.i("Number").unwrap_or(-1),
                model: model.clone(),
                kind,
                // Hard disks don't wear like SSDs; Windows reports 0 for them, which would read as "like new".
                life_used_pct: d.f("WearPct").filter(|_| media != "HDD"),
                power_on_hours: d.f("PowerOnHours"),
                temp_c: d.f("TempC").filter(|t| *t > 0.0),
                uncorrectable: match (d.f("ReadErrorsUncorrected"), d.f("WriteErrorsUncorrected")) {
                    (None, None) => None,
                    (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
                },
                source: "Windows reliability counters".into(),
                ..Default::default()
            };
            if bus == "NVMe" {
                match read_nvme_health(h.disk) {
                    Ok(n) => {
                        h.life_used_pct = Some(n.used_pct as f64);
                        h.temp_c = n.temp_c.or(h.temp_c);
                        h.spare_pct = Some(n.spare_pct as f64);
                        h.tb_written = Some(n.tb_written);
                        h.power_on_hours = Some(n.power_on_hours as f64);
                        h.unsafe_shutdowns = Some(n.unsafe_shutdowns as f64);
                        h.media_errors = Some(n.media_errors as f64);
                        h.predict_failure = Some(n.critical_warning != 0);
                        h.source = "NVMe health log".into();
                    }
                    Err(e) => eprintln!("NVMe health for disk {}: {e}", h.disk),
                }
            } else if let Some(a) = ata.iter().find(|a| a.s("Instance").is_some_and(|i| instance_matches(&i, &model))) {
                let bytes: Vec<u8> = a.a("Data").iter().filter_map(|x| x.as_u64()).map(|x| x as u8).collect();
                let attrs = parse_ata(&bytes);
                let raw = |id: u8| attrs.iter().find(|x| x.id == id).map(|x| x.raw);
                let val = |id: u8| attrs.iter().find(|x| x.id == id).map(|x| x.value);
                h.reallocated = raw(5).map(|r| r as f64);
                h.pending = raw(197).map(|r| r as f64);
                h.uncorrectable = raw(198).map(|r| r as f64).or(h.uncorrectable);
                // Crucial MX500s flip 197 between 0 and 1 by themselves on every firmware, and no
                // update fixes it (smartmontools ticket #1227). A lone 1 with no uncorrectable
                // errors isn't a pending sector; real damage shows in 187 or 198 too.
                if model.to_ascii_uppercase().contains("MX500") && raw(197) == Some(1) && raw(187).unwrap_or(0) == 0 && h.uncorrectable.unwrap_or(0.0) == 0.0 {
                    h.pending = None;
                }
                h.power_on_hours = raw(9).map(|r| (r & 0xFFFF_FFFF) as f64).or(h.power_on_hours);
                h.temp_c = raw(194).map(|r| (r & 0xFF) as f64).filter(|t| *t > 0.0).or(h.temp_c);
                if media == "SSD" {
                    if let Some(v) = [231u8, 233, 202, 169, 177].iter().find_map(|id| val(*id).filter(|v| *v <= 100)) {
                        h.life_used_pct = Some(100.0 - v as f64);
                    }
                }
                h.predict_failure = predict.iter().find(|p| p.s("Instance").is_some_and(|i| instance_matches(&i, &model))).and_then(|p| p.b("PredictFailure"));
                h.attributes = attrs;
                h.source = "SMART table".into();
            }
            h
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvme_health_log_is_parsed() {
        let mut b = vec![0u8; 512];
        b[1..3].copy_from_slice(&313u16.to_le_bytes()); // 40 C
        b[3] = 100;
        b[4] = 10;
        b[5] = 7;
        b[48..56].copy_from_slice(&20_000_000u64.to_le_bytes()); // 10.24 TB
        b[128..136].copy_from_slice(&5000u64.to_le_bytes());
        b[144..152].copy_from_slice(&42u64.to_le_bytes());
        let h = parse_nvme_health(&b).expect("parsed");
        assert_eq!(h.temp_c, Some(40.0));
        assert_eq!(h.used_pct, 7);
        assert!((h.tb_written - 10.24).abs() < 0.001);
        assert_eq!(h.power_on_hours, 5000);
        assert_eq!(h.unsafe_shutdowns, 42);
        assert!(parse_nvme_health(&b[..100]).is_none());
    }

    #[test]
    fn ata_table_is_parsed() {
        let mut d = vec![0u8; 512];
        // entry 0: id 5, value 100, worst 100, raw 8
        d[2] = 5;
        d[5] = 100;
        d[6] = 100;
        d[7] = 8;
        // entry 1: id 194, raw 0x0000_0012_0029 -> low byte 41 C
        d[14] = 194;
        d[17] = 59;
        d[18] = 40;
        d[19] = 41;
        d[21] = 18;
        let a = parse_ata(&d);
        assert_eq!(a.len(), 2);
        assert_eq!((a[0].id, a[0].raw, a[0].value), (5, 8, 100));
        assert_eq!(a[1].raw & 0xFF, 41);
        assert_eq!(a[1].name, "Temperature");
    }

    #[test]
    fn wmi_instance_matches_model() {
        assert!(instance_matches(r"SCSI\Disk&Ven_&Prod_ST1000DM010-2EP1\4&2a5e&0&000100_0", "ST1000DM010-2EP102"));
        assert!(instance_matches(r"IDE\DiskSamsung_SSD_860_EVO_500GB_____RVT02B6Q\5&1", "Samsung SSD 860 EVO 500GB"));
        assert!(!instance_matches(r"SCSI\Disk&Ven_WDC&Prod_WD10EZEX\4&1", "ST1000DM010-2EP102"));
        // Windows splits "WDC WD10EZEX-22MFCA0" into separate vendor and product fields.
        assert!(instance_matches(r"SCSI\Disk&Ven_WDC&Prod_WD10EZEX-22MFCA0\5&2b63&0&000000_0", "WDC WD10EZEX-22MFCA0"));
    }
}
