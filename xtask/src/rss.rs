use std::{
    collections::HashMap,
    fs,
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    thread,
    time::Duration,
};

use nix::{sys::signal::Signal, unistd::Pid};

#[derive(Debug)]
pub struct Measure {
    pub rss_kib_total: u64,
    pub pss_kib_total: u64,
    /// false si algún pid no tenía `smaps_rollup` legible y se usó su VmRSS.
    pub pss_exact: bool,
    pub procs: usize,
    /// Pids medidos (raíz + descendientes), útil para comprobar que murieron.
    #[allow(dead_code)] // solo lo lee el test que comprueba que el árbol murió
    pub pids: Vec<u32>,
}

/// Lee `/proc/<pid>/stat`: (estado, ppid). Tolera nombres con paréntesis/espacios.
fn stat_state_ppid(pid: u32) -> Option<(char, u32)> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let tail = stat.rsplit(')').next()?;
    let mut it = tail.split_whitespace();
    let state = it.next()?.chars().next()?;
    let ppid = it.next()?.parse().ok()?;
    Some((state, ppid))
}

/// Raíz + descendientes: una sola pasada por `/proc` (mapa ppid -> hijos) y luego se recorre.
/// Un proceso que desaparece durante la pasada simplemente se omite.
fn process_tree(root: u32) -> Vec<u32> {
    let mut by_parent: HashMap<u32, Vec<u32>> = HashMap::new();
    for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if let Some((_, ppid)) = stat_state_ppid(pid) {
            by_parent.entry(ppid).or_default().push(pid);
        }
    }
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        if let Some(kids) = by_parent.get(&out[i]) {
            for k in kids {
                if !out.contains(k) {
                    out.push(*k);
                }
            }
        }
        i += 1;
    }
    out
}

fn status_rss_kib(pid: u32) -> Option<u64> {
    let s = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    s.lines()
        .find(|l| l.starts_with("VmRSS:"))
        .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
}

fn rollup_pss_kib(pid: u32) -> Option<u64> {
    let s = fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    s.lines()
        .find(|l| l.starts_with("Pss:"))
        .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
}

/// Vivo = existe en /proc y no es zombi.
pub fn alive(pid: u32) -> bool {
    matches!(stat_state_ppid(pid), Some((s, _)) if s != 'Z' && s != 'X')
}

fn tree_alive(child: &mut Child, pids: &[u32]) -> bool {
    let _ = child.try_wait(); // reaps al raíz para que no quede zombi
    pids.iter().any(|p| alive(*p))
}

/// SIGTERM al grupo, espera hasta 2 s (pasos de 100 ms) y luego SIGKILL.
fn kill_tree(child: &mut Child, pids: &[u32]) {
    let pgid = Pid::from_raw(child.id() as i32);
    let _ = nix::sys::signal::killpg(pgid, Signal::SIGTERM);
    for _ in 0..20 {
        if !tree_alive(child, pids) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    if tree_alive(child, pids) {
        let _ = nix::sys::signal::killpg(pgid, Signal::SIGKILL);
        // procesos que salieron del grupo (setsid) se matan uno a uno
        for pid in pids {
            let _ = nix::sys::signal::kill(Pid::from_raw(*pid as i32), Signal::SIGKILL);
        }
    }
    let _ = child.wait();
}

/// Raíz + descendientes ya vivos: (pids, RSS total, Pss total, Pss exacto).
fn totals(root: u32) -> (Vec<u32>, u64, u64, bool) {
    let pids = process_tree(root);
    let mut rss_kib_total = 0;
    let mut pss_kib_total = 0;
    let mut pss_exact = true;
    for pid in &pids {
        let rss = status_rss_kib(*pid).unwrap_or(0);
        rss_kib_total += rss;
        match rollup_pss_kib(*pid) {
            Some(p) => pss_kib_total += p,
            None => {
                pss_exact = false;
                pss_kib_total += rss;
            }
        }
    }
    (pids, rss_kib_total, pss_kib_total, pss_exact)
}

/// Mide un proceso ya existente (y sus descendientes) sin lanzarlo ni matarlo.
#[allow(dead_code)] // lo usa `poll`; el test de calendario no lo enlaza
pub fn sample(pid: u32) -> Result<Measure, String> {
    if !alive(pid) {
        return Err(format!("el pid {pid} no está vivo"));
    }
    let (pids, rss_kib_total, pss_kib_total, pss_exact) = totals(pid);
    Ok(Measure {
        rss_kib_total,
        pss_kib_total,
        pss_exact,
        procs: pids.len(),
        pids,
    })
}

pub fn measure(cmd: &[String], settle_secs: u64) -> Result<Measure, String> {
    let program = cmd.first().ok_or("comando vacío")?;
    let mut child = Command::new(program)
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    thread::sleep(Duration::from_secs(settle_secs));
    // Si la raíz ya murió no hay nada que medir: nunca devolvemos una medición vacía.
    if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
        let _ = nix::sys::signal::killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
        return Err(format!(
            "el proceso terminó antes de medir (estado {status:?})"
        ));
    }
    let (pids, rss_kib_total, pss_kib_total, pss_exact) = totals(child.id());
    kill_tree(&mut child, &pids);
    Ok(Measure {
        rss_kib_total,
        pss_kib_total,
        pss_exact,
        procs: pids.len(),
        pids,
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
        assert!(m.pss_kib_total > 0);
        assert!(
            m.pss_kib_total <= m.rss_kib_total,
            "Pss no puede superar RSS: {m:?}"
        );
        for pid in &m.pids {
            assert!(!alive(*pid), "el pid {pid} sigue vivo tras medir");
        }
    }

    #[test]
    fn missing_binary_is_an_error() {
        assert!(measure(&["/nonexistent-binary".into()], 0).is_err());
    }

    #[test]
    fn early_exit_is_an_error() {
        let e = measure(&["true".into()], 1).unwrap_err();
        assert!(e.contains("terminó antes de medir"), "{e}");
    }
}
