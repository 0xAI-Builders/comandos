use std::path::Path;

const USAGE: &str =
    "uso: cargo run -p xtask -- web-bench audio-diff --base URL [--out FILE]";

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .filter(|v| !v.starts_with("--"))
}

fn usage() -> i32 {
    eprintln!("{USAGE}");
    2
}

pub fn main(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("audio-diff") => audio_diff(args),
        _ => usage(),
    }
}

fn audio_diff(args: &[String]) -> i32 {
    let Some(base) = flag(args, "--base") else {
        return usage();
    };
    let out = flag(args, "--out").unwrap_or("docs/verification/fase3/web-ui-sounds.json");
    let cues = [
        "open",
        "close",
        "complete",
        "success",
        "level-up",
        "notification",
        "error",
        "warning",
    ];
    let rows: Vec<serde_json::Value> = cues
        .iter()
        .map(|cue| {
            serde_json::json!({
                "cue": cue,
                "max_abs_diff": null,
                "same_duration": null,
                "status": "requires chrome-bg fixture evaluation"
            })
        })
        .collect();
    let value = serde_json::json!({
        "base": base,
        "status": "deferred",
        "reason": "audio-diff needs chrome-bg/macmini runtime; local browser execution is forbidden in this worktree",
        "cues": rows
    });
    if let Some(parent) = Path::new(out).parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("error: {}: {e}", parent.display());
        return 1;
    }
    match serde_json::to_string_pretty(&value)
        .map(|s| s + "\n")
        .and_then(|s| std::fs::write(out, s).map_err(serde_json::Error::io))
    {
        Ok(()) => {
            println!("audio-diff deferred; wrote {out}");
            0
        }
        Err(e) => {
            eprintln!("error: {out}: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::flag;

    #[test]
    fn flag_rejects_missing_values() {
        let args = ["audio-diff", "--base", "--out", "x"]
            .iter()
            .map(|s| (*s).to_string())
            .collect::<Vec<_>>();
        assert_eq!(flag(&args, "--base"), None);
        assert_eq!(flag(&args, "--out"), Some("x"));
    }
}
