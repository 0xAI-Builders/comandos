(function(root){
  "use strict";

  const NS = root.ComandOSPerezOS = root.ComandOSPerezOS || {};
  for(const dependency of ["Core", "Art", "Rig", "Behaviors", "Motion", "Renderer"]){
    if(!NS[dependency]) throw new Error(`ComandOSPerezOS.${dependency} must load before Engine`);
  }

  const Core = NS.Core;
  const Rig = NS.Rig;
  const Behaviors = NS.Behaviors;
  const Motion = NS.Motion;
  const Renderer = NS.Renderer;
  const QUALITY = Renderer.QUALITY;
  const QUALITY_ORDER = Object.freeze([
    QUALITY.FULL, QUALITY.BALANCED, QUALITY.ECONOMY, QUALITY.STATIC,
  ]);
  const STATUS = Object.freeze(["idle", "working", "waiting", "done", "dead"]);
  const PRESSURE = Object.freeze(["low", "medium", "high"]);
  const TIMING_CAPACITY = 240;
  const MAX_RESUME_STEP_MS = 100;
  const POINTER_INTERVAL_MS = 100;
  const ACTIVATION_INTERVAL_MS = 750;
  const DEFAULT_COLORS = Object.freeze({
    brand:"#8B7CFF", panel:"#121722", line:"#222A3A",
  });
  const CADENCE = Object.freeze({
    full:Object.freeze({action:30, idle:12}),
    balanced:Object.freeze({action:20, idle:8}),
    economy:Object.freeze({action:12, idle:4}),
    static:Object.freeze({action:0, idle:0}),
  });
  const QUIET_PHASES = Object.freeze({
    neutral:true, breathe:true, blink:true, refocus:true, perceive:true,
    "orient-gaze":true, "turn-head":true, settle:true, inspect:true, doze:true,
  });
  let nextIdentity = 1;

  function cleanString(value, fallback, maxLength){
    const text = value === undefined || value === null ? fallback : String(value);
    const normalized = text.trim().toLowerCase();
    return (normalized || fallback).slice(0, maxLength);
  }

  function color(value, fallback){
    const normalized = String(value === undefined || value === null ? "" : value)
      .trim().toUpperCase();
    return /^#[0-9A-F]{6}$/.test(normalized) ? normalized : fallback;
  }

  function normalizeContext(input){
    input = input && typeof input === "object" ? input : {};
    const colors = input.colors && typeof input.colors === "object" ? input.colors : {};
    const status = cleanString(input.status, "idle", 16);
    const pressure = cleanString(input.contextPressure, "low", 16);
    const timestamp = Number(input.timestamp);
    return Object.freeze({
      sessionId:String(input.sessionId === undefined || input.sessionId === null ?
        "session-name" : input.sessionId).slice(0, 128),
      status:STATUS.includes(status) ? status : "idle",
      role:cleanString(input.role, "daily", 32),
      costume:cleanString(input.costume, "", 64),
      contextPressure:PRESSURE.includes(pressure) ? pressure : "low",
      theme:cleanString(input.theme, "noche", 64),
      expanded:input.expanded === true || input.expanded === 1,
      timestamp:Number.isFinite(timestamp) && timestamp >= 0 ? timestamp : 0,
      colors:Object.freeze({
        brand:color(colors.brand, DEFAULT_COLORS.brand),
        panel:color(colors.panel, DEFAULT_COLORS.panel),
        line:color(colors.line, DEFAULT_COLORS.line),
      }),
    });
  }

  function contextsEqual(left, right){
    return left.sessionId === right.sessionId && left.status === right.status &&
      left.role === right.role && left.costume === right.costume &&
      left.contextPressure === right.contextPressure && left.theme === right.theme &&
      left.expanded === right.expanded && left.timestamp === right.timestamp &&
      left.colors.brand === right.colors.brand && left.colors.panel === right.colors.panel &&
      left.colors.line === right.colors.line;
  }

  function createTimingRing(){
    return {
      values:new Float64Array(TIMING_CAPACITY),
      scratch:new Float64Array(TIMING_CAPACITY),
      cursor:0, count:0, total:0,
    };
  }

  function clearTiming(ring){
    ring.values.fill(0);
    ring.scratch.fill(0);
    ring.cursor = 0;
    ring.count = 0;
    ring.total = 0;
  }

  function pushTiming(ring, value){
    value = Number.isFinite(value) && value >= 0 ? value : 0;
    if(ring.count === TIMING_CAPACITY){
      const old = ring.values[ring.cursor];
      ring.total -= old;
    }else{
      ring.count += 1;
    }
    ring.values[ring.cursor] = value;
    ring.cursor = (ring.cursor + 1) % TIMING_CAPACITY;
    ring.total += value;
  }

  function timingAverage(ring){ return ring.count ? ring.total / ring.count : 0; }

  function timingP95(ring){
    if(!ring.count) return 0;
    for(let index = 0; index < ring.count; index += 1){
      ring.scratch[index] = ring.values[index];
    }
    for(let index = ring.count; index < TIMING_CAPACITY; index += 1){
      ring.scratch[index] = Number.POSITIVE_INFINITY;
    }
    ring.scratch.sort();
    return ring.scratch[Math.ceil(ring.count * 0.95) - 1];
  }

  function qualityIndex(quality){ return QUALITY_ORDER.indexOf(quality); }

  function weakerQuality(left, right){
    return QUALITY_ORDER[Math.max(qualityIndex(left), qualityIndex(right))];
  }

  function deviceCeiling(env){
    const navigator = env.navigator || {};
    const cores = Number(navigator.hardwareConcurrency);
    const memory = Number(navigator.deviceMemory);
    if((Number.isFinite(cores) && cores <= 2) || (Number.isFinite(memory) && memory <= 2)){
      return QUALITY.ECONOMY;
    }
    if((Number.isFinite(cores) && cores <= 4) || (Number.isFinite(memory) && memory <= 4)){
      return QUALITY.BALANCED;
    }
    return QUALITY.FULL;
  }

  function viewportCeiling(width, height){
    return width < 180 || height < 148 ? QUALITY.ECONOMY : QUALITY.FULL;
  }

  function paletteTheme(theme){
    return theme === "dia" || theme === "day" || theme === "light" ? "light" : "dark";
  }

  function environment(options){
    const env = options && options.env ? options.env : root;
    const performanceNow = env.performance && typeof env.performance.now === "function" ?
      () => env.performance.now() : () => Date.now();
    return {
      source:env,
      document:env.document || null,
      navigator:env.navigator || {},
      requestAnimationFrame:typeof env.requestAnimationFrame === "function" ?
        env.requestAnimationFrame.bind(env) : root.requestAnimationFrame.bind(root),
      cancelAnimationFrame:typeof env.cancelAnimationFrame === "function" ?
        env.cancelAnimationFrame.bind(env) : root.cancelAnimationFrame.bind(root),
      IntersectionObserver:env.IntersectionObserver || root.IntersectionObserver || null,
      ResizeObserver:env.ResizeObserver || root.ResizeObserver || null,
      matchMedia:typeof env.matchMedia === "function" ? env.matchMedia.bind(env) : null,
      now:typeof env.now === "function" ? env.now.bind(env) : performanceNow,
      sampleCost:typeof env.sampleCost === "function" ? env.sampleCost.bind(env) : null,
      canvasFactory:options && typeof options.canvasFactory === "function" ?
        options.canvasFactory : typeof env.canvasFactory === "function" ?
          env.canvasFactory.bind(env) : undefined,
      dpr:Number(env.devicePixelRatio) || Number(root.devicePixelRatio) || 1,
    };
  }

  function measuredCost(internal, kind, start){
    const measured = Math.max(0, internal.browser.now() - start);
    if(!internal.browser.sampleCost) return measured;
    const sampled = internal.browser.sampleCost(kind, measured);
    return Number.isFinite(sampled) && sampled >= 0 ? sampled : measured;
  }

  function personalitySignature(personality){
    return [personality.preferredSide, personality.blinkMs,
      personality.curiosity.toFixed(6), personality.sleepBias.toFixed(6),
      personality.gripCaution.toFixed(6)].join(":");
  }

  function createSession(internal, sessionId){
    const rig = Rig.createRig(sessionId);
    const director = Behaviors.createDirector(sessionId);
    const motion = Motion.createMotion(rig);
    internal.rig = rig;
    internal.director = director;
    internal.motion = motion;
    internal.activePerformance = null;
    internal.sessionGeneration += 1;
    internal.rigIdentity = nextIdentity++;
    internal.personalitySignature = personalitySignature(director.personality);
    motion.onComplete = record => {
      if(record.status === "completed" && internal.activePerformance &&
         record.family === internal.activePerformance.family){
        Behaviors.completePerformance(internal.director, internal.activePerformance,
          internal.visibleTimeMs);
        internal.activePerformance = null;
      }
    };
    internal.bufferValues = rig.values;
    internal.bufferTargets = rig.targets;
    internal.bufferOwners = motion.owners;
  }

  function auditBufferIdentities(internal){
    if(internal.rig.values !== internal.bufferValues ||
       internal.rig.targets !== internal.bufferTargets ||
       internal.motion.owners !== internal.bufferOwners ||
       internal.timing.values !== internal.timingValues ||
       internal.timing.scratch !== internal.timingScratch){
      internal.hotLoopBufferReplacements += 1;
      internal.bufferValues = internal.rig.values;
      internal.bufferTargets = internal.rig.targets;
      internal.bufferOwners = internal.motion.owners;
      internal.timingValues = internal.timing.values;
      internal.timingScratch = internal.timing.scratch;
    }
  }

  function ensurePerformance(internal){
    if(!Motion.isIdle(internal.motion)) return;
    const performance = Behaviors.nextPerformance(internal.director, internal.visibleTimeMs);
    if(Motion.enqueue(internal.motion, performance, internal.visibleTimeMs)){
      internal.activePerformance = performance;
    }
  }

  function hasActiveGesture(internal){
    const phase = internal.motion.phase;
    return !!(phase && !QUIET_PHASES[phase.primitive]);
  }

  function setQuality(internal, quality, automatic){
    if(internal.fallback || internal.reduced) quality = QUALITY.STATIC;
    else quality = weakerQuality(quality, internal.qualityCeiling);
    if(quality === internal.quality) return false;
    internal.quality = quality;
    internal.overBudgetFrames = 0;
    internal.underBudgetFrames = 0;
    clearTiming(internal.timing);
    internal.qualityTransitions += 1;
    if(automatic) internal.governorTransitions += 1;
    if(internal.renderer) Renderer.markDirty(internal.renderer, "quality");
    internal.staticDirty = true;
    return true;
  }

  function governQuality(internal){
    const average = timingAverage(internal.timing);
    const p95 = timingP95(internal.timing);
    const over = average > 1 || p95 > 2;
    const under = average < 0.65 && p95 < 1.25;
    internal.overBudgetFrames = over ? internal.overBudgetFrames + 1 : 0;
    internal.underBudgetFrames = under ? internal.underBudgetFrames + 1 : 0;
    if(internal.overBudgetFrames >= 120){
      const index = qualityIndex(internal.quality);
      if(index >= 0 && index < qualityIndex(QUALITY.ECONOMY)){
        setQuality(internal, QUALITY_ORDER[index + 1], true);
      }else{
        internal.overBudgetFrames = 0;
      }
      return;
    }
    if(internal.underBudgetFrames >= 600){
      const index = qualityIndex(internal.quality);
      const ceilingIndex = qualityIndex(internal.qualityCeiling);
      if(index > ceilingIndex && index <= qualityIndex(QUALITY.ECONOMY)){
        setQuality(internal, QUALITY_ORDER[index - 1], true);
      }else{
        internal.underBudgetFrames = 0;
      }
    }
  }

  function fallbackRender(internal){
    if(!internal.staticDirty) return false;
    const ctx = internal.fallbackContext;
    if(!ctx) return false;
    const width = Number(internal.canvas.width) || internal.viewportWidth;
    const height = Number(internal.canvas.height) || internal.viewportHeight;
    const cx = Math.floor(width / 2);
    const cy = Math.floor(height / 2);
    const scale = internal.dpr;
    try{
      ctx.imageSmoothingEnabled = false;
      if(typeof ctx.setTransform === "function") ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, internal.canvas.width, internal.canvas.height);
      ctx.fillStyle = "#3A281F";
      ctx.fillRect(cx - 18 * scale, cy - 23 * scale, 36 * scale, 53 * scale);
      ctx.fillRect(cx - 28 * scale, cy - 17 * scale, 10 * scale, 42 * scale);
      ctx.fillRect(cx + 18 * scale, cy - 17 * scale, 10 * scale, 42 * scale);
      ctx.fillStyle = "#C7A77C";
      ctx.fillRect(cx - 14 * scale, cy - 18 * scale, 28 * scale, 23 * scale);
      ctx.fillStyle = "#171310";
      ctx.fillRect(cx - 8 * scale, cy - 10 * scale, 4 * scale, 4 * scale);
      ctx.fillRect(cx + 4 * scale, cy - 10 * scale, 4 * scale, 4 * scale);
      ctx.fillRect(cx - 3 * scale, cy - 4 * scale, 6 * scale, 4 * scale);
      ctx.fillStyle = "#D9C6A6";
      for(let side = -1; side <= 1; side += 2){
        ctx.fillRect(cx + side * 24 * scale - scale, cy + 23 * scale,
          2 * scale, 8 * scale);
        ctx.fillRect(cx + side * 28 * scale - scale, cy + 21 * scale,
          2 * scale, 8 * scale);
        ctx.fillRect(cx + side * 32 * scale - scale, cy + 19 * scale,
          2 * scale, 8 * scale);
      }
      internal.staticDirty = false;
      return true;
    }catch(error){
      return false;
    }
  }

  function renderFrame(internal){
    const start = internal.browser.now();
    const rendered = internal.fallback ? fallbackRender(internal) :
      Renderer.render(internal.renderer, internal.rig, internal.context, internal.quality);
    const cost = measuredCost(internal, "render", start);
    if(rendered) internal.renders += 1;
    else internal.skippedCleanRenders += 1;
    return rendered ? cost : -1;
  }

  function updateFrame(internal, stepMs){
    const start = internal.browser.now();
    ensurePerformance(internal);
    const solved = Motion.stepMotion(internal.motion, stepMs / 1000, internal.visibleTimeMs);
    internal.visibleTimeMs += stepMs;
    internal.updates += 1;
    if(stepMs > internal.maxStepMs) internal.maxStepMs = stepMs;
    if(!solved) internal.motionFailures += 1;
    auditBufferIdentities(internal);
    return measuredCost(internal, "update", start);
  }

  function isRunning(internal){
    const documentHidden = internal.browser.document && internal.browser.document.hidden === true;
    return internal.userVisible && !documentHidden && internal.intersecting && !internal.destroyed;
  }

  function schedule(internal){
    if(internal.frameId !== 0 || !isRunning(internal)) return;
    if(internal.quality === QUALITY.STATIC && !internal.staticDirty) return;
    internal.frameId = internal.browser.requestAnimationFrame(internal.frameCallback);
  }

  function cancelSchedule(internal){
    if(internal.frameId !== 0){
      internal.browser.cancelAnimationFrame(internal.frameId);
      internal.frameId = 0;
    }
    internal.lastWakeMs = -1;
    internal.accumulatorMs = 0;
  }

  function refreshScheduling(internal){
    if(isRunning(internal)) schedule(internal);
    else cancelSchedule(internal);
  }

  function runWakeup(internal, timestamp){
    internal.frameId = 0;
    if(!isRunning(internal)) return;
    internal.wakeups += 1;
    if(internal.quality === QUALITY.STATIC){
      const renderCost = renderFrame(internal);
      internal.staticDirty = false;
      if(renderCost >= 0) pushTiming(internal.timing, renderCost);
      return;
    }
    const now = Number.isFinite(timestamp) ? timestamp : internal.browser.now();
    if(internal.lastWakeMs < 0){
      internal.lastWakeMs = now;
      internal.updateImmediately = true;
    }else{
      let elapsed = now - internal.lastWakeMs;
      internal.lastWakeMs = now;
      if(!Number.isFinite(elapsed) || elapsed < 0) elapsed = 0;
      internal.accumulatorMs += Math.min(elapsed, MAX_RESUME_STEP_MS);
    }
    const active = hasActiveGesture(internal);
    const frequency = CADENCE[internal.quality][active ? "action" : "idle"];
    const interval = 1000 / frequency;
    if(internal.updateImmediately || internal.accumulatorMs + 1e-9 >= interval){
      let stepMs = internal.updateImmediately ? 0.001 : internal.accumulatorMs;
      internal.updateImmediately = false;
      internal.accumulatorMs = 0;
      stepMs = Math.min(MAX_RESUME_STEP_MS, Math.max(0.001, stepMs));
      const updateCost = updateFrame(internal, stepMs);
      const renderCost = renderFrame(internal);
      if(renderCost >= 0){
        pushTiming(internal.timing, updateCost + renderCost);
        governQuality(internal);
      }
    }
    schedule(internal);
  }

  function updateCeiling(internal){
    internal.viewportCeiling = viewportCeiling(internal.viewportWidth, internal.viewportHeight);
    internal.qualityCeiling = weakerQuality(internal.deviceCeiling, internal.viewportCeiling);
    if(!internal.reduced && !internal.fallback &&
       qualityIndex(internal.quality) < qualityIndex(internal.qualityCeiling)){
      setQuality(internal, internal.qualityCeiling, false);
    }
  }

  function applyViewport(internal, width, height, dpr){
    if(!Number.isFinite(width) || !Number.isFinite(height) || !Number.isFinite(dpr) ||
       width <= 0 || height <= 0 || dpr <= 0) return false;
    width = Math.floor(width);
    height = Math.floor(height);
    dpr = Math.max(1, Math.floor(dpr));
    if(width === internal.viewportWidth && height === internal.viewportHeight &&
       dpr === internal.dpr) return false;
    internal.viewportWidth = width;
    internal.viewportHeight = height;
    internal.dpr = dpr;
    updateCeiling(internal);
    if(internal.renderer) Renderer.setViewport(internal.renderer, width, height, dpr);
    else{
      try{
        internal.canvas.width = width * dpr;
        internal.canvas.height = height * dpr;
      }catch(error){ /* fallback keeps its last usable backing store */ }
    }
    internal.staticDirty = true;
    schedule(internal);
    return true;
  }

  function attachLifecycle(internal){
    const doc = internal.browser.document;
    if(doc && typeof doc.addEventListener === "function"){
      doc.addEventListener("visibilitychange", internal.visibilityListener);
      internal.visibilityRegistered = true;
      internal.listenerCount += 1;
    }
    if(internal.media){
      if(typeof internal.media.addEventListener === "function"){
        internal.media.addEventListener("change", internal.mediaListener);
        internal.mediaMode = 1;
        internal.listenerCount += 1;
      }else if(typeof internal.media.addListener === "function"){
        internal.media.addListener(internal.mediaListener);
        internal.mediaMode = 2;
        internal.listenerCount += 1;
      }
    }
    if(internal.browser.IntersectionObserver){
      internal.intersectionObserver = new internal.browser.IntersectionObserver(
        internal.intersectionListener);
      internal.intersectionObserver.observe(internal.canvas);
      internal.observerCount += 1;
    }
    if(internal.browser.ResizeObserver){
      internal.resizeObserver = new internal.browser.ResizeObserver(internal.resizeListener);
      internal.resizeObserver.observe(internal.canvas);
      internal.observerCount += 1;
    }
  }

  function detachLifecycle(internal){
    const doc = internal.browser.document;
    if(internal.visibilityRegistered && doc && typeof doc.removeEventListener === "function"){
      doc.removeEventListener("visibilitychange", internal.visibilityListener);
      internal.visibilityRegistered = false;
      internal.listenerCount -= 1;
    }
    if(internal.mediaMode === 1){
      internal.media.removeEventListener("change", internal.mediaListener);
      internal.listenerCount -= 1;
    }else if(internal.mediaMode === 2){
      internal.media.removeListener(internal.mediaListener);
      internal.listenerCount -= 1;
    }
    internal.mediaMode = 0;
    if(internal.intersectionObserver){
      internal.intersectionObserver.disconnect();
      internal.intersectionObserver = null;
      internal.observerCount -= 1;
    }
    if(internal.resizeObserver){
      internal.resizeObserver.disconnect();
      internal.resizeObserver = null;
      internal.observerCount -= 1;
    }
  }

  function createPerezOS(canvas, options){
    if(!canvas || typeof canvas.getContext !== "function"){
      throw new TypeError("createPerezOS requires one canvas");
    }
    options = options && typeof options === "object" ? options : {};
    const browser = environment(options);
    const timing = createTimingRing();
    const initialWidth = Math.max(1, Number(canvas.clientWidth) || Number(canvas.width) || 256);
    const initialHeight = Math.max(1, Number(canvas.clientHeight) || Number(canvas.height) || 208);
    const media = browser.matchMedia ? browser.matchMedia("(prefers-reduced-motion: reduce)") : null;
    const internal = {
      canvas, browser, media, renderer:null, fallbackContext:null,
      controllerIdentity:nextIdentity++, rendererIdentity:nextIdentity++, rigIdentity:0,
      context:normalizeContext({}), timing,
      timingValues:timing.values, timingScratch:timing.scratch,
      rig:null, director:null, motion:null, activePerformance:null,
      bufferValues:null, bufferTargets:null, bufferOwners:null,
      sessionGeneration:0, personalitySignature:"",
      viewportWidth:0, viewportHeight:0, dpr:0,
      deviceCeiling:deviceCeiling(browser), viewportCeiling:QUALITY.FULL,
      qualityCeiling:QUALITY.FULL, quality:QUALITY.FULL,
      userVisible:options.visible !== false, intersecting:true,
      explicitReduced:false, mediaReduced:!!(media && media.matches), reduced:false,
      fallback:false, decodeFailures:0, destroyed:false, staticDirty:true,
      frameId:0, lastWakeMs:-1, accumulatorMs:0, updateImmediately:true,
      visibleTimeMs:0, maxStepMs:0, wakeups:0, updates:0, renders:0,
      skippedCleanRenders:0, motionFailures:0, qualityTransitions:0,
      governorTransitions:0,
      overBudgetFrames:0, underBudgetFrames:0, hotLoopBufferReplacements:0,
      pointerAccepted:0, pointerDropped:0, activationAccepted:0,
      activationDropped:0, lastPointerMs:-POINTER_INTERVAL_MS,
      lastActivationMs:-ACTIVATION_INTERVAL_MS,
      listenerCount:0, observerCount:0, visibilityRegistered:false,
      mediaMode:0, intersectionObserver:null, resizeObserver:null,
      frameCallback:null, visibilityListener:null, mediaListener:null,
      intersectionListener:null, resizeListener:null,
    };
    internal.reduced = internal.explicitReduced || internal.mediaReduced;
    createSession(internal, internal.context.sessionId);
    try{
      internal.renderer = Renderer.createRenderer(canvas, {
        canvasFactory:browser.canvasFactory,
        theme:paletteTheme(internal.context.theme),
      });
    }catch(error){
      internal.fallback = true;
      internal.decodeFailures = 1;
      try{ internal.fallbackContext = canvas.getContext("2d", {alpha:true}); }
      catch(contextError){ internal.fallbackContext = null; }
    }
    internal.frameCallback = timestamp => runWakeup(internal, timestamp);
    internal.visibilityListener = () => refreshScheduling(internal);
    internal.mediaListener = event => {
      internal.mediaReduced = !!(event && event.matches !== undefined ? event.matches :
        internal.media && internal.media.matches);
      const reduced = internal.explicitReduced || internal.mediaReduced;
      if(reduced !== internal.reduced){
        internal.reduced = reduced;
        setQuality(internal, reduced ? QUALITY.STATIC : internal.qualityCeiling, false);
        internal.staticDirty = true;
        internal.updateImmediately = true;
        refreshScheduling(internal);
      }
    };
    internal.intersectionListener = entries => {
      const entry = entries && entries[entries.length - 1];
      const intersecting = !!(entry && entry.isIntersecting && entry.intersectionRatio !== 0);
      if(intersecting === internal.intersecting) return;
      internal.intersecting = intersecting;
      refreshScheduling(internal);
    };
    internal.resizeListener = entries => {
      const entry = entries && entries[entries.length - 1];
      if(!entry || !entry.contentRect) return;
      applyViewport(internal, entry.contentRect.width, entry.contentRect.height,
        internal.browser.dpr);
    };
    applyViewport(internal, initialWidth, initialHeight, browser.dpr);
    if(internal.fallback || internal.reduced) internal.quality = QUALITY.STATIC;
    else internal.quality = internal.qualityCeiling;
    Behaviors.updateContext(internal.director, internal.context, 0);
    attachLifecycle(internal);
    schedule(internal);

    function setContext(input){
      if(internal.destroyed) return false;
      const next = normalizeContext(input);
      if(contextsEqual(internal.context, next)) return false;
      const sessionChanged = next.sessionId !== internal.context.sessionId;
      internal.context = next;
      if(sessionChanged) createSession(internal, next.sessionId);
      Behaviors.updateContext(internal.director, next, internal.visibleTimeMs);
      const performance = Behaviors.nextPerformance(internal.director, internal.visibleTimeMs);
      if(Motion.isIdle(internal.motion)){
        if(Motion.enqueue(internal.motion, performance, internal.visibleTimeMs)){
          internal.activePerformance = performance;
        }
      }else if(Motion.requestInterrupt(internal.motion, performance, internal.visibleTimeMs)){
        internal.activePerformance = performance;
      }
      if(internal.renderer){
        Renderer.setTheme(internal.renderer, paletteTheme(next.theme));
        Renderer.markDirty(internal.renderer, "context");
      }
      internal.staticDirty = true;
      schedule(internal);
      return true;
    }

    function setVisible(visible){
      if(internal.destroyed) return false;
      visible = visible === true;
      if(visible === internal.userVisible) return false;
      internal.userVisible = visible;
      refreshScheduling(internal);
      return true;
    }

    function setReducedMotion(reduced){
      if(internal.destroyed) return false;
      reduced = reduced === true;
      if(reduced === internal.explicitReduced) return false;
      internal.explicitReduced = reduced;
      const effective = internal.explicitReduced || internal.mediaReduced;
      if(effective !== internal.reduced){
        internal.reduced = effective;
        setQuality(internal, effective ? QUALITY.STATIC : internal.qualityCeiling, false);
        internal.staticDirty = true;
        internal.updateImmediately = true;
        refreshScheduling(internal);
      }
      return true;
    }

    function setViewport(width, height, dpr){
      if(internal.destroyed) return false;
      return applyViewport(internal, width, height, dpr);
    }

    function notifyInteraction(kind, x, y){
      if(internal.destroyed || !isRunning(internal) ||
         internal.context.status === "waiting" || internal.context.status === "dead") return false;
      kind = cleanString(kind, "", 24);
      const now = internal.browser.now();
      let behaviorType;
      let accepted;
      if(kind === "pointer" || kind === "hover" || kind === "move"){
        if(internal.reduced) return false;
        if(now - internal.lastPointerMs < POINTER_INTERVAL_MS){
          internal.pointerDropped += 1;
          return false;
        }
        if(!Number.isFinite(x) || !Number.isFinite(y) || x < 0 || y < 0 ||
           x > internal.viewportWidth || y > internal.viewportHeight) return false;
        internal.lastPointerMs = now;
        internal.pointerAccepted += 1;
        behaviorType = "pointer";
        accepted = true;
      }else if(kind === "activate" || kind === "click" || kind === "enter" || kind === "space"){
        if(now - internal.lastActivationMs < ACTIVATION_INTERVAL_MS){
          internal.activationDropped += 1;
          return false;
        }
        internal.lastActivationMs = now;
        internal.activationAccepted += 1;
        behaviorType = "interaction";
        accepted = true;
      }else{
        return false;
      }
      const side = Number.isFinite(x) && x < internal.viewportWidth / 2 ? "left" : "right";
      if(accepted) Behaviors.notify(internal.director, {
        type:behaviorType, target:"viewer", side,
        intensity:kind === "pointer" || kind === "hover" || kind === "move" ? 0.35 : 0.7,
      }, internal.visibleTimeMs);
      internal.staticDirty = true;
      if(internal.renderer) Renderer.markDirty(internal.renderer, "interaction");
      schedule(internal);
      return accepted;
    }

    function destroy(){
      if(internal.destroyed) return false;
      internal.destroyed = true;
      cancelSchedule(internal);
      detachLifecycle(internal);
      if(internal.renderer) Renderer.destroyRenderer(internal.renderer);
      internal.activePerformance = null;
      internal.motion = null;
      internal.director = null;
      internal.rig = null;
      internal.bufferValues = null;
      internal.bufferTargets = null;
      internal.bufferOwners = null;
      clearTiming(internal.timing);
      return true;
    }

    function getDiagnostics(){
      const rendererDiagnostics = internal.renderer ?
        Renderer.rendererDiagnostics(internal.renderer) : null;
      const rigRecoveries = internal.rig && internal.rig.diagnostics ?
        internal.rig.diagnostics.recoveries : 0;
      const drives = internal.director ? internal.director.drives : null;
      return Object.freeze({
        destroyed:internal.destroyed,
        controllerIdentity:internal.controllerIdentity,
        rendererIdentity:internal.rendererIdentity,
        rigIdentity:internal.rigIdentity,
        sessionGeneration:internal.sessionGeneration,
        personalitySignature:internal.personalitySignature,
        context:internal.context,
        quality:internal.quality,
        qualityCeiling:internal.qualityCeiling,
        qualityTransitions:internal.qualityTransitions,
        governorTransitions:internal.governorTransitions,
        visibleTimeMs:internal.visibleTimeMs,
        maxStepMs:internal.maxStepMs,
        wakeups:internal.wakeups,
        updates:internal.updates,
        renders:internal.renders,
        skippedCleanRenders:internal.skippedCleanRenders,
        solverRecoveries:rigRecoveries,
        motionFailures:internal.motionFailures,
        fallback:internal.fallback,
        decodeFailures:internal.decodeFailures,
        decodedBytes:rendererDiagnostics ? rendererDiagnostics.decodedBytes : 0,
        listenerCount:internal.listenerCount,
        observerCount:internal.observerCount,
        hotLoopBufferReplacements:internal.hotLoopBufferReplacements,
        habituation:drives ? drives.habituation : 0,
        interactions:Object.freeze({
          pointerAccepted:internal.pointerAccepted,
          pointerDropped:internal.pointerDropped,
          activationAccepted:internal.activationAccepted,
          activationDropped:internal.activationDropped,
        }),
        timings:Object.freeze({
          capacity:TIMING_CAPACITY,
          count:internal.timing.count,
          average:timingAverage(internal.timing),
          p95:timingP95(internal.timing),
        }),
      });
    }

    return Object.freeze({setContext, setVisible, setReducedMotion, setViewport,
      notifyInteraction, destroy, getDiagnostics});
  }

  Object.defineProperty(NS, "createPerezOS", {
    value:createPerezOS, enumerable:true, configurable:false, writable:false,
  });
})(typeof window !== "undefined" ? window : globalThis);
