"""N2: one classification and delivery policy for every notice producer (D5/D6)."""
from pathlib import Path
import sys
import threading

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "lib"))
import app_state  # noqa: E402
import event_store  # noqa: E402
import notification_delivery as nd  # noqa: E402

NOW = 1_800_000_000_000
PREFS = nd.default_prefs()


def ev(kind, eid=None, project="ComandOS", at=NOW, session="alpha", pane="%1", **extra):
    event = {"eventId": eid or f"{kind}-{at}", "kind": kind, "projectKey": project, "sessionKey": session,
             "paneId": pane, "occurredAtMs": at, "title": kind, "excerpt": "", **extra}
    return event


def client(device, visible=True, seen=NOW, interaction=NOW - 1000, audio=True, hidden_since=None):
    return {"deviceId": device, "visible": visible, "lastSeenAt": seen, "lastInteractionAt": interaction,
            "canPlayAudio": audio, "connected": True, "hiddenSince": hidden_since}


# ---- classification and D5 ------------------------------------------------

@pytest.mark.parametrize("kind,category,sound", [
    ("permission_requested", "attention", True),
    ("input_requested", "attention", True),
    ("turn_failed", "error", True),
    ("focus_completed", "focus", True),
    ("turn_completed", "done", False),        # visual only
    ("news_edition", "news", False),
])
def test_d5_sound_only_when_you_are_needed(kind, category, sound):
    route = nd.route_event(ev(kind), [client("phone")], PREFS, NOW)
    assert route["category"] == category
    assert route["sound"] is sound
    assert route["float"] == {"show": True, "ms": 6000}


def test_lifecycle_noise_is_not_a_notice():
    for kind in ("prompt_accepted", "turn_started"):
        assert nd.route_event(ev(kind), [], PREFS, NOW)["notice"] is False


def test_during_focus_only_permissions_sound():
    clients = [client("phone")]
    assert nd.route_event(ev("permission_requested"), clients, PREFS, NOW, focus_active=True)["sound"] is True
    for kind in ("input_requested", "turn_failed", "focus_completed"):
        assert nd.route_event(ev(kind), clients, PREFS, NOW, focus_active=True)["sound"] is False


def test_modes_volume_and_mute_come_from_prefs():
    prefs = nd.merge_prefs(PREFS, {"modes": {"done": "sound", "attention": "visual"}})
    assert nd.route_event(ev("turn_completed"), [client("a")], prefs, NOW)["sound"] is True
    assert nd.route_event(ev("permission_requested"), [client("a")], prefs, NOW)["sound"] is False
    muted = nd.merge_prefs(PREFS, {"muted": True})
    assert nd.route_event(ev("permission_requested"), [client("a")], muted, NOW)["sound"] is False


def test_prefs_validation_rejects_unknown_values():
    with pytest.raises(ValueError):
        nd.merge_prefs(PREFS, {"modes": {"done": "loud"}})
    with pytest.raises(ValueError):
        nd.merge_prefs(PREFS, {"volume": 3})
    assert nd.merge_prefs(PREFS, {"volume": 0.2})["volume"] == 0.2
    assert nd.default_prefs()["modes"]["done"] == "visual"


# ---- who plays the sound (D5 last explicit interaction, D6 fallback) -----------

def test_sound_goes_to_the_visible_device_with_the_latest_explicit_interaction():
    clients = [client("desk", interaction=NOW - 60_000), client("phone", interaction=NOW - 5_000)]
    assert nd.route_event(ev("permission_requested"), clients, PREFS, NOW)["soundDevice"] == "phone"


def test_hidden_or_stale_clients_never_get_the_sound():
    clients = [client("phone", visible=False, interaction=NOW),
               client("tablet", seen=NOW - 10 * 60_000, interaction=NOW),
               client("desk", interaction=NOW - 60_000)]
    assert nd.route_event(ev("permission_requested"), clients, PREFS, NOW)["soundDevice"] == "desk"


def test_d6_next_visible_client_plays_when_the_last_one_cannot():
    clients = [client("phone", audio=False, interaction=NOW - 1_000), client("desk", interaction=NOW - 90_000)]
    assert nd.route_event(ev("turn_failed"), clients, PREFS, NOW)["soundDevice"] == "desk"


def test_nobody_visible_means_the_local_speaker_fallback_and_later_push():
    route = nd.route_event(ev("permission_requested", at=NOW - 3 * 60_000), [client("phone", visible=False, hidden_since=NOW - 3 * 60_000)], PREFS, NOW)
    assert route["soundDevice"] == nd.LOCAL_SPEAKER
    assert route["push"] is True


def test_push_waits_until_no_client_was_visible_for_two_minutes():
    recent = [client("phone", visible=False, hidden_since=NOW - 30_000)]
    assert nd.route_event(ev("permission_requested"), [client("desk")], PREFS, NOW)["push"] is False
    assert nd.route_event(ev("permission_requested"), recent, PREFS, NOW)["push"] is False
    gone = [client("phone", visible=False, hidden_since=NOW - 3 * 60_000)]
    assert nd.route_event(ev("permission_requested"), gone, PREFS, NOW)["push"] is True
    assert nd.route_event(ev("turn_completed"), gone, PREFS, NOW)["push"] is True
    assert nd.route_event(ev("news_edition"), gone, PREFS, NOW)["push"] is False



def test_push_only_for_events_after_the_last_visible_moment():
    gone = [client("phone", visible=False, hidden_since=NOW - 3 * 60_000)]
    # Seen on screen before the phone was put away: never pushed later.
    assert nd.route_event(ev("permission_requested", at=NOW - 10 * 60_000), gone, PREFS, NOW)["push"] is False
    assert nd.route_event(ev("turn_failed", at=NOW - 3 * 60_000 - 1), gone, PREFS, NOW)["push"] is False
    # Arrived while nobody was looking.
    assert nd.route_event(ev("turn_failed", at=NOW - 3 * 60_000), gone, PREFS, NOW)["push"] is True
    assert nd.route_event(ev("turn_failed", at=NOW - 60_000), gone, PREFS, NOW)["push"] is True
    policy = nd.push_policy(PREFS)
    assert policy(ev("turn_failed", at=NOW - 10 * 60_000), gone, NOW) is False



def test_an_open_but_unattended_window_does_not_block_the_push():
    # Desktop window left open (never minimised) while Jesús is away.
    away = [client("desktop-zion", visible=True, seen=NOW, interaction=NOW - 10 * 60_000)]
    assert nd.route_event(ev("permission_requested", at=NOW - 60_000), away, PREFS, NOW)["push"] is True
    # Someone touched it a moment ago: they are there.
    here = [client("desktop-zion", visible=True, seen=NOW, interaction=NOW - 30_000)]
    assert nd.route_event(ev("permission_requested", at=NOW - 10_000), here, PREFS, NOW)["push"] is False
    # Visible, never touched since it connected: not a person looking.
    never = [client("desktop-zion", visible=True, seen=NOW, interaction=None)]
    assert nd.route_event(ev("permission_requested", at=NOW - 10_000), never, PREFS, NOW - 0)["push"] is True


# ---- grouping, pending and news -------------------------------------------------

def test_bursts_in_one_project_share_a_group_for_ten_seconds():
    events = [ev("turn_completed", "a", at=NOW), ev("turn_completed", "b", at=NOW + 4_000, pane="%2"),
              ev("turn_completed", "c", at=NOW + 12_000), ev("turn_completed", "d", project="Otro", at=NOW + 5_000)]
    groups = nd.group_keys(events, PREFS)
    assert groups["a"] == groups["b"] == groups["c"]      # chain within 10 s of the previous one
    assert groups["d"] != groups["a"]
    far = nd.group_keys([ev("turn_completed", "x", at=NOW), ev("turn_completed", "y", at=NOW + 11_000)], PREFS)
    assert far["x"] != far["y"]


def test_news_never_joins_an_invented_project():
    notice = nd.notice(ev("news_edition", project=None, session=None, pane=None), [], PREFS, NOW, read=set(), group="n")
    assert notice["project"] is None and notice["category"] == "news"


def test_pending_requests_clear_only_on_a_later_event_of_the_same_pane():
    events = [ev("permission_requested", "p1", pane="%1", at=NOW), ev("input_requested", "p2", pane="%2", at=NOW + 1),
              ev("turn_completed", "c1", pane="%1", at=NOW + 2)]
    assert nd.pending_requests(events) == ["p2"]


# ---- durable state: presence, reads, single sound claim ------------------------

@pytest.fixture
def conn(tmp_path):
    c = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(c)
    return c


def store_event(conn, kind, eid, at=NOW, pane="%1"):
    conn.execute("BEGIN IMMEDIATE")
    event_store.append_event(conn, {"eventId": eid, "source": "test", "kind": kind, "evidence": "confirmed",
                                    "correlation": "local", "occurredAtMs": at, "receivedAtMs": at,
                                    "projectKey": "ComandOS", "sessionKey": "alpha", "paneId": pane})
    conn.execute("COMMIT")


def test_presence_interaction_is_only_explicit(conn):
    nd.record_presence(conn, "phone", visible=True, can_play_audio=True, interaction=False, now_ms=NOW, kind="web")
    assert nd.clients(conn, NOW)[0]["lastInteractionAt"] is None
    nd.record_presence(conn, "phone", visible=True, can_play_audio=True, interaction=True, now_ms=NOW + 5)
    nd.record_presence(conn, "phone", visible=False, can_play_audio=True, interaction=False, now_ms=NOW + 9)
    state = nd.clients(conn, NOW + 9)[0]
    assert state["lastInteractionAt"] == NOW + 5 and state["visible"] is False and state["lastSeenAt"] == NOW + 9


def test_reading_never_resolves_a_pending_request(conn):
    store_event(conn, "permission_requested", "perm")
    nd.mark_read(conn, ["perm"], NOW)
    page = nd.list_notices(conn, 0, 50, NOW, device_id="phone")
    assert page["notices"][0]["read"] is True and page["pending"] == ["perm"]


def test_simultaneous_claims_play_the_sound_once(conn, tmp_path):
    store_event(conn, "permission_requested", "perm")
    nd.record_presence(conn, "phone", visible=True, can_play_audio=True, interaction=True, now_ms=NOW)
    results = []

    def claim():
        c = app_state.connect(tmp_path / "state.sqlite3")
        results.append(nd.claim_sound(c, "perm", "phone", NOW + 10)["play"])
    threads = [threading.Thread(target=claim) for _ in range(6)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert results.count(True) == 1


def test_a_device_that_is_not_the_chosen_one_does_not_play(conn):
    store_event(conn, "permission_requested", "perm")
    nd.record_presence(conn, "desk", visible=True, can_play_audio=True, interaction=True, now_ms=NOW - 50_000)
    nd.record_presence(conn, "phone", visible=True, can_play_audio=True, interaction=True, now_ms=NOW)
    answer = nd.claim_sound(conn, "perm", "desk", NOW + 1)
    assert answer["play"] is False
    assert nd.claim_sound(conn, "perm", "phone", NOW + 1) == {"play": True, "cue": "permission"}


def test_visual_only_events_never_claim_sound(conn):
    store_event(conn, "turn_completed", "done")
    nd.record_presence(conn, "phone", visible=True, can_play_audio=True, interaction=True, now_ms=NOW)
    assert nd.claim_sound(conn, "done", "phone", NOW + 1)["play"] is False


def test_prefs_persist(conn):
    assert nd.load_prefs(conn) == nd.default_prefs()
    nd.save_prefs(conn, {"volume": 0.3, "modes": {"news": "sound"}})
    prefs = nd.load_prefs(conn)
    assert prefs["volume"] == 0.3 and prefs["modes"]["news"] == "sound" and prefs["modes"]["done"] == "visual"


def run_intake(tmp_path, payload, *args):
    import json as _json
    import os
    import subprocess
    env = dict(os.environ, COMANDOS_STATE_DB=str(tmp_path / "state.sqlite3"), HOME=str(tmp_path))
    return subprocess.run([sys.executable, str(ROOT / "lib/event_intake.py"), "record", *args],
                          input=_json.dumps(payload), capture_output=True, text=True, env=env, timeout=20)


def hook(kind_event, **extra):
    return {"hookEvent": kind_event, "agent": "claude", "project": "ComandOS", "session": "alpha", "pane": "%1",
            "title": "t", **extra}


def test_hook_plays_locally_only_when_it_wins_the_claim(tmp_path):
    # Nobody visible: the local speaker is the fallback for a permission.
    out = run_intake(tmp_path, hook("Notification", notificationType="permission_prompt"), "--claim-sound", "desktop-x")
    assert out.returncode == 0 and out.stdout.strip() == "play"
    # A finished turn is visual only (D5): the hook stays silent.
    out = run_intake(tmp_path, hook("Stop"), "--claim-sound", "desktop-x")
    assert out.returncode == 0 and out.stdout.strip() == ""


def test_hook_stays_silent_when_the_phone_was_used_last(tmp_path):
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    import time as _time
    nd.record_presence(conn, "phone", visible=True, can_play_audio=True, interaction=True,
                       now_ms=int(_time.time() * 1000))
    out = run_intake(tmp_path, hook("Notification", notificationType="permission_prompt"), "--claim-sound", "desktop-x")
    assert out.returncode == 0 and out.stdout.strip() == ""


def test_claim_reads_an_active_focus_block_from_the_state(conn):
    store_event(conn, "turn_failed", "err")
    store_event(conn, "permission_requested", "perm")
    conn.execute("BEGIN IMMEDIATE")
    conn.execute("INSERT INTO pomodoro_blocks (block_id, mode, status, target_ms, active_ms, deadline_ms, "
                 "started_at_ms, updated_at_ms) VALUES ('b1', 'focus', 'running', 1500000, 0, ?, ?, ?)",
                 (NOW + 60_000, NOW, NOW))
    conn.execute("UPDATE pomodoro_state SET block_id = 'b1' WHERE id = 1")
    conn.execute("COMMIT")
    assert nd.focus_block_active(conn) is True
    assert nd.claim_sound(conn, "err", nd.LOCAL_SPEAKER, NOW)["play"] is False
    assert nd.claim_sound(conn, "perm", nd.LOCAL_SPEAKER, NOW)["play"] is True


def test_announcements_and_usage_alerts_are_not_waiting_requests():
    news = nd.route_event(ev("announcement", project=None, session=None, pane=None), [client("a")], PREFS, NOW)
    assert news["category"] == "news" and news["needsHuman"] is False and news["sound"] is False
    usage = nd.route_event(ev("usage_alert", project="Uso", session=None, pane=None), [client("a")], PREFS, NOW)
    assert usage["category"] == "usage" and usage["needsHuman"] is False and usage["sound"] is False
    assert usage["float"]["show"] is True
    assert nd.merge_prefs(PREFS, {"modes": {"usage": "sound"}})["modes"]["usage"] == "sound"


def test_event_store_accepts_the_new_producer_kinds(conn):
    for kind in ("announcement", "usage_alert"):
        conn.execute("BEGIN IMMEDIATE")
        event_store.append_event(conn, {"eventId": kind, "source": "comandos", "kind": kind, "evidence": "confirmed",
                                        "correlation": "unknown", "occurredAtMs": NOW, "receivedAtMs": NOW})
        conn.execute("COMMIT")
    page = nd.list_notices(conn, 0, 10, NOW)
    assert [n["kind"] for n in page["notices"]] == ["announcement", "usage_alert"]
    assert page["notices"][0]["project"] is None


def test_claude_idle_and_auth_notices_are_not_requests(tmp_path):
    for ntype in ("idle_prompt", "auth_success"):
        out = run_intake(tmp_path, hook("Notification", notificationType=ntype), "--claim-sound", "desktop-x")
        assert out.returncode == 0 and out.stdout.strip() == ""
    conn = app_state.connect(tmp_path / "state.sqlite3")
    app_state.migrate(conn)
    page = nd.list_notices(conn, 0, 50, NOW)
    assert page["notices"] == [] and page["pending"] == []
    # A real elicitation still asks for input.
    out = run_intake(tmp_path, hook("Notification", notificationType="elicitation_dialog"), "--claim-sound", "desktop-x")
    assert out.stdout.strip() == "play"
