//! Use the existing notifyd entry, preserving native argument bytes.
use std::{ffi::OsString, process::ExitCode};

pub fn run(args: &[OsString]) -> ExitCode {
    comandos_notifyd::entry::run(args)
}
