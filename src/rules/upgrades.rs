//! Upgrade advisor: which part to change first for the biggest difference, and what exactly to buy.
//! Built only from what the check measured; no prices or product picks.

use crate::snapshot::Snapshot;
use crate::util::J;

#[derive(Debug, Clone, PartialEq)]
pub struct Upgrade {
    pub id: &'static str,
    pub title: String,
    pub why: String,
    /// What to look for when buying, in plain words.
    pub get: Vec<String>,
}

/// The memory maker's name. Windows often reports "Unknown", but part numbers start with a maker code.
pub fn ram_maker(reported: Option<&str>, part: &str) -> Option<String> {
    if let Some(r) = reported.filter(|r| !r.is_empty() && !r.eq_ignore_ascii_case("unknown") && !r.starts_with("0x")) {
        return Some(r.to_string());
    }
    let codes = [
        ("CM", "Corsair"),
        ("F4-", "G.Skill"),
        ("F5-", "G.Skill"),
        ("F3-", "G.Skill"),
        ("KF", "Kingston"),
        ("KVR", "Kingston"),
        ("HX", "Kingston"),
        ("CT", "Crucial"),
        ("BL", "Crucial"),
        ("M378", "Samsung"),
        ("M471", "Samsung"),
        ("M425", "Samsung"),
        ("HMA", "SK hynix"),
        ("HMT", "SK hynix"),
        ("TF", "TeamGroup"),
        ("TED", "TeamGroup"),
        ("PSD", "Patriot"),
    ];
    let p = part.trim().to_ascii_uppercase();
    codes.iter().find(|(c, _)| p.starts_with(c)).map(|(_, m)| m.to_string())
}

/// Upgrades in order of how much difference they make, biggest first.
pub fn upgrades(s: &Snapshot) -> Vec<Upgrade> {
    let mut out = vec![];
    let dev = s.system.g("device");
    let laptop = dev.b("HasBattery") == Some(true);
    let board = format!("{} {}", dev.s("BoardMaker").unwrap_or_default(), dev.s("BoardProduct").unwrap_or_default()).trim().to_string();

    let system_disk = s.storage.a("disks").into_iter().find(|d| d.b("IsSystem") == Some(true));
    if system_disk.and_then(|d| d.s("Media")).as_deref() == Some("HDD") {
        let mut get = vec!["An SSD of 500 GB or more. This is the single biggest speed-up an older PC can get.".to_string()];
        get.push(if laptop {
            "Check whether the laptop takes a 2.5-inch drive or an M.2 drive; its manual or the maker's site says which.".into()
        } else if board.is_empty() {
            "If the motherboard has an M.2 slot, an NVMe SSD there is fastest. Otherwise any 2.5-inch SATA SSD fits.".into()
        } else {
            format!("Check the {board} manual for an M.2 slot: an NVMe SSD there is fastest. Otherwise any 2.5-inch SATA SSD fits.")
        });
        get.push("Most SSD makers offer a free tool that copies Windows and your files across.".into());
        out.push(Upgrade {
            id: "ssd",
            title: "Move Windows to an SSD".into(),
            why: "Windows runs from a spinning hard disk, which is the slowest part of this PC. Start-up, opening apps and updates all wait on it.".into(),
            get,
        });
    }

    let mem = s.system.g("memory");
    let sticks = mem.a("Sticks");
    let total = mem.f("TotalGB").unwrap_or(0.0);
    let slots = mem.i("Slots").unwrap_or(0).max(sticks.len() as i64);
    let free = slots - sticks.len() as i64;
    let first = sticks.first();
    let kind = first.map(|m| {
        let form = if m.s("FormFactor").as_deref() == Some("SODIMM") { "laptop memory (SODIMM)" } else { "desktop memory (DIMM)" };
        format!("{} {form}", m.s("Type").unwrap_or_else(|| "Same type of".into()))
    });
    let speed = first.and_then(|m| m.i("SpeedMTs")).map(|v| format!(", {v} MT/s or faster")).unwrap_or_default();
    let soldered_note = "Some laptops have part or all of their memory soldered in; check the laptop's manual before buying.";
    if total > 0.0 && total < 7.5 {
        let mut get = vec![];
        if let Some(k) = &kind {
            get.push(format!("{k}{speed}"));
        }
        get.push(if free > 0 {
            format!("Free memory slots: {free} of {slots}. Adding a stick the same size as the one you have keeps both channels working.")
        } else {
            format!("All {slots} slots are full, so replace the sticks with a matching pair, such as 2 × 8 GB.")
        });
        if laptop {
            get.push(soldered_note.into());
        }
        out.push(Upgrade {
            id: "ram-more",
            title: "Add memory to reach 16 GB".into(),
            why: format!(
                "This PC has {total:.0} GB. Windows with a browser and a couple of apps open already uses most of that, so the PC keeps swapping to the drive."
            ),
            get,
        });
    } else if sticks.len() == 1 && slots >= 2 {
        let m = first.expect("one stick");
        let mut get = vec![format!("{} GB {}{speed}", m.i("SizeGB").unwrap_or(0), kind.clone().unwrap_or_default())];
        if let Some(part) = m.s("Part").filter(|p| !p.is_empty()) {
            let maker = ram_maker(m.s("Maker").as_deref(), &part).map(|m| format!("{m} ")).unwrap_or_default();
            get.push(format!("Same model if you can find it: {maker}{part}"));
        }
        get.push(format!("Free memory slots: {free} of {slots}"));
        if laptop {
            get.push(soldered_note.into());
        }
        out.push(Upgrade {
            id: "ram-pair",
            title: "Add a second, matching memory stick".into(),
            why: "With one stick, memory runs on one channel and moves data at half the rate it could. A matching second stick doubles it, which games and integrated graphics notice most.".into(),
            get,
        });
    }

    if let Some(c) = s.storage.a("volumes").into_iter().find(|v| v.s("Letter").as_deref() == Some("C:")) {
        let (size, free_gb) = (c.f("SizeGB").unwrap_or(0.0), c.f("FreeGB").unwrap_or(0.0));
        if size > 0.0 && (free_gb < 20.0 || free_gb / size < 0.1) {
            out.push(Upgrade {
                id: "bigger-drive",
                title: "A bigger drive, or a second one".into(),
                why: format!("Drive C has {free_gb:.0} GB free of {size:.0} GB. Clearing space helps for a while, but if it fills up again, the drive is simply too small for how you use it."),
                get: vec![
                    "Try clearing space first; it may be enough.".into(),
                    if laptop {
                        "Laptops usually have room for one drive, so look for a bigger SSD and copy Windows across with the maker's free tool.".into()
                    } else {
                        "A second SSD of 1 TB or more for games and big files keeps Windows' drive free.".into()
                    },
                ],
            });
        }
    }

    let b = s.system.g("battery");
    if let (Some(design), Some(full)) = (b.f("DesignmWh"), b.f("FullChargemWh")) {
        if laptop && design > 0.0 && full / design < 0.7 {
            let model = dev.s("Model").map(|m| format!(" ({m})")).unwrap_or_default();
            out.push(Upgrade {
                id: "battery",
                title: "Replace the battery".into(),
                why: format!(
                    "It holds {:.0}% of its original charge, so time away from the charger is much shorter than when it was new.",
                    full / design * 100.0
                ),
                get: vec![
                    format!("A battery made for this laptop model{model}, rated about {:.0} Wh.", design / 1000.0),
                    "The maker's own service or an authorised repair shop is the safest choice; cheap unbranded batteries are a fire risk.".into(),
                ],
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pc() -> Snapshot {
        Snapshot {
            system: json!({
                "os": {"Edition": "Microsoft Windows 11 Pro"},
                "cpu": {"Name": "AMD Ryzen 5 3600"},
                "device": {"BoardMaker": "ASUS", "BoardProduct": "TUF B450-PLUS", "HasBattery": false},
                "memory": {"TotalGB": 16, "Slots": 4, "Sticks": [
                    {"SizeGB": 8, "Type": "DDR4", "SpeedMTs": 3200, "FormFactor": "DIMM", "Maker": "Kingston", "Part": "KF432C16BB/8"},
                    {"SizeGB": 8, "Type": "DDR4", "SpeedMTs": 3200, "FormFactor": "DIMM", "Maker": "Kingston", "Part": "KF432C16BB/8"}
                ]}
            }),
            storage: json!({
                "disks": [{"Number": 0, "IsSystem": true, "Media": "SSD", "Model": "CT500MX500SSD1"}],
                "volumes": [{"Letter": "C:", "SizeGB": 465, "FreeGB": 200}]
            }),
            ..Default::default()
        }
    }

    #[test]
    fn memory_makers_come_from_part_numbers_when_windows_does_not_know() {
        assert_eq!(ram_maker(Some("Unknown"), "CMK16GX4M1E3200C16").as_deref(), Some("Corsair"));
        assert_eq!(ram_maker(None, "F4-3200C16-8GVKB").as_deref(), Some("G.Skill"));
        assert_eq!(ram_maker(Some("Kingston"), "KF432C16BB/8").as_deref(), Some("Kingston"));
        assert_eq!(ram_maker(Some("Unknown"), "XYZ123"), None);
    }

    #[test]
    fn a_balanced_pc_needs_nothing() {
        assert!(upgrades(&pc()).is_empty(), "{:?}", upgrades(&pc()));
    }

    #[test]
    fn hard_disk_comes_first() {
        let mut s = pc();
        s.storage["disks"][0]["Media"] = json!("HDD");
        s.system["memory"]["Sticks"] =
            json!([{"SizeGB": 8, "Type": "DDR4", "SpeedMTs": 2666, "FormFactor": "DIMM", "Maker": "Crucial", "Part": "CT8G4DFS8266"}]);
        s.system["memory"]["TotalGB"] = json!(8);
        let u = upgrades(&s);
        assert_eq!(u[0].id, "ssd", "{u:?}");
        let ram = u.iter().find(|x| x.id == "ram-pair").expect("second stick");
        let spec = ram.get.join(" | ");
        assert!(spec.contains("8 GB") && spec.contains("DDR4") && spec.contains("2666") && spec.contains("CT8G4DFS8266"), "{spec}");
        assert!(spec.contains("3 of 4"), "free slots: {spec}");
    }

    #[test]
    fn low_memory_full_drive_and_worn_battery() {
        let mut s = pc();
        s.system["memory"] = json!({"TotalGB": 4, "Slots": 2, "Sticks": [
            {"SizeGB": 2, "Type": "DDR3", "SpeedMTs": 1600, "FormFactor": "SODIMM"},
            {"SizeGB": 2, "Type": "DDR3", "SpeedMTs": 1600, "FormFactor": "SODIMM"}
        ]});
        s.system["device"]["HasBattery"] = json!(true);
        s.system["battery"] = json!({"DesignmWh": 50000, "FullChargemWh": 25000});
        s.storage["volumes"][0]["FreeGB"] = json!(12);
        let ids: Vec<&str> = upgrades(&s).iter().map(|u| u.id).collect();
        assert!(ids.contains(&"ram-more") && ids.contains(&"bigger-drive") && ids.contains(&"battery"), "{ids:?}");
        let ram = upgrades(&s).into_iter().find(|u| u.id == "ram-more").unwrap();
        assert!(ram.get.join(" ").contains("laptop"), "SODIMM means a laptop: {:?}", ram.get);
    }
}
