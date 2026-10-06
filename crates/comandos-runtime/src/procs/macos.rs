//! Darwin ps/lsof adapter. Argument boundaries and process environments are
//! unavailable; command text is whitespace-split, never shell-evaluated.
use super::{ProcInfo, ProcSource};
use std::{collections::HashMap, path::PathBuf, time::Duration};

pub fn parse_ps_line(line: &str) -> Option<ProcInfo> {
    let mut words = line.split_whitespace();
    let pid: i32 = words.next()?.parse().ok()?;
    let ppid: i32 = words.next()?.parse().ok()?;
    if pid <= 0 || ppid < 0 {
        return None;
    }
    let weekday = words.next()?;
    if !["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].contains(&weekday) {
        return None;
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == words.clone().next().unwrap_or(""))?;
    words.next()?;
    let day = words.next()?.parse().ok()?;
    let clock: Vec<_> = words.next()?.split(':').collect();
    let [h, m, s] = clock.as_slice() else {
        return None;
    };
    let year = words.next()?.parse().ok()?;
    let time = chrono::NaiveDate::from_ymd_opt(year, u32::try_from(month).ok()? + 1, day)?
        .and_hms_opt(h.parse().ok()?, m.parse().ok()?, s.parse().ok()?)?;
    let start = u64::try_from(time.and_utc().timestamp()).ok()?;
    let argv: Vec<_> = words.map(str::to_owned).collect();
    if argv.is_empty() {
        return None;
    }
    Some(ProcInfo {
        pid,
        ppid,
        start,
        argv,
        cwd: None,
    })
}

/// Injectable argv transport; production implementations must bound time/output.
pub trait Tools {
    fn run(&self, name: &str, args: &[&str], timeout: Duration) -> Option<String>;
}
pub struct PsSource<T> {
    tools: T,
}
impl<T: Tools> PsSource<T> {
    pub fn new(tools: T) -> Self {
        Self { tools }
    }
    pub fn tools(&self) -> &T {
        &self.tools
    }
    fn rows(&self, args: &[&str]) -> Vec<ProcInfo> {
        let Some(text) = self.tools.run("ps", args, Duration::from_secs(5)) else {
            return Vec::new();
        };
        text.lines().filter_map(parse_ps_line).collect()
    }
    fn with_cwds(&self, mut rows: Vec<ProcInfo>) -> Vec<ProcInfo> {
        if rows.is_empty() {
            return rows;
        }
        let pids = rows
            .iter()
            .map(|p| p.pid.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if let Some(cwds) = self.tools.run(
            "lsof",
            &["-a", "-d", "cwd", "-p", &pids, "-Fn"],
            Duration::from_secs(2),
        ) {
            let mut current = None;
            let mut paths = HashMap::new();
            for line in cwds.lines() {
                if let Some(p) = line.strip_prefix('p') {
                    current = p.parse::<i32>().ok();
                } else if let (Some(pid), Some(n)) = (current, line.strip_prefix('n')) {
                    let path = PathBuf::from(n);
                    if path.is_absolute() {
                        paths.insert(pid, path);
                    }
                }
            }
            for row in &mut rows {
                row.cwd = paths.remove(&row.pid);
            }
        }
        rows
    }
}
impl<T: Tools> ProcSource for PsSource<T> {
    fn snapshot(&self) -> Vec<ProcInfo> {
        self.rows(&["-axww", "-o", "pid=,ppid=,lstart=,command="])
    }
    fn snapshot_with_cwd(&self) -> Vec<ProcInfo> {
        self.with_cwds(self.snapshot())
    }
    fn process(&self, pid: i32) -> Option<ProcInfo> {
        if pid <= 0 {
            return None;
        }
        // Identity/ancestry lookups need no cwd. Avoid an lsof subprocess for
        // each parent/start check; cwd is optional and filled by snapshot().
        self.tools
            .run(
                "ps",
                &[
                    "-ww",
                    "-p",
                    &pid.to_string(),
                    "-o",
                    "pid=,ppid=,lstart=,command=",
                ],
                Duration::from_secs(5),
            )?
            .lines()
            .filter_map(parse_ps_line)
            .find(|p| p.pid == pid)
    }
    fn open_files(&self, pid: i32) -> Vec<PathBuf> {
        if pid <= 0 {
            return Vec::new();
        }
        self.tools
            .run(
                "lsof",
                &["-Fn", "-p", &pid.to_string()],
                Duration::from_secs(2),
            )
            .map(|s| {
                s.lines()
                    .filter_map(|l| l.strip_prefix('n'))
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .collect()
            })
            .unwrap_or_default()
    }
}
pub struct PsSnapshot;
impl ProcSource for PsSnapshot {
    fn snapshot(&self) -> Vec<ProcInfo> {
        PsSource::new(NativeTools::default()).snapshot()
    }
    fn snapshot_with_cwd(&self) -> Vec<ProcInfo> {
        PsSource::new(NativeTools::default()).snapshot_with_cwd()
    }
    fn process(&self, pid: i32) -> Option<ProcInfo> {
        PsSource::new(NativeTools::default()).process(pid)
    }
    fn open_files(&self, pid: i32) -> Vec<PathBuf> {
        PsSource::new(NativeTools::default()).open_files(pid)
    }
}
pub struct NativeTools {
    pub ps: PathBuf,
    pub lsof: PathBuf,
}
impl Default for NativeTools {
    fn default() -> Self {
        Self {
            ps: "/bin/ps".into(),
            lsof: "/usr/sbin/lsof".into(),
        }
    }
}
impl Tools for NativeTools {
    fn run(&self, name: &str, args: &[&str], timeout: Duration) -> Option<String> {
        use nix::{
            fcntl::{FcntlArg, OFlag, fcntl},
            sys::signal::{Signal, killpg},
            unistd::Pid,
        };
        use std::{
            io::Read,
            os::unix::process::CommandExt,
            process::{Child, Command, Stdio},
            time::Instant,
        };
        struct Owned(Option<Child>);
        impl Owned {
            fn finish(&mut self) -> Option<std::process::ExitStatus> {
                let mut c = self.0.take()?;
                if let Ok(pid) = i32::try_from(c.id()) {
                    let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
                }
                c.wait().ok()
            }
        }
        impl Drop for Owned {
            fn drop(&mut self) {
                let _ = self.finish();
            }
        }
        let path = match name {
            "ps" => &self.ps,
            "lsof" => &self.lsof,
            _ => return None,
        };
        let mut owned = Owned(Some(
            Command::new(path)
                .args(args)
                .env("LC_ALL", "C")
                .env("TZ", "UTC")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .ok()?,
        ));
        let mut pipe = owned.0.as_mut()?.stdout.take()?;
        let flags = OFlag::from_bits_truncate(fcntl(&pipe, FcntlArg::F_GETFL).ok()?);
        fcntl(&pipe, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)).ok()?;
        let begin = Instant::now();
        let mut bytes = Vec::new();
        let mut eof = false;
        loop {
            if !eof {
                let mut chunk = [0; 8192];
                match pipe.read(&mut chunk) {
                    Ok(0) => eof = true,
                    Ok(n) => {
                        if bytes.len().checked_add(n)? > 4 * 1024 * 1024 {
                            return None;
                        }
                        bytes.extend_from_slice(&chunk[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return None,
                }
            }
            if eof && super::child_exited_unreaped(owned.0.as_ref()?).ok()? {
                // Python parses available stdout even when lsof returns 1 for
                // a partial inventory. Reap after cleanup, then keep the text.
                owned.finish()?;
                return String::from_utf8(bytes).ok();
            }
            if begin.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
