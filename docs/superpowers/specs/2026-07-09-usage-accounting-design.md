# ComandOS Usage Accounting Design

Date: 2026-07-09
Status: Approved for implementation planning

## Goal

ComandOS needs local, non-hallucinated usage accounting for Codex and Claude. It must show real usage, cost, limits, alerts, trends, and model recommendations without inventing per-session numbers when the provider only exposes aggregate data.

The feature must answer:

- How much Codex and Claude usage is being consumed.
- Which project, tmux session, pane, folder, model, and agent caused that usage.
- Whether the current pace risks daily, weekly, monthly, or provider limits.
- Which model should be used for a given coding workload.
- Which active session/pane should switch models to save usage.

All data and credentials stay local.

## Core Principle

The smallest local attribution unit is:

```text
tmux_session + tmux_pane + pane_pwd + git_root + agent
```

A tmux session can contain multiple splits. Each pane can have a different CLI, different process, and different working directory. Usage must therefore be attributed per pane first, then rolled up to session, project, provider, and model.

## Local Identity Model

Each active agent pane is identified from the existing ComandOS mapping:

```text
agent process pid -> tmux pane pid ancestry -> tmux session + tmux pane
```

For every pane, ComandOS records:

```json
{
  "provider": "codex|claude|openai|anthropic",
  "agent": "codex|claude",
  "agent_pid": 12345,
  "tmux_session": "term-123",
  "tmux_pane": "%18",
  "pane_pwd": "/home/someguy/codebase/0xJesus/ComandOS",
  "git_root": "/home/someguy/codebase/0xJesus/ComandOS",
  "tab_label": "ComandOS",
  "model": "sonnet|opus|haiku|fable|gpt-*",
  "reasoning_effort": "low|medium|high|max|null",
  "started_at": "2026-07-09T12:00:00Z",
  "last_seen_at": "2026-07-09T12:05:00Z"
}
```

`pane_pwd` is resolved from the tmux pane, not from the session. `git_root` is resolved from `pane_pwd` with `git rev-parse --show-toplevel`; when not inside a git repo, it falls back to `pane_pwd`.

## Usage Event Model

Every measured turn/request produces a local event:

```json
{
  "id": "stable local id",
  "provider": "codex|claude|openai|anthropic",
  "agent": "codex|claude",
  "tmux_session": "term-123",
  "tmux_pane": "%18",
  "pane_pwd": "/repo/frontend",
  "git_root": "/repo",
  "model": "sonnet",
  "reasoning_effort": "medium",
  "turn_started_at": 1783600000,
  "turn_finished_at": 1783600300,
  "input_tokens": 10000,
  "output_tokens": 1200,
  "cache_read_tokens": 5000,
  "cache_write_tokens": 0,
  "total_tokens": 16200,
  "cost_usd": 0.12,
  "source": "cli_turn|provider_api|reconciled",
  "confidence": "exact|reconciled|unattributed"
}
```

## Precision Rules

ComandOS must never present an invented number as exact.

Definitions:

- `exact`: The number came from provider API billing/usage or from verified local per-turn telemetry for that pane.
- `reconciled`: Local pane telemetry was matched against provider totals for the same provider, time bucket, model, workspace, project, or API key.
- `unattributed`: Provider total is exact, but cannot be safely assigned to a specific pane/session/project.

Rules:

- Exact by pane is allowed only when telemetry includes that pane identity or can be derived from the CLI process that ran inside that pane.
- Exact by provider is allowed when read from official OpenAI or Anthropic Admin/Usage/Cost APIs.
- If multiple panes share the same provider account/key/model during the same provider bucket and no per-turn telemetry exists, ComandOS stores the provider total as exact but marks the per-pane allocation as `unattributed`.
- No proportional split by time, active pane count, or rough token estimates may be labeled exact.

## Provider Data Sources

### OpenAI / Codex

Primary exact sources:

- OpenAI Admin API `/organization/usage/completions`.
- OpenAI Admin API `/organization/costs`.
- OpenAI project rate-limit endpoints.
- Codex CLI `/usage`, `/status`, and `/model` when invoked by the user or safely through controlled UI actions.

Credential:

- `OPENAI_ADMIN_KEY` for organization/project usage and cost APIs.

Officially supported OpenAI groupings include project, API key, user, model, batch, and service tier for completion usage; costs can be grouped by project, line item, and API key.

### Anthropic / Claude

Primary exact sources:

- Anthropic Admin API Usage and Cost API.
- Anthropic Claude Code Analytics API where available.
- Anthropic Rate Limits API.
- Claude Code local usage/status data when exposed by `/usage`, statusline, transcript, or hooks.

Credential:

- `ANTHROPIC_ADMIN_KEY` for organization usage, cost, analytics, and limits APIs.

Claude Code usage can also be centralized under its Claude Code workspace when authenticated through Console accounts. ComandOS should surface that workspace distinctly when the provider reports it.

## Local Storage

Use a local SQLite database:

```text
~/.claude/hooks/comandos-usage.sqlite
```

Tables:

- `usage_panes`: current and historical pane identity.
- `usage_turns`: local turn/request usage events.
- `provider_usage_buckets`: exact provider usage buckets.
- `provider_cost_buckets`: exact provider cost buckets.
- `usage_reconciliation`: links provider buckets to local events.
- `usage_alerts`: emitted warnings and cooldown state.
- `model_presets`: editable model recommendations.
- `usage_settings`: credentials source names, alert thresholds, polling intervals, and enabled providers.

Secrets are not stored in SQLite. The database stores only whether a credential source exists and the last successful fetch status.

## Credential Handling

ComandOS may read local credentials from:

- Environment variables.
- Existing CLI config when supported and safe.
- OS keyring in a later version.
- A local ComandOS config file containing credential source references, not plaintext secrets by default.

The dashboard API must never return raw credentials. Remote UI sees only aggregated metrics and credential health, such as `configured`, `missing`, `permission_denied`, or `last_success_at`.

## Backend Endpoints

Add backend endpoints to `cc-dash`:

- `GET /usage/state`: summary for dashboard cards, graphs, alerts, and session usage chips.
- `POST /usage/refresh`: fetch provider usage/cost data now.
- `POST /usage/capture`: capture or refresh CLI usage/status for a specific pane/session.
- `GET /usage/events`: paginated local turn events.
- `GET /usage/projects`: project, session, pane, model rollups.
- `GET /usage/alerts`: active and historical alerts.
- `POST /usage/settings`: update thresholds, providers, and polling.
- `POST /model/switch`: initiate model switch for a specific pane/session.

Provider fetching must run with short timeouts and never block `/state` rendering.

## UI

Add a top-level `Uso` panel in `dash/index.html`.

The panel shows:

- Provider cards for Codex/OpenAI and Claude/Anthropic.
- Daily, weekly, and monthly usage/cost.
- Limit bars and reset windows when available.
- 24h and 7d graphs.
- Prediction based on current exact/reconciled trend.
- Spike alerts.
- Ranking by project, session, pane, model, and provider.
- Unattributed exact provider totals.
- Credential health without exposing secrets.

Session cards should show compact usage chips:

```text
Claude · sonnet · $0.18 · exact
Codex · gpt-* · 120k tok · reconciled
```

When a split exists, the UI must display usage per pane, not only per tab.

## Alerts

Alert types:

- Usage crossed configured threshold: 70%, 85%, 95%.
- Cost crossed configured daily/weekly/monthly budget.
- Spike detected: recent usage slope exceeds configured multiplier or absolute increase.
- Provider fetch failure.
- Credential missing or insufficient permission.
- Unattributed usage increased while active panes exist.

Alerts reuse the existing ComandOS notification pipeline where possible and must have cooldowns to avoid repeated noise.

## Model Recommendations

ComandOS includes editable model presets:

- `Ahorro`: cheap/simple tasks.
- `Diario`: default coding work.
- `Difícil`: debugging, architecture, large refactors.
- `Máximo`: very ambiguous or high-risk tasks.

Claude presets use documented aliases when available:

- `haiku`
- `sonnet`
- `opus`
- `fable`

Codex presets should use currently available local model picker/catalog values rather than hard-coding undocumented labels. The UI can show recommended intent first and the resolved installed model second.

Model switching behavior:

- The action targets a pane, not only a session.
- If direct noninteractive switching is not supported, ComandOS sends or opens the provider's model picker in that pane.
- The UI must show when a model switch is pending user confirmation inside the CLI.

## Data Flow

1. `cc-dash` discovers live agent panes using existing process-to-pane mapping.
2. For each pane, it resolves `pane_pwd`, `git_root`, active agent, model, and session label.
3. Hooks/statusline/CLI captures write local usage events into SQLite.
4. Provider pollers fetch exact organization usage/cost buckets with admin keys.
5. Reconciliation links provider buckets to local pane events when the identity dimensions match.
6. The UI reads `/usage/state` for graphs, rankings, alerts, and session chips.
7. Alerts are emitted through the existing notification system with cooldown.

## Failure Handling

- Missing admin key: show setup-needed state, continue local pane tracking.
- Permission denied: show exact error class, disable provider polling until manually retried.
- Network failure: keep stale provider data with `last_success_at` and a warning.
- Provider pagination failure: store complete pages only; do not mix partial pages into exact totals.
- CLI output format changed: mark capture source as unavailable and keep provider exact totals.
- SQLite write failure: surface alert and do not drop provider fetch errors silently.

## Testing

Backend tests:

- Pane identity uses `tmux_pane` and `pane_pwd`, not only session.
- Split panes in one tmux session roll up separately.
- Git root fallback works outside git repos.
- Provider OpenAI usage/cost parser handles grouping and pagination.
- Provider Anthropic usage/cost parser handles grouping and pagination.
- Reconciliation never labels aggregate-only data as exact per pane.
- Alert thresholds and cooldowns work.

Frontend tests:

- Usage panel renders provider cards, graphs, project rollups, pane rows, and unattributed totals.
- Session cards show usage chips.
- Split pane usage is visible and not collapsed incorrectly.
- Credential health never renders secret values.

Manual verification:

- Start at least one Claude pane and one Codex pane in different folders.
- Start a split tmux session with two panes in different folders.
- Confirm usage events attach to the correct pane and git root.
- Confirm provider totals import and reconcile without blocking the dashboard.
- Confirm model switch action targets the selected pane.

## Implementation Boundaries

This design does not require scraping private web pages. It may read local credentials because the feature requires exact provider data, but it must use official provider APIs whenever possible.

No UI number may be marked exact unless its source satisfies the precision rules above.

## Source References

- OpenAI OpenAPI spec: `/organization/usage/completions`, `/organization/costs`, and `/organization/projects/{project_id}/rate_limits`.
- OpenAI Codex slash commands: `/usage`, `/status`, and `/model`.
- Anthropic Admin API: Usage and Cost API, Claude Code Analytics API, and Rate Limits API.
- Anthropic Claude Code costs documentation: Claude Code workspace usage tracking, `/usage`, and workspace spend/rate limits.
- Anthropic Claude Code model configuration: documented model aliases and environment-backed model selection.
