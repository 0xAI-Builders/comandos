//! Native N1 hook intake. Installation/cutover is deliberately a separate step.
use crate::{fresh_id, now_ms, open_state, process_start_time, state_path};
use comandos_core::notifications::LOCAL_SPEAKER;
use comandos_store::{
    Error, Result,
    intake::{self, Reception},
    notifications, state,
};
use serde_json::{Value, json};
use std::io::{self, Read};
use std::path::PathBuf;

/// Punto de entrada compartido: imprime el uso cuando los argumentos no son válidos.
pub fn run(args: &[String]) -> Result<i32> {
    match run_inner(args)? {
        2 => {
            usage();
            Ok(2)
        }
        code => Ok(code),
    }
}

fn run_inner(args: &[String]) -> Result<i32> {
    let (explicit, args) = if args.first().is_some_and(|s| s == "--state") {
        let Some(path) = args.get(1).filter(|s| !s.is_empty()) else {
            return Ok(2);
        };
        (Some(PathBuf::from(path)), &args[2..])
    } else {
        (None, args)
    };
    let valid = match args.first().map(String::as_str) {
        Some("record") => args.len() == 1 || (args.len() == 3 && args[1] == "--claim-sound"),
        Some("claim-local") => args.len() == 3,
        Some("migrate") => args.len() == 1,
        Some("import-legacy") => args.len() == 2,
        Some("--help" | "-h") => {
            usage();
            return Ok(0);
        }
        _ => false,
    };
    if !valid {
        return Ok(2);
    }
    // Parse before opening a database, matching the original hook's no-write
    // behavior for invalid JSON and non-object payloads.
    let payload = if args[0] == "record" {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        let payload: Value =
            comandos_core::json::parse_value(if input.is_empty() { "{}" } else { &input })
                .map_err(|e| Error::Validation(e.to_string()))?;
        if !payload.is_object() {
            return Ok(1);
        }
        payload
    } else {
        Value::Null
    };
    let path = resolve_path(explicit.as_deref())?;
    if args[0] == "migrate" {
        let conn = state::connect(&path)?;
        let result = state::migrate(&conn, state::MIGRATIONS, now_ms()? as f64 / 1000.0)?;
        println!(
            "{}",
            json!({"version":result.version,"backup":result.backup})
        );
        return Ok(0);
    }
    let conn = open_state(&path, 1500)?;
    if args[0] == "import-legacy" {
        println!(
            "{}",
            json!({"count":crate::legacy::import_legacy(&conn,std::path::Path::new(&args[1]))?})
        );
        return Ok(0);
    }
    if args[0] == "claim-local" {
        let now = timestamp()?;
        for device in [args[2].as_str(), LOCAL_SPEAKER] {
            if notifications::claim_sound(&conn, &args[1], device, now, None)?["play"] == true {
                println!("{}", json!({"play":true,"device":device}));
                return Ok(0);
            }
        }
        return Ok(1);
    }
    if record_on(&conn, &payload, args.get(2).map(String::as_str))? {
        println!("play");
    }
    Ok(0)
}

fn resolve_path(explicit: Option<&std::path::Path>) -> Result<PathBuf> {
    let override_path = std::env::var_os("COMANDOS_STATE_DB").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from);
    let home = std::env::var_os("HOME").map(PathBuf::from);
    state_path(
        explicit,
        override_path.as_deref(),
        xdg.as_deref(),
        home.as_deref(),
    )
}

/// `record [--claim-sound device]` en proceso, para los hooks: mismo estado que la
/// CLI y `true` cuando esta máquina gana el sonido (lo que la CLI imprime como
/// `play`). El payload debe ser un objeto.
pub fn record_event(payload: &Value, claim_device: Option<&str>) -> Result<bool> {
    if !payload.is_object() {
        return Err(Error::Validation(
            "el evento debe ser un objeto JSON".into(),
        ));
    }
    let conn = open_state(&resolve_path(None)?, 1500)?;
    record_on(&conn, payload, claim_device)
}

fn record_on(
    conn: &rusqlite::Connection,
    payload: &Value,
    claim_device: Option<&str>,
) -> Result<bool> {
    let started = payload["panePid"]
        .as_str()
        .and_then(process_start_time)
        .map(|n| n.to_string());
    let stored = intake::record(
        conn,
        payload,
        &Reception {
            now_ms: now_ms()?,
            event_id: &fresh_id("event")?,
            receipt_id: &fresh_id("receipt")?,
            process_start: started.as_deref(),
        },
    )?;
    if let (Some(event), Some(device)) = (stored, claim_device.filter(|s| !s.is_empty()))
        && event["duplicate"] != true
    {
        let now = timestamp()?;
        for device in [device, LOCAL_SPEAKER] {
            if notifications::claim_sound(
                conn,
                event["eventId"].as_str().expect("stored event id"),
                device,
                now,
                None,
            )?["play"]
                == true
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn timestamp() -> Result<i64> {
    i64::try_from(now_ms()?).map_err(|e| Error::Validation(e.to_string()))
}
fn usage() {
    eprintln!(
        "uso: comandos-events [--state ruta] record [--claim-sound deviceId] < evento.json\n     comandos-events [--state ruta] claim-local eventId deviceId\n     comandos-events [--state ruta] migrate\n     comandos-events [--state ruta] import-legacy ruta"
    );
}
