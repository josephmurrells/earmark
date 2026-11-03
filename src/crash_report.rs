//! Records crashes, which would otherwise close the window without a trace
//! (the app has no console), and tells the user where the details are.

use std::backtrace::Backtrace;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::panic::{self, PanicHookInfo};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The log is started afresh once it grows past this, in bytes.
const MAX_LOG_BYTES: u64 = 256 * 1024;

/// Installs a panic hook that appends to `crash.log` in `dir` and shows a
/// dialog, then runs the default hook.
pub fn install(dir: Option<PathBuf>) {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let log = dir.as_deref().and_then(|dir| write_log(dir, info));
        let where_saved = match &log {
            Some(path) => format!("Details were saved to:\n{}", path.display()),
            None => "Details couldn't be saved.".to_owned(),
        };
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Earmark ran into a problem")
            .set_description(format!(
                "Something went wrong and Earmark may need to close.\n\n{where_saved}"
            ))
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        default_hook(info);
    }));
}

/// Appends a report to `dir/crash.log`. Returns the log's path, or `None` if
/// it couldn't be written (a crash report must never cause another panic).
fn write_log(dir: &Path, info: &PanicHookInfo<'_>) -> Option<PathBuf> {
    fs::create_dir_all(dir).ok()?;
    let path = dir.join("crash.log");
    let too_big = fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(!too_big)
        .truncate(too_big)
        .open(&path)
        .ok()?;
    let report = format_report(info, &Backtrace::force_capture(), SystemTime::now());
    file.write_all(report.as_bytes()).ok()?;
    Some(path)
}

fn format_report(info: &PanicHookInfo<'_>, backtrace: &Backtrace, now: SystemTime) -> String {
    let thread = std::thread::current();
    format!(
        "--- Earmark {} crashed at {} UTC on thread '{}'\n{info}\n\nBacktrace:\n{backtrace}\n\n",
        env!("CARGO_PKG_VERSION"),
        utc_timestamp(now),
        thread.name().unwrap_or("unnamed"),
    )
}

/// Formats a time as "YYYY-MM-DD HH:MM:SS" in UTC.
fn utc_timestamp(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Converts days since 1970-01-01 to a (year, month, day) date, using Howard
/// Hinnant's `civil_from_days` algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01 00:00:00");
        let leap_day = UNIX_EPOCH + Duration::from_secs(1_709_210_096);
        assert_eq!(utc_timestamp(leap_day), "2024-02-29 12:34:56");
        let new_year = UNIX_EPOCH + Duration::from_secs(1_798_761_599);
        assert_eq!(utc_timestamp(new_year), "2026-12-31 23:59:59");
    }

    #[test]
    fn log_is_appended_and_restarted_when_large() {
        let dir = std::env::temp_dir().join(format!("earmark-test-crash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // Capture a real PanicHookInfo by panicking inside a temporary hook.
        let previous = panic::take_hook();
        let hook_dir = dir.clone();
        panic::set_hook(Box::new(move |info| {
            write_log(&hook_dir, info).expect("write log");
        }));
        for _ in 0..2 {
            let _ = panic::catch_unwind(|| panic!("test crash"));
        }
        panic::set_hook(previous);

        let log = fs::read_to_string(dir.join("crash.log")).expect("read log");
        assert_eq!(log.matches("--- Earmark").count(), 2, "{log}");
        assert!(log.contains("test crash"), "{log}");

        // Past the size limit, the next report starts a fresh file.
        fs::write(
            dir.join("crash.log"),
            vec![b'x'; MAX_LOG_BYTES as usize + 1],
        )
        .expect("fill");
        let hook_dir = dir.clone();
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            write_log(&hook_dir, info).expect("write log");
        }));
        let _ = panic::catch_unwind(|| panic!("after restart"));
        panic::set_hook(previous);
        let log = fs::read_to_string(dir.join("crash.log")).expect("read log");
        assert!(log.starts_with("--- Earmark"), "restarted");
        let _ = fs::remove_dir_all(&dir);
    }
}
