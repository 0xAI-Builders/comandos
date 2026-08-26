#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const http = require("node:http");
const os = require("node:os");
const path = require("node:path");
const {loadPlaywright} = require("./e2e_mobile_remote.js");

const ROOT = path.resolve(__dirname, "..");
const DASH = path.join(ROOT, "dash");
const CHROME = "/usr/bin/google-chrome";
const IDLE_SCREENSHOT = "/tmp/perezos-task9-full-idle.png";
const ACTION_SCREENSHOT = "/tmp/perezos-task9-full-action.png";
const VIEWPORT = Object.freeze({width:1400, height:900});
const PERFORMANCE = Object.freeze({warmupMs:10_000, sampleMs:30_000});
const SCRIPT_NAMES = Object.freeze([
  "core", "art", "rig", "behaviors", "motion", "renderer", "engine",
]);

const MIME = Object.freeze({
  ".css":"text/css; charset=utf-8",
  ".html":"text/html; charset=utf-8",
  ".js":"text/javascript; charset=utf-8",
  ".json":"application/json; charset=utf-8",
  ".png":"image/png",
  ".svg":"image/svg+xml",
});

function assertImportContract(){
  return Object.freeze({
    root:ROOT,
    dash:DASH,
    scripts:SCRIPT_NAMES.slice(),
    viewport:{...VIEWPORT},
  });
}

function requireCachedPlaywright(request){
  if(request !== "playwright") return require(request);
  const cacheRoot = path.join(os.homedir(), ".npm", "_npx");
  let entries = [];
  try{ entries = fs.readdirSync(cacheRoot).sort(); }
  catch(error){ return require(request); }
  for(const entry of entries){
    const candidate = path.join(cacheRoot, entry, "node_modules", "playwright");
    if(fs.existsSync(path.join(candidate, "package.json"))) return require(candidate);
  }
  return require(request);
}

function analyzePixels(rgba, width, height){
  if(!rgba || rgba.length !== width * height * 4 || width <= 0 || height <= 0){
    throw new TypeError("invalid RGBA pixel buffer");
  }
  let hash = 0x811c9dc5;
  let count = 0;
  let sumX = 0;
  let sumY = 0;
  let minX = width;
  let minY = height;
  let maxX = -1;
  let maxY = -1;
  const palette = new Set();
  for(let offset = 0; offset < rgba.length; offset += 4){
    for(let channel = 0; channel < 4; channel += 1){
      hash ^= rgba[offset + channel];
      hash = Math.imul(hash, 0x01000193);
    }
    const alpha = rgba[offset + 3];
    if(alpha === 0) continue;
    const pixel = offset / 4;
    const x = pixel % width;
    const y = Math.floor(pixel / width);
    count += 1;
    sumX += x;
    sumY += y;
    if(x < minX) minX = x;
    if(x > maxX) maxX = x;
    if(y < minY) minY = y;
    if(y > maxY) maxY = y;
    palette.add(`${rgba[offset]},${rgba[offset + 1]},${rgba[offset + 2]},${alpha}`);
  }
  return Object.freeze({
    hash:(hash >>> 0).toString(16).padStart(8, "0"),
    width,
    height,
    nonTransparent:count,
    occupancy:count / (width * height),
    transparent:count < width * height,
    uniquePalette:palette.size,
    centroid:count ? {x:sumX / count, y:sumY / count} : null,
    bounds:count ? {left:minX, top:minY, right:maxX, bottom:maxY,
      width:maxX - minX + 1, height:maxY - minY + 1} : null,
  });
}

function harnessDocument(){
  const scripts = SCRIPT_NAMES.map(name =>
    `<script src="/perezos/${name}.js"></script>`).join("\n");
  return `<!doctype html>
<html lang="es"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<link rel="stylesheet" href="/perezos/perezos.css">
<style>
:root{--brand:#8B7CFF;--panel:#121722;--panel2:#171E2B;--line:#222A3A;
--line2:#303A4D;--shadow:#05070c;--text:#F2F5FA}
*{box-sizing:border-box}html,body{margin:0;min-height:100%;background:#0b1019;color:var(--text)}
body{font-family:ui-monospace,monospace;padding:24px}.panel{position:relative;width:620px;
min-height:300px;padding:24px;border:1px solid var(--line2);border-radius:14px;background:var(--panel)}
#stage-wrap{width:256px;height:208px}.perezos-stage{display:block}
#neighbor,#mascot-toggle{margin:12px 8px 0 0}.offscreen{transform:translateY(1800px)}
</style></head><body><main class="panel" id="panel">
<div id="stage-wrap"><button type="button" class="perezos-stage" id="stage"
 aria-label="PerezOS, mascota de la sesión seleccionada" aria-describedby="mascot-description">
 <canvas class="perezos-canvas" id="canvas" width="224" height="192" aria-hidden="true"></canvas>
</button></div><span id="mascot-description" aria-live="polite">PerezOS descansa con contacto seguro.</span>
<div><button type="button" id="neighbor">Control vecino</button>
<button type="button" id="mascot-toggle" role="switch" aria-checked="true">Mascota PerezOS</button></div>
</main>${scripts}<script>
(() => {
  "use strict";
  let forcedHidden = false;
  try{ Object.defineProperty(document, "hidden", {configurable:true, get:() => forcedHidden}); }
  catch(error){ /* Chromium currently permits this test-only visibility seam. */ }
  const canvas = document.getElementById("canvas");
  const stage = document.getElementById("stage");
  const wrapper = document.getElementById("stage-wrap");
  const description = document.getElementById("mascot-description");
  const toggle = document.getElementById("mascot-toggle");
  const neighbor = document.getElementById("neighbor");
  const controller = window.ComandOSPerezOS.createPerezOS(canvas, {visible:true});
  let timestamp = 1;
  let state = {sessionId:"task9-harness",status:"idle",role:"daily",costume:"bufanda",
    contextPressure:"medium",theme:"noche",expanded:false,
    colors:{brand:"#8B7CFF",panel:"#121722",line:"#222A3A"}};
  let neighboringClicks = 0;
  let visible = true;
  const applyContext = patch => {
    state = {...state, ...patch, timestamp:timestamp++};
    controller.setContext(state);
    description.textContent = "PerezOS: " + state.status;
    return controller.getDiagnostics();
  };
  const localPoint = event => {
    const rect = stage.getBoundingClientRect();
    return {x:Math.max(0, Math.min(rect.width, event.clientX - rect.left)),
      y:Math.max(0, Math.min(rect.height, event.clientY - rect.top))};
  };
  stage.addEventListener("pointermove", event => {
    const point = localPoint(event);
    controller.notifyInteraction("pointer", point.x, point.y);
  }, {passive:true});
  stage.addEventListener("click", event => {
    event.stopPropagation();
    if(controller.notifyInteraction("activate", 128, 104)){
      description.textContent = "PerezOS reconoce tu saludo.";
    }
  });
  neighbor.addEventListener("click", () => { neighboringClicks += 1; });
  toggle.addEventListener("click", () => {
    visible = !visible;
    toggle.setAttribute("aria-checked", String(visible));
    controller.setVisible(visible);
  });
  function pixelSample(target = canvas){
    const ctx = target.getContext("2d", {alpha:true});
    const width = target.width;
    const height = target.height;
    const rgba = ctx.getImageData(0, 0, width, height).data;
    let hash = 0x811c9dc5, count = 0, sumX = 0, sumY = 0;
    let minX = width, minY = height, maxX = -1, maxY = -1;
    const palette = new Set();
    for(let offset = 0; offset < rgba.length; offset += 4){
      for(let channel = 0; channel < 4; channel += 1){
        hash ^= rgba[offset + channel]; hash = Math.imul(hash, 0x01000193);
      }
      if(rgba[offset + 3] === 0) continue;
      const pixel = offset / 4, x = pixel % width, y = Math.floor(pixel / width);
      count += 1; sumX += x; sumY += y;
      minX = Math.min(minX, x); maxX = Math.max(maxX, x);
      minY = Math.min(minY, y); maxY = Math.max(maxY, y);
      palette.add(rgba[offset] + "," + rgba[offset + 1] + "," +
        rgba[offset + 2] + "," + rgba[offset + 3]);
    }
    return {hash:(hash >>> 0).toString(16).padStart(8, "0"), width, height,
      nonTransparent:count, occupancy:count / (width * height),
      transparent:count < width * height, uniquePalette:palette.size,
      centroid:count ? {x:sumX / count, y:sumY / count} : null,
      bounds:count ? {left:minX,top:minY,right:maxX,bottom:maxY,
        width:maxX-minX+1,height:maxY-minY+1} : null};
  }
  function renderSlipRecovery(){
    const target = document.createElement("canvas");
    target.width = 224; target.height = 192;
    const rig = window.ComandOSPerezOS.Rig.createRig("task9-slip-recovery");
    const director = window.ComandOSPerezOS.Behaviors.createDirector("task9-slip-recovery");
    window.ComandOSPerezOS.Behaviors.updateContext(director, {...state,status:"idle",
      supports:{"front-left":{mode:"loaded",load:0.58},
        "front-right":{mode:"released",load:0},"rear-left":{mode:"released",load:0},
        "rear-right":{mode:"loaded",load:0.42}}}, 0);
    let performance = null, now = 0;
    for(let index = 0; index < 400; index += 1){
      performance = window.ComandOSPerezOS.Behaviors.nextPerformance(director, now);
      if(performance.family === "slip-recover") break;
      window.ComandOSPerezOS.Behaviors.completePerformance(director, performance,
        now + performance.durationMs);
      now += 240001;
    }
    if(!performance || performance.family !== "slip-recover") throw new Error("slip seed unreachable");
    const motion = window.ComandOSPerezOS.Motion.createMotion(rig);
    window.ComandOSPerezOS.Motion.enqueue(motion, performance, now);
    for(let frame = 0; frame < 240; frame += 1){
      window.ComandOSPerezOS.Motion.stepMotion(motion, 1/30, now + frame * 1000/30);
      if(motion.phase && motion.phase.primitive === "recover" && frame > 10) break;
    }
    const renderer = window.ComandOSPerezOS.Renderer.createRenderer(target);
    window.ComandOSPerezOS.Renderer.setViewport(renderer, 224, 192, 1);
    window.ComandOSPerezOS.Renderer.render(renderer, rig, state, "full");
    const sample = pixelSample(target);
    window.ComandOSPerezOS.Renderer.destroyRenderer(renderer);
    return sample;
  }
  window.__perezosHarness = Object.freeze({canvas, stage, wrapper, controller,
    applyContext, pixelSample, renderSlipRecovery,
    setVisible(value){ visible = value === true; controller.setVisible(visible);
      toggle.setAttribute("aria-checked", String(visible)); },
    setHidden(value){ forcedHidden = value === true;
      document.dispatchEvent(new Event("visibilitychange")); },
    setOffscreen(value){ wrapper.classList.toggle("offscreen", value === true); },
    get neighboringClicks(){ return neighboringClicks; },
  });
  applyContext(state);
})();
</script></body></html>`;
}

function fixtureFor(pathname){
  const session = {session:"task9-session",project:"PerezOS Task 9",cwd:"/tmp/perezos",
    status:"idle",alive:true,agent:"codex",model:"gpt-5.6",account:"main",
    contextPct:38,ts:1_777_000_000,pane:"%1",last:"validando PerezOS",
    detail:"validando PerezOS",options:"",external:false,paused:false};
  if(pathname === "/state") return [session];
  if(pathname === "/active-tab") return {session:session.session, ts:session.ts};
  if(pathname === "/agent-roles") return {roles:[]};
  if(pathname === "/events" || pathname === "/tab-history") return [];
  if(pathname === "/ssh") return {hosts:[]};
  if(pathname === "/prefs") return {};
  if(pathname === "/conf") return {_lang:"es"};
  if(pathname === "/remote-state") return {remoteOn:false, webterm:false};
  if(pathname === "/usage/state") return {providers:[], sessions:[]};
  if(pathname === "/usage/settings") return {};
  if(pathname === "/model-tiers") return {tiers:[]};
  if(pathname === "/dedication") return {};
  return null;
}

function createServer(){
  const server = http.createServer((request, response) => {
    let pathname;
    try{ pathname = new URL(request.url, "http://127.0.0.1").pathname; }
    catch(error){ response.writeHead(400).end(); return; }
    if(pathname === "/perezos-harness.html"){
      const body = harnessDocument();
      response.writeHead(200, {"Content-Type":MIME[".html"], "Cache-Control":"no-store"});
      response.end(body);
      return;
    }
    const fixture = fixtureFor(pathname);
    if(fixture !== null){
      response.writeHead(200, {"Content-Type":MIME[".json"], "Cache-Control":"no-store"});
      response.end(JSON.stringify(fixture));
      return;
    }
    const relative = pathname === "/" ? "index.html" : decodeURIComponent(pathname.slice(1));
    const resolved = path.resolve(DASH, relative);
    if(resolved !== DASH && !resolved.startsWith(`${DASH}${path.sep}`)){
      response.writeHead(403).end(); return;
    }
    fs.readFile(resolved, (error, bytes) => {
      if(error){ response.writeHead(error.code === "ENOENT" ? 404 : 500).end(); return; }
      response.writeHead(200, {"Content-Type":MIME[path.extname(resolved)] ||
        "application/octet-stream", "Cache-Control":"no-store"});
      response.end(bytes);
    });
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      resolve({server, base:`http://127.0.0.1:${address.port}`});
    });
  });
}

function closeServer(server){
  return new Promise(resolve => server.close(resolve));
}

async function sampleCanvas(page, selector){
  const values = await page.locator(selector).evaluate(canvas => {
    const ctx = canvas.getContext("2d", {alpha:true});
    return {rgba:Array.from(ctx.getImageData(0, 0, canvas.width, canvas.height).data),
      width:canvas.width, height:canvas.height};
  });
  return analyzePixels(Uint8Array.from(values.rgba), values.width, values.height);
}

function recordFailure(target, condition, label, details){
  if(condition) return;
  target.push(details === undefined ? label : `${label}: ${JSON.stringify(details)}`);
}

function assertVisual(sample, name, failures, options = {}){
  recordFailure(failures, sample.transparent, `${name} canvas keeps transparency`, sample);
  recordFailure(failures, sample.nonTransparent > 0, `${name} is not empty`, sample);
  recordFailure(failures, sample.occupancy >= (options.minOccupancy || 0.08),
    `${name} has sufficient figure occupancy`, sample);
  recordFailure(failures, sample.uniquePalette >= 8 && sample.uniquePalette <= 32,
    `${name} preserves an indexed-size authored palette`, sample);
  recordFailure(failures, sample.bounds && sample.bounds.width >= sample.width * 0.42 &&
    sample.bounds.height >= sample.height * 0.5, `${name} has full-body bounds`, sample);
  recordFailure(failures, sample.centroid &&
    Math.abs(sample.centroid.x / sample.width - 0.5) <= 0.22 &&
    Math.abs(sample.centroid.y / sample.height - 0.5) <= 0.25,
    `${name} figure is reasonably centered`, sample);
}

async function waitForProducedFrame(page, previousRenders){
  await page.waitForFunction(previous =>
    window.__perezosHarness.controller.getDiagnostics().renders > previous,
  previousRenders, {timeout:10_000});
}

async function setContextAndSample(page, name, patch){
  const previous = await page.evaluate(() =>
    window.__perezosHarness.controller.getDiagnostics().renders);
  await page.evaluate(value => window.__perezosHarness.applyContext(value), patch);
  await waitForProducedFrame(page, previous);
  await page.waitForTimeout(900);
  return page.evaluate(() => window.__perezosHarness.pixelSample());
}

async function assertPaused(page, action, failures, label){
  await action();
  await page.waitForTimeout(150);
  const before = await page.evaluate(() =>
    window.__perezosHarness.controller.getDiagnostics());
  await page.waitForTimeout(1_100);
  const after = await page.evaluate(() =>
    window.__perezosHarness.controller.getDiagnostics());
  recordFailure(failures, after.updates === before.updates && after.renders === before.renders,
    `${label} performs zero engine work`, {before, after});
  return {before, after};
}

async function runPerezOSE2E(options = {}){
  const {chromium} = loadPlaywright({...options,
    requireFn:options.requireFn || requireCachedPlaywright});
  const {server, base} = await createServer();
  let browser;
  try{
    const launch = {headless:true, args:["--disable-dev-shm-usage", "--no-sandbox"]};
    if(fs.existsSync(CHROME)) launch.executablePath = CHROME;
    browser = await chromium.launch(launch);
    const browserContext = await browser.newContext({viewport:VIEWPORT,
      deviceScaleFactor:1, colorScheme:"dark", reducedMotion:"no-preference"});
    const visualFailures = [];
    const lifecycleFailures = [];
    const accessibilityFailures = [];
    const browserErrors = [];
    const visuals = {};

    const dashboard = await browserContext.newPage();
    dashboard.on("pageerror", error => browserErrors.push(`dashboard: ${error.message}`));
    await dashboard.goto(`${base}/`, {waitUntil:"domcontentloaded"});
    await dashboard.waitForSelector("#centro .perezos-stage .perezos-canvas", {timeout:10_000});
    await dashboard.waitForFunction(() => {
      const canvas = document.querySelector("#centro .perezos-canvas");
      if(!canvas) return false;
      const data = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
      for(let index = 3; index < data.length; index += 4) if(data[index]) return true;
      return false;
    });
    visuals.dashboard = await sampleCanvas(dashboard, "#centro .perezos-canvas");
    assertVisual(visuals.dashboard, "dashboard integration", visualFailures, {minOccupancy:0.08});
    const dashboardContract = await dashboard.evaluate(() => ({
      canvases:document.querySelectorAll("#centro .perezos-canvas").length,
      stages:document.querySelectorAll("#centro .perezos-stage").length,
      role:document.querySelector("#centro .perezos-stage")?.getAttribute("role") ||
        document.querySelector("#centro .perezos-stage")?.tagName.toLowerCase(),
      label:document.querySelector("#centro .perezos-stage")?.getAttribute("aria-label"),
      canvasHidden:document.querySelector("#centro .perezos-canvas")?.getAttribute("aria-hidden"),
      canvasZ:getComputedStyle(document.querySelector("#centro .perezos-canvas")).zIndex,
      edgeZ:getComputedStyle(document.querySelector("#centro .perezos-stage"), "::after").zIndex,
    }));
    recordFailure(visualFailures, dashboardContract.canvases === 1 && dashboardContract.stages === 1,
      "dashboard owns exactly one PerezOS stage/canvas", dashboardContract);
    recordFailure(accessibilityFailures, dashboardContract.role === "button" &&
      /PerezOS/.test(dashboardContract.label || "") && dashboardContract.canvasHidden === "true",
      "dashboard stage exposes button semantics and hides decorative canvas", dashboardContract);
    recordFailure(visualFailures, Number(dashboardContract.edgeZ) > Number(dashboardContract.canvasZ),
      "panel edge occludes the canvas in authored composition", dashboardContract);
    await dashboard.close();

    const page = await browserContext.newPage();
    page.on("pageerror", error => browserErrors.push(`harness: ${error.message}`));
    await page.goto(`${base}/perezos-harness.html`, {waitUntil:"networkidle"});
    await page.waitForFunction(() => window.__perezosHarness &&
      window.__perezosHarness.controller.getDiagnostics().updates > 0 &&
      window.__perezosHarness.controller.getDiagnostics().renders > 0, null, {timeout:10_000});

    const access = await page.evaluate(() => {
      const stage = window.__perezosHarness.stage;
      const description = document.getElementById("mascot-description");
      const toggle = document.getElementById("mascot-toggle");
      return {tag:stage.tagName.toLowerCase(), label:stage.getAttribute("aria-label"),
        describedBy:stage.getAttribute("aria-describedby"), tabIndex:stage.tabIndex,
        descriptionLive:description.getAttribute("aria-live"),
        toggleRole:toggle.getAttribute("role"), toggleChecked:toggle.getAttribute("aria-checked")};
    });
    recordFailure(accessibilityFailures, access.tag === "button" && access.tabIndex === 0 &&
      /PerezOS/.test(access.label || ""), "stage is keyboard reachable with localized name", access);
    recordFailure(accessibilityFailures, access.descriptionLive === "polite" &&
      access.describedBy === "mascot-description", "state description is quiet and associated", access);
    recordFailure(accessibilityFailures, access.toggleRole === "switch" && access.toggleChecked === "true",
      "mascot preference exposes truthful switch semantics", access);

    const identityBefore = await page.evaluate(() => {
      const h = window.__perezosHarness;
      window.__task9Canvas = h.canvas;
      window.__task9Controller = h.controller;
      return h.controller.getDiagnostics();
    });
    visuals.idle = await page.evaluate(() => window.__perezosHarness.pixelSample());
    assertVisual(visuals.idle, "Full idle", visualFailures);

    const stage = page.getByRole("button", {name:/PerezOS/});
    await stage.click();
    await page.waitForTimeout(800);
    await stage.press("Enter");
    await page.waitForTimeout(800);
    await stage.press("Space");
    await page.waitForTimeout(800);
    const interaction = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    recordFailure(lifecycleFailures, interaction.interactions.activationAccepted >= 3,
      "click, Enter, and Space schedule acknowledgements", interaction.interactions);
    const beforeNeighbor = interaction.interactions.activationAccepted;
    await page.getByRole("button", {name:"Control vecino"}).click();
    const afterNeighbor = await page.evaluate(() => ({
      clicks:window.__perezosHarness.neighboringClicks,
      activations:window.__perezosHarness.controller.getDiagnostics().interactions.activationAccepted,
    }));
    recordFailure(lifecycleFailures, afterNeighbor.clicks === 1 &&
      afterNeighbor.activations === beforeNeighbor,
    "neighboring control is isolated from mascot interaction", afterNeighbor);
    await stage.dispatchEvent("pointermove", {clientX:40, clientY:40});
    for(let index = 0; index < 8; index += 1){
      await stage.dispatchEvent("pointermove", {clientX:42, clientY:41});
    }
    await page.waitForTimeout(120);
    await stage.dispatchEvent("pointermove", {clientX:45, clientY:44});
    const pointer = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    recordFailure(lifecycleFailures, pointer.interactions.pointerAccepted >= 2 &&
      pointer.interactions.pointerDropped >= 1 && pointer.habituation > 0,
    "pointer sampling is bounded and habituates", pointer.interactions);

    visuals.activation = await page.evaluate(() => window.__perezosHarness.pixelSample());
    visuals.slipRecovery = await page.evaluate(() => window.__perezosHarness.renderSlipRecovery());
    assertVisual(visuals.slipRecovery, "slip recovery", visualFailures);
    const slipRecoveryRepeat = await page.evaluate(() =>
      window.__perezosHarness.renderSlipRecovery());
    recordFailure(visualFailures,
      slipRecoveryRepeat.hash === visuals.slipRecovery.hash &&
      slipRecoveryRepeat.nonTransparent === visuals.slipRecovery.nonTransparent &&
      JSON.stringify(slipRecoveryRepeat.bounds) === JSON.stringify(visuals.slipRecovery.bounds),
    "seeded slip recovery reproduces the same pixels and bounds",
    {first:visuals.slipRecovery, repeat:slipRecoveryRepeat});
    visuals.working = await setContextAndSample(page, "working", {status:"working"});
    visuals.theme = await setContextAndSample(page, "theme", {theme:"dia",
      colors:{brand:"#3B67FF",panel:"#F2F1EC",line:"#B9BDC8"}});
    visuals.done = await setContextAndSample(page, "done", {status:"done"});
    visuals.waiting = await setContextAndSample(page, "waiting", {status:"waiting"});
    visuals.dead = await setContextAndSample(page, "dead", {status:"dead"});
    for(const [name, sample] of Object.entries(visuals)) assertVisual(sample, name, visualFailures);
    const distinctHashes = new Set(Object.values(visuals).map(sample => sample.hash));
    recordFailure(visualFailures, distinctHashes.size >= 6,
      "state/theme/action poses produce distinct deterministic pixel hashes",
      {count:distinctHashes.size, hashes:Object.fromEntries(Object.entries(visuals)
        .map(([name, sample]) => [name, sample.hash]))});

    await page.evaluate(() => {
      const h = window.__perezosHarness;
      h.setVisible(false);
      h.controller.setViewport(180, 148, 1);
      h.setVisible(true);
    });
    await page.waitForTimeout(500);
    visuals.narrow = await page.evaluate(() => window.__perezosHarness.pixelSample());
    assertVisual(visuals.narrow, "narrow camera", visualFailures, {minOccupancy:0.08});
    recordFailure(visualFailures, visuals.narrow.width === 180 && visuals.narrow.height === 148,
      "narrow camera uses exact authored logical bounds", visuals.narrow);
    await page.evaluate(() => window.__perezosHarness.controller.setViewport(256, 208, 1));
    await page.waitForTimeout(300);

    const identityAfter = await page.evaluate(() => {
      const h = window.__perezosHarness;
      return {sameCanvas:window.__task9Canvas === h.canvas,
        sameController:window.__task9Controller === h.controller,
        diagnostics:h.controller.getDiagnostics()};
    });
    recordFailure(lifecycleFailures, identityAfter.sameCanvas && identityAfter.sameController &&
      identityAfter.diagnostics.controllerIdentity === identityBefore.controllerIdentity &&
      identityAfter.diagnostics.rendererIdentity === identityBefore.rendererIdentity,
    "canvas/controller/renderer identity survives state, theme, and viewport changes", identityAfter);

    await page.emulateMedia({reducedMotion:"reduce"});
    await page.waitForFunction(() =>
      window.__perezosHarness.controller.getDiagnostics().quality === "static");
    const reducedBefore = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    await page.waitForTimeout(1_100);
    const reducedAfter = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    recordFailure(accessibilityFailures, reducedAfter.quality === "static" &&
      reducedAfter.updates === reducedBefore.updates,
    "prefers-reduced-motion selects event-driven Static with no continuous work",
    {before:reducedBefore, after:reducedAfter});
    await page.emulateMedia({reducedMotion:"no-preference"});
    await page.waitForFunction(() =>
      window.__perezosHarness.controller.getDiagnostics().quality !== "static");

    await assertPaused(page,
      () => page.evaluate(() => window.__perezosHarness.setVisible(false)),
      lifecycleFailures, "hidden mascot preference");
    await page.evaluate(() => window.__perezosHarness.setVisible(true));
    await page.waitForTimeout(300);
    await assertPaused(page,
      () => page.evaluate(() => window.__perezosHarness.setOffscreen(true)),
      lifecycleFailures, "offscreen stage");
    await page.evaluate(() => window.__perezosHarness.setOffscreen(false));
    await page.waitForTimeout(300);
    const hiddenPause = await assertPaused(page,
      () => page.evaluate(() => window.__perezosHarness.setHidden(true)),
      lifecycleFailures, "hidden document");
    const resumedAt = Date.now();
    await page.evaluate(() => window.__perezosHarness.setHidden(false));
    await page.waitForTimeout(300);
    const hiddenResume = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    const resumeElapsedMs = Date.now() - resumedAt;
    const maximumFreshUpdates = Math.ceil((resumeElapsedMs + 100) / (1000 / 30)) + 1;
    recordFailure(lifecycleFailures,
      hiddenResume.updates - hiddenPause.after.updates <= maximumFreshUpdates &&
      hiddenResume.visibleTimeMs - hiddenPause.after.visibleTimeMs <= resumeElapsedMs + 100 &&
      hiddenResume.maxStepMs <= 100,
    "document resume advances only fresh visible time without replay burst",
    {paused:hiddenPause.after, resumed:hiddenResume, resumeElapsedMs, maximumFreshUpdates});

    const resizeBefore = await page.evaluate(() => {
      const h = window.__perezosHarness;
      h.setVisible(false);
      return h.controller.getDiagnostics();
    });
    await page.evaluate(() => window.__perezosHarness.controller.setViewport(224, 192, 1));
    const resizeAfter = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    recordFailure(lifecycleFailures, resizeAfter.rigIdentity === resizeBefore.rigIdentity &&
      resizeAfter.poseHash === resizeBefore.poseHash &&
      resizeAfter.contactSignature === resizeBefore.contactSignature &&
      resizeAfter.randomState === resizeBefore.randomState,
    "resize preserves pose, contacts, rig, and deterministic sequence",
    {before:resizeBefore, after:resizeAfter});
    await page.evaluate(() => window.__perezosHarness.setVisible(true));
    await page.waitForTimeout(300);

    const sessionBefore = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    await page.evaluate(() => window.__perezosHarness.applyContext({
      sessionId:"task9-performance",status:"idle",theme:"noche",
      colors:{brand:"#8B7CFF",panel:"#121722",line:"#222A3A"}}));
    await page.waitForTimeout(500);
    const sessionAfter = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    recordFailure(lifecycleFailures, sessionAfter.controllerIdentity === sessionBefore.controllerIdentity &&
      sessionAfter.rendererIdentity === sessionBefore.rendererIdentity &&
      sessionAfter.rigIdentity !== sessionBefore.rigIdentity &&
      sessionAfter.sessionGeneration === sessionBefore.sessionGeneration + 1,
    "session switch resets personality state without replacing controller/renderer",
    {before:sessionBefore, after:sessionAfter});

    await page.waitForTimeout(PERFORMANCE.warmupMs);
    const allocationBaseline = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics().hotLoopBufferReplacements);
    await page.locator("#stage-wrap").screenshot({path:IDLE_SCREENSHOT, animations:"disabled"});
    await page.waitForTimeout(PERFORMANCE.sampleMs);
    const idlePerformance = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    recordFailure(lifecycleFailures, idlePerformance.quality === "full",
      "performance idle sample remains Full", idlePerformance);

    await page.evaluate(() => window.__perezosHarness.applyContext({status:"idle"}));
    const actionDeadline = Date.now() + PERFORMANCE.sampleMs;
    let capturedAction = false;
    while(Date.now() < actionDeadline){
      await page.evaluate(() =>
        window.__perezosHarness.controller.notifyInteraction("activate", 128, 104));
      if(!capturedAction){
        await page.waitForTimeout(350);
        await page.locator("#stage-wrap").screenshot({path:ACTION_SCREENSHOT,
          animations:"disabled"});
        capturedAction = true;
      }
      await page.waitForTimeout(850);
    }
    const actionPerformance = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    const steadyAllocations = actionPerformance.hotLoopBufferReplacements - allocationBaseline;
    const performance = {
      averageMs:Math.max(idlePerformance.timings.average, actionPerformance.timings.average),
      p95Ms:Math.max(idlePerformance.timings.p95, actionPerformance.timings.p95),
      decodedBytes:Math.max(idlePerformance.decodedBytes, actionPerformance.decodedBytes),
      steadyAllocations,
      allocationCounter:"controller hot-loop typed-buffer identity replacements",
      idle:{averageMs:idlePerformance.timings.average, p95Ms:idlePerformance.timings.p95,
        samples:idlePerformance.timings.count, quality:idlePerformance.quality,
        update:idlePerformance.timings.update, render:idlePerformance.timings.render},
      action:{averageMs:actionPerformance.timings.average,
        p95Ms:actionPerformance.timings.p95, samples:actionPerformance.timings.count,
        quality:actionPerformance.quality,
        update:actionPerformance.timings.update, render:actionPerformance.timings.render},
      warmupMs:PERFORMANCE.warmupMs,
      sampleMsPerScenario:PERFORMANCE.sampleMs,
    };
    recordFailure(lifecycleFailures, actionPerformance.quality === "full",
      "performance action sample remains Full", actionPerformance);

    const beforeDestroy = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    const destroyed = await page.evaluate(() => {
      const h = window.__perezosHarness;
      const first = h.controller.destroy();
      const second = h.controller.destroy();
      return {first, second, diagnostics:h.controller.getDiagnostics()};
    });
    await page.waitForTimeout(1_100);
    const afterDestroy = await page.evaluate(() =>
      window.__perezosHarness.controller.getDiagnostics());
    recordFailure(lifecycleFailures, destroyed.first === true && destroyed.second === false &&
      destroyed.diagnostics.listenerCount === 0 && destroyed.diagnostics.observerCount === 0 &&
      afterDestroy.updates === beforeDestroy.updates && afterDestroy.renders === beforeDestroy.renders,
    "destroy is idempotent and leaves zero callbacks/listeners/observers/work",
    {before:beforeDestroy, destroyed, after:afterDestroy});
    recordFailure(lifecycleFailures, browserErrors.length === 0,
      "browser emitted no uncaught page errors", browserErrors);

    await browserContext.close();
    return Object.freeze({
      visualFailures,
      lifecycleFailures,
      accessibilityFailures,
      performance,
      visuals,
      screenshots:Object.freeze({idle:IDLE_SCREENSHOT, action:ACTION_SCREENSHOT}),
      browser:Object.freeze({surface:"Playwright Chromium headless", viewport:VIEWPORT,
        errors:browserErrors}),
    });
  }finally{
    if(browser) await browser.close();
    await closeServer(server);
  }
}

module.exports = {ACTION_SCREENSHOT, IDLE_SCREENSHOT, PERFORMANCE, VIEWPORT,
  analyzePixels, assertImportContract, createServer, harnessDocument,
  requireCachedPlaywright, runPerezOSE2E};

if(require.main === module){
  runPerezOSE2E().then(report => {
    process.stdout.write(`${JSON.stringify(report)}\n`);
  }).catch(error => {
    const message = error && (error.stack || error.message) || String(error);
    if(/Playwright|browserType\.launch|Executable doesn.t exist|Cannot find module/i.test(message)){
      process.stderr.write(`${message}\n`);
      process.exitCode = 77;
      return;
    }
    process.stderr.write(`${message}\n`);
    process.exitCode = 1;
  });
}
