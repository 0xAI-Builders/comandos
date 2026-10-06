use crate::config::RunMode;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct RestorePlan {
    pub actions: Vec<RestoreAction>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RestoreAction {
    MountWeb {
        key: String,
        label: String,
    },
    AttachExisting {
        key: String,
        label: String,
    },
    CreatePlaceholder {
        key: String,
        label: String,
        snapshot: Value,
    },
    RestoreLayout {
        key: String,
        snapshot: Value,
    },
    ResumeExact {
        key: String,
        snapshot: Value,
    },
    ShowAmbiguity {
        key: String,
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    Malformed,
}

impl RestorePlan {
    pub fn build(
        tabs: &Value,
        snapshots: &Value,
        present: &BTreeSet<String>,
        mode: RunMode,
    ) -> Result<Self, RestoreError> {
        let object = tabs.as_object().ok_or(RestoreError::Malformed)?;
        let mut actions = Vec::new();
        for (key, label) in object {
            let label = label.as_str().ok_or(RestoreError::Malformed)?.to_string();
            if key.starts_with("xterm-") || key.starts_with("web:") {
                actions.push(RestoreAction::MountWeb {
                    key: key.clone(),
                    label,
                });
                continue;
            }
            if present.contains(key) {
                actions.push(RestoreAction::AttachExisting {
                    key: key.clone(),
                    label,
                });
                continue;
            }
            if mode == RunMode::Shadow {
                actions.push(RestoreAction::ShowAmbiguity {
                    key: key.clone(),
                    label,
                });
                continue;
            }
            let snapshot = snapshots.get(key).cloned().unwrap_or(Value::Null);
            actions.push(RestoreAction::CreatePlaceholder {
                key: key.clone(),
                label,
                snapshot: snapshot.clone(),
            });
            if snapshot.get("windows").is_some() {
                actions.push(RestoreAction::RestoreLayout {
                    key: key.clone(),
                    snapshot: snapshot.clone(),
                });
            }
            if snapshot.get("windows").is_some()
                || snapshot.get("resume_id").and_then(Value::as_str).is_some()
                || snapshot
                    .pointer("/acp/sessionId")
                    .and_then(Value::as_str)
                    .is_some()
            {
                actions.push(RestoreAction::ResumeExact {
                    key: key.clone(),
                    snapshot,
                });
            }
        }
        Ok(Self { actions })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreResult {
    Complete,
    Partial,
    Failed,
}

#[derive(Default)]
pub struct RestoreCoordinator {
    restoring: bool,
    result: Option<RestoreResult>,
}

impl RestoreCoordinator {
    pub fn begin(&mut self) {
        self.restoring = true;
        self.result = None;
    }

    pub fn ready(&self) -> bool {
        !self.restoring
    }

    pub fn finish(&mut self, result: RestoreResult) {
        self.result = Some(result);
        self.restoring = false;
    }

    pub fn result(&self) -> Option<RestoreResult> {
        self.result
    }
}

/// The executor uses this interface so tests never launch agents or a personal server.
pub trait RestoreTmux {
    type Ownership;
    fn mode(&self) -> RunMode;
    fn read(&self, args: &[&str]) -> Result<String, String>;
    fn mutate(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<String, String>;
    fn create(&self, args: &[&str]) -> Result<(String, Self::Ownership), String>;
    fn validate(&self, _ownership: &Self::Ownership) -> Result<(), String> {
        Ok(())
    }
    fn cleanup(&self, ownership: Self::Ownership) -> Result<(), String>;
}

impl RestoreTmux for crate::tmux::TmuxCtl {
    fn validate(&self, token: &Self::Ownership) -> Result<(), String> {
        self.check_owned_session(token)
            .map_err(|e| format!("{e:?}"))
    }
    type Ownership = crate::tmux::OwnedSession;
    fn mode(&self) -> RunMode {
        self.mode()
    }
    fn read(&self, args: &[&str]) -> Result<String, String> {
        checked(self.read(args).map_err(|e| format!("{e:?}"))?)
    }
    fn mutate(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<String, String> {
        let out = match stdin {
            Some(bytes) => self.mutate_with_stdin(args, bytes),
            None => self.mutate(args),
        }
        .map_err(|e| format!("{e:?}"))?;
        checked(out)
    }
    fn create(&self, args: &[&str]) -> Result<(String, Self::Ownership), String> {
        let (out, owned) = self
            .new_placeholder_session(args)
            .map_err(|e| format!("{e:?}"))?;
        Ok((
            checked(out)?,
            owned.ok_or("placeholder has no ownership identity")?,
        ))
    }
    fn cleanup(&self, ownership: Self::Ownership) -> Result<(), String> {
        checked(
            self.kill_owned_session(ownership)
                .map_err(|e| format!("{e:?}"))?,
        )
        .map(|_| ())
    }
}

fn checked(out: crate::tmux::TmuxOut) -> Result<String, String> {
    if out.ok() {
        Ok(out.stdout.trim_end_matches('\n').to_string())
    } else {
        Err(out.stderr.trim().to_string())
    }
}

/// An explicit command runs inside its tmux pane, in a separate owned scope.
/// Preparing argv does not start a process on the GTK thread.
pub trait ScopeLauncher {
    fn resume(&self, saved: &Value, _home: &std::path::Path) -> Option<String> {
        crate::resume::exact_resume_command(saved)
    }
    fn argv(&self, pane: &str, command: &str) -> Result<Vec<String>, String>;
}

pub struct PaneScopes;
impl ScopeLauncher for PaneScopes {
    fn resume(&self, saved: &Value, home: &std::path::Path) -> Option<String> {
        crate::resume::verified_resume_command(saved, home)
    }
    fn argv(&self, pane: &str, command: &str) -> Result<Vec<String>, String> {
        let mut random = [0u8; 12];
        getrandom::fill(&mut random).map_err(|e| e.to_string())?;
        let suffix = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        Ok(vec![
            "systemd-run".into(),
            "--user".into(),
            "--scope".into(),
            "--collect".into(),
            "--quiet".into(),
            format!(
                "--unit=comandos-pane-{}-{suffix}",
                pane.trim_start_matches('%')
            ),
            "env".into(),
            "-u".into(),
            "NO_COLOR".into(),
            "/bin/sh".into(),
            "-lc".into(),
            command.into(),
        ])
    }
}

/// Private fixtures have no systemd or real agents. They resume a shell command directly.
pub struct FixtureScopes;
impl ScopeLauncher for FixtureScopes {
    fn argv(&self, _pane: &str, command: &str) -> Result<Vec<String>, String> {
        Ok(vec!["/bin/sh".into(), "-lc".into(), command.into()])
    }
}

pub struct CancelableTmux {
    pub tmux: crate::tmux::TmuxCtl,
    pub cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl CancelableTmux {
    fn check(&self) -> Result<(), String> {
        if self.cancelled.load(std::sync::atomic::Ordering::Acquire) {
            Err("restore cancelled".into())
        } else {
            Ok(())
        }
    }
}
impl RestoreTmux for CancelableTmux {
    fn validate(&self, token: &Self::Ownership) -> Result<(), String> {
        self.check()?;
        RestoreTmux::validate(&self.tmux, token)
    }
    type Ownership = crate::tmux::OwnedSession;
    fn mode(&self) -> RunMode {
        self.tmux.mode()
    }
    fn read(&self, args: &[&str]) -> Result<String, String> {
        self.check()?;
        RestoreTmux::read(&self.tmux, args)
    }
    fn mutate(&self, args: &[&str], stdin: Option<&[u8]>) -> Result<String, String> {
        self.check()?;
        RestoreTmux::mutate(&self.tmux, args, stdin)
    }
    fn create(&self, args: &[&str]) -> Result<(String, Self::Ownership), String> {
        self.check()?;
        RestoreTmux::create(&self.tmux, args)
    }
    fn cleanup(&self, token: Self::Ownership) -> Result<(), String> {
        RestoreTmux::cleanup(&self.tmux, token)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestoredTab {
    pub key: String,
    pub label: String,
    pub attached: bool,
    pub pane_mapping: BTreeMap<String, String>,
    pub ambiguity: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestoreExecution {
    pub tabs: Vec<RestoredTab>,
    pub result: RestoreResult,
}

pub fn execute<T: RestoreTmux, S: ScopeLauncher + ?Sized>(
    plan: &RestorePlan,
    tmux: &T,
    scopes: &S,
    home: &std::path::Path,
) -> RestoreExecution {
    let mut tabs = Vec::new();
    for action in &plan.actions {
        if let RestoreAction::MountWeb { key, label } = action {
            tabs.push(RestoredTab {
                key: key.clone(),
                label: label.clone(),
                attached: false,
                pane_mapping: BTreeMap::new(),
                ambiguity: vec![],
                error: None,
            });
            continue;
        }
        let (key, label, missing) = match action {
            RestoreAction::AttachExisting { key, label } => (key, label, false),
            RestoreAction::CreatePlaceholder { key, label, .. } => (key, label, true),
            RestoreAction::ShowAmbiguity { key, label } => (key, label, true),
            _ => continue,
        };
        let mut tab = RestoredTab {
            key: key.clone(),
            label: label.clone(),
            attached: false,
            pane_mapping: BTreeMap::new(),
            ambiguity: Vec::new(),
            error: None,
        };
        // Race with another client creating a session must never trigger a resume.
        let present = tmux
            .read(&["has-session", "-t", &format!("={key}")])
            .is_ok();
        if present && (!missing || tmux.mode() != RunMode::Shadow) {
            tab.attached = true;
        } else if tmux.mode() == RunMode::Shadow {
            tab.ambiguity
                .push("Select an existing session to attach read-only".into());
        } else {
            let snapshot = plan.actions.iter().find_map(|a| match a {
                RestoreAction::CreatePlaceholder {
                    key: k, snapshot, ..
                }
                | RestoreAction::RestoreLayout { key: k, snapshot }
                | RestoreAction::ResumeExact { key: k, snapshot }
                    if k == key =>
                {
                    Some(snapshot)
                }
                _ => None,
            });
            match restore_missing(tmux, scopes, key, snapshot, home) {
                Ok((mapping, ambiguity)) => {
                    tab.attached = true;
                    tab.pane_mapping = mapping;
                    tab.ambiguity = ambiguity;
                }
                Err(error) => tab.error = Some(error),
            }
        }
        tabs.push(tab);
    }
    let errors = tabs.iter().filter(|t| t.error.is_some()).count();
    let result = if errors == 0 {
        RestoreResult::Complete
    } else if errors == tabs.len() {
        RestoreResult::Failed
    } else {
        RestoreResult::Partial
    };
    RestoreExecution { tabs, result }
}

fn cwd<'a>(pane: &'a Value, home: &'a std::path::Path) -> String {
    pane.get("cwd")
        .and_then(Value::as_str)
        .filter(|p| std::path::Path::new(p).is_dir())
        .map(ToString::to_string)
        .unwrap_or_else(|| home.to_string_lossy().into_owned())
}

pub fn remap_layout(layout: &str, mapping: &BTreeMap<String, String>) -> Result<String, String> {
    let (_, body) = layout.split_once(',').ok_or("layout has no checksum")?;
    let leaf =
        regex::Regex::new(r"(\d+x\d+,\d+,\d+),(\d+)([,}\]]|$)").map_err(|e| e.to_string())?;
    let mut error = None;
    let body = leaf.replace_all(body, |caps: &regex::Captures<'_>| {
        let old = format!("%{}", caps.get(2).map_or("", |m| m.as_str()));
        let Some(new) = mapping.get(&old) else {
            error = Some(format!("unmapped pane {old}"));
            return caps.get(0).map_or("", |m| m.as_str()).to_string();
        };
        if !new
            .strip_prefix('%')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            error = Some("invalid new pane identity".into());
        }
        format!(
            "{},{new}{}",
            caps.get(1).map_or("", |m| m.as_str()),
            caps.get(3).map_or("", |m| m.as_str())
        )
        .replace(",%", ",")
    });
    if let Some(error) = error {
        return Err(error);
    }
    Ok(format!(
        "{:04x},{body}",
        comandos_core::workspace::snapshot::layout_checksum(&body)
    ))
}

fn restore_missing<T: RestoreTmux, S: ScopeLauncher + ?Sized>(
    tmux: &T,
    scopes: &S,
    key: &str,
    snapshot: Option<&Value>,
    home: &std::path::Path,
) -> Result<(BTreeMap<String, String>, Vec<String>), String> {
    use comandos_core::workspace::snapshot::{Snapshot, check_snapshot};
    let fallback = serde_json::json!({"windows": []});
    let snapshot = snapshot.unwrap_or(&fallback);
    let windows = snapshot.get("windows").and_then(Value::as_array);
    if windows.is_some()
        && check_snapshot(&serde_json::json!({"version":2,"sessions":{key:snapshot}}))
            != Snapshot::Valid
        && snapshot != &fallback
    {
        return Err("Refusing incomplete or invalid layout snapshot".into());
    }
    let mut owned = None;
    let mut started = false;
    let mut mapping = BTreeMap::new();
    let mut ambiguity = Vec::new();
    let result = (|| {
        let fallback_window = serde_json::json!({"index":0,"name":key,"width":120,"height":32,
            "active":true,"panes":[snapshot]});
        let windows = windows
            .filter(|w| !w.is_empty())
            .cloned()
            .unwrap_or_else(|| vec![fallback_window]);
        let mut starts = Vec::new();
        let mut active_window = None;
        for (index, window) in windows.iter().enumerate() {
            let panes = window
                .get("panes")
                .and_then(Value::as_array)
                .ok_or("no panes")?;
            let first_saved = panes.first().ok_or("empty panes")?;
            let width = window
                .get("width")
                .unwrap_or(&Value::Null)
                .as_u64()
                .ok_or("invalid width")?
                .max(4 * panes.len() as u64)
                .to_string();
            let height = window
                .get("height")
                .unwrap_or(&Value::Null)
                .as_u64()
                .ok_or("invalid height")?
                .to_string();
            let name = window
                .get("name")
                .unwrap_or(&Value::Null)
                .as_str()
                .ok_or("invalid window name")?;
            let target = format!("={key}:{}", window.get("index").unwrap_or(&Value::Null));
            let first_cwd = cwd(first_saved, home);
            let first = if index == 0 {
                let (pane, token) = tmux.create(&[
                    "new-session",
                    "-d",
                    "-P",
                    "-F",
                    "#{pane_id}",
                    "-s",
                    key,
                    "-n",
                    name,
                    "-x",
                    &width,
                    "-y",
                    &height,
                    "-c",
                    &first_cwd,
                    "sleep",
                    "2147483647",
                ])?;
                owned = Some(token);
                pane
            } else {
                mutate_owned(
                    tmux,
                    &owned,
                    &[
                        "new-window",
                        "-d",
                        "-P",
                        "-F",
                        "#{pane_id}",
                        "-t",
                        &target,
                        "-n",
                        name,
                        "-c",
                        &first_cwd,
                        "sleep",
                        "2147483647",
                    ],
                    None,
                )?
            };
            let wid = tmux.read(&["display-message", "-p", "-t", &first, "#{window_id}"])?;
            if index == 0
                && tmux.read(&["display-message", "-p", "-t", &first, "#{window_index}"])?
                    != window
                        .get("index")
                        .and_then(Value::as_i64)
                        .ok_or("invalid window index")?
                        .to_string()
            {
                mutate_owned(
                    tmux,
                    &owned,
                    &["move-window", "-s", &wid, "-t", &target],
                    None,
                )?;
            }
            mutate_owned(
                tmux,
                &owned,
                &["resize-window", "-t", &wid, "-x", &width, "-y", &height],
                None,
            )?;
            let mut last = first;
            for (pindex, pane) in panes.iter().enumerate() {
                let pane_cwd = cwd(pane, home);
                let new = if pindex == 0 {
                    last.clone()
                } else {
                    mutate_owned(
                        tmux,
                        &owned,
                        &[
                            "split-window",
                            "-d",
                            "-h",
                            "-l",
                            "1",
                            "-P",
                            "-F",
                            "#{pane_id}",
                            "-t",
                            &last,
                            "-c",
                            &pane_cwd,
                            "sleep",
                            "2147483647",
                        ],
                        None,
                    )?
                };
                if let Some(old) = pane.get("id").unwrap_or(&Value::Null).as_str() {
                    mapping.insert(old.into(), new.clone());
                }
                if let Some(pane_key) = pane.get("key").unwrap_or(&Value::Null).as_str() {
                    mutate_owned(
                        tmux,
                        &owned,
                        &[
                            "set-option",
                            "-p",
                            "-t",
                            &new,
                            "@comandos-pane-key",
                            pane_key,
                        ],
                        None,
                    )?;
                }
                starts.push((new.clone(), pane.clone()));
                last = new;
                if pindex > 0 {
                    mutate_owned(
                        tmux,
                        &owned,
                        &["select-layout", "-t", &wid, "even-horizontal"],
                        None,
                    )?;
                }
            }
            if let Some(layout) = window.get("layout").unwrap_or(&Value::Null).as_str() {
                mutate_owned(
                    tmux,
                    &owned,
                    &[
                        "select-layout",
                        "-t",
                        &wid,
                        &remap_layout(layout, &mapping)?,
                    ],
                    None,
                )?;
            }
            let width = window.get("width").unwrap_or(&Value::Null).to_string();
            mutate_owned(
                tmux,
                &owned,
                &["resize-window", "-t", &wid, "-x", &width, "-y", &height],
                None,
            )?;
            for (option, field, default) in [
                ("pane-border-status", "border_status", "off"),
                ("window-size", "window_size", "latest"),
                ("automatic-rename", "automatic_rename", "off"),
            ] {
                mutate_owned(
                    tmux,
                    &owned,
                    &[
                        "set-option",
                        "-w",
                        "-t",
                        &wid,
                        option,
                        window[field].as_str().unwrap_or(default),
                    ],
                    None,
                )?;
            }
            if let Some(pane) = panes.iter().find(|p| p["active"].as_bool() == Some(true)) {
                let selected = mapping
                    .get(
                        pane.get("id")
                            .unwrap_or(&Value::Null)
                            .as_str()
                            .unwrap_or(""),
                    )
                    .ok_or("active pane unmapped")?;
                mutate_owned(tmux, &owned, &["select-pane", "-t", selected], None)?;
                if window.get("zoomed").unwrap_or(&Value::Null).as_bool() == Some(true) {
                    mutate_owned(tmux, &owned, &["resize-pane", "-Z", "-t", selected], None)?;
                }
            }
            if window.get("active").unwrap_or(&Value::Null).as_bool() == Some(true) {
                active_window = Some(wid);
            }
        }
        if let Some(wid) = active_window {
            mutate_owned(tmux, &owned, &["select-window", "-t", &wid], None)?;
        }
        // All geometry is installed before any exact conversation resumes.
        for (pane, saved) in starts {
            mutate_owned(
                tmux,
                &owned,
                &[
                    "respawn-pane",
                    "-k",
                    "-t",
                    &pane,
                    "-c",
                    &cwd(&saved, home),
                    "/bin/sh",
                    "-il",
                ],
                None,
            )?;
            if let Some(command) = scopes.resume(&saved, home) {
                let argv = scopes.argv(&pane, &command)?;
                let command = argv
                    .iter()
                    .map(|a| crate::resume::shell_quote(a))
                    .collect::<Vec<_>>()
                    .join(" ");
                let buffer = format!("comandos-restore-{}", pane.trim_start_matches('%'));
                mutate_owned(
                    tmux,
                    &owned,
                    &["load-buffer", "-b", &buffer, "-"],
                    Some(command.as_bytes()),
                )?;
                let paste = mutate_owned(
                    tmux,
                    &owned,
                    &["paste-buffer", "-d", "-b", &buffer, "-t", &pane],
                    None,
                );
                if paste.is_err() {
                    let _ = mutate_owned(tmux, &owned, &["delete-buffer", "-b", &buffer], None);
                }
                paste?;
                // Once Enter might have been delivered, preserve the session even on error.
                started = true;
                mutate_owned(tmux, &owned, &["send-keys", "-t", &pane, "Enter"], None)?;
            } else if saved
                .get("agent")
                .unwrap_or(&Value::Null)
                .as_str()
                .is_some_and(|a| !a.is_empty())
            {
                ambiguity.push(format!(
                    "{pane}: exact conversation unavailable; shell retained"
                ));
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        if !started && let Some(token) = owned {
            let _ = tmux.cleanup(token);
        }
        return Err(error);
    }
    Ok((mapping, ambiguity))
}

fn mutate_owned<T: RestoreTmux>(
    tmux: &T,
    owned: &Option<T::Ownership>,
    args: &[&str],
    stdin: Option<&[u8]>,
) -> Result<String, String> {
    if let Some(token) = owned {
        tmux.validate(token)?;
    }
    tmux.mutate(args, stdin)
}
