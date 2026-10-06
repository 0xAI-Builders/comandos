//! Native tmux/proc adapter. Tests inject a private tree; fixture roots cannot signal PIDs.
use super::{
    Result,
    batch::{self, Process, Runtime, SHELLS},
    install, process, release,
};
use comandos_runtime::pane_snapshot::{PaneInspector, PaneRef};
use comandos_store::domains::LayoutSnapshot;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    ffi::OsStr,
    fs,
    io::{BufRead, BufReader, Read},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
pub struct Local {
    pub home: PathBuf,
    pub proc: PathBuf,
    start: Instant,
    pub cancel: Arc<AtomicBool>,
}
pub(super) fn open_regular(path: &Path) -> Result<fs::File> {
    let m = path.symlink_metadata().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "missing".into()
        } else {
            format!("{}: {e}", path.display())
        }
    })?;
    if !m.is_file() {
        return Err(format!("{}: archivo no regular", path.display()));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags((nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_NOCTTY).bits())
        .open(path)
        .map_err(|e| e.to_string())?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if !opened.is_file() || (m.dev(), m.ino()) != (opened.dev(), opened.ino()) {
        return Err("archivo cambió al abrir".into());
    }
    Ok(file)
}
pub(super) fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let f = open_regular(path)?;
    if f.metadata().map_err(|e| e.to_string())?.len() > limit {
        return Err("archivo excede límite".into());
    }
    let mut body = vec![];
    f.take(limit + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() as u64 > limit {
        return Err("archivo excede límite".into());
    }
    Ok(body)
}
impl Local {
    pub fn new(home: PathBuf, proc: PathBuf, cancel: Arc<AtomicBool>) -> Result<Self> {
        if !proc.is_absolute() {
            return Err("--proc-root requiere ruta absoluta".into());
        }
        Ok(Self {
            home,
            proc,
            start: Instant::now(),
            cancel,
        })
    }
    fn execute(&self, name: &str, args: &[&str], timeout: Duration) -> Result<String> {
        if self.cancel.load(Ordering::SeqCst) {
            return Err("mantenimiento cancelado".into());
        }
        let path = install::which(name).ok_or_else(|| format!("No se encontró {name}"))?;
        let args = args.iter().map(OsStr::new).collect::<Vec<_>>();
        let out = process::run_when(&path, &args, None, timeout, &self.cancel)?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string()).map_err(|s| {
                if s.is_empty() {
                    format!("{name} rechazó la operación")
                } else {
                    s
                }
            });
        }
        String::from_utf8(out.stdout)
            .map(|s| s.trim().into())
            .map_err(|e| e.to_string())
    }
    fn children(&mut self) -> Result<HashMap<i64, Vec<i64>>> {
        let mut map: HashMap<i64, Vec<i64>> = HashMap::new();
        let entries = fs::read_dir(&self.proc).map_err(|e| e.to_string())?;
        for (count, e) in entries.enumerate() {
            if count > 100_000 {
                return Err("inventario de procesos excede límite".into());
            }
            let e = e.map_err(|e| e.to_string())?;
            let Some(name) = e.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(pid) = name.parse::<i64>() else {
                continue;
            };
            if pid <= 1 {
                continue;
            }
            let Ok(stat) = read_regular(&e.path().join("stat"), 1024 * 1024) else {
                continue;
            };
            let Ok(stat) = std::str::from_utf8(&stat) else {
                continue;
            };
            let Some((_, fields)) = stat.rsplit_once(')') else {
                continue;
            };
            let fields = fields.split_whitespace().collect::<Vec<_>>();
            if let Some(ppid) = fields.get(1).and_then(|s| s.parse::<i64>().ok()) {
                map.entry(ppid).or_default().push(pid);
            }
        }
        for kids in map.values_mut() {
            kids.sort_unstable();
        }
        Ok(map)
    }
    fn cli(&mut self, pid: i64, children: &HashMap<i64, Vec<i64>>) -> Result<Option<Process>> {
        let mut queue = VecDeque::from([pid]);
        let mut seen = HashSet::new();
        while let Some(pid) = queue.pop_front() {
            if !seen.insert(pid) {
                continue;
            }
            let p = match self.process(pid) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if p.args
                .first()
                .is_some_and(|s| Path::new(s).file_name() == Some(OsStr::new("codex")))
            {
                if p.args
                    .iter()
                    .skip(1)
                    .any(|s| matches!(s.as_str(), "app-server" | "exec-server"))
                {
                    continue;
                }
                return Ok(Some(p));
            }
            if let Some(kids) = children.get(&pid) {
                queue.extend(kids);
            }
        }
        Ok(None)
    }
    pub fn inventory(&mut self, pending: Option<&[Value]>) -> Result<(Vec<Value>, Vec<String>)> {
        let saved = LayoutSnapshot {
            home: &self.home,
            file: self.home.join(".claude/hooks/app-sessions-v2.json"),
        }
        .read_readonly()
        .map_err(|e| e.to_string())?;
        let inspector = PaneInspector::new(&self.home, &self.proc)
            .map_err(|_| "No se pudo inspeccionar la identidad de los panes".to_string())?;
        let children = self.children()?;
        let text=self.tmux(&["list-panes","-a","-F","#{session_name}\u{1f}#{pane_id}\u{1f}#{pane_pid}\u{1f}#{pane_current_command}\u{1f}#{pane_current_path}"])?;
        let mut plans = vec![];
        let mut failures = vec![];
        for line in text.lines() {
            let parts = line.split('\u{1f}').collect::<Vec<_>>();
            if parts.len() != 5 {
                return Err("tmux devolvió un inventario inválido".into());
            }
            let (session, id, pid, command, cwd) =
                (parts[0], parts[1], parts[2], parts[3], parts[4]);
            if pending.is_some_and(|rows| !rows.iter().any(|p| p["pane"] == id))
                || SHELLS.contains(&command)
            {
                continue;
            }
            let pid = pid.parse::<i64>().map_err(|e| e.to_string())?;
            let Some(agent) = self.cli(pid, &children)? else {
                continue;
            };
            if !super::policy::explicit(&agent.args[1..]) {
                continue;
            }
            let plan = (|| -> Result<Value> {
                let shell = self.process(pid)?;
                if agent.pid == pid
                    || !shell.args.first().is_some_and(|s| {
                        SHELLS.contains(
                            &Path::new(s)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or(""),
                        )
                    })
                {
                    return Err(
                        "El pane no tiene un shell al que regresar; se conserva sin cerrar".into(),
                    );
                }
                let mut info =
                    Value::Object(inspector.inspect(&PaneRef { id, pid, command }).map_err(
                        |_| {
                            "No se identificó la conversación exacta; no se usará --last"
                                .to_string()
                        },
                    )?);
                if !comandos_core::json::truthy(&info["resume_id"])
                    && let Some(windows) = saved["sessions"][session]["windows"].as_array()
                {
                    for window in windows {
                        if let Some(panes) = window["panes"].as_array()
                            && let Some(old) = panes.iter().find(|p| {
                                p["id"] == id
                                    && p["pid"] == pid
                                    && comandos_core::pomodoro::python_str(&p["start"])
                                        == shell.start
                            })
                        {
                            info = old.clone();
                            break;
                        }
                    }
                }
                let sid = info["resume_id"]
                    .as_str()
                    .filter(|s| release::sid(s))
                    .ok_or("No se identificó la conversación exacta; no se usará --last")?;
                let env = read_regular(
                    &self.proc.join(agent.pid.to_string()).join("environ"),
                    4 * 1024 * 1024,
                )?;
                let account = env
                    .split(|b| *b == 0)
                    .find_map(|p| p.strip_prefix(b"CODEX_HOME="))
                    .map(|b| {
                        String::from_utf8(b.to_vec())
                            .map(PathBuf::from)
                            .map_err(|e| e.to_string())
                    })
                    .transpose()?
                    .unwrap_or_else(|| self.home.join(".codex"));
                let account = release::resolve(&account)?;
                let recovery = pending
                    .and_then(|rows| rows.iter().find(|p| p["pane"] == id))
                    .filter(|p| {
                        p["sid"] == sid
                            && p["home"] == account.to_str().unwrap_or("")
                            && p["releaseRecovery"]["pending"] == true
                    });
                let mut paths = rollouts(&account.join("sessions"), sid, 3)?;
                if paths.is_empty() && recovery.is_some() {
                    paths = rollouts(&account.join("archived_sessions"), sid, 0)?;
                }
                if paths.len() != 1 {
                    return Err(
                        "No hay un transcript único para guardar y comprobar la reanudación".into(),
                    );
                }
                let transcript = paths.remove(0);
                let metadata = first_meta(&transcript)?;
                if metadata["type"] != "session_meta" || metadata["payload"]["id"] != sid {
                    return Err("El transcript no corresponde a la conversación exacta".into());
                }
                let mut flags = inspector.flags(agent.pid);
                let size = fs::metadata(&transcript).map_err(|e| e.to_string())?.len();
                let context =
                    match batch::read_context(&transcript, size.saturating_sub(2_000_000))? {
                        Some(ctx) => ctx,
                        None => batch::read_context(&transcript, 0)?.unwrap_or(json!({})),
                    };
                if let Some(model) = context["model"].as_str().filter(|s| !s.is_empty())
                    && !flags
                        .iter()
                        .any(|s| matches!(s.split('=').next(), Some("-m" | "--model")))
                {
                    flags.extend(["--model".into(), model.into()]);
                }
                let effort = context
                    .get("effort")
                    .filter(|v| comandos_core::json::truthy(v))
                    .or(context.get("reasoning_effort"));
                if let Some(effort) = effort.filter(|v| comandos_core::json::truthy(v))
                    && !flags.iter().any(|s| s.contains("model_reasoning_effort="))
                {
                    flags.extend([
                        "-c".into(),
                        format!(
                            "model_reasoning_effort={}",
                            comandos_core::json::dumps(effort, true, false)?
                        ),
                    ]);
                }
                let binary = PathBuf::from(agent.args.first().ok_or("agente sin ejecutable")?);
                if !binary.is_absolute() {
                    return Err("ejecutable del agente no absoluto; se conserva sin cerrar".into());
                }
                let mut p = json!({"session":session,"pane":id,"panePid":pid,"paneStart":shell.start,"pid":agent.pid,"start":agent.start,"sid":sid,"home":account,"cwd":cwd,"flags":flags,"binary":binary,"transcript":transcript});
                p["command"] = json!(batch::launch_command(&p, "continua")?);
                p["recoveryCommand"] = json!(batch::launch_command(&p, "")?);
                Ok(p)
            })();
            match plan {
                Ok(p) => plans.push(p),
                Err(e) => failures.push(format!("{session} {id}: {e}")),
            }
        }
        Ok((plans, failures))
    }
}
fn first_meta(path: &Path) -> Result<Value> {
    let file = open_regular(path)?;
    let mut bytes = vec![];
    BufReader::new(file.take(262145))
        .read_until(b'\n', &mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 262144 {
        return Err("metadata transcript demasiado grande".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}
fn rollouts(root: &Path, sid: &str, depth: usize) -> Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    let mut out = vec![];
    for e in entries {
        let e = e.map_err(|e| e.to_string())?;
        let kind = e.file_type().map_err(|e| e.to_string())?;
        if depth > 0 {
            if kind.is_dir() {
                out.extend(rollouts(&e.path(), sid, depth - 1)?);
            }
        } else if kind.is_file() {
            let name = e.file_name();
            let name = name.to_str().ok_or("transcript basename no UTF-8")?;
            if name.starts_with("rollout-") && name.ends_with(&format!("{sid}.jsonl")) {
                out.push(e.path());
            }
        }
        if out.len() > 100_000 {
            return Err("inventario de transcripts excede límite".into());
        }
    }
    Ok(out)
}
impl Runtime for Local {
    fn tmux(&mut self, args: &[&str]) -> Result<String> {
        self.execute("tmux", args, Duration::from_secs(10))
    }
    fn process(&mut self, pid: i64) -> Result<Process> {
        if pid <= 1 {
            return Err("pid inválido".into());
        }
        let root = self.proc.join(pid.to_string());
        let stat = String::from_utf8(read_regular(&root.join("stat"), 1024 * 1024)?)
            .map_err(|e| e.to_string())?;
        let fields = stat
            .rsplit_once(')')
            .ok_or("stat inválido")?
            .1
            .split_whitespace()
            .collect::<Vec<_>>();
        let start = fields
            .get(19)
            .ok_or("stat sin fecha de nacimiento")?
            .to_string();
        let state = fields.first().ok_or("stat sin estado")?.to_string();
        let args = read_regular(&root.join("cmdline"), 4 * 1024 * 1024)?
            .split(|b| *b == 0)
            .filter(|b| !b.is_empty())
            .map(|b| String::from_utf8(b.to_vec()).map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>>>()?;
        Ok(Process {
            pid,
            start,
            state,
            args,
        })
    }
    fn sleep(&mut self, d: Duration) {
        let deadline = Instant::now() + d;
        while Instant::now() < deadline && !self.cancel.load(Ordering::SeqCst) {
            std::thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(20)),
            );
        }
    }
    fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    fn stty(&mut self, tty: &str) -> Result<()> {
        self.execute("stty", &["sane", "-F", tty], Duration::from_secs(3))?;
        Ok(())
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
    fn terminate_exact(&mut self, plan: &Value) -> Result<()> {
        if self.proc != Path::new("/proc") {
            return Err("raíces proc de prueba nunca pueden enviar señales reales".into());
        }
        batch::check_pane(self, plan)?;
        if !batch::agent_alive(self, plan)? {
            return Ok(());
        }
        let pid = plan["pid"]
            .as_i64()
            .and_then(|p| i32::try_from(p).ok())
            .and_then(rustix::process::Pid::from_raw)
            .ok_or("pid inválido")?;
        let fd = match rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()) {
            Ok(fd) => fd,
            Err(e) if e == rustix::io::Errno::SRCH => return Ok(()),
            Err(e) => return Err(e.to_string()),
        };
        batch::check_pane(self, plan)?;
        if !batch::agent_alive(self, plan)? {
            return Ok(());
        }
        if self.cancelled() {
            return Err("mantenimiento cancelado".into());
        }
        println!(
            "{}: Codex no respondió; cerrando su proceso exacto…",
            release::string(plan, "pane")?
        );
        rustix::process::pidfd_send_signal(&fd, rustix::process::Signal::TERM)
            .map_err(|e| e.to_string())
    }
}
