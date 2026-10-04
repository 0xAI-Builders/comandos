//! Versioned deterministic focus reward rules; no clock or process access.
use crate::{
    json::truthy,
    pomodoro::{MINUTE_MS, REPORT_TZ, local_date},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;
pub fn policy_v1() -> Value {
    json!({"policyVersion":"v1","xpPerMinute":10,"xpPerLevel":1000,"dailyGoalMinutes":100,"countCancelledActive":true,"timezone":"America/Mexico_City","achievements":[{"id":"first","kind":"completedBlocks","threshold":1,"asset":"first","title":"Primer bloque"},{"id":"hundred","kind":"focusMinutes","threshold":100,"asset":"hundred","title":"100 minutos"},{"id":"streak3","kind":"maxStreakDays","threshold":3,"asset":"streak","title":"3 días seguidos"}]})
}
pub fn eligible(block: &Value, policy: &Value, activation: Option<i64>) -> bool {
    if block["mode"] != "focus"
        || block.get("provenance").is_some_and(|p| p != "measured")
        || block["activeMs"].is_null()
        || block["endedAtMs"].is_null()
    {
        return false;
    }
    if let Some(at) = activation
        && numeric_before(&block["endedAtMs"], at).is_none_or(|before| before)
    {
        return false;
    }
    block["status"] == "completed"
        || (block["status"] == "cancelled" && truthy(&policy["countCancelledActive"]))
}
// Compare the Python number before narrowing it for a SQLite insertion.
fn numeric_before(value: &Value, at: i64) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(i64::from(*b) < at),
        Value::Number(n) if crate::pomodoro::integer_token(n.as_str()) => {
            Some(compare_integer(n.as_str(), at).is_lt())
        }
        Value::Number(n) => {
            let end = n.as_str().parse::<f64>().ok()?;
            if end.is_nan() {
                return Some(false);
            }
            if end.is_infinite() {
                return Some(end.is_sign_negative());
            }
            let integer = format!("{:.0}", end.trunc());
            Some(match compare_integer(&integer, at) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => end < end.trunc(),
            })
        }
        _ => None,
    }
}
fn compare_integer(raw: &str, at: i64) -> std::cmp::Ordering {
    let magnitude = raw.trim_start_matches('-').trim_start_matches('0');
    let negative = raw.starts_with('-') && !magnitude.is_empty();
    let target = at.to_string();
    let target_magnitude = target.trim_start_matches('-').trim_start_matches('0');
    let target_negative = at < 0;
    if negative != target_negative {
        return if negative {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        };
    }
    let order = magnitude
        .len()
        .cmp(&target_magnitude.len())
        .then_with(|| magnitude.cmp(target_magnitude));
    if negative { order.reverse() } else { order }
}
/// Preserve Python's ValueError versus OverflowError legacy catch scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegerError {
    Invalid,
    NonFiniteOverflow,
}
impl std::fmt::Display for IntegerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "entero inválido",
            Self::NonFiniteOverflow => "entero no finito fuera de rango",
        })
    }
}
impl std::error::Error for IntegerError {}
/// Integer coercion at legacy policy/record boundaries. Overflow is an error.
pub fn integer_string(value: &Value) -> Result<String, IntegerError> {
    match value {
        Value::Bool(b) => Ok(i64::from(*b).to_string()),
        Value::Number(n) if crate::pomodoro::integer_token(n.as_str()) => Ok(n.as_str().into()),
        Value::Number(n) => {
            let f = n
                .as_str()
                .parse::<f64>()
                .map_err(|_| IntegerError::Invalid)?;
            if f.is_nan() {
                return Err(IntegerError::Invalid);
            }
            if !f.is_finite() {
                return Err(IntegerError::NonFiniteOverflow);
            }
            Ok(format!("{:.0}", f.trunc()))
        }
        Value::String(s) => {
            let s = s.trim_matches(|c: char| c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}'));
            let (sign, digits) = if let Some(s) = s.strip_prefix('-') {
                ("-", s)
            } else {
                ("", s.strip_prefix('+').unwrap_or(s))
            };
            if digits.is_empty() {
                return Err(IntegerError::Invalid);
            }
            let mut out = String::new();
            let mut previous_digit = false;
            for c in digits.chars() {
                if c == '_' {
                    if !previous_digit {
                        return Err(IntegerError::Invalid);
                    }
                    previous_digit = false;
                    continue;
                }
                let d = decimal_digit(c).ok_or(IntegerError::Invalid)?;
                out.push((b'0' + d) as char);
                previous_digit = true;
            }
            if !previous_digit {
                return Err(IntegerError::Invalid);
            }
            let out = out.trim_start_matches('0');
            if out.is_empty() {
                Ok("0".into())
            } else {
                Ok(format!("{sign}{out}"))
            }
        }
        _ => Err(IntegerError::Invalid),
    }
}
fn decimal_digit(c: char) -> Option<u8> {
    // Unicode decimal (Nd) blocks accepted by Python's int string boundary.
    const STARTS: &[u32] = &[
        0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
        0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
        0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0,
        0xff10, 0x104a0, 0x10d30, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450, 0x114d0,
        0x11650, 0x116c0, 0x11730, 0x118e0, 0x11950, 0x11c50, 0x11d50, 0x11da0, 0x16a60, 0x16ac0,
        0x16b50, 0x1d7ce, 0x1d7d8, 0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e950, 0x1fbf0,
    ];
    let n = c as u32;
    STARTS
        .iter()
        .find(|s| n >= **s && n < **s + 10)
        .map(|s| (n - s) as u8)
}
pub fn int(value: &Value) -> Result<i64, String> {
    integer_string(value)
        .map_err(|e| e.to_string())?
        .parse()
        .map_err(|_| "entero fuera de rango".into())
}
/// Python float coercion used by the former focus-file timestamp boundary.
pub fn float(value: &Value) -> Result<f64, String> {
    match value {
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Number(n) => {
            let f = n
                .as_str()
                .parse::<f64>()
                .map_err(|_| "número fuera de rango")?;
            if crate::pomodoro::integer_token(n.as_str()) && !f.is_finite() {
                Err("número fuera de rango".into())
            } else {
                Ok(f)
            }
        }
        Value::String(s) => {
            let chars = s
                .trim_matches(|c: char| c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}'))
                .chars()
                .collect::<Vec<_>>();
            let mut out = String::new();
            for (i, c) in chars.iter().copied().enumerate() {
                if c == '_' {
                    if i == 0
                        || i + 1 == chars.len()
                        || decimal_digit(chars[i - 1]).is_none()
                        || decimal_digit(chars[i + 1]).is_none()
                    {
                        return Err("número inválido".into());
                    }
                    continue;
                }
                if let Some(d) = decimal_digit(c) {
                    out.push((b'0' + d) as char)
                } else {
                    out.push(c)
                }
            }
            match out.to_ascii_lowercase().as_str() {
                "nan" | "+nan" | "-nan" => Ok(f64::NAN),
                "inf" | "infinity" | "+inf" | "+infinity" => Ok(f64::INFINITY),
                "-inf" | "-infinity" => Ok(f64::NEG_INFINITY),
                _ => out.parse().map_err(|_| "número inválido".into()),
            }
        }
        _ => Err("número inválido".into()),
    }
}
pub fn reward_for(block: &Value, policy: &Value) -> Result<Value, String> {
    let minutes = (int(&block["activeMs"])? / MINUTE_MS).max(0);
    let xp = minutes
        .checked_mul(int(&policy["xpPerMinute"])?)
        .ok_or("XP fuera de rango")?;
    Ok(json!({"minutes":minutes,"xp":xp}))
}
pub fn level_for(xp: i64, policy: &Value) -> Result<i64, String> {
    let per = int(&policy["xpPerLevel"])?;
    if per == 0 {
        return Err("xpPerLevel no puede ser 0".into());
    }
    xp.checked_div_euclid(per)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| "nivel fuera de rango".into())
}
pub fn progress(
    blocks: &[Value],
    policy: &Value,
    now: i64,
    activation: Option<i64>,
) -> Result<Value, String> {
    let zone = policy["timezone"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(REPORT_TZ);
    let today = local_date(now, zone)?;
    let mut seen = BTreeSet::new();
    let mut days = BTreeSet::new();
    let (mut xp, mut minutes, mut today_minutes, mut completed) = (0i64, 0i64, 0i64, 0i64);
    for block in blocks {
        if !eligible(block, policy, activation) || !seen.insert(block["blockId"].to_string()) {
            continue;
        }
        let r = reward_for(block, policy)?;
        let m = int(&r["minutes"])?;
        xp = xp.checked_add(int(&r["xp"])?).ok_or("XP fuera de rango")?;
        minutes = minutes.checked_add(m).ok_or("minutos fuera de rango")?;
        let start = if truthy(&block["startedAtMs"]) {
            &block["startedAtMs"]
        } else {
            &block["endedAtMs"]
        };
        let day = local_date(int(start)?, zone)?;
        if day == today {
            today_minutes = today_minutes
                .checked_add(m)
                .ok_or("minutos fuera de rango")?
        }
        if block["status"] == "completed" {
            completed += 1;
            days.insert(day);
        }
    }
    let mut current = 0i64;
    let mut cursor = if days.contains(&today) {
        today
    } else {
        today.pred_opt().ok_or("fecha fuera de rango")?
    };
    while days.contains(&cursor) {
        current += 1;
        cursor = cursor.pred_opt().ok_or("fecha fuera de rango")?;
    }
    let (mut best, mut run, mut previous) = (0i64, 0i64, None);
    for day in days {
        run = if previous.is_some_and(|p: chrono::NaiveDate| p.succ_opt() == Some(day)) {
            run + 1
        } else {
            1
        };
        best = best.max(run);
        previous = Some(day);
    }
    let per = int(&policy["xpPerLevel"])?;
    if per <= 0 {
        return Err("xpPerLevel debe ser positivo".into());
    }
    let remainder = xp.rem_euclid(per);
    let achievements = policy["achievements"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|a| {
                    let mut a = a.clone();
                    let value = match a["kind"].as_str() {
                        Some("completedBlocks") => completed,
                        Some("focusMinutes") => minutes,
                        Some("maxStreakDays") => best,
                        Some("streakDays") => current,
                        _ => 0,
                    };
                    let threshold = int(&a["threshold"])?;
                    a["unlocked"] = json!(value >= threshold);
                    Ok(a)
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(
        json!({"policyVersion":policy["policyVersion"],"xpPerMinute":int(&policy["xpPerMinute"])? ,"xpPerLevel":per,"xp":xp,"level":level_for(xp,policy)?,"levelPct":format!("{:.2}",100.0*remainder as f64/per as f64).parse::<f64>().map_err(|_|"porcentaje inválido")?,"xpToNextLevel":per-remainder,"focusMinutes":minutes,"completedBlocks":completed,"todayMinutes":today_minutes,"dailyGoalMinutes":int(&policy["dailyGoalMinutes"])? ,"streakDays":current,"maxStreakDays":best,"achievements":achievements}),
    )
}
