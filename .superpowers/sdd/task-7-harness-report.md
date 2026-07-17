# Task 7 Browser Harness Report

## Scope

Implemented and corrected `tests/e2e_mobile_remote.js` only. No production
files were changed. The harness was not run against Chrome, any service, or a
tmux server during this correction.

## Safety And Review Corrections

- `new-session` is marked attempted before invocation, creates a 256-bit
  random `COMANDOS_E2E_OWNER_NONCE` session environment variable, and requests
  exact `$session_id|@window_id|%pane_id` output with `-P -F`.
- Cleanup probes the exact `=session` name and environment nonce. It kills only
  when that nonce matches and, once recorded, the exact session ID matches.
  Thus a timed-out creation can be cleaned up, while a pre-existing name
  collision is reported and never killed.
- `/tab-register` is marked attempted immediately before its awaited request.
  Finalization therefore calls `/tab-close` after a registration timeout when,
  and only when, the tmux nonce/session probe still proves ownership.
- All controller send helpers target only the recorded primary `%pane` and
  re-probe session/pane ownership immediately before sending. Split, pane kill,
  UI/API mouse-mode mutation triggers, mouse-option restoration, `/tab-close`,
  and session cleanup use the same ownership boundary immediately before the
  mutation or restore.
- Clipboard manual submit uses a fixed-length raw reader. Cancel, Escape, and
  empty clipboard each use a bounded raw tty probe that must report exactly
  zero bytes; focus is checked after every path.
- Every connection metric resolves the current active iframe and compares it to
  the original with `isSameNode`. Height and theme stability checks require a
  valid baseline: positive identity, at least one iframe load, and exactly one
  frame and page WebSocket.
- Selection first returns xterm to its live bottom, writes the marker, and
  requires the marker in xterm accessibility rows before the Select drag.
- Theme transitions poll both dashboard state and the active terminal frame's
  propagated CSS variables before collecting colors or connection metrics.
- Canvas diagnostics now retain pixel-read errors and require sampled
  non-background foreground pixels, rather than merely two colors. The tab
  locator is scoped to `.apptab .lbl`, and all CDP touch contacts end or cancel
  from `finally` after an error.

## Acceptance Coverage

- Matrix: touch phones `320x568` and `390x844`; coarse-pointer tablets
  `834x1112` portrait and `1194x834` landscape. Every view checks one active
  custom iframe, positive geometry, tab/content separation, split policy,
  unclipped toolbar controls, 40px toolbar buttons, 44px tabs, and foreground
  canvas glyph evidence. Screenshots and geometry remain under
  `/tmp/comandos-v160-e2e/` only during a live invocation.
- Functional checks cover height-only input animation, toolbar bytes,
  successful/denied/manual clipboard paths, zero-byte negative clipboard
  paths, toolbar pan, xterm scroll, rendered selection/copy, disposable pane
  resize, and the Bruno/Day/Bruno theme cycle.
- The token remains restricted to the initial dashboard URL and authenticated
  `X-Comandos-Token` API headers. Diagnostics redact raw and URL-encoded forms.

## Offline Verification Performed

No command below launches Chrome, contacts a service, or invokes tmux.

```text
$ node --check tests/e2e_mobile_remote.js
(exit 0, no output)

$ node <<'NODE'
const fs = require("node:fs");
const cp = require("node:child_process");
const blocked = ["appendFileSync", "chmodSync", "copyFileSync", "cpSync", "mkdirSync", "renameSync", "rmSync", "rmdirSync", "unlinkSync", "writeFileSync"];
for (const name of blocked) fs[name] = () => { throw new Error(`filesystem mutation: ${name}`); };
cp.spawnSync = () => { throw new Error("subprocess invocation"); };
global.fetch = () => { throw new Error("network request"); };
const stdout = process.stdout.write;
const stderr = process.stderr.write;
let output = "";
process.stdout.write = process.stderr.write = () => { output += "output"; return true; };
const harness = require("./tests/e2e_mobile_remote.js");
process.stdout.write = stdout;
process.stderr.write = stderr;
if (output || !harness.TmuxController || !harness.MATRIX) throw new Error("import side effect");
stdout.call(process.stdout, "offline import safety: ok\n");
NODE
offline import safety: ok

$ node <<'NODE'
const fs = require("node:fs");
const s = fs.readFileSync("tests/e2e_mobile_remote.js", "utf8");
const need = (ok, label) => { if (!ok) throw new Error(label); };
need(!s.includes("kill-server"), "kill-server forbidden");
need(s.indexOf("this.creationAttempted = true;") < s.indexOf('"new-session"'), "creation attempt must be marked first");
need(s.includes("crypto.randomBytes(32)") && s.includes("COMANDOS_E2E_OWNER_NONCE=") && s.includes('"-P", "-F", "#{session_id}|#{window_id}|#{pane_id}"'), "nonce or exact creation targets missing");
need(s.includes("ownershipProbe()") && s.includes("nonce === this.nonce") && s.includes("exactSession"), "ownership probe missing");
need(s.includes("refusing to kill a session whose ownership is not proven") && s.includes("assertThat(probe.owned"), "unowned cleanup guard missing");
need(s.indexOf("registrationAttempted = true;") < s.indexOf('api.post("/tab-register"') && s.includes("if (registrationAttempted)"), "uncertain tab registration cleanup missing");
need(!s.includes("sendKeysLiteral(text, target") && !s.includes("sendKey(key, target") && s.includes("sendKeysLiteral(text) {\n    this.assertOwnedPrimaryPane();") && s.includes("sendKey(key) {\n    this.assertOwnedPrimaryPane();"), "owned send boundary missing");
for (const view of ["320x568", "390x844", "834x1112", "1194x834"]) need(s.includes(view), `viewport ${view} missing`);
need(s.includes('"X-Comandos-Token": this.token') && s.includes("redactSecrets"), "authenticated header or redaction missing");
need(s.includes("zeroByteReader") && s.includes("CLIPCANCEL") && s.includes("CLIPEscape") && s.includes("CLIPEMPTY"), "negative clipboard tty probes missing");
need(s.includes("fixedByteReader(manualLabel, Buffer.byteLength(manualText))"), "manual fixed byte reader missing");
need(s.includes("original.isSameNode(current)") && s.includes("validFrameBaseline"), "iframe identity baseline missing");
need(s.includes("waitForThemePropagation") && s.includes("activeTerminalFrame(opened)"), "theme propagation polling missing");
need(s.includes("foregroundPixels") && s.includes("canvas has no foreground glyph"), "canvas glyph evidence missing");
need(s.includes("if (contactActive)") && s.includes('"touchCancel"'), "touch finally cleanup missing");
need(s.includes("xterm return to live bottom before selection") && s.includes("selection marker rendered in xterm accessibility rows"), "selection baseline or marker visibility missing");
need(s.includes(".apptab .lbl"), "terminal tab-specific locator missing");
need(s.includes("tmux.assertOwnedSession();\n    await button.click();") && s.includes("tmux.assertOwnedSession();\n        await api.post(\"/tmux-mouse\""), "mouse mutation ownership checks missing");
process.stdout.write("offline structural safety: ok\n");
NODE
offline structural safety: ok

$ node <<'NODE'
const {HarnessError, TmuxController} = require("./tests/e2e_mobile_remote.js");
function uncertainController(nonce, matchingNonce) {
  const controller = new TmuxController("comandos-e2e-999999", nonce);
  let live = true;
  let kills = 0;
  controller.run = args => {
    if (args[0] === "new-session") throw new HarnessError("simulated timeout");
    if (args[0] === "has-session") return {status: live ? 0 : 1, stdout: "", stderr: ""};
    if (args[0] === "display-message") return {status: 0, stdout: "$77\n", stderr: ""};
    if (args[0] === "show-environment") return matchingNonce ? {status: 0, stdout: `COMANDOS_E2E_OWNER_NONCE=${nonce}\n`, stderr: ""} : {status: 1, stdout: "", stderr: ""};
    if (args[0] === "kill-session") { kills += 1; live = false; return {status: 0, stdout: "", stderr: ""}; }
    throw new Error(`unexpected mock tmux command: ${args[0]}`);
  };
  return {controller, kills: () => kills};
}
const owned = uncertainController("owned-nonce", true);
try { owned.controller.create(); } catch (error) { if (!(error instanceof HarnessError)) throw error; }
if (!owned.controller.creationAttempted || !owned.controller.killSession().killed || owned.kills() !== 1) throw new Error("uncertain owned create was not cleaned up");
const collision = uncertainController("different-nonce", false);
try { collision.controller.create(); } catch (error) { if (!(error instanceof HarnessError)) throw error; }
let rejected = false;
try { collision.controller.killSession(); } catch (error) { rejected = error instanceof HarnessError; }
if (!rejected || collision.kills() !== 0) throw new Error("pre-existing collision was not protected");
process.stdout.write("offline uncertain-create cleanup safety: ok\n");
NODE
offline uncertain-create cleanup safety: ok

$ git diff --check
(exit 0, no output)
```

## Live Execution Status

Not run by design. A later live operator must perform the brief's inventory,
service, browser, screenshot, and release gates. This harness never restarts a
tmux server and its cleanup refuses any session whose ownership is not proven.
