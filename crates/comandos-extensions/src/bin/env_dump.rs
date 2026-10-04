//! Binario de prueba para la paridad de `serve` stdio: vuelca cwd, argv y el entorno
//! relevante como JSON (una línea) y, aparte, su pid. Escribe un marcador por stderr
//! para comprobar que el proxy lo manda a `/dev/null`.
use serde_json::{Map, Value, json};

fn main() {
    let mut env: Vec<(String, String)> = std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .filter(|(k, _)| k.starts_with("COMANDOS_") || k == "HOME")
        .collect();
    env.sort();
    let env: Map<String, Value> = env
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    let argv: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    println!("{}", json!({"cwd": cwd, "argv": argv, "env": env}));
    println!("pid={}", std::process::id());
    eprintln!("env_dump-stderr-marker");
}
