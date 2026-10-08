//! On-demand resource observations. No subprocesses, signals or database writes.
use super::{Answer, Fault, Native, NativeOptions, reply};
use crate::HandlerError;
use http::StatusCode;
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const TTL: Duration = Duration::from_secs(60);
const BUILD_LIMIT: u64 = 16 * 1024 * 1024 * 1024;
const GROUPS: [(&str, &str); 8] = [
    ("app", "Interfaz ComandOS"),
    ("server", "Servidor ComandOS"),
    ("broker", "Broker MCP"),
    ("codex", "Codex"),
    ("claude", "Claude"),
    ("mcp_proxy", "Conexiones MCP"),
    ("mcp_upstream", "Servicios MCP compartidos"),
    ("other", "Herramientas y procesos derivados"),
];

#[derive(Default)]
pub struct Cache(Mutex<Option<(Instant, Value)>>);

pub async fn answer(native: &Arc<Native>) -> Answer {
    let cache = native.resources.clone();
    let opts = native.options().clone();
    let value = tokio::task::spawn_blocking(move || {
        // This lock belongs only to resource observations. A cancelled request
        // still completes and fills the cache for the other waiting clients.
        let mut memo = cache.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, value)) = &*memo
            && at.elapsed() < TTL
        {
            return value.clone();
        }
        let value = sample(&opts);
        *memo = Some((Instant::now(), value.clone()));
        value
    })
    .await
    .map_err(|_| Fault::Error(HandlerError::Failure))?;
    reply(StatusCode::OK, &value)
}

#[derive(Clone)]
struct Process {
    parent: u32,
    start: u64,
    direct: Option<&'static str>,
}

fn stat(path: &Path) -> Option<(u32, u64)> {
    let text = fs::read_to_string(path.join("stat")).ok()?;
    let values: Vec<_> = text.rsplit_once(')')?.1.split_whitespace().collect();
    Some((values.get(1)?.parse().ok()?, values.get(19)?.parse().ok()?))
}

fn classify(exe: &str, comm: &str, words: &[&str]) -> Option<&'static str> {
    let alias = words
        .first()
        .and_then(|s| Path::new(s).file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let has = |v: &str| words.contains(&v);
    match exe {
        "comandos-app" | "comandos-app-mac" => return Some("app"),
        "codex" | "codex-acp" => return Some("codex"),
        "claude" => return Some("claude"),
        _ => {}
    }
    if comm == "claude" {
        return Some("claude");
    }
    if ["comandos", "comandos-extensions"].contains(&exe)
        || ["cc-dash", "cc-extensions"].contains(&alias)
    {
        return Some(if has("serve-direct") {
            "mcp_upstream"
        } else if has("serve") {
            "mcp_proxy"
        } else if has("broker") {
            "broker"
        } else if alias == "cc-dash" || (has("dash") && has("--web-native")) {
            "server"
        } else {
            "other"
        });
    }
    None
}

fn group(pid: u32, processes: &HashMap<u32, Process>) -> Option<&'static str> {
    let mut current = pid;
    let mut visited = HashSet::new();
    while visited.insert(current) {
        let process = processes.get(&current)?;
        if let Some(kind) = process.direct {
            return Some(if current == pid {
                kind
            } else {
                match kind {
                    "broker" | "mcp_upstream" => "mcp_upstream",
                    "app" | "server" | "mcp_proxy" => kind,
                    _ => "other",
                }
            });
        }
        current = process.parent;
    }
    None
}

fn kb_fields(text: &str) -> BTreeMap<&str, u64> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let mut words = value.split_whitespace();
            let n: u64 = words.next()?.parse().ok()?;
            (words.next() == Some("kB")).then(|| (key, n.saturating_mul(1024)))
        })
        .collect()
}

fn memory(proc_root: &Path, warnings: &mut Vec<String>) -> Value {
    let info = fs::read_to_string(proc_root.join("meminfo")).unwrap_or_default();
    let fields = kb_fields(&info);
    let mut processes = HashMap::new();
    let mut unknown = 0_u64;
    let deadline = Instant::now() + Duration::from_millis(1500);
    let mut scanned = 0_usize;
    let mut partial = false;
    match fs::read_dir(proc_root) {
        Ok(entries) => {
            for entry in entries.flatten() {
                scanned += 1;
                if scanned > 12_000 || Instant::now() >= deadline {
                    partial = true;
                    break;
                }
                let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                    continue;
                };
                #[cfg(unix)]
                match entry.metadata() {
                    Ok(m) if m.uid() != nix::unistd::geteuid().as_raw() => continue,
                    Err(_) => {
                        unknown += 1;
                        continue;
                    }
                    _ => {}
                }
                let path = entry.path();
                let Some((parent, start)) = stat(&path) else {
                    unknown += 1;
                    continue;
                };
                let exe = fs::read_link(path.join("exe"))
                    .ok()
                    .and_then(|p| {
                        p.file_name().map(|s| {
                            s.to_string_lossy()
                                .trim_end_matches(" (deleted)")
                                .to_owned()
                        })
                    })
                    .unwrap_or_default();
                let comm = fs::read_to_string(path.join("comm")).unwrap_or_default();
                // Arguments are used only for process classification, never returned.
                let mut command = Vec::new();
                if let Ok(file) = fs::File::open(path.join("cmdline")) {
                    let _ = file.take(65536).read_to_end(&mut command);
                }
                let command = String::from_utf8_lossy(&command);
                let words: Vec<_> = command.split('\0').filter(|s| !s.is_empty()).collect();
                processes.insert(
                    pid,
                    Process {
                        parent,
                        start,
                        direct: classify(&exe, comm.trim(), &words),
                    },
                );
            }
        }
        Err(_) => {
            partial = true;
            warnings.push("No se pudo leer el inventario de procesos.".into());
        }
    }
    let mut sums: HashMap<&str, (u64, u64, u64)> = HashMap::new();
    let mut unreadable = unknown;
    let mut measured = 0_u64;
    for (&pid, process) in &processes {
        let Some(kind) = group(pid, &processes) else {
            continue;
        };
        let (bytes, count, failures) = sums.entry(kind).or_default();
        *count += 1;
        if Instant::now() >= deadline {
            partial = true;
            *failures += 1;
            unreadable += 1;
            continue;
        }
        let path = proc_root.join(pid.to_string());
        let pss = fs::read_to_string(path.join("smaps_rollup"))
            .ok()
            .and_then(|s| kb_fields(&s).get("Pss").copied());
        if stat(&path) != Some((process.parent, process.start)) || pss.is_none() {
            *failures += 1;
            unreadable += 1;
            continue;
        }
        *bytes += pss.unwrap_or(0);
        measured += 1;
    }
    if partial {
        warnings.push(
            "Inventario de memoria parcial: se alcanzó el límite de lectura de procesos.".into(),
        );
    }
    if unreadable > 0 {
        warnings.push(format!("Memoria parcial: {unreadable} procesos no se pudieron medir o cambiaron durante la lectura."));
    }
    if !fields.contains_key("MemTotal") || !fields.contains_key("MemAvailable") {
        warnings.push("Memoria total o disponible del sistema no verificable.".into());
    }
    let swap = fields
        .get("SwapTotal")
        .zip(fields.get("SwapFree"))
        .map(|(total, free)| total.saturating_sub(*free));
    json!({"totalBytes":fields.get("MemTotal"),"availableBytes":fields.get("MemAvailable"),"swapUsedBytes":swap,
        "groups":GROUPS.iter().map(|(id,label)| {
            let (bytes,count,failures)=sums.get(id).copied().unwrap_or_default();
            let measured=count.saturating_sub(failures);
            let known = if measured == 0 && (failures > 0 || partial) { None } else { Some(bytes) };
            json!({"id":id,"label":label,"pssBytes":known,"processCount":count,"measuredProcesses":measured,"unreadableProcesses":failures,"partial":partial || failures > 0})
        }).collect::<Vec<_>>(),
        "measuredProcesses":measured,"unreadableProcesses":unreadable,"partial":partial})
}

struct Budget {
    remaining: usize,
    deadline: Instant,
}

fn allocated(path: &Path, budget: &mut Budget) -> (u64, bool) {
    // Never traverse a symlink used as a root or one of its parents.
    if path
        .ancestors()
        .any(|p| fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()))
    {
        return (0, true);
    }
    let mut todo = vec![path.to_owned()];
    let mut seen = HashSet::new();
    let mut bytes = 0_u64;
    let mut partial = false;
    while let Some(path) = todo.pop() {
        if budget.remaining == 0 || Instant::now() >= budget.deadline {
            partial = true;
            break;
        }
        budget.remaining -= 1;
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => {
                partial = true;
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        #[cfg(unix)]
        {
            if !seen.insert((meta.dev(), meta.ino())) {
                continue;
            }
            bytes = bytes.saturating_add(meta.blocks().saturating_mul(512));
        }
        #[cfg(not(unix))]
        {
            bytes = bytes.saturating_add(meta.len());
        }
        if meta.is_dir() {
            match fs::read_dir(&path) {
                Ok(entries) => {
                    for entry in entries {
                        if todo.len() >= budget.remaining || Instant::now() >= budget.deadline {
                            partial = true;
                            break;
                        }
                        match entry {
                            Ok(entry) => todo.push(entry.path()),
                            Err(_) => partial = true,
                        }
                    }
                }
                Err(_) => partial = true,
            }
        }
    }
    (bytes, partial)
}

fn disk(opts: &NativeOptions, warnings: &mut Vec<String>) -> Value {
    let space = nix::sys::statvfs::statvfs(&opts.home).ok();
    if space.is_none() {
        warnings.push("No se pudo consultar el espacio libre del disco de HOME.".into());
    }
    let mut paths = vec![
        (
            "database",
            "Base unificada",
            opts.home.join(".local/share/comandos/comandos.sqlite3"),
        ),
        (
            "database_wal",
            "WAL de la base unificada",
            opts.home.join(".local/share/comandos/comandos.sqlite3-wal"),
        ),
        ("usage", "Base de uso", opts.usage_db.clone()),
        (
            "operations",
            "Diario de operaciones",
            opts.journal_db.clone(),
        ),
        ("app_state", "Estado de la interfaz", opts.state_db.clone()),
    ];
    for (id, label, path) in [
        ("usage_wal", "WAL de uso", &opts.usage_db),
        ("usage_shm", "SHM de uso", &opts.usage_db),
        ("operations_wal", "WAL de operaciones", &opts.journal_db),
        ("operations_shm", "SHM de operaciones", &opts.journal_db),
        ("app_state_wal", "WAL de interfaz", &opts.state_db),
        ("app_state_shm", "SHM de interfaz", &opts.state_db),
    ] {
        let suffix = if id.ends_with("_wal") { "-wal" } else { "-shm" };
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        paths.push((id, label, std::path::PathBuf::from(name)));
    }
    paths.push((
        "database_shm",
        "SHM de la base unificada",
        opts.home.join(".local/share/comandos/comandos.sqlite3-shm"),
    ));
    if let Some(repo) = &opts.repo_root {
        paths.push((
            "build_cache",
            "Caché de compilación",
            repo.join(".build/target-integration-acp"),
        ));
    } else {
        warnings.push("Ruta del repositorio y caché de compilación no disponible.".into());
    }
    paths.push((
        "releases",
        "Versiones instaladas",
        opts.home.join(".local/share/comandos/releases"),
    ));
    paths.push((
        "components",
        "Componentes instalados",
        opts.home.join(".local/share/comandos/components"),
    ));
    if let Some(repo) = &opts.repo_root {
        paths.push((
            "workspace",
            "Repositorio y archivos de trabajo",
            repo.clone(),
        ));
    }
    let mut budget = Budget {
        remaining: 20_000,
        deadline: Instant::now() + Duration::from_millis(300),
    };
    let mut seen = HashSet::new();
    let mut rows = vec![];
    for (id, label, path) in paths {
        if !path.is_absolute() || !seen.insert(path.clone()) {
            continue;
        }
        let (bytes, partial) = allocated(&path, &mut budget);
        if partial {
            warnings.push(format!(
                "Tamaño parcial de {label}; se limitó el recorrido de disco."
            ));
        }
        let known = if partial && bytes == 0 {
            None
        } else {
            Some(bytes)
        };
        rows.push(json!({"id":id,"label":label,"path":path,"bytes":known,"partial":partial}));
    }
    json!({"totalBytes":space.as_ref().map(|s| s.blocks().saturating_mul(s.fragment_size())),
        "availableBytes":space.as_ref().map(|s| s.blocks_available().saturating_mul(s.fragment_size())),"paths":rows})
}

fn sample(opts: &NativeOptions) -> Value {
    let mut warnings = vec![];
    let memory = memory(&opts.proc_root, &mut warnings);
    let disk = disk(opts, &mut warnings);
    resource_warnings(&memory, &disk, &mut warnings);
    json!({"sampledAt":(opts.clock)(),"memory":memory,"disk":disk,
        "safeguards":{"buildCacheLimitBytes":BUILD_LIMIT,"minFreeDiskBytes":BUILD_LIMIT},"warnings":warnings})
}

fn resource_warnings(memory: &Value, disk: &Value, warnings: &mut Vec<String>) {
    if let Some(available) = memory["availableBytes"].as_u64() {
        if available < 2 * 1024 * 1024 * 1024 {
            warnings.push("Memoria disponible crítica: menos de 2 GiB.".into());
        } else if available < 8 * 1024 * 1024 * 1024
            || memory["totalBytes"]
                .as_u64()
                .is_some_and(|total| available < total / 10)
        {
            warnings.push(
                "Memoria disponible baja; conviene revisar los procesos de mayor consumo.".into(),
            );
        }
    }
    if disk["availableBytes"]
        .as_u64()
        .is_some_and(|n| n < BUILD_LIMIT)
    {
        warnings.push(
            "Disco libre por debajo de 16 GiB: el control de compilación rechaza nuevos builds."
                .into(),
        );
    }
    if disk["paths"].as_array().is_some_and(|rows| {
        rows.iter().any(|r| {
            r["id"] == "build_cache" && r["bytes"].as_u64().is_some_and(|n| n > BUILD_LIMIT)
        })
    }) {
        warnings.push(
            "Caché de compilación por encima de 16 GiB: se rechazan nuevas compilaciones.".into(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nearest_owner_counts_each_pid_once_and_keeps_shared_upstreams_separate() {
        let rows = HashMap::from([
            (
                1,
                Process {
                    parent: 0,
                    start: 1,
                    direct: Some("app"),
                },
            ),
            (
                2,
                Process {
                    parent: 1,
                    start: 1,
                    direct: Some("codex"),
                },
            ),
            (
                3,
                Process {
                    parent: 2,
                    start: 1,
                    direct: Some("mcp_proxy"),
                },
            ),
            (
                4,
                Process {
                    parent: 2,
                    start: 1,
                    direct: None,
                },
            ),
            (
                5,
                Process {
                    parent: 0,
                    start: 1,
                    direct: Some("broker"),
                },
            ),
            (
                6,
                Process {
                    parent: 5,
                    start: 1,
                    direct: None,
                },
            ),
            (
                7,
                Process {
                    parent: 6,
                    start: 1,
                    direct: None,
                },
            ),
            (
                8,
                Process {
                    parent: 0,
                    start: 1,
                    direct: None,
                },
            ),
        ]);
        assert_eq!(
            (1..=8)
                .filter_map(|pid| group(pid, &rows))
                .collect::<Vec<_>>(),
            vec![
                "app",
                "codex",
                "mcp_proxy",
                "other",
                "broker",
                "mcp_upstream",
                "mcp_upstream"
            ]
        );
    }
    #[test]
    fn pss_is_not_rss_and_bad_units_are_unknown() {
        let values = kb_fields("Pss: 40 kB\nRss: 100 kB\nSwapPss: 3 kB\nBroken: 5 pages\n");
        assert_eq!(values.get("Pss"), Some(&40960));
        assert!(!values.contains_key("Broken"));
    }
    #[test]
    fn multicall_and_dedicated_proxies_have_same_group() {
        for exe in ["comandos", "comandos-extensions"] {
            assert_eq!(
                classify(exe, "", &["cc-extensions", "serve", "mail"]),
                Some("mcp_proxy")
            );
            assert_eq!(
                classify(exe, "", &["cc-extensions", "broker"]),
                Some("broker")
            );
        }
        assert_eq!(classify("node", "", &["node", "serve"]), None);
    }
    #[cfg(unix)]
    #[test]
    fn missing_pss_stays_unknown_while_descendants_are_counted_once() {
        let root = std::env::temp_dir().join(format!("resources-proc-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("meminfo"),
            "MemTotal: 1000 kB\nMemAvailable: 600 kB\nSwapTotal: 100 kB\nSwapFree: 40 kB\n",
        )
        .unwrap();
        for (pid, parent, exe, pss) in [
            (100, 0, "codex", None),
            (101, 100, "node", Some(10)),
            (102, 0, "node", Some(1000)),
        ] {
            let path = root.join(pid.to_string());
            fs::create_dir(&path).unwrap();
            let mut fields = vec!["0".to_owned(); 20];
            fields[0] = "S".into();
            fields[1] = parent.to_string();
            fields[19] = "42".into();
            fs::write(
                path.join("stat"),
                format!("{pid} (fixture) {}", fields.join(" ")),
            )
            .unwrap();
            std::os::unix::fs::symlink(format!("/fixture/{exe}"), path.join("exe")).unwrap();
            fs::write(path.join("cmdline"), format!("{exe}\0")).unwrap();
            if let Some(pss) = pss {
                fs::write(
                    path.join("smaps_rollup"),
                    format!("Pss: {pss} kB\nRss: 999 kB\n"),
                )
                .unwrap();
            }
        }
        let value = memory(&root, &mut vec![]);
        let rows = value["groups"].as_array().unwrap();
        let codex = rows.iter().find(|r| r["id"] == "codex").unwrap();
        assert_eq!(codex["processCount"], 1);
        assert!(codex["pssBytes"].is_null());
        assert_eq!(codex["partial"], true);
        let other = rows.iter().find(|r| r["id"] == "other").unwrap();
        assert_eq!(other["processCount"], 1);
        assert_eq!(other["pssBytes"], 10240);
        assert_eq!(value["measuredProcesses"], 1);
        assert_eq!(value["unreadableProcesses"], 1);
        assert_eq!(value["availableBytes"], 614400);
        assert_eq!(value["swapUsedBytes"], 61440);
        fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn repeated_requests_reuse_the_sample_without_reading_changed_proc_files() {
        let root = std::env::temp_dir().join(format!("resources-cache-{}", std::process::id()));
        let proc_root = root.join("proc");
        fs::create_dir_all(&proc_root).unwrap();
        fs::write(
            proc_root.join("meminfo"),
            "MemTotal: 100 kB\nMemAvailable: 50 kB\n",
        )
        .unwrap();
        let mut opts = NativeOptions::for_home(&root, root.join("state.sqlite"));
        opts.proc_root = proc_root.clone();
        let native = Arc::new(Native::new(opts));
        let first = match answer(&native).await {
            Ok(reply) => reply,
            Err(_) => panic!("resource response"),
        };
        fs::write(
            proc_root.join("meminfo"),
            "MemTotal: 200 kB\nMemAvailable: 150 kB\n",
        )
        .unwrap();
        let second = match answer(&native).await {
            Ok(reply) => reply,
            Err(_) => panic!("cached response"),
        };
        let crate::ReplyBody::Bytes(first) = first.body else {
            panic!("JSON bytes")
        };
        let crate::ReplyBody::Bytes(second) = second.body else {
            panic!("JSON bytes")
        };
        assert_eq!(first, second);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn low_resources_warn_without_claiming_a_hard_memory_limit() {
        let mut warnings = vec![];
        resource_warnings(
            &json!({"availableBytes":1024}),
            &json!({"availableBytes":1024,"paths":[{"id":"build_cache","bytes":BUILD_LIMIT+1}]}),
            &mut warnings,
        );
        assert_eq!(warnings.len(), 3);
        warnings.clear();
        resource_warnings(
            &json!({"availableBytes":null}),
            &json!({"availableBytes":null}),
            &mut warnings,
        );
        assert!(warnings.is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn disk_walk_skips_symlinks_deduplicates_links_and_marks_a_budget_stop() {
        let root = std::env::temp_dir().join(format!("resources-disk-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("payload");
        fs::write(&path, vec![1_u8; 8192]).unwrap();
        let duplicate = root.join("hardlink");
        fs::hard_link(&path, &duplicate).unwrap();
        std::os::unix::fs::symlink("/", root.join("external")).unwrap();
        let expected = fs::metadata(&root).unwrap().blocks() * 512
            + fs::metadata(&path).unwrap().blocks() * 512;
        let mut budget = Budget {
            remaining: 100,
            deadline: Instant::now() + Duration::from_secs(2),
        };
        assert_eq!(allocated(&root, &mut budget), (expected, false));
        let mut budget = Budget {
            remaining: 0,
            deadline: Instant::now() + Duration::from_secs(2),
        };
        assert_eq!(allocated(&root, &mut budget), (0, true));
        fs::remove_dir_all(root).unwrap();
    }
}
