"""Web Push for Android (N4) on top of pywebpush (pinned in requirements-push.txt).

- VAPID keys are generated once with private permissions (dir 0700, key 0600)
  next to the state database; only the public key is ever published.
- Subscriptions are accepted only for known push services over HTTPS, without
  credentials, odd ports or IP literals, so the API is not an arbitrary proxy.
- One delivery row per (event, device): 404/410 retire the subscription,
  429/5xx follow Retry-After or exponential backoff, a timeout retries the same
  delivery (same tag, so the phone replaces a duplicate) and never creates a
  new event.
- Lock-screen content follows D6: project + brief title, no excerpt.

pywebpush/cryptography are imported lazily: CommandOS keeps running without
them and `available()` explains what is missing.
"""
from __future__ import annotations

import base64
import email.utils
import hashlib
import ipaddress
import json
import os
import time
import urllib.parse
from dataclasses import dataclass
from pathlib import Path

DEFAULT_TTL_SECONDS = 3600
DEFAULT_SUBJECT = "mailto:comandos@localhost"
VISIBLE_WINDOW_MS = 120_000          # D5: push if no client is visible for 2 min
MAX_ENDPOINT = 2048
BACKOFF_BASE_MS, BACKOFF_CAP_MS = 30_000, 3_600_000
# Hosts of the browser vendors' push services (exact host or subdomain).
ALLOWED_PUSH_HOSTS = ("fcm.googleapis.com", "android.googleapis.com",
                      "push.services.mozilla.com", "notify.windows.com", "push.apple.com")
PUSH_KINDS = {"permission_requested", "input_requested", "turn_completed", "turn_failed", "focus_completed"}
KIND_TITLES = {"permission_requested": "Pide permiso", "input_requested": "Espera tu respuesta",
               "turn_completed": "Terminó el turno", "turn_failed": "El turno falló",
               "focus_completed": "Terminó el foco"}


def available():
    try:
        import pywebpush  # noqa: F401
        import py_vapid  # noqa: F401
        from cryptography.hazmat.primitives.asymmetric import ec  # noqa: F401
    except Exception as exc:  # ImportError or a broken native wheel
        return False, f"Push no disponible: falta pywebpush ({type(exc).__name__}); ver requirements-push.txt"
    return True, ""


def _b64(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


def _unb64(text):
    text = str(text or "")
    if not text or any(c not in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_=" for c in text):
        raise ValueError("clave no es base64url")
    return base64.urlsafe_b64decode(text.rstrip("=") + "=" * (-len(text.rstrip("=")) % 4))


# ---------------------------------------------------------------- VAPID

@dataclass(frozen=True)
class VapidKeys:
    private_key_path: str
    public_key: str          # applicationServerKey, base64url, uncompressed P-256 point


def default_dir():
    override = os.environ.get("COMANDOS_PUSH_DIR")
    if override:
        return Path(override)
    db = os.environ.get("COMANDOS_STATE_DB")
    if db:
        return Path(db).parent / "push"
    base = os.environ.get("XDG_STATE_HOME") or os.path.expanduser("~/.local/state")
    return Path(base) / "comandos" / "push"


def load_or_create_vapid(directory=None):
    from cryptography.hazmat.primitives import serialization
    from cryptography.hazmat.primitives.asymmetric import ec
    directory = Path(directory or default_dir())
    directory.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(directory, 0o700)
    path = directory / "vapid-private.pem"
    if not path.exists():
        key = ec.generate_private_key(ec.SECP256R1())
        pem = key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
                                serialization.NoEncryption())
        try:
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        except FileExistsError:
            pass                      # another process won the race; read its key
        else:
            with os.fdopen(fd, "wb") as fh:
                fh.write(pem)
    os.chmod(path, 0o600)
    key = serialization.load_pem_private_key(path.read_bytes(), password=None)
    public = key.public_key().public_bytes(serialization.Encoding.X962,
                                           serialization.PublicFormat.UncompressedPoint)
    return VapidKeys(str(path), _b64(public))


# ---------------------------------------------------------------- validation

def _allowed_host(host):
    return any(host == h or host.endswith("." + h) for h in ALLOWED_PUSH_HOSTS)


def validate_endpoint(endpoint):
    endpoint = str(endpoint or "")
    if not endpoint or len(endpoint) > MAX_ENDPOINT:
        raise ValueError("endpoint ausente o demasiado largo")
    try:
        parts = urllib.parse.urlsplit(endpoint)
        port = parts.port
    except ValueError:
        raise ValueError("endpoint inválido") from None
    if parts.scheme != "https":
        raise ValueError("el endpoint debe ser https")
    if parts.username or parts.password or "@" in parts.netloc:
        raise ValueError("el endpoint no puede llevar credenciales")
    if port not in (None, 443):
        raise ValueError("puerto no permitido")
    host = (parts.hostname or "").lower().rstrip(".")
    try:
        ipaddress.ip_address(host)
        is_ip = True
    except ValueError:
        is_ip = False
    if is_ip:
        raise ValueError("no se permiten direcciones IP")
    if not _allowed_host(host):
        raise ValueError("servicio push no permitido")
    return endpoint


def validate_subscription(sub):
    from cryptography.hazmat.primitives.asymmetric import ec
    if not isinstance(sub, dict):
        raise ValueError("suscripción inválida")
    endpoint = validate_endpoint(sub.get("endpoint"))
    keys = sub.get("keys") if isinstance(sub.get("keys"), dict) else {}
    try:
        p256dh, auth = _unb64(keys.get("p256dh")), _unb64(keys.get("auth"))
    except (ValueError, TypeError):
        raise ValueError("claves de suscripción inválidas") from None
    if len(p256dh) != 65 or p256dh[0] != 4 or len(auth) != 16:
        raise ValueError("claves de suscripción con longitud inválida")
    try:
        ec.EllipticCurvePublicKey.from_encoded_point(ec.SECP256R1(), p256dh)
    except ValueError:
        raise ValueError("clave p256dh fuera de la curva") from None
    return {"endpoint": endpoint, "keys": {"p256dh": _b64(p256dh), "auth": _b64(auth)}}


def endpoint_id(endpoint):
    return hashlib.sha256(endpoint.encode()).hexdigest()[:32]


# ---------------------------------------------------------------- payload and transport

def _clip(text, limit):
    text = " ".join(str(text or "").split())
    return text if len(text) <= limit else text[:limit - 1] + "…"


def build_payload(event):
    """D6: project + brief title. The excerpt stays in the app, behind auth."""
    eid = str(event.get("eventId") or "")
    kind = str(event.get("kind") or "")
    return {"eventId": eid, "kind": kind,
            "title": _clip(event.get("projectKey") or event.get("project") or "CommandOS", 40),
            "body": _clip(event.get("title") or KIND_TITLES.get(kind) or "Aviso", 80),
            "tag": f"comandos-event-{eid}",
            "url": "/?event=" + urllib.parse.quote(eid, safe="")}


def _topic(payload):
    return _b64(hashlib.sha256(str(payload.get("tag") or payload.get("title") or "").encode()).digest())[:32]


def _retry_after(value, now_ms):
    if not value:
        return None
    value = str(value).strip()
    if value.isdigit():
        return int(value) * 1000
    try:
        at = email.utils.parsedate_to_datetime(value)
    except (TypeError, ValueError):
        return None
    return max(0, int(at.timestamp() * 1000) - now_ms)


def _session():
    import requests

    class NoRedirect(requests.Session):
        def post(self, *a, **kw):
            kw["allow_redirects"] = False
            return super().post(*a, **kw)
    return NoRedirect()


_VAPID_CACHE = {}


def send_push(subscription, payload, *, vapid=None, session=None, ttl=DEFAULT_TTL_SECONDS,
              urgency="high", timeout=10, now_ms=None, subject=None):
    """Encrypt and POST one message. Returns a result dict; never raises for
    push-service answers (only for an invalid destination)."""
    import requests
    from py_vapid import Vapid
    from pywebpush import WebPusher
    sub = validate_subscription(subscription)
    vapid = vapid or load_or_create_vapid()
    now_ms = int(now_ms if now_ms is not None else time.time() * 1000)
    signer = _VAPID_CACHE.get(vapid.private_key_path)
    if signer is None:
        signer = _VAPID_CACHE[vapid.private_key_path] = Vapid.from_file(vapid.private_key_path)
    parts = urllib.parse.urlsplit(sub["endpoint"])
    claims = {"sub": subject or os.environ.get("COMANDOS_VAPID_SUBJECT") or DEFAULT_SUBJECT,
              "aud": f"{parts.scheme}://{parts.netloc}", "exp": int(time.time()) + 12 * 3600}
    headers = dict(signer.sign(claims))
    headers.update({"Urgency": urgency, "Topic": _topic(payload)})
    data = json.dumps(payload, separators=(",", ":"), ensure_ascii=False).encode()
    result = {"ok": False, "status": None, "gone": False, "retryAfterMs": None, "uncertain": False, "error": None}
    try:
        resp = WebPusher(sub, requests_session=session or _session()).send(
            data, headers, ttl=ttl, content_encoding="aes128gcm", timeout=timeout)
    except requests.RequestException as exc:
        result.update(uncertain=True, error=_clip(f"{type(exc).__name__}: {exc}", 200))
        return result
    status = int(resp.status_code)
    result["status"] = status
    result["ok"] = 200 <= status < 300
    result["gone"] = status in (404, 410)
    result["retryAfterMs"] = _retry_after((resp.headers or {}).get("Retry-After"), now_ms)
    if not result["ok"]:
        result["error"] = _clip(f"HTTP {status} {getattr(resp, 'reason', '') or ''}", 200)
    return result


# ---------------------------------------------------------------- subscriptions

def _sub_row(row):
    (sid, endpoint, p256dh, auth, device, agent, created, updated, disabled, reason, backoff, status) = row
    return {"id": sid, "endpoint": endpoint, "keys": {"p256dh": p256dh, "auth": auth}, "deviceId": device,
            "userAgent": agent, "createdAt": created, "updatedAt": updated, "disabledAt": disabled,
            "disabledReason": reason, "backoffUntil": backoff, "lastStatus": status}


_SUB_COLS = ("id, endpoint, p256dh, auth, device_id, user_agent, created_at_ms, updated_at_ms, "
             "disabled_at_ms, disabled_reason, backoff_until_ms, last_status")


def _tx(conn):
    class _Tx:
        def __enter__(self):
            conn.execute("BEGIN IMMEDIATE")

        def __exit__(self, exc_type, exc, tb):
            conn.execute("ROLLBACK" if exc_type else "COMMIT")
            return False
    return _Tx()


def save_subscription(conn, subscription, *, device_id=None, user_agent=None, now_ms=None):
    sub = validate_subscription(subscription)
    now_ms = int(now_ms if now_ms is not None else time.time() * 1000)
    sid = endpoint_id(sub["endpoint"])
    with _tx(conn):
        conn.execute(
            "INSERT INTO push_subscriptions (id, endpoint, p256dh, auth, device_id, user_agent, "
            "created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(endpoint) DO UPDATE SET "
            "p256dh = excluded.p256dh, auth = excluded.auth, device_id = excluded.device_id, "
            "user_agent = excluded.user_agent, updated_at_ms = excluded.updated_at_ms, "
            "disabled_at_ms = NULL, disabled_reason = NULL, backoff_until_ms = 0",
            (sid, sub["endpoint"], sub["keys"]["p256dh"], sub["keys"]["auth"],
             _clip(device_id, 80) or None, _clip(user_agent, 200) or None, now_ms, now_ms))
    return sid


def remove_subscription(conn, endpoint, now_ms=None, reason="retirada por el usuario"):
    now_ms = int(now_ms if now_ms is not None else time.time() * 1000)
    with _tx(conn):
        cur = conn.execute("UPDATE push_subscriptions SET disabled_at_ms = ?, disabled_reason = ?, "
                           "updated_at_ms = ? WHERE endpoint = ? AND disabled_at_ms IS NULL",
                           (now_ms, reason, now_ms, str(endpoint or "")))
    return cur.rowcount > 0


def active_subscriptions(conn, now_ms=None):
    rows = conn.execute(f"SELECT {_SUB_COLS} FROM push_subscriptions WHERE disabled_at_ms IS NULL "
                        "ORDER BY created_at_ms").fetchall()
    return [_sub_row(r) for r in rows]


def find_subscription(conn, endpoint):
    row = conn.execute(f"SELECT {_SUB_COLS} FROM push_subscriptions WHERE endpoint = ? "
                       "AND disabled_at_ms IS NULL", (str(endpoint or ""),)).fetchone()
    return _sub_row(row) if row else None


# ---------------------------------------------------------------- policy and delivery

def default_policy(event, clients, now_ms):
    """D5: push attention events when no client has been visible for 2 min.
    N2 injects the shared notification policy in its place."""
    if str(event.get("kind") or "") not in PUSH_KINDS:
        return False
    for c in clients or []:
        seen = c.get("lastSeenAt") or c.get("lastInteractionAt") or 0
        if c.get("visible") and c.get("connected", True) is not False and now_ms - int(seen) <= VISIBLE_WINDOW_MS:
            return False
    return True


def _backoff(attempts):
    return min(BACKOFF_BASE_MS * 2 ** max(0, attempts - 1), BACKOFF_CAP_MS)


def _delivery(conn, eid, sid):
    row = conn.execute("SELECT state, attempts, next_attempt_ms FROM push_deliveries "
                       "WHERE event_id = ? AND subscription_id = ?", (eid, sid)).fetchone()
    return {"state": row[0], "attempts": row[1], "nextAttemptMs": row[2]} if row else None


def deliveries_for_event(conn, event_id):
    rows = conn.execute(
        "SELECT d.subscription_id, s.device_id, d.state, d.attempts, d.next_attempt_ms, d.last_status, "
        "d.last_error, d.updated_at_ms FROM push_deliveries d JOIN push_subscriptions s "
        "ON s.id = d.subscription_id WHERE d.event_id = ? ORDER BY d.created_at_ms", (str(event_id),)).fetchall()
    return [{"subscriptionId": r[0], "deviceId": r[1], "state": r[2], "attempts": r[3], "nextAttemptMs": r[4],
             "lastStatus": r[5], "lastError": r[6], "updatedAt": r[7]} for r in rows]


def _record(conn, eid, sub, state, attempts, next_ms, result, now_ms):
    with _tx(conn):
        conn.execute(
            "INSERT INTO push_deliveries (event_id, subscription_id, state, attempts, next_attempt_ms, "
            "last_status, last_error, created_at_ms, updated_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) "
            "ON CONFLICT(event_id, subscription_id) DO UPDATE SET state = excluded.state, "
            "attempts = excluded.attempts, next_attempt_ms = excluded.next_attempt_ms, "
            "last_status = excluded.last_status, last_error = excluded.last_error, "
            "updated_at_ms = excluded.updated_at_ms",
            (eid, sub["id"], state, attempts, next_ms, result.get("status"), result.get("error"), now_ms, now_ms))
        conn.execute("UPDATE push_subscriptions SET last_status = ?, updated_at_ms = ? WHERE id = ?",
                     (result.get("status"), now_ms, sub["id"]))
        if state == "gone":
            conn.execute("UPDATE push_subscriptions SET disabled_at_ms = ?, disabled_reason = ? WHERE id = ?",
                         (now_ms, f"el servicio push respondió {result.get('status')}", sub["id"]))
        if result.get("status") == 429 and next_ms:
            conn.execute("UPDATE push_subscriptions SET backoff_until_ms = ? WHERE id = ?", (next_ms, sub["id"]))


def classify(result, attempts, now_ms, max_attempts):
    """→ (state, next_attempt_ms) for one push-service answer."""
    status = result.get("status")
    if result.get("ok"):
        return "sent", None
    if result.get("gone"):
        return "gone", None
    retryable = result.get("uncertain") or status == 429 or (status is not None and status >= 500)
    if not retryable or attempts >= max_attempts:
        return "failed", None
    return "retry", now_ms + (result.get("retryAfterMs") or _backoff(attempts))


def send_due_push(conn, now_ms, policy, events, clients, *, sender=None, max_attempts=5,
                  ttl_ms=DEFAULT_TTL_SECONDS * 1000):
    """Send pending pushes for `events` according to the injected `policy`.
    Returns the delivery attempts made in this call."""
    sender = sender or (lambda sub, payload: send_push(sub, payload, now_ms=now_ms))
    subs = active_subscriptions(conn, now_ms)
    out = []
    for event in events or []:
        eid = str(event.get("eventId") or "")
        if not eid:
            continue
        occurred = int(event.get("occurredAtMs") or event.get("receivedAtMs") or now_ms)
        if now_ms - occurred > ttl_ms:
            with _tx(conn):
                conn.execute("UPDATE push_deliveries SET state = 'failed', last_error = 'caducó sin entregarse', "
                             "updated_at_ms = ? WHERE event_id = ? AND state = 'retry'", (now_ms, eid))
            continue
        if not policy(event, clients, now_ms):
            continue
        payload = build_payload(event)
        for sub in subs:
            if sub["backoffUntil"] > now_ms:
                continue
            prior = _delivery(conn, eid, sub["id"])
            if prior and (prior["state"] != "retry" or (prior["nextAttemptMs"] or 0) > now_ms):
                continue
            attempts = (prior["attempts"] if prior else 0) + 1
            try:
                result = sender(sub, payload)
            except ValueError as exc:          # destination no longer valid
                result = {"ok": False, "status": None, "gone": True, "error": str(exc)}
            state, next_ms = classify(result, attempts, now_ms, max_attempts)
            _record(conn, eid, sub, state, attempts, next_ms, result, now_ms)
            if state == "gone":
                sub["backoffUntil"] = float("inf")
            elif result.get("status") == 429 and next_ms:
                sub["backoffUntil"] = next_ms
            out.append({"eventId": eid, "subscriptionId": sub["id"], "state": state, "attempts": attempts,
                        "nextAttemptMs": next_ms, "status": result.get("status")})
    return out


def run_test(conn, endpoint, sender, now_ms=None):
    """Explicit user action: one test notification to one stored device."""
    now_ms = int(now_ms if now_ms is not None else time.time() * 1000)
    sub = find_subscription(conn, endpoint)
    if not sub:
        raise LookupError("Este dispositivo no tiene una suscripción activa.")
    payload = {"eventId": "", "kind": "test", "title": "CommandOS", "body": "Aviso de prueba",
               "tag": "comandos-test", "url": "/"}
    result = sender(sub, payload)
    if result.get("gone"):
        remove_subscription(conn, endpoint, now_ms, f"el servicio push respondió {result.get('status')}")
    return {**result, "removed": bool(result.get("gone"))}
