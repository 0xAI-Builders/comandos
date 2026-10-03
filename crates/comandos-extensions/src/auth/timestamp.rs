//! Python 3.11 ISO input compatibility for native credential timestamps.
use chrono::{Datelike, Local, NaiveDate, TimeZone, Weekday};

fn digits(s: &str) -> Option<u32> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

fn date(s: &str) -> Option<NaiveDate> {
    let year = digits(s.get(..4)?)? as i32;
    if !(1..=9999).contains(&year) {
        return None;
    }
    let tail = s.get(4..)?;
    let extended = tail.starts_with('-');
    let tail = if extended { &tail[1..] } else { tail };
    if let Some(week) = tail.strip_prefix('W') {
        let number = digits(week.get(..2)?)?;
        let rest = week.get(2..)?;
        let day = if rest.is_empty() {
            1
        } else {
            let rest = if extended {
                rest.strip_prefix('-')?
            } else {
                rest
            };
            if rest.len() != 1 {
                return None;
            }
            digits(rest)?
        };
        let day = match day {
            1 => Weekday::Mon,
            2 => Weekday::Tue,
            3 => Weekday::Wed,
            4 => Weekday::Thu,
            5 => Weekday::Fri,
            6 => Weekday::Sat,
            7 => Weekday::Sun,
            _ => return None,
        };
        let date = NaiveDate::from_isoywd_opt(year, number, day)?;
        (date.year() <= 9999).then_some(date)
    } else {
        let month = digits(tail.get(..2)?)?;
        let rest = tail.get(2..)?;
        let day = if extended {
            rest.strip_prefix('-')?
        } else {
            rest
        };
        if day.len() != 2 {
            return None;
        }
        NaiveDate::from_ymd_opt(year, month, digits(day)?)
    }
}

// An ISO date may be followed by any single Unicode separator. Numeric
// separators adjacent to an ISO week need the same deterministic boundary.
fn date_end(s: &[char]) -> Option<usize> {
    if s.len() < 7 {
        return None;
    }
    if s.len() == 7 {
        return Some(7);
    }
    Some(if s[4] == '-' {
        if s[5] != 'W' {
            10
        } else if s.len() > 8 && s[8] == '-' {
            if s.len() > 10 && s[10].is_ascii_digit() {
                8
            } else {
                10
            }
        } else {
            8
        }
    } else if s[4] == 'W' {
        let end = (7..s.len())
            .find(|&i| !s[i].is_ascii_digit())
            .unwrap_or(s.len());
        if end < 9 {
            end
        } else if end % 2 == 0 {
            7
        } else {
            8
        }
    } else {
        8
    })
}

// Fractions apply to seconds even when supplied after hours or minutes.
fn clock(s: &str) -> Option<[u32; 4]> {
    let (whole, fraction) = match s.find(['.', ',']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let mut values = [0; 4];
    if whole.contains(':') {
        let parts: Vec<_> = whole.split(':').collect();
        if !(2..=3).contains(&parts.len()) {
            return None;
        }
        for (i, part) in parts.iter().enumerate() {
            if part.len() != 2 {
                return None;
            }
            values[i] = digits(part)?;
        }
    } else {
        if ![2, 4, 6].contains(&whole.len()) || !whole.is_ascii() {
            return None;
        }
        for (i, part) in whole.as_bytes().chunks(2).enumerate() {
            values[i] = digits(std::str::from_utf8(part).ok()?)?;
        }
    }
    if let Some(fraction) = fraction {
        if fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let length = fraction.len().min(6);
        values[3] = digits(&fraction[..length])? * 10u32.pow(6 - length as u32);
    }
    Some(values)
}

fn local_seconds(naive: chrono::NaiveDateTime) -> Option<i64> {
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(value) => Some(value.timestamp()),
        // Python's default fold=0 selects the first occurrence.
        chrono::LocalResult::Ambiguous(a, b) => Some(a.timestamp().min(b.timestamp())),
        chrono::LocalResult::None => {
            // A nonexistent wall time uses the pre-transition offset. Probe
            // either side without changing the process timezone or its clock.
            let before = naive.checked_sub_signed(chrono::Duration::days(1))?;
            let after = naive.checked_add_signed(chrono::Duration::days(1))?;
            let before = Local.from_local_datetime(&before).earliest()?;
            let after = Local.from_local_datetime(&after).latest()?;
            let offset = before
                .offset()
                .local_minus_utc()
                .min(after.offset().local_minus_utc());
            Some(naive.and_utc().timestamp() - i64::from(offset))
        }
    }
}

pub(super) fn parse(s: &str) -> Option<f64> {
    // Match received_timestamp's replacement, including invalid embedded Zs.
    let s = s.replace('Z', "+00:00");
    let chars: Vec<_> = s.chars().collect();
    let end = date_end(&chars)?;
    let day: String = chars.get(..end)?.iter().collect();
    let day = date(&day)?;
    let (time, offset) = if end == chars.len() {
        ([0; 4], None)
    } else {
        let tail: String = chars.get(end + 1..)?.iter().collect();
        let (time, offset) = match tail.find(['+', '-']) {
            Some(i) => {
                let parsed = clock(&tail[i + 1..])?;
                let seconds =
                    i64::from(parsed[0]) * 3600 + i64::from(parsed[1]) * 60 + i64::from(parsed[2]);
                let micros = if seconds == 0 {
                    0
                } else {
                    seconds * 1_000_000 + i64::from(parsed[3])
                };
                if micros >= 86_400_000_000 {
                    return None;
                }
                let sign = if tail.as_bytes()[i] == b'-' { -1 } else { 1 };
                (&tail[..i], Some(sign * micros))
            }
            None => (tail.as_str(), None),
        };
        (clock(time)?, offset)
    };
    let naive = day.and_hms_micro_opt(time[0], time[1], time[2], time[3])?;
    if let Some(offset) = offset {
        let micros = naive.and_utc().timestamp_micros() - offset;
        // Decimal parsing rounds the rational once, unlike casting the large
        // microsecond integer to f64 before division (which loses low bits).
        let sign = if micros < 0 { "-" } else { "" };
        let magnitude = micros.unsigned_abs();
        format!(
            "{sign}{}.{:06}",
            magnitude / 1_000_000,
            magnitude % 1_000_000
        )
        .parse()
        .ok()
    } else {
        Some(local_seconds(naive)? as f64 + f64::from(time[3]) / 1_000_000.0)
    }
}
