mod rss;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("rss") => {
            let sep = args
                .iter()
                .position(|a| a == "--")
                .expect("uso: xtask rss [--samples N] -- <cmd…>");
            let samples: usize = args[1..sep]
                .windows(2)
                .find(|w| w[0] == "--samples")
                .and_then(|w| w[1].parse().ok())
                .unwrap_or(1);
            let cmd = &args[sep + 1..];
            let mut best = u64::MAX;
            let mut last = None;
            for _ in 0..samples {
                let m = rss::measure(cmd, 2).expect("medición");
                best = best.min(m.rss_kib_total);
                last = Some(m);
            }
            let m = last.expect("al menos una muestra");
            let line = serde_json::json!({"cmd": cmd, "rss_kib_total": m.rss_kib_total, "rss_kib_min": best, "procs": m.procs, "startup_ms": m.startup_ms, "date": chrono_free_now()});
            println!("{line}");
            std::fs::create_dir_all("docs/verification").unwrap();
            use std::io::Write;
            writeln!(
                std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open("docs/verification/rss.jsonl")
                    .unwrap(),
                "{line}"
            )
            .unwrap();
        }
        _ => eprintln!("subcomandos: rss"),
    }
}

// Segundos desde epoch, sin dependencias de fecha.
fn chrono_free_now() -> String {
    let s = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    s.to_string()
}
