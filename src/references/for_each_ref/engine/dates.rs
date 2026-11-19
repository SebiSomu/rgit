//! Date atoms: date modes, timezone math, relative dates, `strftime` formats.

use crate::core::date::{civil_from_days, days_from_civil};
use crate::references::for_each_ref::types::{RefDateKind, RefDateMode};
use std::io::IsTerminal;

pub(super) fn parse_strtol_prefix(text: &str) -> i64 {
    let t = text.trim_start();
    let (negative, rest) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let n: i64 = rest[..end].parse().unwrap_or(0);
    if negative { -n } else { n }
}

/// Parses a date format like git's `parse_date_format` (`relative`, `iso`, `iso-strict`, `rfc`,
/// `short`, `default`, `human`, `raw`, `unix`, `format:<strftime>`, `local`, `auto:<mode>`,
/// each optionally `-local`).
// Used by `for-each-ref` (`%(authordate:<format>)` and friends).
pub fn parse_ref_date_mode(format: &str) -> Result<RefDateMode, String> {
    let mut format = format.to_string();
    if let Some(rest) = format.strip_prefix("auto:") {
        format = if std::io::stdout().is_terminal() { rest.to_string() } else { "default".to_string() };
    }
    if format == "local" {
        format = "default-local".to_string();
    }
    let kinds: &[(&str, RefDateKind)] = &[
        ("relative", RefDateKind::Relative),
        ("iso8601-strict", RefDateKind::IsoStrict),
        ("iso-strict", RefDateKind::IsoStrict),
        ("iso8601", RefDateKind::Iso),
        ("iso", RefDateKind::Iso),
        ("rfc2822", RefDateKind::Rfc),
        ("rfc", RefDateKind::Rfc),
        ("short", RefDateKind::Short),
        ("default", RefDateKind::Normal),
        ("human", RefDateKind::Human),
        ("raw", RefDateKind::Raw),
        ("unix", RefDateKind::Unix),
        ("format", RefDateKind::Strftime),
    ];
    let Some((prefix, kind)) = kinds.iter().find(|(p, _)| format.starts_with(p)) else {
        return Err(format!("unknown date format {}", format));
    };
    let mut rest = &format[prefix.len()..];
    let mut mode = RefDateMode { kind: *kind, ..Default::default() };
    if let Some(r) = rest.strip_prefix("-local") {
        mode.local = true;
        rest = r;
    }
    if mode.kind == RefDateKind::Strftime {
        match rest.strip_prefix(':') {
            Some(fmt) => mode.strftime = fmt.to_string(),
            None => return Err(format!("date format missing colon separator: {}", format)),
        }
    } else if !rest.is_empty() {
        return Err(format!("unknown date format {}", format));
    }
    Ok(mode)
}

#[derive(Clone, Copy, Default)]
struct Tm {
    year: i64,
    mon: i64,
    mday: i64,
    hour: i64,
    min: i64,
    sec: i64,
    wday: i64,
    yday: i64,
}

fn tz_minutes(tz: i64) -> i64 {
    let m = tz.abs();
    let m = (m / 100) * 60 + m % 100;
    if tz < 0 { -m } else { m }
}

fn gmtime(t: i64) -> Tm {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let (jan1, _, _) = (days_from_civil(year, 1, 1), 0, 0);
    Tm {
        year,
        mon: month - 1,
        mday: day,
        hour: secs / 3600,
        min: secs % 3600 / 60,
        sec: secs % 60,
        wday: (days + 4).rem_euclid(7),
        yday: days - jan1,
    }
}

fn now_seconds() -> i64 {
    if let Ok(v) = std::env::var("GIT_TEST_DATE_NOW") {
        return parse_strtol_prefix(&v);
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn relative_date(time: i64) -> String {
    let now = now_seconds();
    if now < time {
        return "in the future".to_string();
    }
    let plural = |n: i64, unit: &str| format!("{} {}{} ago", n, unit, if n == 1 { "" } else { "s" });
    let mut diff = now - time;
    if diff < 90 {
        return plural(diff, "second");
    }
    diff = (diff + 30) / 60;
    if diff < 90 {
        return plural(diff, "minute");
    }
    diff = (diff + 30) / 60;
    if diff < 36 {
        return plural(diff, "hour");
    }
    diff = (diff + 12) / 24;
    if diff < 14 {
        return plural(diff, "day");
    }
    if diff < 70 {
        return plural((diff + 3) / 7, "week");
    }
    if diff < 365 {
        return plural((diff + 15) / 30, "month");
    }
    if diff < 1825 {
        let total = (diff * 12 * 2 + 365) / (365 * 2);
        let (years, months) = (total / 12, total % 12);
        if months > 0 {
            return format!("{} year{}, {} month{} ago", years, if years == 1 { "" } else { "s" }, months, if months == 1 { "" } else { "s" });
        }
        return plural(years, "year");
    }
    plural((diff + 183) / 365, "year")
}

/// Formats a timestamp and `+hhmm`-style zone (as git's decimal parse, `-0100` = -100) like
/// git's `show_date`. The local time zone is taken to be UTC.
// Used by `for-each-ref` (date atoms).
pub fn show_ref_date(time: i64, tz: i64, mode: &RefDateMode) -> String {
    if mode.kind == RefDateKind::Unix {
        return time.to_string();
    }
    let tz = if mode.local { 0 } else { tz };
    if mode.kind == RefDateKind::Raw {
        return format!("{} {}", time, format_tz(tz));
    }
    if mode.kind == RefDateKind::Relative {
        return relative_date(time);
    }
    let tm = gmtime(time + tz_minutes(tz) * 60);
    match mode.kind {
        RefDateKind::Short => format!("{:04}-{:02}-{:02}", tm.year, tm.mon + 1, tm.mday),
        RefDateKind::Iso => format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} {}", tm.year, tm.mon + 1, tm.mday, tm.hour, tm.min, tm.sec, format_tz(tz)),
        RefDateKind::IsoStrict => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
            tm.year,
            tm.mon + 1,
            tm.mday,
            tm.hour,
            tm.min,
            tm.sec,
            if tz >= 0 { '+' } else { '-' },
            tz.abs() / 100,
            tz.abs() % 100
        ),
        RefDateKind::Rfc => format!(
            "{}, {} {} {} {:02}:{:02}:{:02} {}",
            &WEEKDAYS[tm.wday as usize][..3],
            tm.mday,
            &MONTHS[tm.mon as usize][..3],
            tm.year,
            tm.hour,
            tm.min,
            tm.sec,
            format_tz(tz)
        ),
        RefDateKind::Strftime => strftime(&mode.strftime, &tm, tz, time, mode.local),
        RefDateKind::Human => {
            let now = gmtime(now_seconds());
            show_date_normal(time, &tm, tz, Some((&now, 0)), mode.local)
        }
        _ => show_date_normal(time, &tm, tz, None, mode.local),
    }
}

fn format_tz(tz: i64) -> String {
    format!("{}{:04}", if tz < 0 { '-' } else { '+' }, tz.abs())
}

fn show_date_normal(time: i64, tm: &Tm, tz: i64, human: Option<(&Tm, i64)>, local: bool) -> String {
    let (human_tm, human_tz) = match human {
        Some((t, z)) => (*t, z),
        None => (Tm { year: 1900, ..Default::default() }, -1),
    };
    let mut hide_tz = local || tz == human_tz;
    let hide_year = tm.year == human_tm.year;
    let (mut hide_date, mut hide_wday, mut hide_time, mut hide_seconds) = (false, false, false, false);
    if hide_year && tm.mon == human_tm.mon {
        if tm.mday > human_tm.mday {
        } else if tm.mday == human_tm.mday {
            hide_date = true;
            hide_wday = true;
        } else if tm.mday + 5 > human_tm.mday {
            hide_date = true;
        }
    }
    if hide_wday {
        return relative_date(time);
    }
    if human_tm.year != 1900 {
        hide_seconds = true;
        hide_tz |= !hide_date;
        hide_wday = !hide_year;
        hide_time = !hide_year;
    }
    let mut out = String::new();
    if !hide_wday {
        out.push_str(&format!("{} ", &WEEKDAYS[tm.wday as usize][..3]));
    }
    if !hide_date {
        out.push_str(&format!("{} {} ", &MONTHS[tm.mon as usize][..3], tm.mday));
    }
    if !hide_time {
        out.push_str(&format!("{:02}:{:02}", tm.hour, tm.min));
        if !hide_seconds {
            out.push_str(&format!(":{:02}", tm.sec));
        }
    } else {
        out.truncate(out.trim_end().len());
    }
    if !hide_year {
        out.push_str(&format!(" {}", tm.year));
    }
    if !hide_tz {
        out.push_str(&format!(" {}", format_tz(tz)));
    }
    out
}

fn iso_week(tm: &Tm) -> (i64, i64) {
    let wday = (tm.wday + 6) % 7;
    let mut year = tm.year;
    let mut week = (tm.yday - wday + 10) / 7;
    let weeks_in = |y: i64| {
        let jan1 = (days_from_civil(y, 1, 1) + 4).rem_euclid(7);
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        if jan1 == 4 || (leap && jan1 == 3) { 53 } else { 52 }
    };
    if week < 1 {
        year -= 1;
        week = weeks_in(year);
    } else if week > weeks_in(year) {
        year += 1;
        week = 1;
    }
    (year, week)
}

fn strftime(fmt: &str, tm: &Tm, tz: i64, time: i64, local: bool) -> String {
    let mut out = String::new();
    let mut chars = fmt.chars().peekable();
    let hour12 = if tm.hour % 12 == 0 { 12 } else { tm.hour % 12 };
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(spec) = chars.next() else {
            out.push('%');
            break;
        };
        let piece = match spec {
            '%' => "%".to_string(),
            'a' => WEEKDAYS[tm.wday as usize][..3].to_string(),
            'A' => WEEKDAYS[tm.wday as usize].to_string(),
            'b' | 'h' => MONTHS[tm.mon as usize][..3].to_string(),
            'B' => MONTHS[tm.mon as usize].to_string(),
            'c' => format!(
                "{} {} {:>2} {:02}:{:02}:{:02} {}",
                &WEEKDAYS[tm.wday as usize][..3],
                &MONTHS[tm.mon as usize][..3],
                tm.mday,
                tm.hour,
                tm.min,
                tm.sec,
                tm.year
            ),
            'C' => format!("{:02}", tm.year.div_euclid(100)),
            'd' => format!("{:02}", tm.mday),
            'D' => format!("{:02}/{:02}/{:02}", tm.mon + 1, tm.mday, tm.year.rem_euclid(100)),
            'e' => format!("{:>2}", tm.mday),
            'F' => format!("{:04}-{:02}-{:02}", tm.year, tm.mon + 1, tm.mday),
            'g' => format!("{:02}", iso_week(tm).0.rem_euclid(100)),
            'G' => iso_week(tm).0.to_string(),
            'H' => format!("{:02}", tm.hour),
            'I' => format!("{:02}", hour12),
            'j' => format!("{:03}", tm.yday + 1),
            'k' => format!("{:>2}", tm.hour),
            'l' => format!("{:>2}", hour12),
            'm' => format!("{:02}", tm.mon + 1),
            'M' => format!("{:02}", tm.min),
            'n' => "\n".to_string(),
            'p' => if tm.hour < 12 { "AM" } else { "PM" }.to_string(),
            'P' => if tm.hour < 12 { "am" } else { "pm" }.to_string(),
            'r' => format!("{:02}:{:02}:{:02} {}", hour12, tm.min, tm.sec, if tm.hour < 12 { "AM" } else { "PM" }),
            'R' => format!("{:02}:{:02}", tm.hour, tm.min),
            's' => time.to_string(),
            'S' => format!("{:02}", tm.sec),
            't' => "\t".to_string(),
            'T' => format!("{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec),
            'u' => (if tm.wday == 0 { 7 } else { tm.wday }).to_string(),
            'U' => format!("{:02}", (tm.yday + 7 - tm.wday) / 7),
            'V' => format!("{:02}", iso_week(tm).1),
            'w' => tm.wday.to_string(),
            'W' => format!("{:02}", (tm.yday + 7 - (tm.wday + 6) % 7) / 7),
            'x' => format!("{:02}/{:02}/{:02}", tm.mon + 1, tm.mday, tm.year.rem_euclid(100)),
            'X' => format!("{:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec),
            'y' => format!("{:02}", tm.year.rem_euclid(100)),
            'Y' => tm.year.to_string(),
            'z' => format_tz(tz),
            'Z' => if local { "UTC".to_string() } else { String::new() },
            other => format!("%{}", other),
        };
        out.push_str(&piece);
    }
    out
}
