//! Sugerencias por pane (`_annotate_suggestion` 6976) sobre el contexto de
//! `_suggestion_context` (6953). Una excepción del Python dentro del bucle
//! detiene la anotación de las tarjetas restantes sin error: el
//! `try/except Exception: pass` de `read_states`.
use super::{PyFloat, StateFault, is_text, py_float, py_round_text, py_str, str_or_empty};
use crate::dash::native::py::{Conversion, int_of};
use comandos_core::json::{python_eq, truthy};
use regex::Regex;
use serde_json::{Map, Value};
use std::{collections::BTreeSet, sync::LazyLock};

/// `(guard, routes, latency)` de `_suggestion_context`.
#[derive(Debug, Clone, Default)]
pub struct SuggestContext {
    /// `token_guard_with_forecast()` (`{}` si falló).
    pub guard: Value,
    /// Ids de rutas seleccionables.
    pub routes: BTreeSet<String>,
    /// `{(modelo, esfuerzo): (durationP50Ms, attempts)}` en orden de inserción.
    pub latency: Vec<((Value, Value), (Value, Value))>,
}

/// `_EXPENSIVE_MODEL_RE = re.compile(r"fable|opus|-sol", re.I)`.
static EXPENSIVE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?i)fable|opus|-sol").ok());

fn expensive(model: &str) -> Result<bool, StateFault> {
    EXPENSIVE
        .as_ref()
        .map(|re| re.is_match(model))
        .ok_or(StateFault::Failure)
}

/// Un objetivo de `_CHEAP_TARGETS` / `_CLAUDE_FALLBACK`.
struct Target {
    route_id: &'static str,
    model: &'static str,
    effort: &'static str,
    label: &'static str,
}

fn cheap_target(agent: &str) -> Option<Target> {
    let (route_id, model, effort, label) = match agent {
        "claude" => ("claude:codex", "gpt-5.6-luna", "low", "Codex Luna · low"),
        "codex" => ("codex:codex", "gpt-5.6-luna", "low", "Luna · low"),
        "grok" => ("grok:grok", "grok-4.5", "low", "Grok 4.5 · low"),
        _ => return None,
    };
    Some(Target {
        route_id,
        model,
        effort,
        label,
    })
}

const CLAUDE_FALLBACK: Target = Target {
    route_id: "claude:claude",
    model: "claude-sonnet-5",
    effort: "low",
    label: "Sonnet · low",
};

/// Por qué se corta la anotación de una tarjeta.
enum Halt {
    /// Excepción del Python: se detienen las restantes, sin error.
    Raise,
    /// Incierto o fallo propio de la ruta.
    Fault(StateFault),
}

impl From<StateFault> for Halt {
    fn from(fault: StateFault) -> Self {
        Halt::Fault(fault)
    }
}

type Step<T> = Result<T, Halt>;

/// `x.get(key)`: un no-`dict` es `AttributeError`.
fn get<'a>(value: &'a Value, key: &str) -> Step<Option<&'a Value>> {
    match value {
        Value::Object(map) => Ok(map.get(key)),
        _ => Err(Halt::Raise),
    }
}

/// `int(x)`.
fn py_int(value: &Value) -> Step<i64> {
    match int_of(value) {
        Ok(n) => Ok(n),
        Err(Conversion::Exotic) => Err(Halt::Fault(StateFault::Decline)),
        Err(_) => Err(Halt::Raise),
    }
}

/// `float(x)`.
fn py_float_step(value: &Value) -> Step<f64> {
    match py_float(value) {
        PyFloat::Value(f) => Ok(f),
        PyFloat::Raises => Err(Halt::Raise),
        PyFloat::Unsure => Err(Halt::Fault(StateFault::Decline)),
    }
}

/// `round(x)` de un `float` como texto.
fn round_text(x: f64) -> Step<String> {
    py_round_text(x).ok_or(Halt::Raise)
}

/// `for x in (v or [])` cuando cada elemento se usa con `.get`: solo una lista
/// sirve; otro iterable no vacío da `AttributeError` y un no-iterable `TypeError`.
fn list_or_empty(value: Option<&Value>) -> Step<&[Value]> {
    match value.filter(|v| truthy(v)) {
        None => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(Halt::Raise),
    }
}

/// `MOTOR_RESULT.get(opkey) or {}` con un cambio en curso (`stage` sin `ok`).
fn pending_change(motor: &Map<String, Value>, key: &str) -> Result<bool, StateFault> {
    match motor.get(key).filter(|v| truthy(v)) {
        None => Ok(false),
        Some(Value::Object(pending)) => {
            Ok(pending.contains_key("stage") && !pending.contains_key("ok"))
        }
        // La Tarea 5 declina antes un `motor-results.json` con valores no objeto.
        Some(_) => Err(StateFault::Decline),
    }
}

fn opkey(item: &Map<String, Value>) -> Result<String, StateFault> {
    let session = py_str(item.get("session").unwrap_or(&Value::Null))?;
    let pane = str_or_empty(item.get("pane"))?;
    Ok(format!("{session}|{pane}"))
}

fn suggestible_agent(item: &Map<String, Value>) -> Option<&str> {
    let agent = item.get("agent").and_then(Value::as_str)?;
    matches!(agent, "claude" | "codex" | "grok").then_some(agent)
}

/// Si alguna tarjeta consultaría el contexto (`guard`, rutas, latencia): viva,
/// con agente sugerible, sin cambio en curso y trabajando o con modelo caro.
/// Las demás producen lo mismo con un contexto vacío.
pub fn needs_context(items: &[Value], motor: &Map<String, Value>) -> Result<bool, StateFault> {
    for item in items {
        let Some(item) = item.as_object() else {
            continue;
        };
        if !item.get("alive").is_some_and(truthy) || suggestible_agent(item).is_none() {
            continue;
        }
        if pending_change(motor, &opkey(item)?)? {
            continue;
        }
        if is_text(item.get("status"), "working") || expensive(&str_or_empty(item.get("model"))?)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// El proyecto del `guard` que corresponde a la tarjeta (`next(...)`).
fn guard_project<'a>(
    item: &Map<String, Value>,
    guard: &'a Value,
) -> Step<Option<&'a Map<String, Value>>> {
    for project in list_or_empty(get(guard, "projects")?)? {
        let Value::Object(project) = project else {
            return Err(Halt::Raise);
        };
        let Some(name) = project.get("project").filter(|v| truthy(v)) else {
            continue;
        };
        if python_eq(item.get("project").unwrap_or(&Value::Null), name) {
            return Ok(Some(project));
        }
        let cwd = str_or_empty(item.get("cwd"))?;
        // `"/" + p["project"]` con un no-str: `TypeError`.
        let Value::String(name) = name else {
            return Err(Halt::Raise);
        };
        if cwd.ends_with(&format!("/{name}")) {
            return Ok(Some(project));
        }
    }
    Ok(None)
}

fn suggestion(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn switch(icon: &str, confidence: &str, target: &Target, text: String) -> Value {
    suggestion(vec![
        ("kind", Value::from("switch")),
        ("icon", Value::from(icon)),
        ("confidence", Value::from(confidence)),
        ("routeId", Value::from(target.route_id)),
        ("model", Value::from(target.model)),
        ("effort", Value::from(target.effort)),
        ("label", Value::from(target.label)),
        ("text", Value::from(text)),
        ("button", Value::from(format!("Cambiar a {}", target.label))),
    ])
}

/// `round(x)` sobre lo que dé el `guard` (`int` intacto, `float` a mitad par).
fn round_value(value: &Value) -> Step<String> {
    match value {
        Value::Bool(b) => Ok(u8::from(*b).to_string()),
        Value::Number(n) if !n.as_str().contains(['.', 'e', 'E', 'N', 'I']) => Ok(py_str(value)?),
        Value::Number(_) => round_text(py_float_step(value)?),
        _ => Err(Halt::Raise),
    }
}

/// `measured[0] / 1000` de Python (división verdadera, siempre `float`).
fn per_thousand(value: &Value) -> Step<f64> {
    match value {
        Value::Bool(b) => Ok(f64::from(u8::from(*b)) / 1000.0),
        Value::Number(n) => {
            let raw = n.as_str();
            let x: f64 = raw.parse().map_err(|_| Halt::Fault(StateFault::Decline))?;
            // Un entero mayor que 2^53 no cabe exacto en el `double` que divide.
            if !raw.contains(['.', 'e', 'E', 'N', 'I']) && x.abs() > 9_007_199_254_740_992.0 {
                return Err(Halt::Fault(StateFault::Decline));
            }
            Ok(x / 1000.0)
        }
        _ => Err(Halt::Raise),
    }
}

fn annotate_one(
    item: &mut Map<String, Value>,
    ctx: &SuggestContext,
    motor: &Map<String, Value>,
    now: f64,
) -> Step<()> {
    if !item.get("alive").is_some_and(truthy) {
        return Ok(());
    }
    let Some(agent) = suggestible_agent(item).map(str::to_owned) else {
        return Ok(());
    };
    if pending_change(motor, &opkey(item)?)? {
        return Ok(()); // ya hay un cambio en curso: no sugerir encima
    }
    let guard = &ctx.guard;
    let project = guard_project(item, guard)?;
    if is_text(item.get("status"), "working") {
        let ts = match item.get("ts").filter(|v| truthy(v)) {
            Some(ts) => py_float_step(ts)?,
            None => now,
        };
        let minutes = (now - ts) / 60.0;
        let calls10 = match project
            .and_then(|p| p.get("calls10m"))
            .filter(|v| truthy(v))
        {
            Some(calls) => py_int(calls)?,
            None => 0,
        };
        if minutes >= 15.0 && calls10 == 0 {
            let text = format!(
                "Turno de {} min y CERO llamadas al modelo en 10 min — parece colgado (tool atorada o espera infinita)",
                round_text(minutes)?
            );
            item.insert(
                "suggestion".into(),
                suggestion(vec![
                    ("kind", Value::from("key")),
                    ("icon", Value::from("zap")),
                    ("key", Value::from("Escape")),
                    ("confidence", Value::from("medida")),
                    ("text", Value::from(text)),
                    ("button", Value::from("Interrumpir (Esc)")),
                ]),
            );
            return Ok(());
        }
        if minutes >= 20.0 && calls10 >= 60 {
            let text = format!(
                "Posible loop: {calls10} llamadas/10 min y el turno ya lleva {} min sin terminar",
                round_text(minutes)?
            );
            item.insert(
                "suggestion".into(),
                suggestion(vec![
                    ("kind", Value::from("key")),
                    ("icon", Value::from("bell")),
                    ("key", Value::from("Escape")),
                    ("confidence", Value::from("medida")),
                    ("text", Value::from(text)),
                    ("button", Value::from("Interrumpir (Esc)")),
                ]),
            );
            return Ok(());
        }
    }
    let context = match item.get("contextPct").filter(|v| truthy(v)) {
        Some(pct) => py_float_step(pct)?,
        None => 0.0,
    };
    if context >= 80.0 {
        let text = format!(
            "Contexto al {}% — cada turno paga esa cache completa",
            round_text(context)?
        );
        item.insert(
            "suggestion".into(),
            suggestion(vec![
                ("kind", Value::from("send")),
                ("icon", Value::from("zap")),
                ("command", Value::from("/compact")),
                ("text", Value::from(text)),
                ("button", Value::from("Compactar ahora")),
            ]),
        );
        return Ok(());
    }
    let model = str_or_empty(item.get("model"))?;
    if !expensive(&model)? {
        return Ok(());
    }
    let target = match cheap_target(&agent).filter(|t| ctx.routes.contains(t.route_id)) {
        Some(target) => target,
        None if agent == "claude" && ctx.routes.contains(CLAUDE_FALLBACK.route_id) => {
            CLAUDE_FALLBACK
        }
        None => return Ok(()),
    };
    // `{f.get("scope"): f for f in ...}`: gana el último de cada alcance.
    let (mut fable, mut general) = (None, None);
    for forecast in list_or_empty(get(guard, "forecasts")?)? {
        let Value::Object(forecast) = forecast else {
            return Err(Halt::Raise);
        };
        match forecast.get("scope") {
            Some(Value::Array(_) | Value::Object(_)) => return Err(Halt::Raise),
            Some(Value::String(s)) if s == "Fable" => fable = Some(forecast),
            Some(Value::String(s)) if s == "General" => general = Some(forecast),
            _ => {}
        }
    }
    let critical = if model.to_lowercase().contains("fable") {
        fable
    } else {
        general
    };
    let measured = ctx
        .latency
        .iter()
        .rev()
        .find(|((m, e), _)| is_text(Some(m), target.model) && is_text(Some(e), target.effort));
    let (certainty, confidence) = match measured {
        Some((_, (p50, attempts))) if truthy(p50) => {
            let seconds = round_text(per_thousand(p50)?)?;
            let n = py_str(attempts)?;
            (format!(" · medido: p50 {seconds}s (n={n})"), "medida")
        }
        _ => (String::new(), "estimada"),
    };
    if let Some(critical) = critical.filter(|c| is_text(c.get("level"), "critical")) {
        let hours = match critical.get("downtimeHours").filter(|v| truthy(v)) {
            Some(hours) => round_value(hours)?,
            None => "0".to_owned(),
        };
        let scope = py_str(critical.get("scope").unwrap_or(&Value::Null))?;
        let text = format!(
            "Proyección: ~{hours}h sin {scope} antes del reset, y este pane usa {model}{certainty}"
        );
        item.insert(
            "suggestion".into(),
            switch("sprout", confidence, &target, text),
        );
    } else if let Some(project) = project
        .filter(|p| is_text(p.get("level"), "warning") || is_text(p.get("level"), "critical"))
    {
        let name = py_str(project.get("project").unwrap_or(&Value::Null))?;
        let calls = py_str(project.get("calls10m").unwrap_or(&Value::from(0)))?;
        let tokens = py_int(project.get("tokensHour").unwrap_or(&Value::from(0)))?;
        let millions = tokens.div_euclid(1_000_000);
        let text =
            format!("{name}: {calls} llamadas/10min · {millions}M tok/h en {model}{certainty}");
        item.insert(
            "suggestion".into(),
            switch("bell", confidence, &target, text),
        );
    }
    Ok(())
}

/// El bucle de `_annotate_suggestion` sobre las tarjetas en su orden de
/// construcción. Una excepción del Python detiene las restantes sin error.
pub fn annotate_all(
    items: &mut [Value],
    ctx: &SuggestContext,
    motor: &Map<String, Value>,
    now: f64,
) -> Result<(), StateFault> {
    for item in items.iter_mut() {
        let Value::Object(item) = item else {
            continue;
        };
        match annotate_one(item, ctx, motor, now) {
            Ok(()) => {}
            Err(Halt::Raise) => return Ok(()),
            Err(Halt::Fault(fault)) => return Err(fault),
        }
    }
    Ok(())
}

/// La latencia medida de `cc_usage.experiment_analytics` como la arma la
/// comprensión de 6966: cualquier excepción deja la tabla vacía.
pub fn latency_from(stats: &Value) -> Vec<((Value, Value), (Value, Value))> {
    let Value::Object(stats) = stats else {
        return Vec::new();
    };
    let configurations: &[Value] = match stats.get("configurations") {
        None => &[],
        Some(Value::Array(items)) => items,
        Some(Value::Object(map)) if map.is_empty() => &[],
        Some(Value::String(s)) if s.is_empty() => &[],
        Some(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for configuration in configurations {
        let Value::Object(c) = configuration else {
            return Vec::new();
        };
        let Some(p50) = c.get("durationP50Ms").filter(|v| truthy(v)) else {
            continue;
        };
        let model = c.get("model").cloned().unwrap_or(Value::Null);
        let effort = c.get("effort").cloned().unwrap_or(Value::Null);
        if model.is_array() || model.is_object() || effort.is_array() || effort.is_object() {
            return Vec::new();
        }
        let attempts = c.get("attempts").cloned().unwrap_or(Value::Null);
        out.push(((model, effort), (p50.clone(), attempts)));
    }
    out
}
