//! Recover Markdown labels stripped by tmux < 3.4. Read-only, bounded, on demand.
use super::{
    Answer, Native,
    light::{data, error, read_reply},
};
use crate::Request;
use comandos_runtime::{
    pane_snapshot::{PaneInspector, PaneRef},
    pane_typing::TmuxResult,
    session_configuration, terminal_history,
};
use http::StatusCode;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
};
use tokio::{runtime::Handle, sync::Semaphore};
static GATE: Semaphore = Semaphore::const_new(1);
const MAX_TAIL: u64 = 8 * 1024 * 1024;

fn resolve_records(records: &str, before: &str, after: &str) -> Option<String> {
    let display = format!("{before}{after}");
    let point = before.len();
    let mut matches = BTreeSet::new();
    for row in records.lines().rev() {
        let Ok(record) = serde_json::from_str::<Value>(row) else {
            continue;
        };
        let message = if record["type"] == "response_item" {
            &record["payload"]
        } else {
            &record["message"]
        };
        if message["role"] != "assistant" {
            continue;
        }
        let Some(content) = message["content"].as_array() else {
            continue;
        };
        for part in content {
            if !matches!(part["type"].as_str(), Some("text" | "output_text")) {
                continue;
            }
            let Some(text) = part["text"].as_str() else {
                continue;
            };
            let mut link: Option<(String, String)> = None;
            for event in Parser::new(text) {
                match event {
                    Event::Start(Tag::Link { dest_url, .. }) => {
                        link = Some((dest_url.to_string(), String::new()))
                    }
                    Event::Text(t) | Event::Code(t) => {
                        if let Some((_, label)) = &mut link {
                            label.push_str(&t)
                        }
                    }
                    Event::SoftBreak | Event::HardBreak => {
                        if let Some((_, label)) = &mut link {
                            label.push(' ')
                        }
                    }
                    Event::End(TagEnd::Link) => {
                        let Some((url, label)) = link.take() else {
                            continue;
                        };
                        if label.is_empty() || url.len() > 8192 || url.chars().any(char::is_control)
                        {
                            continue;
                        }
                        let Ok(parsed) = url::Url::parse(&url) else {
                            continue;
                        };
                        if !matches!(parsed.scheme(), "http" | "https") {
                            continue;
                        }
                        for (start, _) in display.match_indices(&label) {
                            let end = start + label.len();
                            let boundary = display[..start]
                                .chars()
                                .next_back()
                                .is_none_or(|c| !c.is_alphanumeric())
                                && display[end..]
                                    .chars()
                                    .next()
                                    .is_none_or(|c| !c.is_alphanumeric());
                            if start <= point && point < end && boundary {
                                matches.insert(url.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if matches.len() > 1 {
            return None;
        }
    }
    if matches.len() == 1 {
        matches.into_iter().next()
    } else {
        None
    }
}

pub(super) async fn answer(native: &Native, request: &Request) -> Answer {
    let body = data(request)?;
    let (Some(before), Some(after)) = (
        body.get("before").and_then(Value::as_str),
        body.get("after").and_then(Value::as_str),
    ) else {
        return error(StatusCode::BAD_REQUEST, "Texto del enlace inválido");
    };
    if before.len() + after.len() > 8192 || after.is_empty() {
        return error(StatusCode::BAD_REQUEST, "Texto del enlace inválido");
    }
    let (before, after) = (before.to_owned(), after.to_owned());
    let mut selection = Value::Object(body.clone());
    // The caller cannot supply a path, transcript, harness, or another pane ID.
    selection.as_object_mut().unwrap().remove("pane");
    selection["lines"] = json!(1);
    if selection["col"].as_u64().is_none() || selection["row"].as_u64().is_none() {
        return error(StatusCode::BAD_REQUEST, "Posición del enlace inválida");
    }
    let home = native.options().home.clone();
    let proc_root = native.options().proc_root.clone();
    let tmux = native.options().tmux.clone();
    let handle = Handle::current();
    let Ok(_permit) = GATE.try_acquire() else {
        return error(
            StatusCode::TOO_MANY_REQUESTS,
            "Consulta de enlace ocupada; vuelve a tocar",
        );
    };
    let result = tokio::task::spawn_blocking(move || -> Option<String> {
        let run = |args: &[&str]| {
            tmux.run_blocking(&handle, args)
                .map(|r| TmuxResult {
                    returncode: if r.ok { 0 } else { 1 },
                    stdout: r.stdout,
                    stderr: r.stderr,
                })
                .unwrap_or(TmuxResult {
                    returncode: 1,
                    ..Default::default()
                })
        };
        let capture = terminal_history::capture(run, &selection, home.to_str()?).ok()?;
        let identity = run(&[
            "display-message",
            "-p",
            "-t",
            &capture.pane,
            "#{pane_pid}\t#{pane_current_command}",
        ]);
        if identity.returncode != 0 {
            return None;
        }
        let (pid, command) = identity.stdout.trim_end().split_once('\t')?;
        let inspector = PaneInspector::new(&home, &proc_root).ok()?;
        let snapshot = inspector
            .inspect(&PaneRef {
                id: &capture.pane,
                pid: pid.parse().ok()?,
                command,
            })
            .ok()?;
        if !matches!(
            snapshot.get("agent").and_then(Value::as_str),
            Some("codex" | "claude")
        ) {
            return None;
        }
        let path = session_configuration::snapshot_transcript_at(&home, &snapshot).ok()?;
        let mut file = File::open(path).ok()?;
        let meta = file.metadata().ok()?;
        if !meta.is_file() {
            return None;
        }
        let start = meta.len().saturating_sub(MAX_TAIL);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::new();
        file.take(MAX_TAIL).read_to_end(&mut bytes).ok()?;
        let slice = if start > 0 {
            &bytes[bytes.iter().position(|b| *b == b'\n')? + 1..]
        } else {
            &bytes
        };
        let records = std::str::from_utf8(slice).ok()?;
        // Ensure the pane did not change owners while reading the transcript.
        let current = run(&[
            "display-message",
            "-p",
            "-t",
            &capture.pane,
            "#{pane_pid}\t#{pane_current_command}",
        ]);
        if current.returncode != 0 || current.stdout != identity.stdout {
            return None;
        }
        resolve_records(records, &before, &after)
    })
    .await
    .ok()
    .flatten();
    read_reply(&json!({"ok":true,"url":result}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn codex(text: &str) -> String {
        json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":text}]}}).to_string()
    }
    #[test]
    fn restores_destination_of_rendered_word() {
        let log = codex("Abre [prototipo](https://example.com/full/path?q=1) para verlo.");
        assert_eq!(
            resolve_records(&log, "Abre pro", "totipo para verlo."),
            Some("https://example.com/full/path?q=1".into())
        );
    }
    #[test]
    fn claude_and_formatted_labels() {
        let log=json!({"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"[**Vista** final](https://example.com/claude)"}]}}).to_string();
        assert_eq!(
            resolve_records(&log, "Vista fi", "nal"),
            Some("https://example.com/claude".into())
        );
    }
    #[test]
    fn ignores_plain_words_unsafe_destinations_and_ambiguous_labels() {
        assert_eq!(
            resolve_records(&codex("[ver](javascript:alert(1))"), "v", "er"),
            None
        );
        assert_eq!(
            resolve_records(&codex("[ver](https://example.com)"), "con", "versar"),
            None
        );
        let log = format!(
            "{}\n{}",
            codex("[ver](https://one.example)"),
            codex("[ver](https://two.example)")
        );
        assert_eq!(resolve_records(&log, "v", "er"), None);
    }
    #[test]
    fn excludes_tool_and_user_text() {
        let log=json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"[ver](https://example.com)"}]}}).to_string();
        assert_eq!(resolve_records(&log, "v", "er"), None);
    }
}
