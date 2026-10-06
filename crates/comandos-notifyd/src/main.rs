//! Native notifyd and the desktop adapter share one entry.
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    comandos_notifyd::entry::run(&args)
}
