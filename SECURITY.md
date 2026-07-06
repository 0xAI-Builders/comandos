# Security

ComandOS runs a local HTTP server (`cc-dash`, `127.0.0.1:4777`) that controls
tmux sessions, edits `~/.ssh/config`, and can type into your agents. That is
powerful, so the security model matters — especially once you expose the
dashboard to your phone.

## Threat model & defenses

**The server binds to `127.0.0.1` only.** It is never opened to `0.0.0.0`.
Remote access happens exclusively through `tailscale serve` (WireGuard + TLS,
tailnet-only — never Tailscale Funnel / the public internet).

Layered protections, all on by default:

| Threat | Defense |
|---|---|
| A malicious website `fetch()`-ing `http://127.0.0.1:4777` (drive-by CSRF → RCE) | **Origin allowlist**: any request with a cross-site `Origin` is rejected (403). Browsers always send `Origin` on cross-origin requests. |
| DNS-rebinding a domain to `127.0.0.1` | **Host header allowlist** (loopback + `*.ts.net` only). |
| A tailnet peer (or anyone reaching the port) operating the dashboard | **Access token** (`~/.claude/hooks/dash-token`, `0600`, compared with `hmac.compare_digest`). Local *direct* clients don't need it; remote/proxied requests do. Delivered via `X-Comandos-Token` header (from localStorage), `Authorization: Bearer`, or `?token=` query. The static shell (HTML/JS/icons/manifest) is served without a token — it holds no secrets; only data (`/state`) and actions are gated. Cookie-independent so it works in mobile/PWA webviews. |
| `~/.ssh/config` injection (`ProxyCommand` → RCE on connect) | `hostname`/`user`/`port`/`identity` are validated; newlines and control chars are rejected. |
| Popup spoofing / approval social-engineering (`cc-notifyd`) | Loopback-only + Origin-gated; concurrent popups capped. |
| Telegram takeover | Identity is checked by **numeric user id** (`TELEGRAM_ALLOWED_USER_ID`, immutable) and chat id — on messages **and** button callbacks. Fails **closed** if unconfigured. |
| Web terminal (embedded) | The terminal (`ttyd`, port 8443) is **tailnet-only** — no separate password, so it embeds in the dashboard without a per-frame prompt. Same boundary as SSH-over-Tailscale (WireGuard + device auth). The control token still gates the dashboard. |
| Secrets on disk | `telegram.env`, `cc-notify.conf`, `dash-token` are `chmod 600`. |

The dashboard renderer (`mdHtml`) is XSS-safe: agent/hook text is HTML-escaped
first and only ever placed into element *content* (never an attribute or an
`href`), so AI output cannot inject script.

## What is intentionally powerful

`/send`, `/key`, and Telegram `/run` are **designed** to type into your agents
and run commands — that is the product. They are all behind the auth gate.
Anyone with your access token (or your tailnet + a valid token) can drive your
agents. Treat the token like a password; `cc-mobile off` revokes remote access.

## Reporting

Found something? Open a private security advisory on the repo, or email the
maintainer. Please don't file public issues for exploitable bugs.
