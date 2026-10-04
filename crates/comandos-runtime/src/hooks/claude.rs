//! `comandos hook claude`: transcripción de `hooks/cc-notify.sh` (el oráculo de
//! `tests/hook_claude_parity.rs`). Mismo estado, misma línea de timeline, mismo
//! evento N1, misma contabilidad de uso, mismo POST a cc-notifyd y mismo sonido.
use super::conf::Conf;
use super::events_jsonl;
use super::input::{
    Input, Place, adapter_input, agent_name, clock, env_bytes, hostname_device, locate, path,
    stdin_input,
};
use super::jq::{J, jq_compact};
use super::notify_http::{self, Desktop, Voice};
use super::state_file::{self, State};
use super::text::{contains_ci, cut, head_c, jq_lossy, mdclean, strip_nl, tr};
use super::transcript::{last_block, question, turn_text};
use super::usage_hook::{self, Identity};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::thread::JoinHandle;

struct Hook {
    home: PathBuf,
    state_dir: PathBuf,
    events: PathBuf,
    conf: Conf,
    input: Input,
    agent_name: Vec<u8>,
    proj: Vec<u8>,
    now: i64,
    now_ms: i64,
    pane: String,
    session: Vec<u8>,
    pane_pid: Vec<u8>,
    state_key: Vec<u8>,
    state_file: PathBuf,
    notice_play: &'static str,
    usage: Vec<JoinHandle<()>>,
}

impl Hook {
    fn identity(&self) -> Identity {
        let s = |b: &[u8]| jq_lossy(b);
        Identity {
            agent: s(&self.input.agent),
            session: s(&self.session),
            pane: self.pane.clone(),
            cwd: s(&self.input.cwd),
            prompt_id: s(&self.input.prompt_id),
            agent_session_id: s(&self.input.agent_session_id),
            now: self.now,
            now_ms: self.now_ms,
        }
    }

    fn lifecycle(&mut self, status: &str, capture: bool) {
        let id = self.identity();
        let capture = capture
            .then(|| usage_hook::capture_payload(&id, status))
            .flatten();
        let life = usage_hook::lifecycle_payload(&id, status);
        self.usage
            .push(usage_hook::spawn(self.home.clone(), capture, life));
    }

    /// `write_state`: estado atómico, línea de timeline y contabilidad de uso.
    fn write_state(&mut self, status: &str, detail: &[u8], options: &[u8], last: &[u8]) {
        let state = State {
            project: &self.proj,
            status,
            detail,
            cwd: &self.input.cwd,
            ts: self.now,
            options,
            last,
            agent: &self.input.agent,
            session: &self.session,
            pane: &self.pane,
        };
        if !state_file::write(&self.state_dir, &self.state_key, &self.state_file, &state) {
            return;
        }
        let (project, detail) = (jq_lossy(&self.proj), jq_lossy(&cut(detail, 280)));
        let line = jq_compact(&[
            ("project", J::S(&project)),
            ("status", J::S(status)),
            ("detail", J::S(&detail)),
            ("ts", J::I(self.now)),
        ]);
        events_jsonl::append(&self.events, &line);
        self.lifecycle(status, true);
    }

    /// `event_v2`: evento N1 en SQLite y reclamo del sonido para esta máquina.
    fn event_v2(&mut self, title: &[u8], excerpt: &[u8]) {
        let i = &self.input;
        let hook = if i.hook_event_arg.is_empty() {
            &i.event
        } else {
            &i.hook_event_arg
        };
        let session: &[u8] = if self.pane.is_empty() {
            b""
        } else {
            &self.session
        };
        let s = |b: &[u8]| Value::String(jq_lossy(b));
        let payload = json!({
            "hookEvent": s(hook), "agent": s(&i.agent), "cwd": s(&i.cwd), "project": s(&self.proj),
            "session": s(session), "pane": self.pane, "panePid": s(&self.pane_pid),
            "conversationId": s(&i.agent_session_id), "turnId": s(&i.turn_id), "promptId": s(&i.prompt_id),
            "requestId": s(&i.request_id), "notificationType": s(&i.notification_type),
            "title": s(title), "excerpt": s(&cut(excerpt, 500)), "occurredAtMs": self.now_ms,
        });
        self.notice_play = match crate::events_cli::record_event(&payload, Some(&hostname_device()))
        {
            Ok(true) => "play",
            Ok(false) => "",
            Err(_) => "legacy",
        };
    }

    fn finish(self) -> i32 {
        for handle in self.usage {
            let _ = handle.join();
        }
        0
    }
}

pub fn run(args: &[String]) -> i32 {
    if std::env::var_os("COMANDOS_SILENT_AGENT").is_some_and(|v| v == "1") {
        return 0;
    }
    let home = path(&env_bytes("HOME"));
    let hooks_dir = home.join(".claude/hooks");
    let state_dir = hooks_dir.join("state");
    let _ = std::fs::create_dir_all(&state_dir);
    let conf = Conf::load(&hooks_dir, &env_bytes("LANG"));
    let adapter = matches!(
        args.first().map(String::as_str),
        Some("--agent" | "--event")
    );
    let Some(input) = (if adapter {
        Some(adapter_input(args))
    } else {
        stdin_input()
    }) else {
        return 0;
    };
    let place = locate(&input);
    let (now, now_ms) = clock();
    let Place {
        proj,
        pane,
        session,
        pane_pid,
        state_key,
    } = place;
    let mut file_name = state_key.clone();
    file_name.extend_from_slice(b".json");
    let mut hook = Hook {
        state_file: state_dir.join(OsStr::from_bytes(&file_name)),
        home,
        events: hooks_dir.join("events.jsonl"),
        state_dir,
        conf,
        agent_name: agent_name(&input.agent),
        input,
        proj,
        now,
        now_ms,
        pane,
        session,
        pane_pid,
        state_key,
        notice_play: "legacy",
        usage: Vec::new(),
    };
    dispatch(&mut hook);
    hook.finish()
}

fn joined(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

/// El `case "$event"` del bash y todo lo que sigue.
fn dispatch(h: &mut Hook) {
    let lang = h.conf.lang;
    let event = h.input.event.clone();
    let name = h.agent_name.clone();
    let last = env_bytes("LAST");
    let transcript = path(&h.input.transcript);
    let has_transcript = !h.input.transcript.is_empty() && transcript.is_file();
    let (title, body, full, options, sound, kind);
    match event.as_slice() {
        b"UserPromptSubmit" => {
            let last = state_file::previous_answer(&h.state_file);
            h.write_state("working", b"", b"", &last);
            h.event_v2(&name, b"");
            return;
        }
        b"SessionEnd" => {
            h.event_v2(&name, b"");
            h.lifecycle("end", false);
            let _ = std::fs::remove_file(&h.state_file);
            let mut grok = b".".to_vec();
            grok.extend_from_slice(&h.state_key);
            grok.extend_from_slice(b".grok");
            let _ = std::fs::remove_file(h.state_dir.join(OsStr::from_bytes(&grok)));
            return;
        }
        b"Notification" => {
            let (prev_s, prev_t) = state_file::previous_status(&h.state_file);
            title = joined(&[
                "🟡 [".as_bytes(),
                &h.proj,
                b"] ",
                &name,
                b" ",
                lang.attn.as_bytes(),
            ]);
            let mut b = if h.input.msg.is_empty() {
                lang.wait_body.as_bytes().to_vec()
            } else {
                h.input.msg.clone()
            };
            let mut opts = h.input.options_arg.clone();
            let mut f = h.input.full_arg.clone();
            if has_transcript {
                match last_block(&transcript).as_ref().and_then(question) {
                    Some(q) => {
                        let qtext = cut(&q.full, 260);
                        opts = q.options;
                        if !qtext.is_empty() {
                            b = qtext;
                        }
                        f = q.full.clone();
                        if !q.list.is_empty() {
                            f = joined(&[&q.full, b"\n\n", lang.opts.as_bytes(), b"\n", &q.list]);
                        }
                    }
                    None => {
                        let raw = turn_text(&transcript);
                        let preview = head_c(&tr(&raw, b'\n', b' '), 260).to_vec();
                        if !preview.is_empty() {
                            b = joined(&[&b, b"\n", &preview]);
                        }
                        if !raw.is_empty() {
                            f = cut(&raw, 60000);
                        }
                    }
                }
            }
            if opts.is_empty() && contains_ci(&h.input.msg, b"permi") {
                opts = lang.yes_always.as_bytes().to_vec();
            }
            let detail = cut(if f.is_empty() { &b } else { &f }, 60000);
            h.write_state("waiting", &detail, &opts, &last);
            h.event_v2(&joined(&[&name, b" ", lang.attn.as_bytes()]), &b);
            // Anti-spam: ya estaba esperando hace menos de 10 minutos.
            let prev_t = if prev_t.is_empty() {
                Some(0)
            } else {
                prev_t.parse::<i64>().ok()
            };
            if prev_s == "waiting" && prev_t.is_some_and(|t| h.now - t < 600) {
                return;
            }
            (body, full, options, sound, kind) =
                (b, f, opts, h.conf.sound_attention.clone(), "waiting");
        }
        _ => {
            title = joined(&[
                "✅ [".as_bytes(),
                &h.proj,
                b"] ",
                &name,
                b" ",
                lang.done.as_bytes(),
            ]);
            let mut f = h.input.full_arg.clone();
            let mut preview = if f.is_empty() {
                Vec::new()
            } else {
                head_c(&tr(&f, b'\n', b' '), 180).to_vec()
            };
            if has_transcript {
                let raw = turn_text(&transcript);
                preview = head_c(&tr(&raw, b'\n', b' '), 180).to_vec();
                if !raw.is_empty() {
                    f = cut(&raw, 60000);
                }
            }
            let b = if preview.is_empty() {
                lang.done_body.as_bytes().to_vec()
            } else {
                preview
            };
            let detail = cut(if f.is_empty() { &b } else { &f }, 60000);
            h.write_state("done", &detail, b"", &last);
            h.event_v2(&joined(&[&name, b" ", lang.done.as_bytes()]), &b);
            (body, full, options, sound, kind) =
                (b, f, Vec::new(), h.conf.sound_done.clone(), "done");
        }
    }
    let body = mdclean(&body);
    if (kind == "done" && !h.conf.notify_on_done)
        || (kind == "waiting" && !h.conf.notify_on_attention)
    {
        return;
    }
    notify_http::spawn(
        h.conf.volume,
        voice(h, kind, sound),
        desktop(h, &title, &body, kind, &options, &full),
    );
}

/// `notify_voice` decidido aquí; lo ejecuta el proceso de entrega.
fn voice(h: &Hook, kind: &str, sound: Vec<u8>) -> Option<Voice> {
    if !matches!(h.notice_play, "play" | "legacy") {
        return None;
    }
    let lang = &h.conf.lang;
    let speak = match kind {
        "waiting" if h.conf.speak_attention => {
            Some(joined(&[&h.proj, b" ", lang.speak_wait.as_bytes()]))
        }
        "done" if h.conf.speak_done => Some(joined(&[&h.proj, b" ", lang.speak_done.as_bytes()])),
        _ => None,
    };
    if speak.is_none() && !h.conf.sound_enabled {
        return None;
    }
    let mut model = b".local/share/piper-voices/".to_vec();
    model.extend_from_slice(&h.conf.piper_voice);
    model.extend_from_slice(b".onnx");
    Some(Voice {
        sound: speak.is_none().then_some(sound),
        speak,
        piper_model: h.home.join(OsStr::from_bytes(&model)),
        spd_lang: lang.spd_lang,
    })
}

/// `notify_desktop`: el cuerpo de `jq -cn` para cc-notifyd y el respaldo de macOS.
fn desktop(
    h: &Hook,
    title: &[u8],
    body: &[u8],
    kind: &str,
    options: &[u8],
    full: &[u8],
) -> Option<Desktop> {
    if !h.conf.desktop_notify {
        return None;
    }
    let texts = [title, body, &h.session, &h.proj, options, full].map(jq_lossy);
    let mut fields = vec![
        ("title", J::S(&texts[0])),
        ("body", J::S(&texts[1])),
        ("session", J::S(&texts[2])),
        ("kind", J::S(kind)),
        ("project", J::S(&texts[3])),
        ("options", J::S(&texts[4])),
        ("full", J::S(&texts[5])),
    ];
    if !h.pane.is_empty() {
        fields.push(("pane", J::S(&h.pane)));
    }
    let osa_script = joined(&[
        b"display notification \"",
        strip_nl(&tr(head_c(body, 200), b'"', b'\'')),
        b"\" with title \"",
        strip_nl(&tr(title, b'"', b'\'')),
        b"\"",
    ]);
    Some(Desktop {
        payload: jq_compact(&fields),
        osa_script,
    })
}
