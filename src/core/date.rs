use regex::Regex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Converts days since the Unix epoch into a `(year, month, day)` civil date.
// Used by `format_rfc2822_date`.
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

/// Converts a `+HHMM` / `-HHMM` timezone into an offset in seconds.
// Used by `format_rfc2822_date` and `am` date parsing.
pub fn tz_offset_seconds(tz: &str) -> i64 {
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let digits = tz.trim_start_matches(['+', '-']);
    if digits.len() != 4 {
        return 0;
    }
    let hours: i64 = digits[..2].parse().unwrap_or(0);
    let minutes: i64 = digits[2..].parse().unwrap_or(0);
    sign * (hours * 3600 + minutes * 60)
}

/// Parses the date forms `--since` / `--until` accept: Unix timestamps (`1700000000` or
/// `@1700000000`), `YYYY-MM-DD[ HH:MM[:SS]]` with optional `T` separator and `+HHMM`/`Z`
/// zone (UTC when absent), `now`, `yesterday`, and `<n> <unit>[s] ago` (also dotted, like
/// `2.weeks.ago`) for seconds through years.
// Used by `rev-list`.
pub fn parse_approx_date(text: &str) -> Option<i64> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let t = text.trim().to_ascii_lowercase();
    if let Some(ts) = t.strip_prefix('@').unwrap_or(&t).parse::<i64>().ok().filter(|_| t.starts_with('@') || t.len() >= 9) {
        return Some(ts);
    }
    match t.as_str() {
        "now" => return Some(now),
        "yesterday" => return Some(now - 86_400),
        _ => {}
    }

    let words: Vec<&str> = t.split(|c: char| c == ' ' || c == '.').filter(|w| !w.is_empty()).collect();
    if words.len() == 3 && words[2] == "ago" {
        let n: i64 = words[0].parse().ok()?;
        let unit = words[1].trim_end_matches('s');
        let seconds = match unit {
            "second" => 1,
            "minute" => 60,
            "hour" => 3600,
            "day" => 86_400,
            "week" => 7 * 86_400,
            "month" => 30 * 86_400,
            "year" => 365 * 86_400,
            _ => return None,
        };
        return Some(now - n * seconds);
    }

    let re = Regex::new(r"^(\d{4})-(\d{2})-(\d{2})(?:[ t](\d{1,2}):(\d{2})(?::(\d{2}))?)?\s*(z|[+-]\d{4})?$").ok()?;
    let caps = re.captures(&t)?;
    let num = |i: usize| caps.get(i).map(|m| m.as_str().parse::<i64>().unwrap_or(0)).unwrap_or(0);
    let (year, month, day) = (num(1), num(2), num(3));
    let local = days_from_civil(year, month, day) * 86_400 + num(4) * 3600 + num(5) * 60 + num(6);
    let tz = caps.get(7).map(|m| m.as_str()).filter(|z| *z != "z").unwrap_or("+0000");
    Some(local - tz_offset_seconds(tz))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}
