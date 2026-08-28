#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "vendor" / "claude-codex"
SERVER = (VENDOR / "src/server.rs").read_text()
REGISTRY = (VENDOR / "src/registry.rs").read_text()
ALLOW = (VENDOR / "src/providers/grok/translate/model_allowlist.rs").read_text()
REQUEST = (VENDOR / "src/providers/grok/translate/request.rs").read_text()
TOKEN_STORE = (VENDOR / "src/providers/grok/auth/token_store.rs").read_text()
SERVICE = (ROOT / "systemd/cc-proxy.service").read_text()


def test_messages_body_limit_is_independent_and_large_enough_for_long_claude_sessions():
    assert "MAX_ANTHROPIC_REQUEST_BYTES: usize = 64 * 1024 * 1024" in SERVER
    dispatch = SERVER.split("async fn dispatch_request", 1)[1]
    assert "to_bytes(req.into_body(), MAX_ANTHROPIC_REQUEST_BYTES)" in dispatch


def test_gateway_catalog_and_allowlist_use_live_grok_models():
    assert 'GROK_MODELS: &[&str] = &["grok-4.6", "grok-4.5"]' in REGISTRY
    assert '"grok-4.6" | "grok-4.5"' in ALLOW
    assert "grok-composer-2.5-fast" not in ALLOW


def test_gateway_forwards_model_specific_reasoning_effort():
    assert "pub reasoning: Option<GrokReasoning>" in REQUEST
    assert '"grok-4.6" => &["low", "medium", "high", "xhigh"]' in REQUEST
    assert '"grok-4.5" => &["low", "medium", "high"]' in REQUEST


def test_gateway_reuses_official_grok_subscription_without_api_key():
    assert "OfficialGrokAuthStore" in TOKEN_STORE
    assert 'GROK_HOME' in TOKEN_STORE
    assert 'refresh_token' in TOKEN_STORE
    assert 'XAI_API_KEY' not in TOKEN_STORE


def test_systemd_runs_pinned_comandos_gateway():
    assert "ExecStart=%h/.local/bin/cc-model-proxy" in SERVICE
    assert "PORT=18765" in SERVICE
