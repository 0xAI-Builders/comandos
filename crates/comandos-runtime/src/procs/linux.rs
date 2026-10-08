//! Injectable procfs; fake roots use exactly the same reader as Linux.
use super::{ProcInfo, ProcSource};
use std::{fs, path::PathBuf};
pub struct ProcFs {
    root: PathBuf,
}
impl ProcFs {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}
impl ProcSource for ProcFs {
    fn process(&self, pid: i32) -> Option<ProcInfo> {
        if pid <= 0 {
            return None;
        }
        let dir = self.root.join(pid.to_string());
        let stat = fs::read_to_string(dir.join("stat")).ok()?;
        let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
        let ppid = fields.get(1)?.parse().ok()?;
        let start = fields.get(19)?.parse().ok()?;
        let bytes = fs::read(dir.join("cmdline")).ok()?;
        let argv = bytes
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).into_owned())
            .collect();
        Some(ProcInfo {
            pid,
            ppid,
            start,
            argv,
            cwd: fs::read_link(dir.join("cwd")).ok(),
        })
    }
    fn snapshot(&self) -> Vec<ProcInfo> {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name();
                let s = name.to_str()?;
                if !s.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                self.process(s.parse().ok()?)
            })
            .collect()
    }
    fn open_files(&self, pid: i32) -> Vec<PathBuf> {
        if pid <= 0 {
            return Vec::new();
        }
        fs::read_dir(self.root.join(pid.to_string()).join("fd"))
            .map(|d| {
                d.flatten()
                    .filter_map(|e| fs::read_link(e.path()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }
}
