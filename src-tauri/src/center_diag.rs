//! When the Yuval Center dies before it connects, find out why and say it in one message box,
//! so a field report is a single photo instead of typed commands on an offline PC.
//!
//! The Center is started with the .NET runtime's own crash reporting switched on (a mini dump plus a
//! JSON crash report from `createdump.exe`, which ships next to the Center). A watcher follows the
//! child: if it exits within [`WATCH`] without having connected to the pipe, it reads the newest crash
//! report and the tail of `center.log` and shows: exit code, last startup stage, exception type and
//! the top frames. Nothing here holds user content: the Center logs stages and codes only, and the
//! crash report summary keeps type and method names.

use crate::paths;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// How long a freshly started Center has to connect before a quiet exit counts as a failure.
const WATCH: Duration = Duration::from_secs(20);
/// createdump writes the report after the process is gone; give it this long to appear.
const REPORT_WAIT: Duration = Duration::from_secs(8);
const DUMP_PREFIX: &str = "center-crash-";
/// Old dumps are removed so the logs folder never fills up.
const KEEP_DUMPS: usize = 3;

static CONNECTED: AtomicBool = AtomicBool::new(false);

/// A Center connected to the pipe (any process): the start worked.
pub fn note_connected() {
    CONNECTED.store(true, Ordering::SeqCst);
}

/// Environment for the Center process: the runtime writes a dump + crash report on a fatal error.
pub fn crash_report_env(command: &mut std::process::Command) {
    let Ok(dir) = paths::logs_dir() else { return };
    let name = dir.join(format!("{DUMP_PREFIX}%p.dmp"));
    command
        .env("DOTNET_DbgEnableMiniDump", "1")
        // 1 = MiniDumpNormal: thread stacks and module list, no heap.
        .env("DOTNET_DbgMiniDumpType", "1")
        .env("DOTNET_EnableCrashReport", "1")
        .env("DOTNET_DbgMiniDumpName", name);
}

/// Follow a started Center on its own thread.
pub fn watch(mut child: Child, started: SystemTime) {
    CONNECTED.store(false, Ordering::SeqCst);
    let pid = child.id();
    let _ = std::thread::Builder::new().name("companyisland-center-watch".into()).spawn(move || {
        crate::debug_log::catch("center watch", || {
            let deadline = Instant::now() + WATCH;
            let code = loop {
                if CONNECTED.load(Ordering::SeqCst) {
                    return;
                }
                match child.try_wait() {
                    Ok(Some(status)) => break status.code(),
                    Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(200)),
                    // Still running after the watch: it may be a redirect target or just slow; the spawn
                    // guard in `center` reports a Center that never connects.
                    _ => return,
                }
            };
            if CONNECTED.load(Ordering::SeqCst) {
                return;
            }
            // Exit 0 without connecting is the normal single-instance redirect to a running Center.
            if code == Some(0) {
                dlog!("INFO", "center", "center pid {} handed over to a running instance", pid);
                return;
            }
            let report = wait_for_report(started);
            let stage = last_center_log_lines(2);
            let summary = Summary { pid, exit_code: code, stage, report: report.as_deref().and_then(summarize_report) };
            dlog!("WARN", "center", "APP-034 center pid {} exited before connecting: {}", pid, summary.one_line());
            prune_dumps();
            show(&summary.message());
        });
    });
}

#[derive(Debug, Default, PartialEq)]
pub struct ReportSummary {
    pub exception: Option<String>,
    pub frames: Vec<String>,
}

#[derive(Debug, Default)]
struct Summary {
    pid: u32,
    exit_code: Option<i32>,
    stage: Vec<String>,
    report: Option<ReportSummary>,
}

impl Summary {
    fn code_hex(&self) -> String {
        self.exit_code.map(|c| format!("0x{:08X}", c as u32)).unwrap_or_else(|| "?".into())
    }

    fn one_line(&self) -> String {
        let r = self.report.as_ref();
        format!(
            "exit={} stage={} exception={} frames={}",
            self.code_hex(),
            self.stage.last().map(String::as_str).unwrap_or("-"),
            r.and_then(|r| r.exception.as_deref()).unwrap_or("-"),
            r.map(|r| r.frames.join(" < ")).unwrap_or_default()
        )
    }

    fn message(&self) -> String {
        let mut m = format!("מרכז יובל נסגר מיד אחרי ההפעלה.\nקוד יציאה: {}\n", self.code_hex());
        if !self.stage.is_empty() {
            m.push_str("\nשלב אחרון (center.log):\n");
            for l in &self.stage {
                m.push_str(l);
                m.push('\n');
            }
        } else {
            m.push_str("\ncenter.log: אין שורות (הקריסה קרתה לפני הקוד של מרכז יובל)\n");
        }
        match &self.report {
            Some(r) => {
                m.push_str(&format!("\nשגיאה: {}\n", r.exception.as_deref().unwrap_or("(native)")));
                for f in r.frames.iter().take(8) {
                    m.push_str("  ");
                    m.push_str(f);
                    m.push('\n');
                }
            }
            None => m.push_str("\nלא נוצר דוח קריסה של .NET.\n"),
        }
        m.push_str("\nצלמו את ההודעה הזאת ושלחו לתמיכה.");
        m
    }
}

fn show(text: &str) {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONWARNING, MB_OK, MB_RIGHT, MB_RTLREADING, MB_SETFOREGROUND, MB_TOPMOST,
    };
    unsafe {
        MessageBoxW(
            HWND::default(),
            &HSTRING::from(text),
            &HSTRING::from("Yuval – מרכז יובל"),
            MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST | MB_RTLREADING | MB_RIGHT,
        );
    }
}

/// The newest `center-crash-*.crashreport.json` written after `since`, waiting for createdump.
fn wait_for_report(since: SystemTime) -> Option<String> {
    let dir = paths::logs_dir().ok()?;
    let until = Instant::now() + REPORT_WAIT;
    loop {
        if let Some(p) = newest_report(&dir, since) {
            // createdump may still be writing: read when the size is stable.
            std::thread::sleep(Duration::from_millis(300));
            return std::fs::read_to_string(p).ok();
        }
        if Instant::now() >= until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn newest_report(dir: &Path, since: SystemTime) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with(DUMP_PREFIX) && n.ends_with(".crashreport.json")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(t, _)| *t >= since)
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

fn prune_dumps() {
    let Ok(dir) = paths::logs_dir() else { return };
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let mut dumps: Vec<(SystemTime, PathBuf)> = rd
        .filter_map(Result::ok)
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            n.starts_with(DUMP_PREFIX) && n.ends_with(".dmp")
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    dumps.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, p) in dumps.into_iter().skip(KEEP_DUMPS) {
        let _ = std::fs::remove_file(&p);
        let mut json = p.into_os_string();
        json.push(".crashreport.json");
        let _ = std::fs::remove_file(json);
    }
}

/// The last `n` non-empty lines of `center.log`, without the timestamp (they hold stages and codes only).
fn last_center_log_lines(n: usize) -> Vec<String> {
    let Ok(dir) = paths::logs_dir() else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(dir.join("center.log")) else { return Vec::new() };
    tail_lines(&text, n)
}

fn tail_lines(text: &str, n: usize) -> Vec<String> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..]
        .iter()
        .map(|l| {
            // "2026-10-08 20:17:35.123 [INFO] [1234] stage" -> "[INFO] stage"
            let rest = l.get(24..).unwrap_or(l).trim();
            let rest = match rest.find("] [") {
                Some(i) => {
                    let level = &rest[..=i];
                    let after = rest[i + 2..].splitn(2, "] ").nth(1).unwrap_or("");
                    format!("{level} {after}")
                }
                None => rest.to_string(),
            };
            rest.chars().take(160).collect()
        })
        .collect()
}

/// Exception type and top frames of a .NET crash report (createdump JSON). Tolerant of format
/// differences between runtime versions: it looks for the crashed thread and well-known keys.
pub fn summarize_report(json: &str) -> Option<ReportSummary> {
    let v: Value = serde_json::from_str(json).ok()?;
    let threads = find_array(&v, "threads")?;
    let crashed = threads
        .iter()
        .find(|t| t.get("crashed").and_then(Value::as_bool) == Some(true))
        .or_else(|| threads.iter().find(|t| t.get("managed_exception_type").is_some()))
        .or_else(|| threads.first())?;
    let exception = ["managed_exception_type", "exception_type"]
        .iter()
        .find_map(|k| crashed.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
        .map(|s| {
            let hr = crashed.get("managed_exception_hresult").and_then(Value::as_str).unwrap_or("");
            if hr.is_empty() { s.to_string() } else { format!("{s} ({hr})") }
        });
    let frames = crashed
        .get("stack_frames")
        .and_then(Value::as_array)
        .map(|fs| {
            fs.iter()
                .filter_map(|f| {
                    let method = f.get("method_name").and_then(Value::as_str).filter(|s| !s.is_empty());
                    let module = f.get("filename").or_else(|| f.get("module_name")).and_then(Value::as_str).unwrap_or("");
                    let module = module.rsplit(['\\', '/']).next().unwrap_or(module);
                    match method {
                        Some(m) => Some(format!("{m} [{module}]")),
                        None if !module.is_empty() => Some(format!("[{module}]")),
                        None => None,
                    }
                })
                .take(12)
                .collect()
        })
        .unwrap_or_default();
    Some(ReportSummary { exception, frames })
}

fn find_array<'a>(v: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    match v {
        Value::Object(map) => map
            .get(key)
            .and_then(Value::as_array)
            .or_else(|| map.values().find_map(|x| find_array(x, key))),
        Value::Array(items) => items.iter().find_map(|x| find_array(x, key)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_the_crashed_thread_of_a_dotnet_crash_report() {
        let json = r#"{"payload":{"protocol_version":"1.0.0","threads":[
            {"is_managed":true,"crashed":false,"stack_frames":[{"method_name":"Idle","filename":"x.dll"}]},
            {"is_managed":true,"crashed":true,"managed_exception_type":"System.TypeLoadException","managed_exception_hresult":"0x80131522",
             "stack_frames":[{"is_managed":true,"method_name":"CompanyIsland.Center.App..ctor()","filename":"C:\\Program Files\\Yuval\\center\\Yuval.Center.dll"},
                             {"is_managed":false,"filename":"coreclr.dll"}]}]}}"#;
        let s = summarize_report(json).unwrap();
        assert_eq!(s.exception.as_deref(), Some("System.TypeLoadException (0x80131522)"));
        assert_eq!(s.frames[0], "CompanyIsland.Center.App..ctor() [Yuval.Center.dll]");
        assert_eq!(s.frames[1], "[coreclr.dll]");
    }

    #[test]
    fn a_native_only_report_still_gives_frames() {
        let json = r#"{"payload":{"threads":[{"crashed":true,"stack_frames":[{"module_name":"KERNELBASE.dll"},{"module_name":"coreclr.dll"}]}]}}"#;
        let s = summarize_report(json).unwrap();
        assert_eq!(s.exception, None);
        assert_eq!(s.frames, vec!["[KERNELBASE.dll]", "[coreclr.dll]"]);
        assert!(summarize_report("not json").is_none());
    }

    #[test]
    fn center_log_tail_drops_timestamps_and_pids() {
        let log = "2026-10-08 20:17:35.100 [INFO] [1234] process start 1.0.13 os=19045\n\n2026-10-08 20:17:35.300 [INFO] [1234] app resources loaded\n";
        assert_eq!(tail_lines(log, 2), vec!["[INFO] process start 1.0.13 os=19045", "[INFO] app resources loaded"]);
        assert!(tail_lines("", 2).is_empty());
    }

    #[test]
    fn message_names_the_exit_code_and_stage() {
        let s = Summary {
            pid: 1,
            exit_code: Some(0x80131506u32 as i32),
            stage: vec!["[INFO] app resources loaded".into()],
            report: Some(ReportSummary { exception: Some("System.X".into()), frames: vec!["A [b.dll]".into()] }),
        };
        let m = s.message();
        assert!(m.contains("0x80131506") && m.contains("app resources loaded") && m.contains("System.X") && m.contains("A [b.dll]"));
        assert!(s.one_line().contains("exit=0x80131506"));
        assert!(m.starts_with("מרכז יובל נסגר"), "{m}");
        assert!(!m.contains("CompanyIsland") && !m.contains("מרכז האי"), "{m}");
    }
}
