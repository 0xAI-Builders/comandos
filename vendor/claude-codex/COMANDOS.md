# ComandOS vendored gateway

Upstream: `claude-codex` 0.3.1 from crates.io / https://github.com/fcakyon/claude-code-with-codex
License: MIT (`LICENSE`)

ComandOS patches:
- explicit Messages/count_tokens body limits for long Claude Code sessions;
- Grok 4.6 support and per-model reasoning effort;
- import/reuse of the official Grok Build subscription OAuth session from `~/.grok/auth.json`;
- pinned build for deterministic Claude/Codex/Grok routing.

The Grok subscription transport calls the CLI subscription endpoint, not a public xAI API. It is intentionally labeled Labs in the UI and may need updates when xAI changes its service.
