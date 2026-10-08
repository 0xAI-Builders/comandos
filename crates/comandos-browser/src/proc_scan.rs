use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn process_identity(proc_root: &Path, pid: i32) -> Option<(u64, i32)> {
    let raw = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    let tail = raw.get(raw.rfind(')')? + 2..)?;
    let fields = tail.split_whitespace().collect::<Vec<_>>();
    let ppid = fields.get(1)?.parse().ok()?;
    let start = fields.get(19)?.parse().ok()?;
    Some((start, ppid))
}

pub fn owned_processes(proc_root: &Path, root_pid: i32, profile: &Path) -> BTreeMap<i32, u64> {
    let mut processes = BTreeMap::new();
    let mut selected = BTreeSet::from([root_pid]);
    let needle = format!("--user-data-dir={}", profile.display());
    let Ok(entries) = std::fs::read_dir(proc_root) else {
        return BTreeMap::new();
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        let Some(identity) = process_identity(proc_root, pid) else {
            continue;
        };
        processes.insert(pid, identity);
        if std::fs::read(entry.path().join("cmdline"))
            .map(|raw| {
                raw.split(|byte| *byte == 0)
                    .any(|arg| arg == needle.as_bytes())
            })
            .unwrap_or(false)
        {
            selected.insert(pid);
        }
    }
    loop {
        let children = processes
            .iter()
            .filter_map(|(pid, (_, ppid))| selected.contains(ppid).then_some(*pid))
            .collect::<BTreeSet<_>>();
        if children.is_subset(&selected) {
            break;
        }
        selected.extend(children);
    }
    selected
        .into_iter()
        .filter_map(|pid| processes.get(&pid).map(|(start, _)| (pid, *start)))
        .collect()
}

pub fn signal_owned(proc_root: &Path, owned: &BTreeMap<i32, u64>, sig: Signal) {
    for (pid, generation) in owned {
        if process_identity(proc_root, *pid)
            .map(|(current, _)| current == *generation)
            .unwrap_or(false)
        {
            let _ = kill(Pid::from_raw(*pid), sig);
        }
    }
}
