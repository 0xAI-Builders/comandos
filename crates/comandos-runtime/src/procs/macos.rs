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
    fn fill_cwds(&self, mut rows: Vec<ProcInfo>) -> Vec<ProcInfo> {
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
        self.fill_cwds(self.snapshot())
    }
    fn with_cwds(&self, rows: Vec<ProcInfo>) -> Vec<ProcInfo> {
        self.fill_cwds(rows)
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
    fn with_cwds(&self, rows: Vec<ProcInfo>) -> Vec<ProcInfo> {
        PsSource::new(NativeTools::default()).with_cwds(rows)
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
        String::from_utf8(self.output(name, args, timeout, None, None)?.stdout).ok()
    }
}
impl NativeTools {
    // Preserve status for authority checks, while Tools::run retains the
    // original partial-stdout contract even for a nonzero exit.
    fn output(
        &self,
        name: &str,
        args: &[&str],
        timeout: Duration,
        command_mode: Option<&str>,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> Option<std::process::Output> {
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
        let mut command = Command::new(path);
        command
            .args(args)
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0);
        if let Some(mode) = command_mode {
            command.env("COMMAND_MODE", mode);
        }
        if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Acquire)) {
            return None;
        }
        let mut owned = Owned(Some(command.spawn().ok()?));
        let mut pipe = owned.0.as_mut()?.stdout.take()?;
        let flags = OFlag::from_bits_truncate(fcntl(&pipe, FcntlArg::F_GETFL).ok()?);
        fcntl(&pipe, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)).ok()?;
        let begin = Instant::now();
        let mut bytes = Vec::new();
        let mut eof = false;
        loop {
            // Check every read, including successful/Interrupted retries, so
            // continuous output cannot starve the caller's deadline.
            if begin.elapsed() >= timeout
                || cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Acquire))
            {
                return None;
            }
            if !eof {
                let mut chunk = [0; 8192];
                match pipe.read(&mut chunk) {
                    Ok(0) => eof = true,
                    Ok(n) => {
                        if bytes.len().checked_add(n)? > 4 * 1024 * 1024 {
                            return None;
                        }
                        bytes.extend_from_slice(&chunk[..n]);
                        // Drain ready bytes without a per-chunk sleep. Darwin
                        // pipes can return short reads even while more is ready.
                        continue;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return None,
                }
            }
            if eof && super::child_exited_unreaped(owned.0.as_ref()?).ok()? {
                // Python parses available stdout even when lsof returns 1 for
                // a partial inventory. Reap after cleanup, then keep the text.
                let status = owned.finish()?;
                return Some(std::process::Output {
                    status,
                    stdout: bytes,
                    stderr: Vec::new(),
                });
            }
            if begin.elapsed() >= timeout {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Darwin's killpg can return EPERM for a group containing only zombies.
/// Prove that case for an exited, unreaped child which owns its group; never
/// infer permission to signal an arbitrary PID from a ps observation.
#[cfg(target_os = "macos")]
pub fn exited_child_group_is_zombie_only(
    child: &std::process::Child,
    timeout: Duration,
    cancel: &std::sync::atomic::AtomicBool,
) -> std::io::Result<bool> {
    inspect_exited_group(child, &NativeTools::default(), timeout, cancel)
}

#[cfg(any(target_os = "macos", test))]
fn inspect_exited_group(
    child: &std::process::Child,
    tools: &NativeTools,
    timeout: Duration,
    cancel: &std::sync::atomic::AtomicBool,
) -> std::io::Result<bool> {
    use std::{io, sync::atomic::Ordering, time::Instant};
    let begin = Instant::now();
    if cancel.load(Ordering::Acquire) || !super::child_exited_unreaped(child)? {
        return Ok(false);
    }
    let group = i32::try_from(child.id())
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid child pid"))?;
    // The caller established process_group(0) at spawn. Darwin getpgid
    // excludes zombies, so verify the exact leader/PGID in the ps result.
    if begin.elapsed() >= timeout || cancel.load(Ordering::Acquire) {
        return Ok(false);
    }
    let group_arg = group.to_string();
    let Some(output) = tools.output(
        "ps",
        &["-g", &group_arg, "-o", "pid=,pgid=,stat="],
        timeout.saturating_sub(begin.elapsed()),
        Some("unix2003"),
        Some(cancel),
    ) else {
        return Ok(false);
    };
    if !output.status.success()
        || begin.elapsed() >= timeout
        || cancel.load(Ordering::Acquire)
        || !super::child_exited_unreaped(child)?
    {
        return Ok(false);
    }
    Ok(std::str::from_utf8(&output.stdout).is_ok_and(|raw| group_rows_all_zombie(raw, group)))
}

#[cfg(any(target_os = "macos", test))]
fn group_rows_all_zombie(raw: &str, group: i32) -> bool {
    if group <= 0 || !raw.ends_with('\n') {
        return false;
    }
    let mut seen = std::collections::HashSet::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let [pid, pgid, state] = fields.as_slice() else {
            return false;
        };
        let Ok(pid) = pid.parse::<i32>() else {
            return false;
        };
        // Flags emitted by Apple's ps/print.c after the primary Z state.
        let mut flags = state.chars().peekable();
        if flags.next() != Some('Z') {
            return false;
        }
        for slot in ["<N", "X", "V", "L", "s", "+"] {
            if flags.peek().is_some_and(|flag| slot.contains(*flag)) {
                flags.next();
            }
        }
        if pid <= 0
            || pgid.parse::<i32>() != Ok(group)
            || !seen.insert(pid)
            || flags.next().is_some()
        {
            return false;
        }
    }
    seen.contains(&group)
}
#[cfg(test)]
mod group_tests {
    use super::{NativeTools, Tools, group_rows_all_zombie, inspect_exited_group};
    use std::{
        fs,
        os::unix::{fs::PermissionsExt, process::CommandExt},
        path::PathBuf,
        process::{Child, Command, Stdio},
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        time::{Duration, Instant},
    };
    fn shell_path(path: &std::path::Path) -> String {
        format!("'{}'", path.to_str().unwrap().replace('\'', "'\"'\"'"))
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "own-group-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            Self(path)
        }
        fn tools(&self, body: &str) -> NativeTools {
            let path = self.0.join("ps-fake");
            fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            NativeTools {
                ps: path.clone(),
                lsof: path,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    struct Owned(Child);
    impl Owned {
        fn exited() -> Self {
            let child = Command::new("/usr/bin/true")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()
                .unwrap();
            let owned = Self(child);
            let end = Instant::now() + Duration::from_secs(2);
            while !super::super::child_exited_unreaped(&owned.0).unwrap() {
                assert!(Instant::now() < end);
                std::thread::sleep(Duration::from_millis(2));
            }
            owned
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            if super::super::child_exited_unreaped(&self.0).is_ok()
                && let Ok(pid) = i32::try_from(self.0.id())
            {
                let _ = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    #[test]
    fn zombie_group_requires_complete_exact_leader_and_all_rows() {
        for raw in ["812 812 Z\n", "812 812 Zs\n813 812 Z+\n"] {
            assert!(group_rows_all_zombie(raw, 812), "{raw:?}");
        }
        for raw in [
            "",
            "\n",
            "812 812 Z",
            "813 812 Z\n",
            "812 813 Z\n",
            "812 812 R\n",
            "812 812 Z\n813 812 S\n",
            "812 812 Z\n813 813 Z\n",
            "812 812 Zombie\n",
            "812 812 Z+++\n",
            "812 812 ZZ\n",
            "812 812 Z<N\n",
            "812 812 Z extra\n",
            "812 812 Z\n812 812 Z\n",
            "0 812 Z\n",
            "-1 812 Z\n",
            "x 812 Z\n",
            "812 812 Z\npartial\n",
        ] {
            assert!(!group_rows_all_zombie(raw, 812), "{raw:?}");
        }
        assert!(!group_rows_all_zombie("812 812 Z\n", -1));
    }

    #[test]
    fn owned_group_query_is_directed_strict_and_leader_remains_unreaped() {
        let f = Fixture::new();
        let child = Owned::exited();
        let tools = f.tools("[ \"$#\" -eq 4 ] && [ \"$1\" = -g ] && [ \"$3\" = -o ] && [ \"$4\" = 'pid=,pgid=,stat=' ]\n[ \"${COMMAND_MODE-}\" = unix2003 ]\nprintf '%s %s Z\\n' \"$2\" \"$2\"");
        let cancel = AtomicBool::new(false);
        let proof =
            inspect_exited_group(&child.0, &tools, Duration::from_secs(2), &cancel).unwrap();
        assert!(
            proof,
            "directed proof refused; diagnostic capture: {:?}",
            tools.output(
                "ps",
                &["-g", &child.0.id().to_string(), "-o", "pid=,pgid=,stat="],
                Duration::from_secs(2),
                Some("unix2003"),
                Some(&cancel)
            )
        );
        assert!(super::super::child_exited_unreaped(&child.0).unwrap());
        for body in [
            "printf '%s %s Z\\n' \"$2\" \"$2\"; exit 1",
            "printf '%s %s Z\\n999 %s S\\n' \"$2\" \"$2\" \"$2\"",
            "printf '%s %s Z\\npartial\\n' \"$2\" \"$2\"",
            "printf '%s %s Z' \"$2\" \"$2\"",
            "true",
            "printf '\\377\\n'",
        ] {
            let tools = f.tools(body);
            assert!(
                !inspect_exited_group(&child.0, &tools, Duration::from_secs(2), &cancel).unwrap(),
                "{body}"
            );
        }
    }

    #[test]
    fn owned_query_cancellation_timeout_and_cap_never_prove_an_empty_group() {
        let f = Fixture::new();
        let child = Owned::exited();
        let cancel = AtomicBool::new(true);
        let marker = f.0.join("unexpected-query");
        let tools = f.tools(&format!("printf query > {}", shell_path(&marker)));
        assert!(!inspect_exited_group(&child.0, &tools, Duration::from_secs(2), &cancel).unwrap());
        assert!(!marker.exists());
        cancel.store(false, Ordering::Release);
        assert!(!inspect_exited_group(&child.0, &tools, Duration::ZERO, &cancel).unwrap());
        assert!(!marker.exists());
        let tools = f.tools(&format!("printf query > {}; sleep 30", shell_path(&marker)));
        let begin = Instant::now();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let end = Instant::now() + Duration::from_secs(2);
                while !marker.exists() && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(2));
                }
                cancel.store(true, Ordering::Release);
            });
            assert!(
                !inspect_exited_group(&child.0, &tools, Duration::from_secs(5), &cancel).unwrap()
            );
        });
        assert!(marker.exists());
        assert!(begin.elapsed() < Duration::from_secs(3));
        cancel.store(false, Ordering::Release);
        let tools = f.tools("printf '%s %s Z\\n' \"$2\" \"$2\"; sleep 30");
        let begin = Instant::now();
        assert!(
            !inspect_exited_group(&child.0, &tools, Duration::from_millis(40), &cancel).unwrap()
        );
        assert!(begin.elapsed() < Duration::from_secs(2));
        let tools = f.tools(&format!(
            "while :; do printf '%s' '{}'; done",
            "x".repeat(65536)
        ));
        let begin = Instant::now();
        assert!(!inspect_exited_group(&child.0, &tools, Duration::from_secs(5), &cancel).unwrap());
        assert!(begin.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn public_tools_still_return_partial_nonzero_stdout_with_status_retained() {
        let f = Fixture::new();
        let tools = f.tools("printf 'partial\\n'; exit 1");
        let output = tools
            .output("ps", &[], Duration::from_secs(2), None, None)
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"partial\n");
        assert_eq!(
            tools.run("ps", &[], Duration::from_secs(2)).as_deref(),
            Some("partial\n")
        );
    }

    #[test]
    fn alive_reaped_and_nonleader_children_never_authorize_cleanup() {
        let f = Fixture::new();
        let marker = f.0.join("unexpected-query");
        let tools = f.tools(&format!("printf query > {}", shell_path(&marker)));
        let cancel = AtomicBool::new(false);
        let alive = Owned(
            Command::new("/bin/sleep")
                .arg("30")
                .process_group(0)
                .spawn()
                .unwrap(),
        );
        assert!(!inspect_exited_group(&alive.0, &tools, Duration::from_secs(2), &cancel).unwrap());
        let mut exited = Owned::exited();
        exited.0.wait().unwrap();
        assert!(inspect_exited_group(&exited.0, &tools, Duration::from_secs(2), &cancel).is_err());
        assert!(!marker.exists());
        let mut inherited = Command::new("/usr/bin/true").spawn().unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        while !super::super::child_exited_unreaped(&inherited).unwrap() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(2));
        }
        let inherited_pid = inherited.id();
        let actual_group = nix::unistd::getpgrp().as_raw();
        let tools = f.tools(&format!("[ \"$#\" -eq 4 ] && [ \"$1\" = -g ] && [ \"$2\" = '{inherited_pid}' ] && [ \"$3\" = -o ] && [ \"$4\" = 'pid=,pgid=,stat=' ]\nprintf '{inherited_pid} {actual_group} Z\\n'"));
        let proof = inspect_exited_group(&inherited, &tools, Duration::from_secs(2), &cancel);
        inherited.wait().unwrap();
        assert!(!proof.unwrap());
        assert!(!marker.exists());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn actual_darwin_ps_proves_a_zombie_leader_and_rejects_a_live_owned_descendant() {
        let cancel = AtomicBool::new(false);
        let child = Owned::exited();
        assert!(
            super::exited_child_group_is_zombie_only(&child.0, Duration::from_secs(2), &cancel)
                .unwrap()
        );
        let f = Fixture::new();
        let leaf = f.0.join("leaf-ready");
        let descendant = format!("printf ready > {}; exec sleep 30", shell_path(&leaf));
        let quoted = format!("'{}'", descendant.replace('\'', "'\"'\"'"));
        let tools = f.tools(&format!("/bin/sh -c {quoted} </dev/null >/dev/null 2>&1 &\nwhile [ ! -f {} ]; do sleep .002; done",shell_path(&leaf)));
        let owned = Owned(Command::new(&tools.ps).process_group(0).spawn().unwrap());
        let end = Instant::now() + Duration::from_secs(2);
        while !super::super::child_exited_unreaped(&owned.0).unwrap() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(leaf.exists());
        assert!(
            !super::exited_child_group_is_zombie_only(&owned.0, Duration::from_secs(2), &cancel)
                .unwrap()
        );
    }
}
