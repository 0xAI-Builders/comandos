"""N4 Web Push contract. Real payload encryption (pywebpush), fake transport:
no request ever leaves the process and no real device receives anything."""
import base64
import json
import os
import stat
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))

pytest.importorskip("pywebpush")
import http_ece  # noqa: E402
from cryptography.hazmat.primitives import serialization  # noqa: E402
from cryptography.hazmat.primitives.asymmetric import ec  # noqa: E402

import app_state  # noqa: E402
import web_push as wp  # noqa: E402

NOW = 1_800_000_000_000


def b64(raw):
    return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()


class Receiver:
    """A browser-side key pair, so the test can decrypt what was sent."""

    def __init__(self, endpoint="https://fcm.googleapis.com/fcm/send/abc123"):
        self.private = ec.generate_private_key(ec.SECP256R1())
        public = self.private.public_key().public_bytes(serialization.Encoding.X962,
                                                        serialization.PublicFormat.UncompressedPoint)
        self.auth = os.urandom(16)
        self.subscription = {"endpoint": endpoint, "keys": {"p256dh": b64(public), "auth": b64(self.auth)}}

    def decrypt(self, body):
        return json.loads(http_ece.decrypt(body, private_key=self.private, auth_secret=self.auth,
                                           version="aes128gcm"))


class FakeResponse:
    def __init__(self, status, headers=None):
        self.status_code, self.headers, self.text, self.reason = status, headers or {}, "", ""


class FakeSession:
    def __init__(self, *responses):
        self.responses, self.requests = list(responses), []

    def post(self, endpoint, timeout=None, data=None, headers=None, **kw):
        self.requests.append({"endpoint": endpoint, "data": data, "headers": dict(headers), "timeout": timeout})
        nxt = self.responses.pop(0) if self.responses else FakeResponse(201)
        if isinstance(nxt, Exception):
            raise nxt
        return nxt


@pytest.fixture
def conn(tmp_path):
    c = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(c)
    yield c
    c.close()


@pytest.fixture
def vapid(tmp_path):
    return wp.load_or_create_vapid(tmp_path / "push")


# ---------------------------------------------------------------- keys

def test_vapid_keys_are_private_and_stable(tmp_path):
    keys = wp.load_or_create_vapid(tmp_path / "push")
    pem = Path(keys.private_key_path)
    assert stat.S_IMODE(pem.stat().st_mode) == 0o600
    assert stat.S_IMODE(pem.parent.stat().st_mode) == 0o700
    raw = base64.urlsafe_b64decode(keys.public_key + "==")
    assert len(raw) == 65 and raw[0] == 4
    assert wp.load_or_create_vapid(tmp_path / "push").public_key == keys.public_key
    assert "PRIVATE" not in keys.public_key


def test_loose_permissions_are_tightened(tmp_path):
    keys = wp.load_or_create_vapid(tmp_path / "push")
    os.chmod(keys.private_key_path, 0o644)
    wp.load_or_create_vapid(tmp_path / "push")
    assert stat.S_IMODE(Path(keys.private_key_path).stat().st_mode) == 0o600


# ---------------------------------------------------------------- validation

@pytest.mark.parametrize("endpoint", [
    "https://fcm.googleapis.com/fcm/send/x",
    "https://updates.push.services.mozilla.com/wpush/v2/x",
    "https://wns2-par02p.notify.windows.com/w/?token=x",
    "https://web.push.apple.com/QGx",
])
def test_known_push_services_are_accepted(endpoint):
    sub = Receiver(endpoint).subscription
    assert wp.validate_subscription(sub)["endpoint"] == endpoint


@pytest.mark.parametrize("endpoint", [
    "http://fcm.googleapis.com/fcm/send/x",                # not https
    "https://user:pw@fcm.googleapis.com/fcm/send/x",       # credentials
    "https://127.0.0.1/push",                              # IP literal
    "https://[::1]/push",
    "https://10.0.0.5/push",
    "https://fcm.googleapis.com:8443/fcm/send/x",          # odd port
    "https://evil.example/fcm.googleapis.com",             # host not allowed
    "https://fcm.googleapis.com.evil.example/x",
    "https://localhost/push",
    "https://fcm.googleapis.com/" + "x" * 3000,            # too long
])
def test_arbitrary_destinations_are_rejected(endpoint):
    sub = Receiver().subscription
    sub["endpoint"] = endpoint
    with pytest.raises(ValueError):
        wp.validate_subscription(sub)


def test_bad_keys_are_rejected():
    sub = Receiver().subscription
    for bad in ({"p256dh": sub["keys"]["p256dh"], "auth": b64(b"short")},
                {"p256dh": b64(b"\x04" + b"\x00" * 64), "auth": sub["keys"]["auth"]},
                {"p256dh": "***", "auth": sub["keys"]["auth"]},
                {}):
        with pytest.raises(ValueError):
            wp.validate_subscription({"endpoint": sub["endpoint"], "keys": bad})
    with pytest.raises(ValueError):
        wp.validate_subscription(["not", "a", "dict"])


# ---------------------------------------------------------------- payload and transport

def test_locked_screen_payload_carries_project_and_brief_title_only():
    payload = wp.build_payload({"eventId": "ev-42", "projectKey": "comandos",
                                "title": "Pide permiso para ejecutar una herramienta que borra archivos del repo",
                                "excerpt": "SECRETO: contenido del turno", "kind": "permission_requested"})
    assert payload["title"] == "comandos"
    assert len(payload["body"]) <= 80
    assert "SECRETO" not in json.dumps(payload)
    assert payload["tag"] == "comandos-event-ev-42"
    assert payload["url"] == "/?event=ev-42"
    assert "token" not in payload["url"]


def test_event_ids_are_encoded_in_the_url():
    payload = wp.build_payload({"eventId": "a b&c=d", "projectKey": "p", "title": "t"})
    assert payload["url"] == "/?event=a%20b%26c%3Dd"


def test_send_encrypts_signs_and_uses_the_injected_transport(vapid):
    receiver = Receiver()
    session = FakeSession(FakeResponse(201))
    payload = wp.build_payload({"eventId": "ev-1", "projectKey": "proj", "title": "Terminó"})
    result = wp.send_push(receiver.subscription, payload, vapid=vapid, session=session)
    assert result == {"ok": True, "status": 201, "gone": False, "retryAfterMs": None, "uncertain": False, "error": None}
    sent = session.requests[0]
    assert sent["endpoint"] == receiver.subscription["endpoint"]
    assert sent["headers"]["content-encoding"] == "aes128gcm"
    assert sent["headers"]["ttl"] == str(wp.DEFAULT_TTL_SECONDS)
    assert sent["headers"]["urgency"] == "high"
    assert len(sent["headers"]["topic"]) <= 32
    assert sent["headers"]["authorization"].startswith("vapid t=")
    assert "k=" + vapid.public_key in sent["headers"]["authorization"]
    assert receiver.decrypt(sent["data"]) == payload
    assert b"proj" not in sent["data"]                       # payload is encrypted on the wire


@pytest.mark.parametrize("status,gone", [(404, True), (410, True)])
def test_expired_subscription_is_reported_gone(vapid, status, gone):
    result = wp.send_push(Receiver().subscription, {"title": "x"}, vapid=vapid,
                          session=FakeSession(FakeResponse(status)))
    assert result["gone"] is gone and result["ok"] is False


def test_retry_after_seconds_and_date(vapid):
    r = wp.send_push(Receiver().subscription, {"title": "x"}, vapid=vapid,
                     session=FakeSession(FakeResponse(429, {"Retry-After": "120"})), now_ms=NOW)
    assert r["retryAfterMs"] == 120_000
    r = wp.send_push(Receiver().subscription, {"title": "x"}, vapid=vapid,
                     session=FakeSession(FakeResponse(503, {"Retry-After": "Fri, 15 Jan 2027 08:05:00 GMT"})),
                     now_ms=NOW)
    assert r["retryAfterMs"] == 300_000


def test_timeout_is_uncertain_not_a_new_event(vapid):
    import requests
    r = wp.send_push(Receiver().subscription, {"title": "x"}, vapid=vapid,
                     session=FakeSession(requests.Timeout("slow")))
    assert r["uncertain"] is True and r["ok"] is False and r["status"] is None


def test_send_refuses_an_invalid_destination_without_a_request(vapid):
    session = FakeSession()
    sub = Receiver().subscription
    sub["endpoint"] = "https://169.254.169.254/latest"
    with pytest.raises(ValueError):
        wp.send_push(sub, {"title": "x"}, vapid=vapid, session=session)
    assert session.requests == []


# ---------------------------------------------------------------- subscriptions and due deliveries

def test_subscription_roundtrip_and_retirement(conn):
    receiver = Receiver()
    sid = wp.save_subscription(conn, receiver.subscription, device_id="android-1", user_agent="Chrome", now_ms=NOW)
    assert [s["id"] for s in wp.active_subscriptions(conn, NOW)] == [sid]
    # Re-subscribing the same endpoint updates it instead of duplicating.
    assert wp.save_subscription(conn, receiver.subscription, device_id="android-1", now_ms=NOW + 1) == sid
    assert wp.remove_subscription(conn, receiver.subscription["endpoint"], NOW + 2) is True
    assert wp.active_subscriptions(conn, NOW + 3) == []
    assert wp.remove_subscription(conn, receiver.subscription["endpoint"], NOW + 4) is False


def visible_client(seen_ms):
    return {"deviceId": "desk", "visible": True, "connected": True, "lastSeenAt": seen_ms}


def test_default_policy_pushes_only_when_no_client_is_visible_for_two_minutes():
    event = {"eventId": "e", "kind": "permission_requested", "projectKey": "p", "title": "t"}
    assert wp.default_policy(event, [visible_client(NOW - 30_000)], NOW) is False
    assert wp.default_policy(event, [visible_client(NOW - 121_000)], NOW) is True
    hidden = {"deviceId": "desk", "visible": False, "connected": True, "lastSeenAt": NOW}
    assert wp.default_policy(event, [hidden], NOW) is True
    assert wp.default_policy(event, [], NOW) is True
    assert wp.default_policy({**event, "kind": "news_edition"}, [], NOW) is False


class Sender:
    def __init__(self, *results):
        self.results, self.calls = list(results), []

    def __call__(self, subscription, payload):
        self.calls.append((subscription["id"], payload))
        r = self.results.pop(0) if self.results else {"ok": True, "status": 201}
        return {"ok": False, "status": None, "gone": False, "retryAfterMs": None, "uncertain": False,
                "error": None, **r}


EVENT = {"eventId": "ev-9", "kind": "turn_completed", "projectKey": "proj", "title": "Terminó",
         "occurredAtMs": NOW}


def test_due_push_is_sent_once_per_event_and_device(conn):
    wp.save_subscription(conn, Receiver().subscription, device_id="a", now_ms=NOW)
    wp.save_subscription(conn, Receiver("https://fcm.googleapis.com/fcm/send/other").subscription,
                         device_id="b", now_ms=NOW)
    sender = Sender()
    out = wp.send_due_push(conn, NOW, wp.default_policy, [EVENT], [], sender=sender)
    assert len(sender.calls) == 2 and {r["state"] for r in out} == {"sent"}
    again = wp.send_due_push(conn, NOW + 1000, wp.default_policy, [EVENT], [], sender=sender)
    assert len(sender.calls) == 2 and again == []


def test_policy_is_injectable_and_visible_client_suppresses_push(conn):
    wp.save_subscription(conn, Receiver().subscription, now_ms=NOW)
    sender = Sender()
    assert wp.send_due_push(conn, NOW, wp.default_policy, [EVENT], [visible_client(NOW - 1000)], sender=sender) == []
    assert wp.send_due_push(conn, NOW, lambda e, c, n: False, [EVENT], [], sender=sender) == []
    assert sender.calls == []
    wp.send_due_push(conn, NOW, lambda e, c, n: True, [EVENT], [visible_client(NOW)], sender=sender)
    assert len(sender.calls) == 1


def test_gone_subscription_is_retired(conn):
    sid = wp.save_subscription(conn, Receiver().subscription, now_ms=NOW)
    out = wp.send_due_push(conn, NOW, wp.default_policy, [EVENT], [], sender=Sender({"status": 410, "gone": True}))
    assert out[0]["state"] == "gone"
    assert wp.active_subscriptions(conn, NOW) == []
    reason = conn.execute("SELECT disabled_reason FROM push_subscriptions WHERE id = ?", (sid,)).fetchone()[0]
    assert "410" in reason


def test_429_backs_off_with_retry_after_then_retries(conn):
    wp.save_subscription(conn, Receiver().subscription, now_ms=NOW)
    sender = Sender({"status": 429, "retryAfterMs": 60_000}, {"ok": True, "status": 201})
    out = wp.send_due_push(conn, NOW, wp.default_policy, [EVENT], [], sender=sender)
    assert out[0]["state"] == "retry" and out[0]["nextAttemptMs"] == NOW + 60_000
    assert wp.send_due_push(conn, NOW + 30_000, wp.default_policy, [EVENT], [], sender=sender) == []
    done = wp.send_due_push(conn, NOW + 61_000, wp.default_policy, [EVENT], [], sender=sender)
    assert done[0]["state"] == "sent" and len(sender.calls) == 2


def test_5xx_without_retry_after_uses_exponential_backoff_and_gives_up(conn):
    wp.save_subscription(conn, Receiver().subscription, now_ms=NOW)
    sender = Sender(*[{"status": 503}] * 10)
    t, states = NOW, []
    for _ in range(8):
        out = wp.send_due_push(conn, t, wp.default_policy, [{**EVENT, "occurredAtMs": NOW}], [],
                               sender=sender, max_attempts=4, ttl_ms=10**9)
        states += [r["state"] for r in out]
        t += 10**7
    assert states == ["retry", "retry", "retry", "failed"]
    delivery = wp.deliveries_for_event(conn, EVENT["eventId"])[0]
    assert delivery["state"] == "failed" and delivery["lastStatus"] == 503


def test_timeout_keeps_the_same_delivery_and_does_not_create_an_event(conn):
    wp.save_subscription(conn, Receiver().subscription, now_ms=NOW)
    sender = Sender({"uncertain": True, "error": "timeout"}, {"ok": True, "status": 201})
    wp.send_due_push(conn, NOW, wp.default_policy, [EVENT], [], sender=sender)
    wp.send_due_push(conn, NOW + 10**6, wp.default_policy, [EVENT], [], sender=sender)
    rows = wp.deliveries_for_event(conn, EVENT["eventId"])
    assert len(rows) == 1 and rows[0]["state"] == "sent" and rows[0]["attempts"] == 2
    # Both attempts carry the same tag, so a duplicate replaces the first on the phone.
    assert sender.calls[0][1]["tag"] == sender.calls[1][1]["tag"]


def test_stale_event_is_not_pushed_late(conn):
    wp.save_subscription(conn, Receiver().subscription, now_ms=NOW)
    sender = Sender()
    old = {**EVENT, "occurredAtMs": NOW - 2 * wp.DEFAULT_TTL_SECONDS * 1000}
    assert wp.send_due_push(conn, NOW, wp.default_policy, [old], [], sender=sender) == []
    assert sender.calls == []


def test_other_4xx_fails_without_retry(conn):
    wp.save_subscription(conn, Receiver().subscription, now_ms=NOW)
    out = wp.send_due_push(conn, NOW, wp.default_policy, [EVENT], [], sender=Sender({"status": 400, "error": "bad"}))
    assert out[0]["state"] == "failed"
    assert wp.send_due_push(conn, NOW + 10**7, wp.default_policy, [EVENT], [], sender=Sender()) == []
