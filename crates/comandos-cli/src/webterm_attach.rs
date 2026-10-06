//! Authenticated legacy attachment with an injectable process boundary.
use crate::webterm::Runner;
use comandos_server::dash::term::attach::valid_session;
use std::{
    io::{self, BufRead, Write},
    path::Path,
};
pub fn run_with(
    home: &Path,
    args: &[String],
    runner: &dyn Runner,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    shell: &str,
) -> io::Result<i32> {
    let expected =
        std::fs::read_to_string(comandos_server::dash::token_path(home)).unwrap_or_default();
    let presented = args.first().map(String::as_str).unwrap_or("").trim();
    if expected.trim().is_empty()
        || !comandos_core::dashboard_access::token_matches(
            presented.as_bytes(),
            expected.trim().as_bytes(),
        )
    {
        writeln!(out, "Acceso denegado al terminal de ComandOS.")?;
        return Ok(1);
    }
    let version = runner
        .run("tmux", &["-V".into()])
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let active = comandos_server::dash::term::attach::supports_active_pane(&version);
    let attach = |session: &str| -> io::Result<i32> {
        let mut args = vec!["attach".into()];
        if active {
            args.extend(["-f".into(), "active-pane".into()]);
        }
        args.extend(["-t".into(), format!("={session}")]);
        Err(runner.exec("tmux", &args))
    };
    if let Some(session) = args.get(1).filter(|s| valid_session(s)) {
        if runner
            .run(
                "tmux",
                &["has-session".into(), "-t".into(), format!("={session}")],
            )
            .is_ok_and(|o| o.status.success())
        {
            return attach(session);
        }
        writeln!(out, "La sesion '{session}' ya no existe.\n")?;
    }
    let listed = runner.run(
        "tmux",
        &[
            "list-sessions".into(),
            "-F".into(),
            "#{session_name}".into(),
        ],
    );
    let text = listed
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let sessions: Vec<_> = text
        .lines()
        .filter(|s| !s.is_empty() && !matches!(*s, "local" | "hub" | "control"))
        .collect();
    if sessions.is_empty() {
        writeln!(out, "No hay sesiones. Crea una:  ccx nombre\n")?;
        return Err(runner.exec(shell, &[]));
    }
    writeln!(out, "Sesiones de ComandOS:")?;
    for (i, s) in sessions.iter().enumerate() {
        writeln!(out, "  {:2}) {s}", i + 1)?
    }
    write!(out, "\nNumero (o Enter para shell libre): ")?;
    out.flush()?;
    let mut line = String::new();
    input.read_line(&mut line)?;
    let line = line.trim_end_matches(['\r', '\n']);
    if !line.is_empty()
        && line.bytes().all(|b| b.is_ascii_digit())
        && let Ok(n) = line.parse::<usize>()
        && n >= 1
        && n <= sessions.len()
    {
        return attach(sessions[n - 1]);
    }
    Err(runner.exec(shell, &[]))
}
pub fn main(args: &[String]) -> i32 {
    let Some(home) = std::env::var_os("HOME").filter(|s| !s.is_empty()) else {
        eprintln!("HOME no está definido");
        return 1;
    };
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
    run_with(
        Path::new(&home),
        args,
        &crate::webterm::SystemRunner,
        &mut io::stdin().lock(),
        &mut io::stdout().lock(),
        &shell,
    )
    .unwrap_or_else(|e| {
        eprintln!("comandos webterm-attach: {e}");
        1
    })
}
