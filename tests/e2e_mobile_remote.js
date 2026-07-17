#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");
const {spawnSync} = require("node:child_process");

const CHROME = "/usr/bin/google-chrome";
const OUTPUT_DIR = "/tmp/comandos-v160-e2e";
const POLL_TIMEOUT_MS = 10_000;
const API_TIMEOUT_MS = 7_000;
const ZERO_READER_WINDOW_MS = 3_000;
const ZERO_READER_MIN_TAIL_MS = 500;
const INITIAL_MARKER = "COMANDOS-E2E-READY";
const HEIGHT_TEXT = "abcdefghijklmno";
const HEIGHT_MARKER = `mobile-input-${HEIGHT_TEXT}`;
const MATRIX = Object.freeze([
  {name: "phone-320x568", width: 320, height: 568, split: false},
  {name: "phone-390x844", width: 390, height: 844, split: false},
  {name: "tablet-834x1112", width: 834, height: 1112, split: false},
  {name: "tablet-1194x834", width: 1194, height: 834, split: true},
]);

class HarnessError extends Error {
  constructor(label, details) {
    const suffix = details === undefined ? "" : `: ${diagnostic(details)}`;
    super(`${label}${suffix}`);
    this.name = "HarnessError";
    this.details = details;
  }
}

function diagnostic(value) {
  if (typeof value === "string") return value;
  try {
    return JSON.stringify(value);
  } catch (_) {
    return String(value);
  }
}

function assertThat(condition, label, details) {
  if (!condition) throw new HarnessError(label, details);
}

function sessionNameFor(pid) {
  assertThat(Number.isSafeInteger(pid) && pid > 0, "invalid process id", {pid});
  return `comandos-e2e-${pid}`;
}

function tmuxTargetsFor(session) {
  assertThat(/^comandos-e2e-[1-9][0-9]*$/.test(session), "unsafe tmux session name", {session});
  return Object.freeze({
    session: `=${session}`,
    window: `=${session}:0`,
    primaryPane: `=${session}:0.0`,
  });
}

function ownershipNonce() {
  return crypto.randomBytes(32).toString("hex");
}

function normalizeXtermPaste(text) {
  return String(text).replace(/\r?\n/g, "\r");
}

function locateVisibleMarker(lines, marker, viewportY, rows, cols) {
  if (!Array.isArray(lines) || typeof marker !== "string" || marker.length === 0 ||
      !Number.isInteger(viewportY) || !Number.isInteger(rows) || !Number.isInteger(cols) ||
      viewportY < 0 || rows <= 0 || cols <= 0) return null;
  const first = Math.min(viewportY, lines.length);
  const last = Math.min(lines.length, viewportY + rows);
  for (let bufferRow = last - 1; bufferRow >= first; bufferRow -= 1) {
    const line = typeof lines[bufferRow] === "string" ? lines[bufferRow] : "";
    const column = line.indexOf(marker);
    if (column >= 0 && column + marker.length <= cols) {
      return {
        bufferRow,
        visibleRow: bufferRow - viewportY,
        column,
        length: marker.length,
      };
    }
  }
  return null;
}

function markerCanvasRegion(location, cols, rows, width, height) {
  if (!location || !Number.isInteger(cols) || !Number.isInteger(rows) ||
      !Number.isFinite(width) || !Number.isFinite(height) || cols <= 0 || rows <= 0 ||
      width <= 0 || height <= 0) return null;
  const left = Math.floor(location.column * width / cols);
  const top = Math.floor(location.visibleRow * height / rows);
  const right = Math.ceil((location.column + location.length) * width / cols);
  const bottom = Math.ceil((location.visibleRow + 1) * height / rows);
  if (left < 0 || top < 0 || right > width || bottom > height || right <= left || bottom <= top) {
    return null;
  }
  return {left, top, right, bottom, width: right - left, height: bottom - top};
}

function registrationStateAfterError(error) {
  const status = error?.details?.status;
  return Number.isInteger(status) && status >= 400 && status < 500 ? "rejected" : "uncertain";
}

function registrationCleanupDecision(state, ownership) {
  assertThat(["not-attempted", "uncertain", "confirmed", "rejected"].includes(state),
    "invalid registration state", {state});
  if (state === "confirmed") return "close";
  if (state === "uncertain") return ownership?.owned ? "close" : "blocked";
  return "skip";
}

function loadPlaywright(options = {}) {
  const requireFn = options.requireFn || require;
  const spawnFn = options.spawnFn || spawnSync;
  let localError;
  try {
    return requireFn("playwright");
  } catch (error) {
    localError = error;
  }
  const npmCommand = options.npmCommand || (process.platform === "win32" ? "npm.cmd" : "npm");
  const npmRoot = spawnFn(npmCommand, ["root", "-g"], {
    encoding: "utf8",
    timeout: 5_000,
    maxBuffer: 1024 * 1024,
  });
  if (npmRoot.error || npmRoot.status !== 0) {
    throw new HarnessError("Playwright resolution failed", {
      local: localError?.message || String(localError),
      npmCommand,
      npmRootStatus: npmRoot.status,
      npmRootError: npmRoot.error?.message || "",
      npmRootStderr: (npmRoot.stderr || "").trim(),
    });
  }
  const root = (npmRoot.stdout || "").trim();
  assertThat(path.isAbsolute(root), "global npm root is not an absolute path", {npmCommand, root});
  const globalPlaywright = path.join(root, "playwright");
  try {
    return requireFn(globalPlaywright);
  } catch (error) {
    throw new HarnessError("Playwright is unavailable from local and global npm resolution", {
      local: localError?.message || String(localError),
      npmCommand,
      globalPlaywright,
      global: error?.message || String(error),
    });
  }
}

function normalizeBase(raw) {
  assertThat(typeof raw === "string" && raw.length > 0, "CC_REMOTE_BASE is required");
  const parsed = new URL(raw);
  assertThat(["http:", "https:"].includes(parsed.protocol), "CC_REMOTE_BASE must use HTTP(S)", {
    protocol: parsed.protocol,
  });
  assertThat(!parsed.username && !parsed.password, "CC_REMOTE_BASE must not contain credentials");
  assertThat(!parsed.search && !parsed.hash, "CC_REMOTE_BASE must not contain a query or fragment");
  parsed.pathname = parsed.pathname.replace(/\/+$/, "");
  return parsed.toString().replace(/\/$/, "");
}

function redactSecrets(value, secrets) {
  let text = value instanceof Error ? (value.stack || value.message) : String(value);
  for (const secret of secrets.filter(Boolean)) {
    text = text.split(secret).join("[REDACTED]");
    try {
      text = text.split(encodeURIComponent(secret)).join("[REDACTED]");
    } catch (_) {}
  }
  return text;
}

function delay(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

async function poll(label, predicate, options = {}) {
  const timeoutMs = options.timeoutMs ?? POLL_TIMEOUT_MS;
  const intervalMs = options.intervalMs ?? 50;
  const deadline = Date.now() + timeoutMs;
  let attempts = 0;
  let lastValue;
  let lastError;
  while (Date.now() <= deadline) {
    attempts += 1;
    try {
      lastValue = await predicate();
      if (lastValue) return lastValue;
      lastError = undefined;
    } catch (error) {
      lastError = error;
    }
    const remaining = deadline - Date.now();
    if (remaining > 0) await delay(Math.min(intervalMs, remaining));
  }
  throw new HarnessError(`timed out waiting for ${label}`, {
    timeoutMs,
    attempts,
    lastValue,
    lastError: lastError && (lastError.message || String(lastError)),
  });
}

class TmuxController {
  constructor(session, nonce = ownershipNonce()) {
    this.session = session;
    this.targets = tmuxTargetsFor(session);
    this.nonce = nonce;
    this.creationAttempted = false;
    this.created = false;
    this.mouseSnapshot = null;
  }

  run(args, options = {}) {
    const result = spawnSync("tmux", args, {
      encoding: "utf8",
      timeout: options.timeoutMs ?? 5_000,
      maxBuffer: 4 * 1024 * 1024,
    });
    if (result.error) {
      throw new HarnessError("tmux invocation failed", {
        args,
        error: result.error.message,
      });
    }
    if (result.status !== 0 && !options.allowFailure) {
      throw new HarnessError("tmux command failed", {
        args,
        status: result.status,
        stderr: (result.stderr || "").trim(),
      });
    }
    return {
      status: result.status,
      stdout: result.stdout || "",
      stderr: result.stderr || "",
    };
  }

  create() {
    this.creationAttempted = true;
    const created = this.run([
      "new-session", "-d", "-P", "-F", "#{session_id}|#{window_id}|#{pane_id}",
      "-e", `COMANDOS_E2E_OWNER_NONCE=${this.nonce}`,
      "-s", this.session, "-x", "100", "-y", "30", "-n", "e2e", "-c", "/tmp",
      "/bin/bash --noprofile --norc",
    ]).stdout.trim();
    const [sessionId, window, primaryPane] = created.split("|");
    assertThat(/^\$\d+$/.test(sessionId) && /^@\d+$/.test(window) && /^%\d+$/.test(primaryPane),
      "tmux new-session did not return exact disposable targets", {created});
    this.targets = Object.freeze({session: this.targets.session, sessionId, window, primaryPane});
    this.assertOwnedSession();
    this.assertOwnedPrimaryPane();
    this.created = true;
    const sessionTarget = this.sessionTarget();
    const effective = this.run([
      "show-options", "-A", "-v", "-t", sessionTarget, "mouse",
    ]).stdout.trim();
    const local = this.run([
      "show-options", "-q", "-v", "-t", sessionTarget, "mouse",
    ], {allowFailure: true}).stdout.trim();
    assertThat(["on", "off"].includes(effective), "unexpected tmux mouse value", {effective});
    this.mouseSnapshot = {effective, hadLocal: local === "on" || local === "off", local};
  }

  sessionTarget() {
    return this.targets.sessionId || this.targets.session;
  }

  exists(target = this.sessionTarget()) {
    return this.run(["has-session", "-t", target], {allowFailure: true}).status === 0;
  }

  ownershipProbe() {
    if (!this.creationAttempted || !this.exists(this.targets.session)) {
      return {owned: false, exists: false};
    }
    const sessionId = this.run([
      "display-message", "-p", "-t", this.targets.session, "#{session_id}",
    ], {allowFailure: true}).stdout.trim();
    const environment = this.run([
      "show-environment", "-t", this.targets.session, "COMANDOS_E2E_OWNER_NONCE",
    ], {allowFailure: true});
    const nonce = environment.status === 0
      ? environment.stdout.trim().slice("COMANDOS_E2E_OWNER_NONCE=".length)
      : "";
    const exactSession = !this.targets.sessionId || sessionId === this.targets.sessionId;
    return {
      owned: environment.status === 0 && nonce === this.nonce && exactSession,
      exists: true,
      sessionId,
      nonceMatches: nonce === this.nonce,
      exactSession,
    };
  }

  assertOwnedSession() {
    const probe = this.ownershipProbe();
    assertThat(probe.owned, "refusing to operate on an unowned disposable session", {
      session: this.session,
      target: this.targets.session,
      probe,
    });
  }

  assertOwnedPrimaryPane() {
    this.assertOwnedSession();
    assertThat(/^%\d+$/.test(this.targets.primaryPane), "missing exact primary pane target", {
      primaryPane: this.targets.primaryPane,
    });
    const actual = this.run([
      "display-message", "-p", "-t", this.targets.primaryPane, "#{session_id}|#{pane_id}",
    ]).stdout.trim();
    assertThat(actual === `${this.targets.sessionId}|${this.targets.primaryPane}`,
      "refusing to operate on a non-owned primary pane", {
        expected: `${this.targets.sessionId}|${this.targets.primaryPane}`,
        actual,
      });
  }

  capture(lines = 240) {
    this.assertOwnedPrimaryPane();
    return this.run([
      "capture-pane", "-p", "-J", "-t", this.targets.primaryPane, "-S", `-${lines}`,
    ]).stdout;
  }

  sendKeysLiteral(text) {
    this.assertOwnedPrimaryPane();
    this.run(["send-keys", "-t", this.targets.primaryPane, "-l", text]);
  }

  sendKey(key) {
    this.assertOwnedPrimaryPane();
    this.run(["send-keys", "-t", this.targets.primaryPane, key]);
  }

  sendShell(command) {
    this.sendKeysLiteral(command);
    this.sendKey("Enter");
  }

  panes() {
    this.assertOwnedSession();
    const output = this.run([
      "list-panes", "-t", this.targets.window,
      "-F", "#{pane_id}|#{session_name}|#{window_index}|#{pane_index}|#{pane_left}|#{pane_top}|#{pane_width}|#{pane_height}|#{pane_current_command}",
    ]).stdout.trim();
    if (!output) return [];
    return output.split("\n").map(line => {
      const [id, session, windowIndex, paneIndex, left, top, width, height, command] = line.split("|");
      return {
        id,
        session,
        windowIndex: Number(windowIndex),
        paneIndex: Number(paneIndex),
        left: Number(left),
        top: Number(top),
        width: Number(width),
        height: Number(height),
        command,
      };
    });
  }

  windowSize() {
    this.assertOwnedSession();
    const output = this.run([
      "display-message", "-p", "-t", this.targets.window,
      "#{window_width}|#{window_height}",
    ]).stdout.trim();
    const [width, height] = output.split("|").map(Number);
    assertThat(width > 0 && height > 0, "invalid disposable tmux window geometry", {output});
    return {width, height};
  }

  clientCount() {
    this.assertOwnedSession();
    const result = this.run(["list-clients", "-F", "#{client_session}"], {allowFailure: true});
    if (result.status !== 0) return 0;
    return result.stdout.split("\n").filter(name => name === this.session).length;
  }

  split(flag) {
    assertThat(flag === "-h" || flag === "-v", "invalid split orientation", {flag});
    this.assertOwnedPrimaryPane();
    const paneId = this.run([
      "split-window", flag, "-d", "-P", "-F", "#{pane_id}",
      "-t", this.targets.primaryPane, "-c", "/tmp",
      "/bin/bash --noprofile --norc",
    ]).stdout.trim();
    assertThat(/^%\d+$/.test(paneId), "tmux did not return a pane id", {flag, paneId});
    this.assertOwnedPane(paneId);
    return paneId;
  }

  assertOwnedPane(paneId) {
    assertThat(/^%\d+$/.test(paneId), "invalid pane id", {paneId});
    this.assertOwnedSession();
    const actual = this.run([
      "display-message", "-p", "-t", paneId, "#{session_id}",
    ]).stdout.trim();
    assertThat(actual === this.targets.sessionId, "refusing to operate on a non-disposable pane", {
      paneId,
      expectedSessionId: this.targets.sessionId,
      actualSession: actual,
    });
  }

  killPane(paneId) {
    this.assertOwnedPane(paneId);
    this.run(["kill-pane", "-t", paneId]);
  }

  restoreMouseLocal() {
    if (!this.created || !this.mouseSnapshot) return;
    this.assertOwnedSession();
    const sessionTarget = this.sessionTarget();
    if (this.mouseSnapshot.hadLocal) {
      this.run(["set-option", "-t", sessionTarget, "mouse", this.mouseSnapshot.local]);
    } else {
      this.run(["set-option", "-q", "-u", "-t", sessionTarget, "mouse"]);
    }
  }

  killSession() {
    const probe = this.ownershipProbe();
    if (!probe.exists) return {killed: false, probe};
    assertThat(probe.owned, "refusing to kill a session whose ownership is not proven", {
      session: this.session,
      target: this.targets.session,
      probe,
    });
    const sessionTarget = this.sessionTarget();
    this.run(["kill-session", "-t", sessionTarget]);
    assertThat(!this.exists(sessionTarget), "exact disposable tmux session still exists after cleanup", {
      target: sessionTarget,
      probe,
    });
    return {killed: true, probe};
  }
}

class ApiClient {
  constructor(base, token) {
    this.base = base;
    this.token = token;
  }

  async request(route, body) {
    assertThat(route.startsWith("/"), "API route must be absolute", {route});
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), API_TIMEOUT_MS);
    const headers = {"X-Comandos-Token": this.token};
    const options = {headers, signal: controller.signal, cache: "no-store"};
    if (body !== undefined) {
      options.method = "POST";
      headers["Content-Type"] = "application/json";
      options.body = JSON.stringify(body);
    }
    try {
      const response = await fetch(`${this.base}${route}`, options);
      const payload = await response.json().catch(() => ({}));
      if (!response.ok) {
        throw new HarnessError(`authenticated API ${route} failed`, {
          status: response.status,
          error: payload.error || response.statusText,
        });
      }
      return payload;
    } finally {
      clearTimeout(timer);
    }
  }

  get(route) {
    return this.request(route);
  }

  post(route, body) {
    return this.request(route, body);
  }
}

function instrumentationScript() {
  return `(() => {
    if (window.__comandosE2EInstrumentation) return;
    const state = window.__comandosE2EInstrumentation = {
      webSockets: 0, socketMessages: 0, frameLoads: 0,
    };
    let terminalConstructor;
    Object.defineProperty(window, "Terminal", {
      configurable: true,
      get() { return terminalConstructor; },
      set(NativeTerminal) {
        if (typeof NativeTerminal !== "function") {
          terminalConstructor = NativeTerminal;
          return;
        }
        function InstrumentedTerminal(...args) {
          const instance = Reflect.construct(NativeTerminal, args, NativeTerminal);
          window.__comandosE2ETerminal = instance;
          return instance;
        }
        Object.setPrototypeOf(InstrumentedTerminal, NativeTerminal);
        InstrumentedTerminal.prototype = NativeTerminal.prototype;
        terminalConstructor = InstrumentedTerminal;
      },
    });
    if (window.top === window) {
      window.addEventListener("load", event => {
        if (event.target instanceof HTMLIFrameElement) state.frameLoads += 1;
      }, true);
    }
    const NativeWebSocket = window.WebSocket;
    function InstrumentedWebSocket(...args) {
      const socket = new NativeWebSocket(...args);
      state.webSockets += 1;
      socket.addEventListener("message", () => { state.socketMessages += 1; });
      return socket;
    }
    Object.setPrototypeOf(InstrumentedWebSocket, NativeWebSocket);
    InstrumentedWebSocket.prototype = NativeWebSocket.prototype;
    window.WebSocket = InstrumentedWebSocket;
  })();`;
}

async function createTouchContext(browser, view) {
  const context = await browser.newContext({
    viewport: {width: view.width, height: view.height},
    screen: {width: view.width, height: view.height},
    deviceScaleFactor: 1,
    hasTouch: true,
    isMobile: true,
    locale: "es-MX",
  });
  await context.addInitScript({content: instrumentationScript()});
  return context;
}

async function openTerminal(context, base, token, session) {
  const page = await context.newPage();
  const sockets = [];
  page.on("websocket", socket => sockets.push(socket.url()));
  const dashboardUrl = new URL(`${base}/`);
  dashboardUrl.searchParams.set("token", token);
  await page.goto(dashboardUrl.toString(), {waitUntil: "domcontentloaded", timeout: 15_000});
  await poll("dashboard token removal", () => !new URL(page.url()).searchParams.has("token"));
  const label = page.locator(".apptab .lbl", {hasText: session}).filter({hasText: session}).first();
  await label.waitFor({state: "visible", timeout: POLL_TIMEOUT_MS});
  assertThat(await label.textContent() === session, "terminal tab label did not resolve exactly", {session});
  await label.locator("xpath=..").click();

  const iframeLocator = page.locator("iframe.termpane.on");
  await poll("one active terminal iframe", async () => await iframeLocator.count() === 1);
  const iframeHandle = await iframeLocator.elementHandle();
  assertThat(iframeHandle, "active terminal iframe handle is missing");
  const frame = await iframeHandle.contentFrame();
  assertThat(frame, "active terminal iframe has no content frame");
  await frame.locator(".xterm-helper-textarea").waitFor({state: "attached", timeout: POLL_TIMEOUT_MS});
  await poll("custom terminal readiness", async () => frame.evaluate(() => {
    const error = document.getElementById("err");
    const screen = document.querySelector(".xterm-screen");
    const term = window.__comandosE2ETerminal;
    return !!screen && !!term?.buffer?.active && typeof term.buffer.active.getLine === "function" &&
      (!error || getComputedStyle(error).display === "none");
  }));

  const compatibility = await iframeLocator.getAttribute("data-compat");
  const frameUrl = new URL(frame.url());
  const expectedTerm = new URL(`${base}/term/`);
  assertThat(compatibility === "0", "compatibility terminal endpoint was used unexpectedly", {
    compatibility,
    frameOrigin: frameUrl.origin,
    framePath: frameUrl.pathname,
  });
  assertThat(frameUrl.origin === expectedTerm.origin && frameUrl.pathname.startsWith("/term/"),
    "custom /term iframe URL is unexpected", {
      frameOrigin: frameUrl.origin,
      framePath: frameUrl.pathname,
    });

  const identity = await page.evaluate(iframe => {
    const top = window;
    top.__e2eFrameIds ||= new WeakMap();
    top.__e2eFrameIdSequence ||= 0;
    if (!top.__e2eFrameIds.has(iframe)) {
      top.__e2eFrameIds.set(iframe, ++top.__e2eFrameIdSequence);
    }
    return top.__e2eFrameIds.get(iframe);
  }, iframeHandle);
  return {page, frame, iframeLocator, iframeHandle, sockets, identity};
}

async function activeTerminalFrame(opened) {
  const currentHandle = await opened.iframeLocator.elementHandle();
  assertThat(currentHandle, "current active terminal iframe handle is missing");
  const currentFrame = await currentHandle.contentFrame();
  assertThat(currentFrame, "current active terminal iframe has no content frame");
  return currentFrame;
}

async function frameMetrics(opened) {
  const currentFrame = await activeTerminalFrame(opened);
  const [loadCount, frameState, identityState] = await Promise.all([
    opened.page.evaluate(() => window.__comandosE2EInstrumentation?.frameLoads ?? -1),
    currentFrame.evaluate(() => ({
      webSockets: window.__comandosE2EInstrumentation?.webSockets ?? -1,
      socketMessages: window.__comandosE2EInstrumentation?.socketMessages ?? -1,
    })),
    opened.page.evaluate(original => {
      const current = document.querySelector("iframe.termpane.on");
      if (!current || !original) return {sameNode: false, identity: -1};
      const sameNode = original.isSameNode(current);
      window.__e2eFrameIds ||= new WeakMap();
      window.__e2eFrameIdSequence ||= 0;
      if (!window.__e2eFrameIds.has(current)) {
        window.__e2eFrameIds.set(current, ++window.__e2eFrameIdSequence);
      }
      return {sameNode, identity: window.__e2eFrameIds.get(current)};
    }, opened.iframeHandle),
  ]);
  return {
    identity: identityState.identity,
    sameNode: identityState.sameNode,
    loadCount,
    frameWebSockets: frameState.webSockets,
    socketMessages: frameState.socketMessages,
    pageWebSockets: opened.sockets.length,
  };
}

async function validFrameBaseline(opened, label) {
  return poll(`${label} valid iframe/socket baseline`, async () => {
    const metrics = await frameMetrics(opened);
    const valid = metrics.sameNode && metrics.identity > 0 && metrics.loadCount >= 1 &&
      metrics.frameWebSockets === 1 && metrics.pageWebSockets === 1;
    return valid ? metrics : false;
  });
}

function stableConnectionDiagnostics(before, after) {
  return {before, after};
}

function assertStableConnection(before, after, label) {
  assertThat(before.sameNode && before.identity > 0 && before.loadCount >= 1 &&
    before.frameWebSockets === 1 && before.pageWebSockets === 1,
  `${label}: invalid pre-change iframe/socket baseline`, before);
  assertThat(after.sameNode && after.identity > 0 && after.loadCount >= 1 &&
    after.frameWebSockets === 1 && after.pageWebSockets === 1,
  `${label}: invalid post-change iframe/socket baseline`, after);
  assertThat(after.identity === before.identity, `${label}: iframe node identity changed`,
    stableConnectionDiagnostics(before, after));
  assertThat(after.loadCount === before.loadCount, `${label}: iframe load count changed`,
    stableConnectionDiagnostics(before, after));
  assertThat(after.frameWebSockets === before.frameWebSockets,
    `${label}: iframe WebSocket creation count changed`, stableConnectionDiagnostics(before, after));
  assertThat(after.pageWebSockets === before.pageWebSockets,
    `${label}: page WebSocket creation count changed`, stableConnectionDiagnostics(before, after));
}

async function markerRenderEvidence(frame, marker) {
  return frame.evaluate(expected => {
    const term = window.__comandosE2ETerminal;
    if (!term?.buffer?.active || typeof term.buffer.active.getLine !== "function") {
      return {rendered: false, marker: expected, error: "real xterm public buffer is unavailable", canvases: []};
    }
    const buffer = term.buffer.active;
    const viewportY = buffer.viewportY;
    let location = null;
    for (let bufferRow = Math.min(buffer.length, viewportY + term.rows) - 1;
      bufferRow >= viewportY; bufferRow -= 1) {
      const line = buffer.getLine(bufferRow)?.translateToString(false) || "";
      const column = line.indexOf(expected);
      if (column >= 0 && column + expected.length <= term.cols) {
        location = {
          bufferRow,
          visibleRow: bufferRow - viewportY,
          column,
          length: expected.length,
        };
        break;
      }
    }
    if (!location) {
      return {
        rendered: false,
        marker: expected,
        error: "marker is not visible in the real xterm buffer",
        buffer: {length: buffer.length, viewportY, rows: term.rows, cols: term.cols},
        canvases: [],
      };
    }
    const rawBackground = String(term.options?.theme?.background ||
      getComputedStyle(document.documentElement).getPropertyValue("--term-bg") || "").trim();
    const hex = rawBackground.match(/^#([0-9a-f]{6})$/i);
    const background = hex ? [
      Number.parseInt(hex[1].slice(0, 2), 16),
      Number.parseInt(hex[1].slice(2, 4), 16),
      Number.parseInt(hex[1].slice(4, 6), 16),
    ] : null;
    const canvases = [...document.querySelectorAll(".xterm-screen canvas")].map(canvas => {
      const evidence = {
        width: canvas.width,
        height: canvas.height,
        region: null,
        sampled: 0,
        foregroundPixels: 0,
        distinctForeground: 0,
        error: "",
      };
      try {
        if (!background) throw new Error(`unsupported terminal background: ${rawBackground}`);
        const context = canvas.getContext("2d", {willReadFrequently: true});
        if (!context) throw new Error("2d canvas context is unavailable");
        if (!canvas.width || !canvas.height) throw new Error("canvas has zero dimensions");
        const left = Math.floor(location.column * canvas.width / term.cols);
        const top = Math.floor(location.visibleRow * canvas.height / term.rows);
        const right = Math.ceil((location.column + location.length) * canvas.width / term.cols);
        const bottom = Math.ceil((location.visibleRow + 1) * canvas.height / term.rows);
        if (left < 0 || top < 0 || right > canvas.width || bottom > canvas.height ||
            right <= left || bottom <= top) throw new Error("marker canvas region is invalid");
        evidence.region = {left, top, right, bottom, width: right - left, height: bottom - top};
        const pixels = context.getImageData(left, top, right - left, bottom - top).data;
        const colors = new Set();
        for (let index = 0; index < pixels.length; index += 4) {
          evidence.sampled += 1;
          const alpha = pixels[index + 3];
          const isBackground = pixels[index] === background[0] &&
            pixels[index + 1] === background[1] && pixels[index + 2] === background[2];
          if (alpha > 0 && !isBackground) {
            evidence.foregroundPixels += 1;
            colors.add(`${pixels[index]},${pixels[index + 1]},${pixels[index + 2]},${alpha}`);
          }
        }
        evidence.distinctForeground = colors.size;
      } catch (error) {
        evidence.error = error?.message || String(error);
      }
      return evidence;
    });
    const rendered = canvases.some(canvas => canvas.error === "" &&
      canvas.foregroundPixels >= Math.max(12, expected.length) && canvas.distinctForeground > 0);
    return {
      rendered,
      marker: expected,
      location,
      buffer: {length: buffer.length, viewportY, rows: term.rows, cols: term.cols},
      background: rawBackground,
      canvases,
      error: rendered ? "" : "marker region has no foreground glyph pixels",
    };
  }, marker);
}

async function waitForMarkerRender(frame, marker, label) {
  return poll(label, async () => {
    const evidence = await markerRenderEvidence(frame, marker);
    if (!evidence.rendered) throw new HarnessError(`${label}: marker render evidence unavailable`, evidence);
    return evidence;
  });
}

async function inspectGeometry(opened, view) {
  const markerEvidence = await waitForMarkerRender(opened.frame, INITIAL_MARKER,
    `${view.name} ${INITIAL_MARKER} canvas glyph region`);
  const parent = await opened.page.evaluate(() => {
    const rect = element => {
      const value = element.getBoundingClientRect();
      return {left: value.left, top: value.top, right: value.right, bottom: value.bottom,
        width: value.width, height: value.height};
    };
    const activeFrames = [...document.querySelectorAll("iframe.termpane.on")]
      .filter(frame => getComputedStyle(frame).display !== "none");
    const tabbar = document.getElementById("tabbar");
    const termArea = document.getElementById("term-area");
    return {
      viewport: {width: innerWidth, height: innerHeight},
      split: document.body.classList.contains("split"),
      body: rect(document.body),
      tabbar: rect(tabbar),
      terminal: rect(termArea),
      activeFrames: activeFrames.length,
      tabs: [...tabbar.querySelectorAll(".apptab")]
        .filter(tab => tab.getClientRects().length > 0)
        .map(tab => ({text: tab.textContent.trim(), ...rect(tab)})),
    };
  });

  const terminal = await opened.frame.evaluate(evidence => {
    const rect = element => {
      const value = element.getBoundingClientRect();
      return {left: value.left, top: value.top, right: value.right, bottom: value.bottom,
        width: value.width, height: value.height};
    };
    const toolbar = document.getElementById("term-toolbar");
    const term = document.getElementById("term");
    const screen = document.querySelector(".xterm-screen");
    const viewport = document.querySelector(".xterm-viewport");
    return {
      body: rect(document.body),
      term: rect(term),
      screen: rect(screen),
      viewport: rect(viewport),
      toolbar: {
        ...rect(toolbar),
        hidden: toolbar.hidden,
        clientWidth: toolbar.clientWidth,
        scrollWidth: toolbar.scrollWidth,
      },
      buttons: [...toolbar.querySelectorAll("button")].map(button => ({
        key: button.dataset.key || button.dataset.action,
        ...rect(button),
      })),
      markerEvidence: evidence,
      errorVisible: getComputedStyle(document.getElementById("err")).display !== "none",
    };
  }, markerEvidence);

  const result = {name: view.name, expected: view, parent, terminal};
  assertThat(parent.viewport.width === view.width && parent.viewport.height === view.height,
    `${view.name}: viewport mismatch`, result);
  assertThat(parent.body.width > 0 && parent.body.height > 0 &&
    parent.terminal.width > 0 && parent.terminal.height > 0,
  `${view.name}: body or terminal geometry is non-positive`, result);
  assertThat(parent.activeFrames === 1, `${view.name}: expected exactly one active iframe`, result);
  assertThat(parent.split === view.split, `${view.name}: split policy mismatch`, result);
  assertThat(parent.tabbar.bottom <= parent.terminal.top + 1,
    `${view.name}: tabbar overlaps terminal content`, result);
  assertThat(parent.tabs.length > 0 && parent.tabs.every(tab => Math.abs(tab.height - 44) <= 0.75),
    `${view.name}: tab controls are not 44px high`, result);
  assertThat(!terminal.errorVisible && terminal.body.width > 0 && terminal.body.height > 0 &&
    terminal.term.width > 0 && terminal.term.height > 0 &&
    terminal.screen.width > 0 && terminal.screen.height > 0,
  `${view.name}: terminal is blank, errored, or has non-positive geometry`, result);
  assertThat(!terminal.toolbar.hidden && terminal.buttons.length === 9,
    `${view.name}: expected nine visible toolbar controls`, result);
  assertThat(terminal.buttons.every(button => button.height >= 40),
    `${view.name}: a visible toolbar control is under 40px high`, result);
  assertThat(terminal.toolbar.top >= terminal.term.bottom - 1 &&
    terminal.toolbar.bottom <= terminal.body.bottom + 1 &&
    terminal.buttons.every(button => button.top >= terminal.toolbar.top - 1 &&
      button.bottom <= terminal.toolbar.bottom + 1),
  `${view.name}: toolbar row is clipped or overlaps terminal rows`, result);
  assertThat(terminal.markerEvidence.rendered && terminal.markerEvidence.marker === INITIAL_MARKER &&
    terminal.markerEvidence.canvases.some(canvas => canvas.width > 0 && canvas.height > 0 &&
      canvas.error === "" && canvas.foregroundPixels >= INITIAL_MARKER.length),
  `${view.name}: ${INITIAL_MARKER} canvas region has no foreground glyph evidence`, result);
  return result;
}

function pythonCommand(source) {
  const encoded = Buffer.from(source, "utf8").toString("base64");
  return `python3 -c "import base64;exec(base64.b64decode('${encoded}'))"`;
}

function fixedByteReader(label, byteCount) {
  return pythonCommand(`
import os, sys, termios, tty
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
data = b""
try:
    tty.setraw(fd)
    print(${JSON.stringify(`${label}-READY`)}, flush=True)
    while len(data) < ${byteCount}:
        chunk = os.read(fd, ${byteCount} - len(data))
        if not chunk:
            break
        data += chunk
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
print("\\r\\n" + ${JSON.stringify(`${label}-HEX:`)} + data.hex(), flush=True)
`);
}

function zeroByteReader(label, windowMs = ZERO_READER_WINDOW_MS) {
  return pythonCommand(`
import os, select, sys, termios, time, tty
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
window_seconds = ${windowMs} / 1000.0
deadline = time.monotonic() + window_seconds
deadline_wall_ms = int((time.time() + window_seconds) * 1000)
data = b""
try:
    tty.setraw(fd)
    print(${JSON.stringify(`${label}-READY:`)} + str(deadline_wall_ms), flush=True)
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            break
        readable, _, _ = select.select([fd], [], [], remaining)
        if not readable:
            break
        chunk = os.read(fd, 512)
        if not chunk:
            break
        data += chunk
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
print("\\r\\n" + ${JSON.stringify(`${label}-HEX:`)} + data.hex(), flush=True)
`);
}

function enterTerminatedReader(label, maxBytes = 512) {
  return pythonCommand(`
import os, sys, termios, tty
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
data = b""
try:
    tty.setraw(fd)
    print(${JSON.stringify(`${label}-READY`)}, flush=True)
    while len(data) < ${maxBytes}:
        chunk = os.read(fd, 1)
        if not chunk or chunk in (b"\\r", b"\\n"):
            break
        data += chunk
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
print("\\r\\n" + ${JSON.stringify(`${label}-HEX:`)} + data.hex(), flush=True)
`);
}

async function waitForCapture(tmux, needle, label = needle) {
  return poll(`tmux capture ${label}`, () => {
    const capture = tmux.capture();
    return capture.includes(needle) ? capture : false;
  });
}

async function startReader(tmux, command, readyMarker) {
  tmux.sendShell(command);
  return waitForCapture(tmux, readyMarker);
}

async function waitForReaderHex(tmux, label, expectedHex, description = label) {
  return poll(`tty reader ${description}`, () => {
    const capture = tmux.capture();
    const match = capture.match(new RegExp(`${label}-HEX:([0-9a-f]*)`));
    if (!match) return false;
    assertThat(match[1] === expectedHex, `${description}: unexpected tty bytes`, {
      expectedHex,
      actualHex: match[1],
      capture,
    });
    return match[1];
  });
}

async function focusTerminal(frame) {
  const textarea = frame.locator(".xterm-helper-textarea");
  await textarea.focus();
  await poll("xterm focus", () => frame.evaluate(() =>
    document.activeElement?.classList.contains("xterm-helper-textarea") || false));
}

async function runHeightAnimation(opened, tmux) {
  const label = "HEIGHT";
  await startReader(tmux, enterTerminatedReader(label), `${label}-READY`);
  const before = await validFrameBaseline(opened, "height animation");
  await focusTerminal(opened.frame);
  await opened.page.keyboard.type("mobile-input-");
  for (let index = 0; index < HEIGHT_TEXT.length; index += 1) {
    await opened.page.setViewportSize({width: 390, height: 844 - ((index + 1) * 4)});
    await opened.page.keyboard.type(HEIGHT_TEXT[index]);
  }
  await opened.page.keyboard.press("Enter");
  const expectedHex = Buffer.from(HEIGHT_MARKER).toString("hex");
  await waitForReaderHex(tmux, label, expectedHex, "ordered height-animation marker");
  const afterInput = await frameMetrics(opened);
  assertStableConnection(before, afterInput, "height animation");
  await opened.page.setViewportSize({width: 390, height: 844});
  await poll("height restoration", () => opened.page.evaluate(() => innerHeight === 844));
  const afterRestore = await frameMetrics(opened);
  assertStableConnection(before, afterRestore, "height restoration");
  return {marker: HEIGHT_MARKER, before, after: afterRestore};
}

async function runToolbarBytes(opened, tmux) {
  const label = "TOOLBAR";
  await startReader(tmux, fixedByteReader(label, 16), `${label}-READY`);
  const toolbar = opened.frame.locator("#term-toolbar");
  for (const key of ["escape", "tab", "left", "up", "down", "right"]) {
    await toolbar.locator(`[data-key="${key}"]`).click();
  }
  await toolbar.locator('[data-action="ctrl"]').click();
  await opened.page.keyboard.type("a");
  await opened.page.keyboard.press("Control+C");
  const expected = Buffer.from("\x1b\t\x1b[D\x1b[A\x1b[B\x1b[C\x01\x03", "binary").toString("hex");
  await waitForReaderHex(tmux, label, expected, "exact toolbar byte sequence");
  return {hex: expected, bytes: 16};
}

async function stubClipboardRead(frame, mode, value = "") {
  await frame.evaluate(({mode, value}) => {
    const clipboard = navigator.clipboard;
    if (!clipboard || typeof clipboard.readText !== "function") {
      throw new Error("Clipboard read API is unavailable in the terminal frame");
    }
    window.__e2eNativeClipboardRead ||= clipboard.readText.bind(clipboard);
    const replacement = mode === "resolve"
      ? async () => value
      : mode === "reject"
        ? async () => { throw new DOMException("denied by e2e", "NotAllowedError"); }
        : window.__e2eNativeClipboardRead;
    Object.defineProperty(clipboard, "readText", {
      configurable: true,
      writable: true,
      value: replacement,
    });
  }, {mode, value});
}

async function assertTerminalFocused(frame, label) {
  const focused = await frame.evaluate(() =>
    document.activeElement?.classList.contains("xterm-helper-textarea") || false);
  assertThat(focused, `${label}: focus did not return to xterm`);
}

async function assertNoTtyBytes(tmux, action, label) {
  const readyCapture = await startReader(tmux, zeroByteReader(label), `${label}-READY:`);
  const ready = readyCapture.match(new RegExp(`${label}-READY:(\\d+)`));
  assertThat(ready, `${label}: zero-byte reader deadline is missing`, {readyCapture});
  const deadlineMs = Number(ready[1]);
  await action();
  const completedAtMs = Date.now();
  const remainingMs = deadlineMs - completedAtMs;
  assertThat(remainingMs >= ZERO_READER_MIN_TAIL_MS,
    `${label}: UI action left too little delayed-byte observation time`, {
      deadlineMs,
      completedAtMs,
      remainingMs,
      minimumTailMs: ZERO_READER_MIN_TAIL_MS,
    });
  await waitForReaderHex(tmux, label, "", `${label} zero-byte tty probe`);
  return {remainingMs, observedUntilMs: deadlineMs};
}

async function runClipboardChecks(opened, tmux) {
  const pasteButton = opened.frame.locator('[data-action="paste"]');
  const dialog = opened.frame.locator("#paste-dialog");
  const successText = "line one\nline two";
  const expectedSuccessText = normalizeXtermPaste(successText);
  const successLabel = "CLIPOK";
  await startReader(tmux, fixedByteReader(successLabel, Buffer.byteLength(expectedSuccessText)),
    `${successLabel}-READY`);
  await stubClipboardRead(opened.frame, "resolve", successText);
  await pasteButton.click();
  await waitForReaderHex(tmux, successLabel, Buffer.from(expectedSuccessText).toString("hex"),
    "successful clipboard paste bytes");
  await assertTerminalFocused(opened.frame, "successful clipboard paste");

  const manualText = "manual paste";
  const manualLabel = "CLIPMANUAL";
  await startReader(tmux, fixedByteReader(manualLabel, Buffer.byteLength(manualText)), `${manualLabel}-READY`);
  await stubClipboardRead(opened.frame, "reject");
  await pasteButton.click();
  await dialog.waitFor({state: "visible", timeout: POLL_TIMEOUT_MS});
  await dialog.locator("#paste-text").fill(manualText);
  await dialog.locator("#paste-submit").click();
  await dialog.waitFor({state: "hidden", timeout: POLL_TIMEOUT_MS});
  await waitForReaderHex(tmux, manualLabel, Buffer.from(manualText).toString("hex"),
    "manual clipboard paste bytes");
  await assertTerminalFocused(opened.frame, "manual clipboard submit");

  await stubClipboardRead(opened.frame, "reject");
  await assertNoTtyBytes(tmux, async () => {
    await pasteButton.click();
    await dialog.waitFor({state: "visible", timeout: POLL_TIMEOUT_MS});
    await dialog.locator('button[value="cancel"]').click();
    await dialog.waitFor({state: "hidden", timeout: POLL_TIMEOUT_MS});
  }, "CLIPCANCEL");
  await assertTerminalFocused(opened.frame, "manual clipboard Cancel");

  await assertNoTtyBytes(tmux, async () => {
    await pasteButton.click();
    await dialog.waitFor({state: "visible", timeout: POLL_TIMEOUT_MS});
    await opened.page.keyboard.press("Escape");
    await dialog.waitFor({state: "hidden", timeout: POLL_TIMEOUT_MS});
  }, "CLIPEscape");
  await assertTerminalFocused(opened.frame, "manual clipboard Escape");

  await stubClipboardRead(opened.frame, "resolve", "");
  await assertNoTtyBytes(tmux, async () => {
    await pasteButton.click();
    await poll("empty clipboard focus", () => opened.frame.evaluate(() =>
      document.activeElement?.classList.contains("xterm-helper-textarea") || false));
  }, "CLIPEMPTY");
  await assertTerminalFocused(opened.frame, "empty clipboard");
  await stubClipboardRead(opened.frame, "restore");
  return {successBytes: Buffer.byteLength(expectedSuccessText), manualText};
}

async function dispatchTouchDrag(cdp, points, options = {}) {
  assertThat(points.length >= 2, "touch drag requires at least two points", {points});
  const id = options.id ?? 1;
  let contactActive = false;
  try {
    await cdp.send("Input.dispatchTouchEvent", {
      type: "touchStart",
      touchPoints: [{x: points[0].x, y: points[0].y, id}],
    });
    contactActive = true;
    if (options.afterStart) await options.afterStart();
    for (const point of points.slice(1)) {
      await cdp.send("Input.dispatchTouchEvent", {
        type: "touchMove",
        touchPoints: [{x: point.x, y: point.y, id}],
      });
      await delay(20);
    }
    await cdp.send("Input.dispatchTouchEvent", {type: "touchEnd", touchPoints: []});
    contactActive = false;
  } finally {
    if (contactActive) {
      try {
        await cdp.send("Input.dispatchTouchEvent", {type: "touchEnd", touchPoints: []});
      } catch (_) {
        await cdp.send("Input.dispatchTouchEvent", {type: "touchCancel", touchPoints: []});
      }
    }
  }
}

function interpolate(start, end, steps = 8) {
  return Array.from({length: steps + 1}, (_, index) => ({
    x: start.x + ((end.x - start.x) * index / steps),
    y: start.y + ((end.y - start.y) * index / steps),
  }));
}

async function runToolbarPan(opened, cdp) {
  const toolbar = opened.frame.locator("#term-toolbar");
  await opened.frame.evaluate(() => {
    const target = document.getElementById("term-toolbar");
    target.scrollLeft = 0;
    window.__e2eToolbarTouchMoves = [];
    document.addEventListener("touchmove", event => {
      if (event.composedPath().includes(target)) {
        window.__e2eToolbarTouchMoves.push({defaultPrevented: event.defaultPrevented});
      }
    }, {passive: true});
  });
  const box = await toolbar.boundingBox();
  const dimensions = await toolbar.evaluate(element => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
  }));
  assertThat(box && dimensions.scrollWidth > dimensions.clientWidth,
    "toolbar is not horizontally scrollable", {box, dimensions});
  const y = box.y + box.height / 2;
  const points = interpolate(
    {x: box.x + box.width - 12, y},
    {x: box.x + 20, y},
    10,
  );
  await dispatchTouchDrag(cdp, points, {id: 21});
  const result = await poll("native toolbar horizontal pan", () => opened.frame.evaluate(() => {
    const target = document.getElementById("term-toolbar");
    const moves = window.__e2eToolbarTouchMoves || [];
    return target.scrollLeft > 0 ? {scrollLeft: target.scrollLeft, moves} : false;
  }));
  assertThat(result.moves.length > 0 && result.moves.every(move => !move.defaultPrevented),
    "toolbar touch move was canceled by terminal gesture handling", result);
  return result;
}

async function runTerminalScroll(opened, tmux, cdp) {
  const beforeMessages = (await frameMetrics(opened)).socketMessages;
  tmux.sendShell("for i in $(seq 1 200); do printf 'E2E-SCROLL-%03d\\n' \"$i\"; done");
  await waitForCapture(tmux, "E2E-SCROLL-200", "200 generated scroll lines");
  await poll("scroll output delivery to iframe", async () =>
    (await frameMetrics(opened)).socketMessages > beforeMessages);
  const viewport = opened.frame.locator(".xterm-viewport");
  const screen = opened.frame.locator(".xterm-screen");
  const box = await screen.boundingBox();
  const before = await viewport.evaluate(element => ({
    scrollTop: element.scrollTop,
    scrollHeight: element.scrollHeight,
    clientHeight: element.clientHeight,
  }));
  assertThat(box && before.scrollHeight > before.clientHeight,
    "terminal has no scrollback after generating 200 lines", {box, before});
  const x = box.x + box.width / 2;
  const points = interpolate(
    {x, y: box.y + box.height * 0.25},
    {x, y: box.y + box.height * 0.82},
    10,
  );
  await dispatchTouchDrag(cdp, points, {id: 22});
  const after = await poll("terminal touch scroll position change", async () => {
    const state = await viewport.evaluate(element => ({scrollTop: element.scrollTop}));
    return state.scrollTop !== before.scrollTop ? state : false;
  });
  return {before, after};
}

async function interactionState(api, session) {
  const state = await api.get(`/tmux-mouse?session=${encodeURIComponent(session)}`);
  assertThat(state.mouse === "on" || state.mouse === "off", "invalid tmux interaction API state", state);
  return state.mouse;
}

async function setInteraction(opened, tmux, api, session, selecting) {
  const button = opened.frame.locator('[data-action="mode"]');
  await poll("interaction mode button readiness", async () => !(await button.isDisabled()));
  const pressed = await button.getAttribute("aria-pressed");
  if ((pressed === "true") !== selecting) {
    tmux.assertOwnedSession();
    await button.click();
  }
  await poll(`confirmed ${selecting ? "Select" : "Interact"} mode`, async () => {
    const state = await button.evaluate(element => ({
      pressed: element.getAttribute("aria-pressed"),
      disabled: element.disabled,
      label: element.textContent.trim(),
    }));
    const apiMouse = await interactionState(api, session);
    const expectedLabel = selecting ? "Interactuar" : "Seleccionar";
    const ready = !state.disabled && state.pressed === String(selecting) &&
      state.label === expectedLabel && apiMouse === (selecting ? "off" : "on");
    return ready ? {state, apiMouse} : false;
  });
}

async function runSelectionCopy(opened, tmux, api, session) {
  const marker = `SELECT-COPY-${process.pid}`;
  await opened.page.context().grantPermissions(["clipboard-read", "clipboard-write"], {
    origin: new URL(opened.page.url()).origin,
  });
  await opened.page.evaluate(() => navigator.clipboard.writeText(""));
  await opened.frame.locator(".xterm-viewport").evaluate(element => {
    element.scrollTop = element.scrollHeight;
  });
  await poll("xterm return to live bottom before selection", () => opened.frame.evaluate(() => {
    const viewport = document.querySelector(".xterm-viewport");
    return Math.abs(viewport.scrollHeight - viewport.clientHeight - viewport.scrollTop) <= 2;
  }));
  const beforeMessages = (await frameMetrics(opened)).socketMessages;
  tmux.sendShell(`printf '\\033[2J\\033[H%s\\n' '${marker}'`);
  await waitForCapture(tmux, marker, "selection marker");
  await poll("selection marker delivery to iframe", async () =>
    (await frameMetrics(opened)).socketMessages > beforeMessages);
  const rendered = await waitForMarkerRender(opened.frame, marker,
    "selection marker real xterm buffer/canvas region");
  await setInteraction(opened, tmux, api, session, true);

  const screen = opened.frame.locator(".xterm-screen");
  const box = await screen.boundingBox();
  assertThat(box && box.width > 0 && box.height > 0, "selection screen has invalid geometry", {box});
  const region = markerCanvasRegion(rendered.location, rendered.buffer.cols, rendered.buffer.rows,
    box.width, box.height);
  assertThat(region, "selection marker CSS region is invalid", {box, rendered});
  const cellWidth = box.width / rendered.buffer.cols;
  const y = box.y + region.top + (region.height / 2);
  await opened.page.mouse.move(box.x + region.left + (cellWidth * 0.15), y);
  await opened.page.mouse.down();
  await opened.page.mouse.move(box.x + region.right - (cellWidth * 0.15), y, {steps: 12});
  await opened.page.mouse.up();
  await opened.page.keyboard.press("Control+C");
  const copied = await poll("known terminal selection in browser clipboard", async () => {
    const value = await opened.page.evaluate(() => navigator.clipboard.readText());
    return value.includes(marker) ? value : false;
  });
  assertThat(copied.includes(marker), "copied terminal selection omitted known marker", {marker, copied});
  await setInteraction(opened, tmux, api, session, false);
  return {marker, copied, rendered};
}

function paneGeometryFor(panes, id) {
  const pane = panes.find(candidate => candidate.id === id);
  assertThat(pane, "pane geometry not found", {id, panes});
  return pane;
}

async function resizeOneAxis(opened, tmux, cdp, flag) {
  let addedPane = null;
  try {
    addedPane = tmux.split(flag);
    await poll(`${flag} disposable split`, () => tmux.panes().length === 2);
    const before = tmux.panes();
    const primary = paneGeometryFor(before, before.find(pane => pane.paneIndex === 0).id);
    const other = paneGeometryFor(before, addedPane);
    const screenBox = await opened.frame.locator(".xterm-screen").boundingBox();
    const windowSize = tmux.windowSize();
    assertThat(screenBox, "terminal screen is unavailable for pane resize", {flag});
    const cellWidth = screenBox.width / windowSize.width;
    const cellHeight = screenBox.height / windowSize.height;
    let start;
    let end;
    if (flag === "-h") {
      const leftPane = [primary, other].sort((a, b) => a.left - b.left)[0];
      const borderColumn = leftPane.left + leftPane.width;
      start = {
        x: screenBox.x + (borderColumn + 0.5) * cellWidth,
        y: screenBox.y + (leftPane.top + Math.floor(leftPane.height / 2) + 0.5) * cellHeight,
      };
      end = {x: start.x + cellWidth * 4, y: start.y};
    } else {
      const topPane = [primary, other].sort((a, b) => a.top - b.top)[0];
      const borderRow = topPane.top + topPane.height;
      start = {
        x: screenBox.x + (topPane.left + Math.floor(topPane.width / 2) + 0.5) * cellWidth,
        y: screenBox.y + (borderRow + 0.5) * cellHeight,
      };
      end = {x: start.x, y: start.y + cellHeight * 3};
    }
    await dispatchTouchDrag(cdp, interpolate(start, end, 6), {
      id: flag === "-h" ? 31 : 32,
      afterStart: () => poll(`${flag} long-press resize engagement`, () => opened.frame.evaluate(() =>
        [...document.body.children].some(element =>
          element.textContent.includes("redimensionando") && getComputedStyle(element).display !== "none")),
      {timeoutMs: 2_000, intervalMs: 25}),
    });
    const changed = await poll(`${flag} pane geometry change`, () => {
      const after = tmux.panes();
      if (after.length !== 2) return false;
      const beforePrimary = paneGeometryFor(before, primary.id);
      const afterPrimary = paneGeometryFor(after, primary.id);
      const axisChanged = flag === "-h"
        ? afterPrimary.width !== beforePrimary.width
        : afterPrimary.height !== beforePrimary.height;
      return axisChanged ? {before, after} : false;
    });
    return changed;
  } finally {
    if (addedPane && tmux.exists()) {
      const ids = tmux.panes().map(pane => pane.id);
      if (ids.includes(addedPane)) tmux.killPane(addedPane);
      await poll(`${flag} split cleanup`, () => tmux.panes().length === 1);
    }
  }
}

async function runPaneResize(opened, tmux, api, session, cdp) {
  await setInteraction(opened, tmux, api, session, false);
  await opened.frame.locator(".xterm-viewport").evaluate(element => {
    element.scrollTop = element.scrollHeight;
  });
  await poll("xterm return to live bottom before pane resize", () => opened.frame.evaluate(() => {
    const viewport = document.querySelector(".xterm-viewport");
    return Math.abs(viewport.scrollHeight - viewport.clientHeight - viewport.scrollTop) <= 2;
  }));
  const horizontal = await resizeOneAxis(opened, tmux, cdp, "-h");
  const vertical = await resizeOneAxis(opened, tmux, cdp, "-v");
  assertThat(tmux.panes().length === 1, "test-created panes remain after resize checks", tmux.panes());
  return {horizontal, vertical};
}

const THEME_CSS = Object.freeze({
  noche: {background: "#0a0d13", brand: "#8b7cff"},
  dia: {background: "#f7f8fb", brand: "#5b4bd6"},
  calido: {background: "#161009", brand: "#e0a458"},
  termius: {background: "#0e1620", brand: "#4ce07a"},
  bruno: {background: "#1a1a1a", brand: "#e4ae49"},
});

async function themePropagation(opened) {
  const frame = await activeTerminalFrame(opened);
  const [dashboardTheme, terminal] = await Promise.all([
    opened.page.evaluate(() => document.documentElement.dataset.theme || "noche"),
    frame.evaluate(() => {
      const root = getComputedStyle(document.documentElement);
      return {
        background: root.getPropertyValue("--term-bg").trim().toLowerCase(),
        brand: root.getPropertyValue("--term-brand").trim().toLowerCase(),
        colorScheme: root.colorScheme,
      };
    }),
  ]);
  return {dashboardTheme, terminal};
}

async function waitForThemePropagation(opened, name) {
  const expected = THEME_CSS[name];
  assertThat(expected, "unknown theme propagation expectation", {name});
  return poll(`dashboard and terminal ${name} theme propagation`, async () => {
    const state = await themePropagation(opened);
    return state.dashboardTheme === name && state.terminal.background === expected.background &&
      state.terminal.brand === expected.brand ? state : false;
  });
}

async function clickThemeUntil(opened, name) {
  const button = opened.page.locator("#btn-theme");
  for (let attempt = 0; attempt < 5; attempt += 1) {
    const current = await opened.page.evaluate(() =>
      document.documentElement.dataset.theme || "noche");
    if (current === name) return waitForThemePropagation(opened, name);
    await button.click();
    const next = await poll(`dashboard theme transition away from ${current}`, () => opened.page.evaluate(previous => {
      const actual = document.documentElement.dataset.theme || "noche";
      return actual !== previous ? actual : false;
    }, current));
    const propagated = await waitForThemePropagation(opened, next);
    if (next === name) return propagated;
  }
  const actual = await opened.page.evaluate(() => document.documentElement.dataset.theme || "noche");
  assertThat(actual === name, `could not cycle to ${name}`, {actual});
  return waitForThemePropagation(opened, name);
}

async function themeColors(opened, activateTool = false) {
  const dashboard = await opened.page.evaluate(() => ({
    theme: document.documentElement.dataset.theme || "noche",
    background: getComputedStyle(document.body).backgroundColor,
    rootBackground: getComputedStyle(document.documentElement).getPropertyValue("--bg").trim(),
    rootBrand: getComputedStyle(document.documentElement).getPropertyValue("--brand").trim(),
    colorScheme: getComputedStyle(document.documentElement).colorScheme,
  }));
  const frame = await activeTerminalFrame(opened);
  const ctrl = frame.locator('[data-action="ctrl"]');
  if (activateTool) await ctrl.click();
  const terminal = await frame.evaluate(() => {
    const root = getComputedStyle(document.documentElement);
    const body = getComputedStyle(document.body);
    const toolbar = getComputedStyle(document.getElementById("term-toolbar"));
    const ctrl = getComputedStyle(document.querySelector('[data-action="ctrl"]'));
    return {
      background: body.backgroundColor,
      rootBackground: root.getPropertyValue("--term-bg").trim(),
      rootBrand: root.getPropertyValue("--term-brand").trim(),
      colorScheme: root.colorScheme,
      toolbarBackground: toolbar.backgroundColor,
      activeToolBackground: ctrl.backgroundColor,
    };
  });
  if (activateTool) await ctrl.click();
  return {dashboard, terminal};
}

async function runThemeChecks(opened, tmux) {
  const before = await validFrameBaseline(opened, "theme cycle");
  const clientsBefore = tmux.clientCount();
  assertThat(clientsBefore > 0, "disposable tmux session has no attached web terminal client", {clientsBefore});
  await clickThemeUntil(opened, "bruno");
  const bruno = await themeColors(opened, true);
  assertThat(bruno.dashboard.background === "rgb(26, 26, 26)" &&
    bruno.dashboard.rootBackground.toLowerCase() === "#1a1a1a" &&
    bruno.dashboard.rootBrand.toLowerCase() === "#e4ae49",
  "Bruno dashboard colors are incorrect", bruno);
  assertThat(bruno.terminal.background === "rgb(26, 26, 26)" &&
    bruno.terminal.rootBackground.toLowerCase() === "#1a1a1a" &&
    bruno.terminal.rootBrand.toLowerCase() === "#e4ae49" &&
    bruno.terminal.activeToolBackground === "rgb(228, 174, 73)",
  "Bruno terminal/tool chrome colors are incorrect", bruno);

  await clickThemeUntil(opened, "dia");
  const day = await themeColors(opened);
  assertThat(day.dashboard.colorScheme.includes("light") && day.terminal.colorScheme.includes("light"),
    "Day theme did not apply a light color scheme", day);
  await clickThemeUntil(opened, "bruno");
  const finalBruno = await themeColors(opened, true);
  assertThat(finalBruno.dashboard.background === "rgb(26, 26, 26)" &&
    finalBruno.terminal.background === "rgb(26, 26, 26)" &&
    finalBruno.terminal.activeToolBackground === "rgb(228, 174, 73)",
  "Bruno colors were not restored after Day", finalBruno);
  const after = await frameMetrics(opened);
  const clientsAfter = tmux.clientCount();
  assertStableConnection(before, after, "Bruno/Day theme cycle");
  assertThat(clientsAfter === clientsBefore, "theme cycle changed disposable tmux client count", {
    clientsBefore,
    clientsAfter,
  });
  return {before, after, clientsBefore, clientsAfter, bruno, day, finalBruno};
}

async function runMatrix(browser, base, token, session, contexts) {
  const geometry = [];
  for (const view of MATRIX) {
    const context = await createTouchContext(browser, view);
    contexts.add(context);
    try {
      const opened = await openTerminal(context, base, token, session);
      const measured = await inspectGeometry(opened, view);
      geometry.push(measured);
      await opened.page.screenshot({
        path: path.join(OUTPUT_DIR, `${view.name}.png`),
        fullPage: false,
      });
      fs.writeFileSync(path.join(OUTPUT_DIR, `${view.name}.json`),
        `${JSON.stringify(measured, null, 2)}\n`, {mode: 0o600});
    } finally {
      await context.close();
      contexts.delete(context);
    }
  }
  return geometry;
}

async function runFunctionalChecks(browser, base, token, session, tmux, api, contexts) {
  const view = MATRIX.find(candidate => candidate.name === "phone-390x844");
  const context = await createTouchContext(browser, view);
  contexts.add(context);
  try {
    const opened = await openTerminal(context, base, token, session);
    const cdp = await context.newCDPSession(opened.page);
    const results = {};
    results.heightAnimation = await runHeightAnimation(opened, tmux);
    results.toolbarBytes = await runToolbarBytes(opened, tmux);
    results.clipboard = await runClipboardChecks(opened, tmux);
    results.toolbarPan = await runToolbarPan(opened, cdp);
    results.terminalScroll = await runTerminalScroll(opened, tmux, cdp);
    results.selectionCopy = await runSelectionCopy(opened, tmux, api, session);
    results.paneResize = await runPaneResize(opened, tmux, api, session, cdp);
    results.theme = await runThemeChecks(opened, tmux);
    return results;
  } finally {
    await context.close();
    contexts.delete(context);
  }
}

async function closeContexts(contexts, cleanupErrors) {
  for (const context of [...contexts]) {
    try {
      await context.close();
    } catch (error) {
      cleanupErrors.push(`browser context close: ${error.message}`);
    } finally {
      contexts.delete(context);
    }
  }
}

function validateRuntime(rawBase, token) {
  const base = normalizeBase(rawBase);
  assertThat(typeof token === "string" && token.length > 0, "CC_REMOTE_TOKEN is required");
  assertThat(fs.existsSync(CHROME), "system Chrome is unavailable", {expected: CHROME});
  return {base, token};
}

async function main() {
  const secrets = [process.env.CC_REMOTE_TOKEN || ""];
  let runtime;
  let playwright;
  try {
    runtime = validateRuntime(process.env.CC_REMOTE_BASE, process.env.CC_REMOTE_TOKEN);
    playwright = loadPlaywright();
  } catch (error) {
    throw new HarnessError("runtime validation failed", redactSecrets(error, secrets));
  }

  const session = sessionNameFor(process.pid);
  const tmux = new TmuxController(session);
  const api = new ApiClient(runtime.base, runtime.token);
  const contexts = new Set();
  const cleanupErrors = [];
  let browser = null;
  let primaryError = null;
  let registrationState = "not-attempted";
  let result = null;

  fs.rmSync(OUTPUT_DIR, {recursive: true, force: true});
  fs.mkdirSync(OUTPUT_DIR, {recursive: true, mode: 0o700});
  try {
    tmux.create();
    tmux.assertOwnedSession();
    registrationState = "uncertain";
    try {
      await api.post("/tab-register", {session, label: session});
      registrationState = "confirmed";
    } catch (error) {
      registrationState = registrationStateAfterError(error);
      throw error;
    }
    tmux.sendShell(`printf '\\033[2J\\033[H${INITIAL_MARKER}\\n'`);
    await waitForCapture(tmux, INITIAL_MARKER, "initial terminal marker");
    browser = await playwright.chromium.launch({
      executablePath: CHROME,
      headless: true,
      args: ["--no-sandbox", "--disable-dev-shm-usage"],
    });
    const geometry = await runMatrix(browser, runtime.base, runtime.token, session, contexts);
    const functional = await runFunctionalChecks(
      browser, runtime.base, runtime.token, session, tmux, api, contexts,
    );
    result = {
      ok: true,
      session,
      output: OUTPUT_DIR,
      viewports: geometry.map(item => ({
        name: item.name,
        split: item.parent.split,
        activeFrames: item.parent.activeFrames,
      })),
      functional,
    };
    fs.writeFileSync(path.join(OUTPUT_DIR, "results.json"),
      `${JSON.stringify(result, null, 2)}\n`, {mode: 0o600});
  } catch (error) {
    primaryError = error;
  } finally {
    await closeContexts(contexts, cleanupErrors);
    if (browser) {
      try {
        await browser.close();
      } catch (error) {
        cleanupErrors.push(`browser close: ${error.message}`);
      }
    }
    let registrationOwnership = null;
    if (registrationState === "uncertain") {
      try {
        registrationOwnership = tmux.ownershipProbe();
      } catch (error) {
        cleanupErrors.push(`uncertain /tab-register ownership probe: ${error.message}`);
      }
    }
    const registrationCleanup = registrationCleanupDecision(registrationState, registrationOwnership);
    if (registrationCleanup === "close") {
      try {
        await api.post("/tab-close", {session, ephemeral: true});
      } catch (error) {
        cleanupErrors.push(`ephemeral /tab-close: ${error.message}`);
      }
    } else if (registrationCleanup === "blocked") {
      cleanupErrors.push("uncertain /tab-register not closed because session ownership is unproven");
    }
    if (tmux.mouseSnapshot) {
      try {
        tmux.assertOwnedSession();
        await api.post("/tmux-mouse", {
          session,
          enabled: tmux.mouseSnapshot.effective === "on",
        });
      } catch (error) {
        cleanupErrors.push(`mouse API restore: ${error.message}`);
      }
      try {
        tmux.restoreMouseLocal();
      } catch (error) {
        cleanupErrors.push(`mouse local-option restore: ${error.message}`);
      }
    }
    if (tmux.creationAttempted) {
      try {
        tmux.killSession();
      } catch (error) {
        cleanupErrors.push(`exact kill-session: ${error.message}`);
      }
    }
  }

  if (primaryError || cleanupErrors.length) {
    const details = {
      failure: primaryError ? redactSecrets(primaryError, secrets) : undefined,
      cleanup: cleanupErrors.map(error => redactSecrets(error, secrets)),
      registrationState,
      exactSession: session,
    };
    throw new HarnessError("Task 7 browser harness failed", details);
  }
  process.stdout.write(`${JSON.stringify({
    ok: result.ok,
    session: result.session,
    output: result.output,
    viewports: result.viewports,
  })}\n`);
  return result;
}

module.exports = {
  ApiClient,
  HarnessError,
  INITIAL_MARKER,
  MATRIX,
  OUTPUT_DIR,
  TmuxController,
  assertThat,
  enterTerminatedReader,
  fixedByteReader,
  loadPlaywright,
  locateVisibleMarker,
  markerCanvasRegion,
  normalizeBase,
  normalizeXtermPaste,
  poll,
  redactSecrets,
  registrationCleanupDecision,
  registrationStateAfterError,
  sessionNameFor,
  tmuxTargetsFor,
  zeroByteReader,
};

if (require.main === module) {
  main().catch(error => {
    const safe = redactSecrets(error, [process.env.CC_REMOTE_TOKEN || ""]);
    process.stderr.write(`${safe}\n`);
    process.exitCode = 1;
  });
}
