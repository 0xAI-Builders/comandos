use std::{
    fs,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct Measure {
    pub rss_kib_total: u64,
    pub procs: usize,
    pub startup_ms: u128,
}

/// Raíz + descendientes, leyendo ppid de `/proc/*/stat`.
/// Un proceso que desaparece durante el recorrido simplemente se omite.
fn children(root: u32) -> Vec<u32> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let parent = out[i];
        for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Ok(stat) = fs::read_to_string(format!("/proc/{pid}/stat")) else {
                continue;
            };
            // campo 4 = ppid, después del último ')'
            let tail = stat.rsplit(')').next().unwrap_or("");
            if tail.split_whitespace().nth(1) == Some(&parent.to_string()) && !out.contains(&pid) {
                out.push(pid);
            }
        }
        i += 1;
    }
    out
}

fn rss_kib(pid: u32) -> u64 {
    fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

pub fn measure(cmd: &[String], settle_secs: u64) -> Result<Measure, String> {
    let start = Instant::now();
    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", cmd[0]))?;
    thread::sleep(Duration::from_secs(settle_secs));
    let startup_ms = start.elapsed().as_millis();
    let pids = children(child.id());
    let rss_kib_total = pids.iter().map(|p| rss_kib(*p)).sum();
    let procs = pids.len();
    for pid in pids.iter().rev() {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(*pid as i32),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
    thread::sleep(Duration::from_secs(2));
    for pid in pids.iter().rev() {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(*pid as i32),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    let _ = child.wait();
    Ok(Measure {
        rss_kib_total,
        procs,
        startup_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measures_sleep_process_tree() {
        let m = measure(&["sh".into(), "-c".into(), "sleep 3 & sleep 3".into()], 1).unwrap();
        assert!(m.procs >= 2, "debe contar hijos: {m:?}");
        assert!(m.rss_kib_total > 0);
    }
}
