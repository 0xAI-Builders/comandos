#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod support;
use comandos_app::{config::RunMode, ui::extensions::wizard_target_when};
use std::os::unix::fs::DirBuilderExt;

#[test]
fn wizard_resolves_real_active_and_explicit_panes_with_literal_cwd() {
    let f = support::tmux::TestTmux::for_mode(RunMode::Sandbox)
        .expect("private tmux required for wizard regression");
    let root = f.config.sandbox_root().unwrap();
    let first_cwd = root.join("first path | 雪");
    let second_cwd = root.join("second path | literal '");
    for p in [&first_cwd, &second_cwd] {
        std::fs::DirBuilder::new().mode(0o700).create(p).unwrap();
    }
    let first = f.tmux.raw(&[
        "new-session",
        "-d",
        "-s",
        "local",
        "-c",
        first_cwd.to_str().unwrap(),
        "-P",
        "-F",
        "#{pane_id}",
        "/bin/sh",
    ]);
    assert!(first.status.success(), "{first:?}");
    let first = String::from_utf8(first.stdout).unwrap().trim().to_string();
    let second = f.tmux.raw(&[
        "split-window",
        "-h",
        "-t",
        "=local:",
        "-c",
        second_cwd.to_str().unwrap(),
        "-P",
        "-F",
        "#{pane_id}",
        "/bin/sh",
    ]);
    assert!(second.status.success(), "{second:?}");
    let second = String::from_utf8(second.stdout).unwrap().trim().to_string();
    assert_eq!(
        wizard_target_when(&f.ctl, "local", "", || true).unwrap(),
        ("local".into(), second, second_cwd.to_str().unwrap().into())
    );
    assert_eq!(
        wizard_target_when(&f.ctl, "local", &first, || true).unwrap(),
        ("local".into(), first, first_cwd.to_str().unwrap().into())
    );
    assert!(f.ctl.read(&["has-session", "-t", "=local"]).unwrap().ok());
}

#[test]
fn sandbox_attach_preserves_unicode_without_an_inherited_locale() {
    let f = support::tmux::TestTmux::for_mode(RunMode::Sandbox)
        .expect("private tmux required for UTF-8 regression");
    f.tmux.new_session("local", 80, 24);
    let code = r#"import fcntl,os,pty,select,signal,struct,subprocess,sys,termios,time
master,slave=pty.openpty()
fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',24,80,0,0))
env={'PATH':'/usr/bin:/bin','HOME':sys.argv[3],'TERM':'xterm-256color'}
child=subprocess.Popen(['/bin/sh','-c',sys.argv[1]],env=env,stdin=slave,stdout=slave,stderr=slave,start_new_session=True)
os.close(slave)
data=bytearray()
try:
    time.sleep(.2)
    subprocess.run(['/usr/bin/tmux','-S',sys.argv[2],'send-keys','-t','=local:','printf \'\\351\\233\\252\\n\'','Enter'],env=env,check=True,capture_output=True,timeout=2)
    deadline=time.monotonic()+3
    while time.monotonic()<deadline:
        if select.select([master],[],[],.1)[0]:
            try: chunk=os.read(master,65536)
            except OSError: break
            if not chunk: break
            data.extend(chunk)
            if '雪'.encode() in data: break
    assert '雪'.encode() in data,bytes(data)
finally:
    # Never poll/reap before signalling the fresh owned group.
    try: os.killpg(child.pid,signal.SIGTERM)
    except ProcessLookupError: pass
    try: child.wait(timeout=2)
    except subprocess.TimeoutExpired:
        os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=2)
    os.close(master)
"#;
    let attach = f.ctl.attach_argv("local");
    let spec = comandos_app::proc::ProcSpec {
        program: "/usr/bin/python3".into(),
        args: vec![
            "-I".into(),
            "-c".into(),
            code.into(),
            attach[2].clone().into(),
            f.ctl.socket_path().as_os_str().to_owned(),
            f.config.home().as_os_str().to_owned(),
        ],
        env: vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), f.config.home().as_os_str().to_owned()),
            ("TMPDIR".into(), std::env::temp_dir().into_os_string()),
        ],
        clear_env: true,
        env_remove: vec![],
        stdin: None,
        cwd: None,
        timeout: std::time::Duration::from_secs(10),
    };
    let out = comandos_app::proc::run(&spec).unwrap();
    assert!(
        out.code == Some(0) && !out.timed_out,
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
