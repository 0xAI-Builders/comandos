//! Restart the same validated pane/thread; newly appended turn context is the receipt.
use super::{Result, policy, release};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::Duration,
};
pub const SHELLS: &[&str] = &["zsh", "bash", "sh", "fish", "dash", "ksh"];
#[derive(Clone, Debug)]
pub struct Process {
    pub pid: i64,
    pub start: String,
    pub state: String,
    pub args: Vec<String>,
}
pub trait Runtime {
    fn tmux(&mut self, args: &[&str]) -> Result<String>;
    fn process(&mut self, pid: i64) -> Result<Process>;
    fn sleep(&mut self, duration: Duration);
    fn elapsed(&self) -> Duration;
    fn stty(&mut self, tty: &str) -> Result<()>;
    fn terminate_exact(&mut self, plan: &Value) -> Result<()>;
    fn cancelled(&self) -> bool;
}
fn guard(rt: &impl Runtime) -> Result<()> {
    if rt.cancelled() {
        Err("mantenimiento cancelado".into())
    } else {
        Ok(())
    }
}
pub fn launch_command(plan: &Value, prompt: &str) -> Result<String> {
    let mut args = vec![
        "resume".into(),
        release::string(plan, "sid")?.into(),
        "-C".into(),
        release::string(plan, "cwd")?.into(),
    ];
    args.extend(
        plan["flags"]
            .as_array()
            .ok_or("flags inválidos")?
            .iter()
            .map(|s| {
                s.as_str()
                    .map(str::to_owned)
                    .ok_or("flag no textual".into())
            })
            .collect::<Result<Vec<_>>>()?,
    );
    args.push(policy::YOLO.into());
    if !prompt.is_empty() {
        args.push(prompt.into());
    }
    let mut command = vec![
        "env".into(),
        format!("CODEX_HOME={}", release::string(plan, "home")?),
        release::string(plan, "binary")?.into(),
    ];
    command.extend(policy::normalize(&args)?);
    Ok(command
        .iter()
        .map(|s| policy::quote(s))
        .collect::<Vec<_>>()
        .join(" "))
}
pub fn check_pane(rt: &mut impl Runtime, plan: &Value) -> Result<()> {
    guard(rt)?;
    let pane = release::string(plan, "pane")?;
    if rt.tmux(&["display-message", "-p", "-t", pane, "#{session_name}"])?
        != release::string(plan, "session")?
    {
        return Err("El pane cambió de sesión".into());
    }
    let pid = plan["panePid"]
        .as_i64()
        .filter(|p| *p > 1)
        .ok_or("panePid inválido")?;
    if rt.tmux(&["display-message", "-p", "-t", pane, "#{pane_pid}"])? != pid.to_string() {
        return Err("Cambió el proceso del pane".into());
    }
    if rt.process(pid)?.start != release::string(plan, "paneStart")? {
        return Err("Cambió el shell del pane".into());
    }
    Ok(())
}
pub fn agent_alive(rt: &mut impl Runtime, plan: &Value) -> Result<bool> {
    let Some(pid) = plan["pid"].as_i64() else {
        return Ok(false);
    };
    if pid <= 1 {
        return Err("pid agente inválido".into());
    }
    match rt.process(pid) {
        Ok(p) => Ok(p.start == release::string(plan, "start")? && p.state != "Z"),
        Err(e) if e == "missing" => Ok(false),
        Err(e) => Err(e),
    }
}
pub fn select_retry(
    pending: &[Value],
    fresh: &[Value],
    rt: &mut impl Runtime,
) -> Result<Vec<Value>> {
    let mut selected = vec![];
    for old in pending {
        let pane = release::string(old, "pane")?;
        if let Some(current) = fresh.iter().find(|p| p["pane"] == pane) {
            for k in ["session", "sid", "home", "panePid", "paneStart"] {
                if comandos_core::pomodoro::python_str(&current[k])
                    != comandos_core::pomodoro::python_str(&old[k])
                {
                    return Err(format!(
                        "{pane}: cambió la sesión pendiente; se conserva sin cerrar"
                    ));
                }
            }
            let mut current = current.clone();
            if old["releaseRecovery"]["pending"] == true {
                current["releaseRecovery"] = old["releaseRecovery"].clone();
            }
            selected.push(current);
        } else {
            check_pane(rt, old)?;
            if !SHELLS.contains(
                &rt.tmux(&[
                    "display-message",
                    "-p",
                    "-t",
                    pane,
                    "#{pane_current_command}",
                ])?
                .as_str(),
            ) {
                return Err(format!(
                    "{pane}: cambió el agente; no se reiniciará otra conversación"
                ));
            }
            let mut restored = old.clone();
            restored["pid"] = Value::Null;
            restored["start"] = Value::Null;
            restored["command"] = json!(launch_command(&restored, "continua")?);
            restored["recoveryCommand"] = json!(launch_command(&restored, "")?);
            selected.push(restored);
        }
    }
    Ok(selected)
}
fn wait_stopped(rt: &mut impl Runtime, plan: &Value, seconds: u64) -> Result<bool> {
    let deadline = rt.elapsed() + Duration::from_secs(seconds);
    while rt.elapsed() < deadline {
        guard(rt)?;
        if !agent_alive(rt, plan)? {
            return Ok(true);
        }
        rt.sleep(Duration::from_millis(200));
    }
    agent_alive(rt, plan).map(|alive| !alive)
}
fn agent_keys(rt: &mut impl Runtime, plan: &Value, keys: &[&str]) -> Result<bool> {
    check_pane(rt, plan)?;
    if !agent_alive(rt, plan)? {
        return Ok(false);
    }
    let pane = release::string(plan, "pane")?;
    if SHELLS.contains(
        &rt.tmux(&[
            "display-message",
            "-p",
            "-t",
            pane,
            "#{pane_current_command}",
        ])?
        .as_str(),
    ) {
        return Err(
            "El shell volvió mientras el agente sigue vivo; se conserva sin teclear".into(),
        );
    }
    let mut args = vec!["send-keys", "-t", pane];
    args.extend_from_slice(keys);
    rt.tmux(&args)?;
    Ok(true)
}
pub fn stop_agent(rt: &mut impl Runtime, plan: &Value) -> Result<()> {
    println!("{}: cerrando Codex…", release::string(plan, "pane")?);
    for _ in 0..3 {
        if !agent_keys(rt, plan, &["C-c"])? {
            return Ok(());
        }
        rt.sleep(Duration::from_millis(300));
    }
    if wait_stopped(rt, plan, 3)? {
        return Ok(());
    }
    for keys in [
        &["Escape"][..],
        &["C-u"],
        &["-l", "--", "/exit"],
        &["Enter"],
    ] {
        if !agent_keys(rt, plan, keys)? {
            return Ok(());
        }
        rt.sleep(Duration::from_millis(150));
    }
    if wait_stopped(rt, plan, 5)? {
        return Ok(());
    }
    check_pane(rt, plan)?;
    if !agent_alive(rt, plan)? {
        return Ok(());
    }
    guard(rt)?;
    rt.terminate_exact(plan)?;
    if !wait_stopped(rt, plan, 10)? {
        return Err("Codex no salió tras SIGTERM; no se tecleó otro comando".into());
    }
    Ok(())
}
pub fn full_access(context: &Value) -> bool {
    context["approval_policy"] == "never"
        && context["sandbox_policy"]["type"] == "danger-full-access"
        && (!comandos_core::json::truthy(&context["permission_profile"])
            || context["permission_profile"]["type"] == "disabled")
        && (!comandos_core::json::truthy(&context["file_system_sandbox_policy"])
            || context["file_system_sandbox_policy"]["type"] == "unrestricted")
}
pub fn read_context(path: &Path, offset: u64) -> Result<Option<Value>> {
    let m = path.symlink_metadata().map_err(|e| e.to_string())?;
    if !m.is_file() {
        return Err("transcript no regular".into());
    }
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags((nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_NOCTTY).bits())
        .open(path)
        .map_err(|e| e.to_string())?;
    let o = file.metadata().map_err(|e| e.to_string())?;
    if !o.is_file() || (m.dev(), m.ino()) != (o.dev(), o.ino()) {
        return Err("transcript cambió al abrir".into());
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    let mut last = None;
    let mut total = 0usize;
    loop {
        let mut raw = vec![];
        let n = reader
            .by_ref()
            .take(4 * 1024 * 1024 + 1)
            .read_until(b'\n', &mut raw)
            .map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total = total.saturating_add(n);
        if n > 4 * 1024 * 1024 || total > 64 * 1024 * 1024 {
            return Err("turn_context excede límite de lectura".into());
        }
        if let Ok(v) = serde_json::from_slice::<Value>(&raw)
            && v["type"] == "turn_context"
            && v["payload"].is_object()
        {
            last = Some(v["payload"].clone());
        }
    }
    Ok(last)
}
pub fn restart<R: Runtime>(
    rt: &mut R,
    plan: &mut Value,
    mut release: impl FnMut(&mut Value, &mut dyn FnMut(&Value) -> Result<()>) -> Result<()>,
    checkpoint: &mut dyn FnMut(&Value) -> Result<()>,
) -> Result<Value> {
    check_pane(rt, plan)?;
    if !plan["pid"].is_null() {
        if !agent_alive(rt, plan)? {
            return Err("Cambió el agente".into());
        }
        stop_agent(rt, plan)?;
    }
    let pane = release::string(plan, "pane")?.to_owned();
    let deadline = rt.elapsed() + Duration::from_secs(5);
    while !SHELLS.contains(
        &rt.tmux(&[
            "display-message",
            "-p",
            "-t",
            &pane,
            "#{pane_current_command}",
        ])?
        .as_str(),
    ) {
        guard(rt)?;
        if rt.elapsed() >= deadline {
            return Err(
                "El shell todavía no está disponible; usa el comando de recuperación guardado"
                    .into(),
            );
        }
        rt.sleep(Duration::from_millis(200));
    }
    check_pane(rt, plan)?;
    println!("{pane}: comprobando y liberando el bloqueo de la conversación…");
    release(plan, checkpoint)?;
    check_pane(rt, plan)?;
    let tty = rt.tmux(&["display-message", "-p", "-t", &pane, "#{pane_tty}"])?;
    rt.stty(&tty)?;
    let transcript = release::string(plan, "transcript")?.to_owned();
    let offset = fs::metadata(&transcript).map_err(|e| e.to_string())?.len();
    println!("{pane}: reanudando la misma conversación con continua…");
    let command = launch_command(plan, "continua")?;
    for keys in [&["C-u"][..], &["-l", "--", command.as_str()], &["Enter"]] {
        check_pane(rt, plan)?;
        let mut args = vec!["send-keys", "-t", &pane];
        args.extend_from_slice(keys);
        rt.tmux(&args)?;
    }
    let deadline = rt.elapsed() + Duration::from_secs(60);
    while rt.elapsed() < deadline {
        guard(rt)?;
        if let Some(context) = read_context(Path::new(&transcript), offset)? {
            if !full_access(&context) {
                return Ok(
                    json!({"status":"restricted","error":"El nuevo turno sigue restringido; no se confirmó acceso total"}),
                );
            }
            return Ok(
                json!({"status":"confirmed","conversationId":plan["sid"],"prompt":"continua"}),
            );
        }
        rt.sleep(Duration::from_millis(500));
    }
    Ok(
        json!({"status":"unverified","error":"Se envió el arranque con continua, pero no apareció un nuevo turno verificable","screen":rt.tmux(&["capture-pane","-p","-t",&pane,"-S","-12"])?}),
    )
}
