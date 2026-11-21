//! Regular check-ups: a Task Scheduler task runs `bootmend --scheduled`, which checks quietly and
//! shows a Windows notification only when something new needs a look. Nothing is ever fixed on its own.

use crate::rules::{Finding, Level};
use crate::snapshot::{self, Snapshot};
use crate::util::{powershell, ps_quote, APP};
use anyhow::{anyhow, Result};
use chrono::{Datelike, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

pub const TASK: &str = "Bootmend check-up";
const AUMID: &str = "Bootmend.CheckUp";
/// Clicking a notification opens `bootmend:open`, which Windows hands to the app.
const PROTOCOL: &str = "bootmend";
/// What the check-up was registered as before the app was renamed.
const OLD_TASK: &str = "rigscan check-up";
const OLD_AUMID: &str = "rigscan.checkup";
const OLD_PROTOCOL: &str = "rigscan";
/// Scheduled checks keep at most this many saved checks; the oldest go first.
pub const KEEP: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Every {
    #[default]
    Off,
    Daily,
    Weekly,
    Monthly,
}

impl Every {
    pub fn label(self) -> &'static str {
        match self {
            Every::Off => "Off",
            Every::Daily => "Every day",
            Every::Weekly => "Every week",
            Every::Monthly => "Every month",
        }
    }
}

/// When check-ups run unless the person picks another hour.
pub const NOON: u8 = 12;

/// "09:00" for a whole hour. A saved hour outside 0-23 counts as noon.
pub fn hour_label(hour: u8) -> String {
    format!("{:02}:00", if hour < 24 { hour } else { NOON })
}

/// Escapes XML's special characters and drops the control characters XML 1.0 can't hold.
fn xml_escape(s: &str) -> String {
    let s: String = s.chars().filter(|&c| matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && !matches!(c, '\u{FFFE}' | '\u{FFFF}'))).collect();
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// The task definition. Runs at the chosen hour, or as soon as the PC is on after a missed time, on battery too.
pub fn task_xml(every: Every, hour: u8, exe: &str, user: &str, today: chrono::NaiveDate) -> String {
    const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let schedule = match every {
        Every::Daily | Every::Off => "<ScheduleByDay><DaysInterval>1</DaysInterval></ScheduleByDay>".to_string(),
        Every::Weekly => format!(
            "<ScheduleByWeek><DaysOfWeek><{0}/></DaysOfWeek><WeeksInterval>1</WeeksInterval></ScheduleByWeek>",
            DAYS[today.weekday().num_days_from_monday() as usize]
        ),
        Every::Monthly => format!(
            "<ScheduleByMonth><DaysOfMonth><Day>{}</Day></DaysOfMonth><Months>{}</Months></ScheduleByMonth>",
            today.day().min(28),
            MONTHS.iter().map(|m| format!("<{m}/>")).collect::<String>()
        ),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Runs a Bootmend check-up and lets you know if something new needs a look. Nothing is changed on its own.</Description></RegistrationInfo>
  <Triggers><CalendarTrigger><StartBoundary>{date}T{time}:00</StartBoundary><Enabled>true</Enabled>{schedule}</CalendarTrigger></Triggers>
  <Principals><Principal id="Author"><UserId>{user}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <ExecutionTimeLimit>PT30M</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author"><Exec><Command>{exe}</Command><Arguments>--scheduled</Arguments></Exec></Actions>
</Task>"#,
        date = today.format("%Y-%m-%d"),
        time = hour_label(hour),
        user = xml_escape(user),
        exe = xml_escape(exe),
    )
}

/// Turns regular check-ups on at the chosen pace and hour, or off. Also registers how Windows shows
/// the app's notifications and opens the app when one is clicked.
pub fn set(every: Every, hour: u8) -> Result<()> {
    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    let script = if every == Every::Off {
        format!(
            "Unregister-ScheduledTask -TaskName {task} -Confirm:$false -ErrorAction SilentlyContinue\n\
             Remove-Item -Path 'HKCU:\\Software\\Classes\\AppUserModelId\\{AUMID}' -Recurse -ErrorAction SilentlyContinue\n\
             Remove-Item -Path 'HKCU:\\Software\\Classes\\{PROTOCOL}' -Recurse -ErrorAction SilentlyContinue",
            task = ps_quote(TASK)
        )
    } else {
        let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
        let xml = task_xml(every, hour, &exe, &user, Local::now().date_naive());
        format!(
            "$ErrorActionPreference = 'Stop'\n\
             $k = New-Item -Path 'HKCU:\\Software\\Classes\\AppUserModelId\\{AUMID}' -Force\n\
             Set-ItemProperty -Path $k.PSPath -Name DisplayName -Value '{APP}'\n\
             $p = New-Item -Path 'HKCU:\\Software\\Classes\\{PROTOCOL}' -Force\n\
             Set-ItemProperty -Path $p.PSPath -Name '(default)' -Value 'URL:{APP}'\n\
             Set-ItemProperty -Path $p.PSPath -Name 'URL Protocol' -Value ''\n\
             $c = New-Item -Path 'HKCU:\\Software\\Classes\\{PROTOCOL}\\shell\\open\\command' -Force\n\
             Set-ItemProperty -Path $c.PSPath -Name '(default)' -Value {cmd}\n\
             Register-ScheduledTask -TaskName {task} -Xml {xml} -Force | Out-Null",
            cmd = ps_quote(&format!("\"{exe}\"")),
            task = ps_quote(TASK),
            xml = ps_quote(&xml),
        )
    };
    let what = if every == Every::Off {
        "removing the scheduled check-up".to_string()
    } else {
        format!("scheduling check-ups: {} at {}", every.label(), hour_label(hour))
    };
    log!("schedule", "{what}");
    let out = powershell(&script, Duration::from_secs(60)).inspect_err(|e| log!("schedule", "failed: {e:#}"))?;
    if out.code != Some(0) {
        log!("schedule", "failed: Task Scheduler said: {}", out.stderr.trim());
        return Err(anyhow!("Task Scheduler said: {}", out.stderr.trim()));
    }
    log!("schedule", "{}", if every == Every::Off { "scheduled check-up removed" } else { "scheduled check-up saved" });
    Ok(())
}

/// Check-ups set up before the rename ran as "rigscan check-up". If that task exists, the same pace
/// and hour are set up under the new name, then the old task and its notification settings go.
pub fn move_old_task(every: Every, hour: u8) -> Result<()> {
    let found =
        powershell(&format!("if (Get-ScheduledTask -TaskName {} -ErrorAction SilentlyContinue) {{ 'yes' }}", ps_quote(OLD_TASK)), Duration::from_secs(30))?;
    if found.stdout.trim() != "yes" {
        return Ok(());
    }
    log!("schedule", "moving the check-up set up under the old app name");
    if every != Every::Off {
        set(every, hour)?;
    }
    let out = powershell(
        &format!(
            "Unregister-ScheduledTask -TaskName {} -Confirm:$false -ErrorAction SilentlyContinue\n\
             Remove-Item -Path 'HKCU:\\Software\\Classes\\AppUserModelId\\{OLD_AUMID}' -Recurse -ErrorAction SilentlyContinue\n\
             Remove-Item -Path 'HKCU:\\Software\\Classes\\{OLD_PROTOCOL}' -Recurse -ErrorAction SilentlyContinue",
            ps_quote(OLD_TASK)
        ),
        Duration::from_secs(60),
    )?;
    if out.code != Some(0) {
        log!("schedule", "removing the old check-up failed: Task Scheduler said: {}", out.stderr.trim());
        return Err(anyhow!("Task Scheduler said: {}", out.stderr.trim()));
    }
    log!("schedule", "old check-up removed");
    Ok(())
}

/// Suggestions in `now` that weren't in `before`: only the ones worth interrupting someone for.
pub fn new_items<'a>(before: &[Finding], now: &'a [Finding]) -> Vec<&'a Finding> {
    let old: HashSet<&str> = before.iter().filter(|f| f.level != Level::AllGood).map(|f| f.id.as_str()).collect();
    now.iter().filter(|f| matches!(f.level, Level::DoFirst | Level::WorthDoing) && !old.contains(f.id.as_str())).collect()
}

/// The notification: the first new item by name, and how many more.
pub fn toast_xml(items: &[&Finding]) -> String {
    let first = items.first().map(|f| f.title.as_str()).unwrap_or_default();
    let body = match items.len() {
        0 | 1 => first.to_string(),
        n => format!("{first}, and {} more", n - 1),
    };
    format!(
        r#"<toast activationType="protocol" launch="{PROTOCOL}:open"><visual><binding template="ToastGeneric"><text>{APP} found something new</text><text>{}</text><text>Open {APP} to see what it means and what to do. Nothing was changed.</text></binding></visual></toast>"#,
        xml_escape(&body)
    )
}

/// After a scheduled check-up, once per new version.
pub fn update_toast_xml(version: &str) -> String {
    format!(
        r#"<toast activationType="protocol" launch="{PROTOCOL}:open"><visual><binding template="ToastGeneric"><text>{APP} {} is ready</text><text>Open {APP} to install it.</text></binding></visual></toast>"#,
        xml_escape(version)
    )
}

pub fn notify(xml: &str) -> Result<()> {
    let script = format!(
        "$null = [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]\n\
         $null = [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime]\n\
         $x = New-Object Windows.Data.Xml.Dom.XmlDocument\n\
         $x.LoadXml({})\n\
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{AUMID}').Show([Windows.UI.Notifications.ToastNotification]::new($x))",
        ps_quote(xml)
    );
    let out = powershell(&script, Duration::from_secs(30))?;
    if out.code != Some(0) {
        log!("schedule", "notification failed: {}", out.stderr.trim());
        return Err(anyhow!("Windows didn't show the notification: {}", out.stderr.trim()));
    }
    log!("schedule", "notification shown");
    Ok(())
}

/// Deletes the oldest saved checks beyond `keep`. Returns how many were removed.
pub fn prune(host: &str, keep: usize) -> Result<usize> {
    let files = snapshot::list(host)?;
    let extra = files.len().saturating_sub(keep);
    for f in &files[..extra] {
        std::fs::remove_file(f)?;
    }
    if extra > 0 {
        log!("history", "removed {extra} old saved check{}", if extra == 1 { "" } else { "s" });
    }
    Ok(extra)
}

/// What `bootmend --scheduled` does: check, save, and tell the person only about new suggestions.
pub fn run_scheduled(snap: Snapshot) -> Result<()> {
    log!("schedule", "scheduled check-up running");
    let before = snapshot::previous(&snap.host, &snap.id).map(|p| crate::rules::evaluate(&p)).unwrap_or_default();
    snapshot::save(&snap)?;
    prune(&snap.host, KEEP)?;
    let now = crate::rules::evaluate(&snap);
    let new = new_items(&before, &now);
    log!("schedule", "scheduled check-up finished: {} new suggestion{}", new.len(), if new.len() == 1 { "" } else { "s" });
    if !new.is_empty() {
        notify(&toast_xml(&new))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Area;

    fn f(id: &str, level: Level) -> Finding {
        Finding::new(id, Area::Security, level, format!("Title {id}"))
    }

    #[test]
    fn xml_drops_characters_xml_cannot_hold() {
        assert_eq!(xml_escape("a\u{1}b\u{1b}c\td<\"&'\u{FFFF}"), "abc\td&lt;&quot;&amp;&apos;");
    }

    fn task_exists(name: &str) -> String {
        powershell(
            &format!("if (Get-ScheduledTask -TaskName {} -ErrorAction SilentlyContinue) {{ 'yes' }} else {{ 'no' }}", ps_quote(name)),
            Duration::from_secs(30),
        )
        .unwrap()
        .stdout
        .trim()
        .to_string()
    }

    /// Creates the real task, shows one real notification, then removes everything again.
    #[test]
    #[ignore = "changes Task Scheduler and shows a notification"]
    fn sets_notifies_and_clears_for_real() {
        let exists = || task_exists(TASK);
        set(Every::Weekly, 9).unwrap();
        assert_eq!(exists(), "yes");
        let start = powershell(&format!("(Get-ScheduledTask -TaskName {}).Triggers[0].StartBoundary", ps_quote(TASK)), Duration::from_secs(30)).unwrap();
        assert!(start.stdout.contains("T09:00:00"), "{}", start.stdout);
        let a = f("test", Level::DoFirst);
        notify(&toast_xml(&[&a])).unwrap();
        set(Every::Off, NOON).unwrap();
        assert_eq!(exists(), "no");
    }

    /// Registers a check-up under the old name, moves it, and checks only the new one is left. Then removes it.
    #[test]
    #[ignore = "changes Task Scheduler"]
    fn moves_a_check_up_set_up_under_the_old_name_for_real() {
        let exe = std::env::current_exe().unwrap().to_string_lossy().to_string();
        let user = format!("{}\\{}", std::env::var("USERDOMAIN").unwrap_or_default(), std::env::var("USERNAME").unwrap_or_default());
        let xml = task_xml(Every::Weekly, 9, &exe, &user, Local::now().date_naive());
        let made =
            powershell(&format!("Register-ScheduledTask -TaskName {} -Xml {} -Force | Out-Null", ps_quote(OLD_TASK), ps_quote(&xml)), Duration::from_secs(60))
                .unwrap();
        assert_eq!(made.code, Some(0), "{}", made.stderr);
        assert_eq!(task_exists(OLD_TASK), "yes");
        move_old_task(Every::Weekly, 9).unwrap();
        assert_eq!(task_exists(OLD_TASK), "no");
        assert_eq!(task_exists(TASK), "yes");
        let start = powershell(&format!("(Get-ScheduledTask -TaskName {}).Triggers[0].StartBoundary", ps_quote(TASK)), Duration::from_secs(30)).unwrap();
        assert!(start.stdout.contains("T09:00:00"), "{}", start.stdout);
        set(Every::Off, NOON).unwrap();
        assert_eq!(task_exists(TASK), "no");
    }

    #[test]
    fn only_new_important_items_interrupt() {
        let before = vec![f("a", Level::DoFirst), f("b", Level::AllGood)];
        let now = vec![f("a", Level::DoFirst), f("b", Level::WorthDoing), f("c", Level::NiceToHave), f("d", Level::DoFirst)];
        let ids: Vec<&str> = new_items(&before, &now).iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "d"]);
        assert!(new_items(&now, &now).is_empty());
    }

    #[test]
    fn notification_names_the_first_and_counts_the_rest() {
        let a = f("a", Level::DoFirst);
        let b = f("b<&>", Level::DoFirst);
        let x = toast_xml(&[&a, &b]);
        assert!(x.contains("Title a, and 1 more"), "{x}");
        assert!(x.contains("Bootmend found something new") && x.contains(r#"launch="bootmend:open""#) && !x.contains("rigscan"), "{x}");
        let y = toast_xml(&[&b]);
        assert!(y.contains("Title b&lt;&amp;&gt;") && !y.contains("b<&>"), "{y}");
        let u = update_toast_xml("0.2.0");
        assert!(u.contains("Bootmend 0.2.0 is ready") && u.contains(r#"launch="bootmend:open""#), "{u}");
        assert!(update_toast_xml("<x>").contains("&lt;x&gt;"));
    }

    #[test]
    fn task_runs_at_the_chosen_pace() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 30).unwrap();
        let w = task_xml(Every::Weekly, NOON, r"C:\Apps\boot & mend\bootmend.exe", r"PC\me", d);
        assert!(w.contains("<Monday/>") && w.contains("boot &amp; mend") && w.contains("<StartWhenAvailable>true"), "{w}");
        assert!(w.contains("Runs a Bootmend check-up") && !w.contains("rigscan"), "{w}");
        assert_eq!(TASK, "Bootmend check-up");
        assert!(w.contains("2026-11-30T12:00:00"));
        let m = task_xml(Every::Monthly, NOON, "r.exe", "u", d);
        assert!(m.contains("<Day>28</Day>") && m.contains("<December/>"), "{m}");
        let e = task_xml(Every::Daily, 9, "r.exe", "u", d);
        assert!(e.contains("<DaysInterval>1</DaysInterval>") && e.contains("2026-11-30T09:00:00"), "{e}");
        assert_eq!(hour_label(9), "09:00");
        assert_eq!(hour_label(30), "12:00", "a bad saved hour falls back to noon");
    }
}
