#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
use comandos_app::resume::{exact_resume_command, resume_command, sane_flags};
use serde_json::json;

#[test]
fn sane_flags_keep_codex_config_c_and_drop_resume_ids() {
    assert_eq!(
        sane_flags(
            &json!([
                "-c",
                "model_provider=openai",
                "--resume",
                "old",
                "--last",
                "--model",
                "gpt-5"
            ]),
            "codex"
        ),
        vec!["-c", "model_provider=openai", "--model", "gpt-5"]
    );
}

#[test]
fn exact_resume_never_uses_last_or_continue() {
    assert!(exact_resume_command(&json!({"agent":"claude"})).is_none());
    assert_eq!(
        resume_command(&json!({"agent":"codex","resume_id":"12345678-1234-1234-1234-123456789abc","flags":["-c","x=y"]})).unwrap(),
        "codex resume 12345678-1234-1234-1234-123456789abc -c x=y"
    );
}
