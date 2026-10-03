#[path = "../audit_support/process.rs"]
mod process;
pub use process::*;
#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
mod tests {
    use super::*;
    fn path(name: &str) -> std::path::PathBuf {
        let dir = std::env::var_os("REGEX_FRONTEND_UNICODE_TEST_EVIDENCE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::env::temp_dir().join(format!(
                    "schema-regex-frontend-unicode-protocol-{}",
                    std::process::id()
                ))
            });
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }
    #[test]
    fn actual_child_success_exit_timeout_overflow_and_signal() {
        assert!(
            run(
                "/bin/true",
                &[],
                b"",
                Duration::from_secs(10),
                &path("success")
            )
            .is_ok()
        );
        assert!(
            run(
                "/bin/false",
                &[],
                b"",
                Duration::from_secs(10),
                &path("exit")
            )
            .unwrap_err()
            .starts_with("child exit")
        );
        assert_eq!(
            run(
                "/bin/sleep",
                &["30".into()],
                b"",
                Duration::from_millis(30),
                &path("timeout")
            )
            .unwrap_err(),
            "timeout"
        );
        assert_eq!(
            run(
                "/usr/bin/yes",
                &[],
                b"",
                Duration::from_secs(10),
                &path("overflow")
            )
            .unwrap_err(),
            "output overflow"
        );
        let err = run_inner(
            "/bin/sleep",
            &["30".into()],
            b"",
            Duration::from_secs(10),
            &path("signal"),
            |pid| {
                nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGSEGV,
                )
                .map_err(|e| e.to_string())
            },
        )
        .unwrap_err();
        assert!(err.contains("signal"), "{err}");
        let data: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path("timeout").with_extension("json")).unwrap())
                .unwrap();
        assert_eq!(data["reaped"], true);
    }
    #[test]
    fn input_overflow_is_rejected_before_spawn() {
        assert_eq!(
            run(
                "/bin/true",
                &[],
                &vec![0; INPUT_CAP + 1],
                Duration::from_secs(10),
                &path("input-overflow")
            )
            .unwrap_err(),
            "input overflow"
        );
    }
    #[test]
    fn combined_stream_excess_is_rejected() {
        let binary = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.migration-build/target/debug/examples/audit_unicode14");
        let result = run(
            binary.to_str().unwrap(),
            &["--emit-output".into(), "600000".into(), "600000".into()],
            b"",
            Duration::from_secs(10),
            &path("combined-overflow-final"),
        );
        assert!(matches!(result,Err(e) if e=="output overflow"));
        let meta: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path("combined-overflow-final").with_extension("json")).unwrap(),
        )
        .unwrap();
        assert_eq!(meta["output_truncated"], true);
        assert_eq!(meta["reaped"], true);
        assert!(meta["combined_retained_bytes"].as_u64().unwrap() <= OUTPUT_CAP as u64);
    }
    #[test]
    fn combined_counter_exact_below_above_includes_framing_and_resource() {
        let framing = b"[]\n";
        let resource = b"\nRESOURCE cpu_user=0.01 cpu_system=0.00 rss_kib=10304 status=0\n";
        for size in [OUTPUT_CAP - 1, OUTPUT_CAP, OUTPUT_CAP + 1] {
            let mut q = OutputQuota::default();
            assert_eq!(q.reserve(framing.len()), framing.len());
            assert_eq!(
                q.reserve(size - framing.len() - resource.len()),
                size - framing.len() - resource.len()
            );
            let n = q.reserve(resource.len());
            assert_eq!(n, resource.len() - usize::from(size > OUTPUT_CAP));
            assert_eq!(q.used, size.min(OUTPUT_CAP));
            assert_eq!(q.observed, size);
            assert_eq!(q.truncated, size > OUTPUT_CAP);
        }
    }
    #[test]
    fn stdout_only_stderr_only_overflow_and_recovery() {
        let binary = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.migration-build/target/debug/examples/audit_unicode14");
        for (out, err, label) in [
            (OUTPUT_CAP, 0, "stdout-resource-overflow"),
            (0, OUTPUT_CAP, "stderr-resource-overflow"),
            (OUTPUT_CAP - 512, 0, "near-cap-success"),
        ] {
            let result = run(
                binary.to_str().unwrap(),
                &["--emit-output".into(), out.to_string(), err.to_string()],
                b"",
                Duration::from_secs(10),
                &path(label),
            );
            if label == "near-cap-success" {
                assert!(result.is_ok());
            } else {
                assert!(matches!(result,Err(e) if e=="output overflow"));
            }
        }
        assert!(
            run(
                "/bin/true",
                &[],
                b"",
                Duration::from_secs(10),
                &path("post-overflow-recovery")
            )
            .is_ok()
        );
    }
    #[test]
    fn actual_reap_failure_is_preserved() {
        let err = run_inner(
            "/bin/true",
            &[],
            b"",
            Duration::from_secs(10),
            &path("reap-failure"),
            |pid| {
                nix::sys::wait::waitpid(nix::unistd::Pid::from_raw(pid as i32), None)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            },
        )
        .unwrap_err();
        assert!(err.starts_with("reap failure"), "{err}");
        let data: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path("reap-failure").with_extension("json")).unwrap(),
        )
        .unwrap();
        assert_eq!(data["reaped"], false);
    }
}
