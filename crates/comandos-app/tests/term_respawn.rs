#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
use comandos_app::term::{
    lifecycle::RespawnPolicy,
    pty::{PtyDrain, PtySession, ReadOutcome},
};
#[test]
fn owned_pty_eof_stops_mosaic_and_cleanup_stops_every_client_kind() {
    let root = std::env::temp_dir();
    for kind in ["interactive", "side", "zoom", "mosaic"] {
        let mut policy = RespawnPolicy::default();
        policy.set_enabled(kind != "mosaic");
        assert!(policy.may_start(), "first spawn: {kind}");
        let mut pty = PtySession::spawn_with_env(
            &[
                "/bin/sh".into(),
                "-c".into(),
                "printf owned-eof; exit 0".into(),
            ],
            40,
            8,
            &root,
            &[("HOME".into(), std::env::var("HOME").unwrap())],
            true,
        )
        .unwrap();
        let mut drain = PtyDrain::default();
        let start = std::time::Instant::now();
        let mut output = vec![];
        while drain.finished().is_none() {
            assert!(start.elapsed() < std::time::Duration::from_secs(3));
            drain.observe_exit(&mut pty);
            let mut buf = [0u8; 256];
            match pty.read_chunk(&mut buf) {
                ReadOutcome::Data(n) => {
                    output.extend_from_slice(&buf[..n]);
                }
                ReadOutcome::Closed => {
                    drain.close();
                }
                ReadOutcome::WouldBlock => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        }
        assert_eq!(drain.finished(), Some(0));
        assert_eq!(output, b"owned-eof");
        assert_eq!(policy.exited(), kind != "mosaic", "EOF relaunch: {kind}");
        assert_eq!(policy.may_start(), kind != "mosaic");
        policy.shutdown();
        assert!(!policy.may_start());
        assert!(!policy.exited());
        drop(pty);
    }
}
