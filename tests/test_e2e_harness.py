#!/usr/bin/env python3
import subprocess
import textwrap
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
HARNESS = ROOT / "tests" / "e2e_mobile_remote.js"


def run_node(source):
    return subprocess.run(
        ["node", "-e", textwrap.dedent(source)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=True,
    ).stdout.strip()


def test_harness_import_has_no_io_process_network_or_output_side_effects():
    output = run_node(
        r"""
        const fs = require("node:fs");
        const childProcess = require("node:child_process");
        for (const name of [
          "appendFileSync", "chmodSync", "copyFileSync", "cpSync", "mkdirSync",
          "renameSync", "rmSync", "rmdirSync", "unlinkSync", "writeFileSync",
        ]) fs[name] = () => { throw new Error(`filesystem mutation: ${name}`); };
        childProcess.spawnSync = () => { throw new Error("subprocess invocation"); };
        global.fetch = () => { throw new Error("network request"); };
        const stdout = process.stdout.write;
        const stderr = process.stderr.write;
        let output = "";
        process.stdout.write = process.stderr.write = () => { output += "unexpected"; return true; };
        const harness = require("./tests/e2e_mobile_remote.js");
        process.stdout.write = stdout;
        process.stderr.write = stderr;
        if (output || typeof harness.loadPlaywright !== "function") throw new Error("import side effect");
        console.log("import-safe");
        """
    )
    assert output == "import-safe"


def test_harness_paste_marker_and_registration_helpers():
    output = run_node(
        r"""
        const h = require("./tests/e2e_mobile_remote.js");
        const equal = (actual, expected, label) => {
          if (JSON.stringify(actual) !== JSON.stringify(expected)) {
            throw new Error(`${label}: ${JSON.stringify({actual, expected})}`);
          }
        };
        equal(h.normalizeXtermPaste("one\ntwo\r\nthree\rfour"),
          "one\rtwo\rthree\rfour", "xterm paste normalization");
        const marker = h.locateVisibleMarker(
          ["old", "", "abcKNOWN", "prompt"], "KNOWN", 2, 2, 20);
        equal(marker, {bufferRow: 2, visibleRow: 0, column: 3, length: 5},
          "visible marker location");
        equal(h.markerCanvasRegion(marker, 20, 4, 200, 80),
          {left: 30, top: 0, right: 80, bottom: 20, width: 50, height: 20},
          "marker canvas region");
        if (h.locateVisibleMarker(["KNOWN"], "KNOWN", 1, 1, 20) !== null) {
          throw new Error("offscreen marker accepted");
        }
        equal(h.registrationStateAfterError(new h.HarnessError("missing", {status: 404})),
          "rejected", "rejected registration");
        equal(h.registrationStateAfterError(new h.HarnessError("server", {status: 500})),
          "uncertain", "uncertain registration");
        equal(h.registrationCleanupDecision("confirmed", null), "close", "confirmed cleanup");
        equal(h.registrationCleanupDecision("uncertain", {owned: true}), "close", "owned uncertain cleanup");
        equal(h.registrationCleanupDecision("uncertain", {owned: false}), "blocked", "unowned uncertain cleanup");
        equal(h.registrationCleanupDecision("rejected", {owned: true}), "skip", "rejected cleanup");
        equal(h.readerHexFromCapture("line\nEMPTY-HEX:\n", "EMPTY"), "", "empty tty result");
        equal(h.readerHexFromCapture("line\nFULL-HEX:0d0a\n", "FULL"), "0d0a", "nonempty tty result");
        equal(h.readerHexFromCapture("line without marker", "EMPTY"), null, "missing tty result");
        equal(h.parsePaneScrollState("0|"), {inMode: false, scrollPosition: null},
          "pane outside copy mode");
        equal(h.parsePaneScrollState("1|17"), {inMode: true, scrollPosition: 17},
          "pane copy-mode scroll position");
        console.log("helpers-ok");
        """
    )
    assert output == "helpers-ok"


def test_playwright_loader_prefers_local_then_uses_explicit_global_root():
    output = run_node(
        r"""
        const h = require("./tests/e2e_mobile_remote.js");
        const local = {chromium: {source: "local"}};
        const localLoaded = h.loadPlaywright({
          requireFn(specifier) {
            if (specifier !== "playwright") throw new Error(`unexpected require ${specifier}`);
            return local;
          },
          spawnFn() { throw new Error("npm discovery ran for local Playwright"); },
        });
        if (localLoaded !== local) throw new Error("local Playwright was not preferred");

        const global = {chromium: {source: "global"}};
        const requires = [];
        const globalLoaded = h.loadPlaywright({
          requireFn(specifier) {
            requires.push(specifier);
            if (specifier === "playwright") throw new Error("not local");
            if (specifier === "/opt/npm-global/playwright") return global;
            throw new Error(`unexpected global require ${specifier}`);
          },
          spawnFn(command, args) {
            if (!command.startsWith("npm") || JSON.stringify(args) !== JSON.stringify(["root", "-g"])) {
              throw new Error("unexpected npm discovery command");
            }
            return {status: 0, stdout: "/opt/npm-global\n", stderr: ""};
          },
        });
        if (globalLoaded !== global || requires.length !== 2) throw new Error("global Playwright was not loaded");
        console.log("loader-ok");
        """
    )
    assert output == "loader-ok"


def test_readers_enter_raw_before_ready_and_capture_joins_wrapped_lines():
    output = run_node(
        r"""
        const h = require("./tests/e2e_mobile_remote.js");
        const decode = command => {
          const match = command.match(/b64decode\('([^']+)'\)/);
          if (!match) throw new Error("encoded Python reader is missing");
          return Buffer.from(match[1], "base64").toString("utf8");
        };
        for (const [name, command] of [
          ["fixed", h.fixedByteReader("FIXED", 3)],
          ["enter", h.enterTerminatedReader("ENTER")],
          ["zero", h.zeroByteReader("ZERO", 3000)],
        ]) {
          const source = decode(command);
          const raw = source.indexOf("tty.setraw(fd)");
          const ready = source.indexOf(`${name === "fixed" ? "FIXED" : name === "enter" ? "ENTER" : "ZERO"}-READY`);
          if (raw < 0 || ready < 0 || raw > ready) throw new Error(`${name} reader announces READY before raw mode`);
          if (!source.includes("termios.tcsetattr")) throw new Error(`${name} reader does not restore tty state`);
        }
        const zero = decode(h.zeroByteReader("ZERO", 3000));
        if (!zero.includes("deadline = time.monotonic() + window_seconds") ||
            !zero.includes("select.select([fd], [], [], remaining)")) {
          throw new Error("zero-byte reader is not absolutely bounded");
        }
        const capture = h.TmuxController.prototype.capture.toString();
        if (!capture.includes('"-J"')) throw new Error("capture-pane does not join wrapped lines");
        console.log("readers-ok");
        """
    )
    assert output == "readers-ok"


def test_iframe_load_instrumentation_observes_inserted_frames_directly():
    source = HARNESS.read_text()
    instrumentation = source.split("function instrumentationScript()", 1)[1].split(
        "async function createTouchContext", 1
    )[0]
    assert "new MutationObserver" in instrumentation
    assert 'frame.addEventListener("load"' in instrumentation
    assert "watchedFrames.has(frame)" in instrumentation
    assert 'window.addEventListener("load"' not in instrumentation


def test_terminal_scroll_uses_tmux_history_and_restores_live_mode():
    source = HARNESS.read_text()
    terminal_scroll = source.split("async function runTerminalScroll", 1)[1].split(
        "async function interactionState", 1
    )[0]
    marker_wait = 'waitForMarkerRender(opened.frame, "E2E-SCROLL-200"'
    assert marker_wait in terminal_scroll
    assert "tmux.paneScrollState()" in terminal_scroll
    assert "scrollPosition > 0" in terminal_scroll
    assert "tmux.cancelCopyMode()" in terminal_scroll
    assert "scrollHeight" not in terminal_scroll


def test_pane_resize_uses_exact_created_pane_id_not_global_base_index():
    source = HARNESS.read_text()
    resize = source.split("async function resizeOneAxis", 1)[1].split(
        "async function runPaneResize", 1
    )[0]
    assert "tmux.targets.primaryPane" in resize
    assert "pane.paneIndex === 0" not in resize


def test_mobile_theme_cycle_uses_visible_panel_control_and_returns_to_terminal():
    source = HARNESS.read_text()
    visible_click = source.split("async function clickVisibleThemeControl", 1)[1].split(
        "async function clickThemeUntil", 1
    )[0]
    cycle = source.split("async function clickThemeUntil", 1)[1].split(
        "async function themeColors", 1
    )[0]
    assert "button.isVisible()" in visible_click
    assert "⌂ Panel" in visible_click
    assert "opened.session" in visible_click
    assert "force:" not in visible_click
    assert "clickVisibleThemeControl(opened)" in cycle


def test_uncertain_tmux_creation_kills_only_nonce_owned_session():
    output = run_node(
        r"""
        const {HarnessError, TmuxController} = require("./tests/e2e_mobile_remote.js");
        function controllerFor(nonce, matchingNonce) {
          const controller = new TmuxController("comandos-e2e-999999", nonce);
          let live = true;
          let kills = 0;
          controller.run = args => {
            if (args[0] === "new-session") throw new HarnessError("simulated timeout");
            if (args[0] === "has-session") return {status: live ? 0 : 1, stdout: "", stderr: ""};
            if (args[0] === "list-sessions") return {
              status: 0,
              stdout: "user-session|$4\ncomandos-e2e-999999|$77\n",
              stderr: "",
            };
            if (args[0] === "show-environment") return matchingNonce
              ? {status: 0, stdout: `COMANDOS_E2E_OWNER_NONCE=${nonce}\n`, stderr: ""}
              : {status: 1, stdout: "", stderr: ""};
            if (args[0] === "kill-session") {
              if (args[2] !== "=comandos-e2e-999999") throw new Error("uncertain cleanup lost exact name target");
              kills += 1;
              live = false;
              return {status: 0, stdout: "", stderr: ""};
            }
            throw new Error(`unexpected tmux command ${args[0]}`);
          };
          return {controller, kills: () => kills};
        }
        const owned = controllerFor("owned", true);
        try { owned.controller.create(); } catch (error) { if (!(error instanceof HarnessError)) throw error; }
        if (!owned.controller.killSession().killed || owned.kills() !== 1) throw new Error("owned uncertain create leaked");
        const collision = controllerFor("other", false);
        try { collision.controller.create(); } catch (error) { if (!(error instanceof HarnessError)) throw error; }
        let refused = false;
        try { collision.controller.killSession(); } catch (error) { refused = error instanceof HarnessError; }
        if (!refused || collision.kills() !== 0) throw new Error("colliding session was killed");
        console.log("ownership-ok");
        """
    )
    assert output == "ownership-ok"
