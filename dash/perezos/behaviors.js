(function(root){
  "use strict";

  const NS = root.ComandOSPerezOS = root.ComandOSPerezOS || {};
  if(!NS.Core) throw new Error("ComandOSPerezOS.Core must load before Behaviors");

  const Core = NS.Core;
  const MAX_SAFE_TIME = Number.MAX_SAFE_INTEGER;
  const MAX_DRIVE_STEP_MS = 60 * 60 * 1000;
  const INTERACTION_LIFETIME_MS = 12000;
  const MEMORY_SIZE = 8;
  const INTERACTION_CAPACITY = 8;
  const DIRECTORS = new WeakMap();
  const SIDES = Object.freeze(["left", "right"]);
  const LIMBS = Object.freeze(["front-left", "front-right", "rear-left", "rear-right"]);
  const STATUSES = Object.freeze(["idle", "working", "waiting", "done", "dead"]);
  const PRESSURES = Object.freeze(["low", "medium", "high"]);
  const PRIORITIES = Object.freeze({dead:100, waiting:90, interaction:50,
    done:40, working:30, idle:10});
  const DEADLINES = Object.freeze({dead:8000, waiting:5000});
  const DRIVE_NAMES = Object.freeze([
    "sleepiness", "curiosity", "attention", "gripConfidence", "fatigue", "comfort",
    "boredom", "satisfaction", "alertness", "habituation",
  ]);
  const RARE_COOLDOWNS = Object.freeze({
    scratch:75000,
    yawn:90000,
    doze:180000,
    "slip-recover":120000,
  });

  const clamp01 = value => Core.clamp(Number.isFinite(value) ? value : 0, 0, 1);
  const round3 = value => Math.round(value * 1000) / 1000;
  const sideSign = side => side === "left" ? -1 : 1;
  const finiteParam = (params, name, fallback) =>
    Number.isFinite(params && params[name]) ? params[name] : fallback;
  const normalizedSide = value => value === "right" ? "right" : "left";
  const freezeTargets = targets => Object.freeze(targets);

  function clawChannels(suffix){
    const channels = [];
    for(const side of SIDES){
      for(let claw = 1; claw <= 3; claw += 1){
        channels.push(`claw-front-${side}-${claw}-${suffix}`);
      }
    }
    return channels;
  }

  function frontChannels(suffixes){
    const channels = [];
    for(const side of SIDES){
      for(const suffix of suffixes) channels.push(`front-${side}-${suffix}`);
    }
    return channels;
  }

  function sideFrontTargets(params, values){
    const side = normalizedSide(params && params.side);
    const result = {};
    for(const [suffix, value] of Object.entries(values)){
      result[`front-${side}-${suffix}`] = value;
    }
    return result;
  }

  function sideClawTargets(params, curl, spread){
    const side = normalizedSide(params && params.side);
    const result = {};
    for(let claw = 1; claw <= 3; claw += 1){
      result[`claw-front-${side}-${claw}-curl`] = clamp01(curl);
      if(spread !== undefined){
        result[`claw-front-${side}-${claw}-spread`] = Core.clamp(spread, -1, 1);
      }
    }
    return result;
  }

  function supportMode(support){
    return support && support.mode === "loaded" ? "loaded" : "released";
  }

  function sideIsFree(environment, params){
    const side = normalizedSide(params && params.side);
    const support = environment && environment.supports && environment.supports[`front-${side}`];
    return supportMode(support) !== "loaded";
  }

  function stableSupport(environment){
    if(!environment || !environment.supports) return true;
    let loaded = 0;
    for(const limb of LIMBS){
      const support = environment.supports[limb];
      if(supportMode(support) === "loaded") loaded += clamp01(support.load);
    }
    return loaded === 0 || loaded >= 0.5;
  }

  const always = () => true;
  const requiresFreeSide = (environment, params) => sideIsFree(environment, params);
  const requiresStableSupport = environment => stableSupport(environment);

  function primitive(channels, minMs, maxMs, interruptible, safeEnd, precondition, targets){
    return Object.freeze({
      channels:Object.freeze(channels.slice()),
      duration:Object.freeze({minMs, maxMs}),
      interruptible,
      safeEnd,
      precondition,
      targets(params){ return freezeTargets(targets(params || {})); },
    });
  }

  const PRIMITIVES = {
    "perceive":primitive(
      ["brow-left-lift", "brow-right-lift", "eye-left-look-y", "eye-right-look-y"],
      180, 420, true, 0.7, always, params => {
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"brow-left-lift":0.18 + intensity * 0.28,
          "brow-right-lift":0.18 + intensity * 0.28,
          "eye-left-look-y":-0.08 * intensity, "eye-right-look-y":-0.08 * intensity};
      }),
    "orient-gaze":primitive(
      ["eye-left-look-x", "eye-left-look-y", "eye-right-look-x", "eye-right-look-y",
        "face-turn"], 240, 700, true, 0.75, always, params => {
        const sign = sideSign(normalizedSide(params.side));
        const distance = clamp01(finiteParam(params, "distance", 0.5));
        const lead = clamp01(finiteParam(params, "headLead", 0.5));
        return {"eye-left-look-x":sign * distance, "eye-left-look-y":-0.12 * distance,
          "eye-right-look-x":sign * distance, "eye-right-look-y":-0.12 * distance,
          "face-turn":sign * lead * 0.35};
      }),
    "refocus":primitive(
      ["eye-left-look-x", "eye-left-look-y", "eye-right-look-x", "eye-right-look-y",
        "nose-twitch"], 160, 380, true, 0.65, always, params => {
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"eye-left-look-x":0, "eye-left-look-y":0, "eye-right-look-x":0,
          "eye-right-look-y":0, "nose-twitch":intensity * 0.3};
      }),
    "blink":primitive(
      ["lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower"],
      90, 220, false, 1, always, params => {
        const closure = 0.75 + clamp01(finiteParam(params, "intensity", 0.5)) * 0.25;
        return {"lid-left-upper":closure, "lid-left-lower":closure * 0.3,
          "lid-right-upper":closure, "lid-right-lower":closure * 0.3};
      }),
    "turn-head":primitive(
      ["head-yaw", "head-roll", "face-turn", "fur-head-crest"],
      320, 900, true, 0.75, always, params => {
        const sign = sideSign(normalizedSide(params.side));
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        const follow = clamp01(finiteParam(params, "furFollowThrough", 0.5));
        return {"head-yaw":sign * (0.18 + 0.5 * intensity),
          "head-roll":-sign * 0.08 * intensity, "face-turn":sign * 0.5 * intensity,
          "fur-head-crest":sign * 0.25 * follow};
      }),
    "breathe":primitive(
      ["chest-expand", "belly-compress", "fur-belly-chest"],
      700, 1800, true, 0.55, always, params => {
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"chest-expand":0.12 + intensity * 0.28,
          "belly-compress":-0.08 - intensity * 0.2,
          "fur-belly-chest":intensity * 0.15};
      }),
    "brace":primitive(
      ["body-lean-x", "body-lift", "cable-tension", "spine-pelvis-angle"],
      280, 720, false, 0.9, requiresStableSupport, params => {
        const sign = sideSign(normalizedSide(params.side));
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"body-lean-x":-sign * intensity * 2.5, "body-lift":intensity * 1.5,
          "cable-tension":0.96 + intensity * 0.04,
          "spine-pelvis-angle":-sign * intensity * 0.06};
      }),
    "shift-weight":primitive(
      ["body-lean-x", "spine-pelvis-x", "spine-lower-angle", "cable-contact-bias"],
      420, 1100, false, 0.85, requiresStableSupport, params => {
        const sign = sideSign(normalizedSide(params.side));
        const distance = clamp01(finiteParam(params, "distance", 0.5));
        return {"body-lean-x":sign * (1.5 + distance * 4), "spine-pelvis-x":sign * distance * 3,
          "spine-lower-angle":-sign * distance * 0.15,
          "cable-contact-bias":sign * distance * 0.25};
      }),
    "reach":primitive(
      frontChannels(["reach-x", "reach-y", "lift", "wrist-angle"]),
      420, 1200, false, 0.9, requiresFreeSide, params => {
        const sign = sideSign(normalizedSide(params.side));
        const distance = clamp01(finiteParam(params, "distance", 0.5));
        return sideFrontTargets(params, {"reach-x":sign * (4 + distance * 10),
          "reach-y":-2 - distance * 6, "lift":3 + distance * 6,
          "wrist-angle":-sign * 0.2 * distance});
      }),
    "search":primitive(
      ["head-yaw", "eye-left-look-x", "eye-right-look-x", "light-searching-pulse",
        "fur-head-nape"], 500, 1400, true, 0.7, always, params => {
        const sign = sideSign(normalizedSide(params.side));
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"head-yaw":sign * (0.25 + intensity * 0.45),
          "eye-left-look-x":-sign * 0.7, "eye-right-look-x":-sign * 0.7,
          "light-searching-pulse":0.2 + intensity * 0.7,
          "fur-head-nape":sign * intensity * 0.2};
      }),
    "open-grip":primitive(
      [...clawChannels("curl"), ...clawChannels("spread")],
      220, 600, false, 1, requiresFreeSide, params =>
        sideClawTargets(params, 0.08, sideSign(normalizedSide(params.side)) * 0.45)),
    "release":primitive(
      clawChannels("curl"), 180, 520, false, 1, requiresFreeSide,
      params => sideClawTargets(params, 0.05)),
    "swing":primitive(
      [...frontChannels(["shoulder-angle", "elbow-angle", "lift"]),
        "cable-sway", "fur-neck-ruff-left", "fur-neck-ruff-right"],
      600, 1500, false, 0.9, requiresFreeSide, params => {
        const sign = sideSign(normalizedSide(params.side));
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return freezeTargets({...sideFrontTargets(params, {"shoulder-angle":sign * intensity * 0.8,
          "elbow-angle":-sign * intensity * 0.6, "lift":4 + intensity * 7}),
          "cable-sway":sign * intensity * 7, "fur-neck-ruff-left":-sign * intensity * 0.25,
          "fur-neck-ruff-right":sign * intensity * 0.25});
      }),
    "touch":primitive(
      [...frontChannels(["reach-x", "reach-y"]), ...clawChannels("curl")],
      250, 650, false, 0.95, requiresFreeSide, params => {
        const distance = clamp01(finiteParam(params, "distance", 0.5));
        return {...sideFrontTargets(params, {"reach-x":sideSign(normalizedSide(params.side)) *
          (5 + distance * 8), "reach-y":-3 - distance * 4}),
          ...sideClawTargets(params, 0.3 + clamp01(finiteParam(params, "grip", 0.5)) * 0.25)};
      }),
    "close-grip":primitive(
      clawChannels("curl"), 240, 650, false, 1, requiresStableSupport,
      params => sideClawTargets(params, 0.45 + clamp01(finiteParam(params, "grip", 0.5)) * 0.5)),
    "pull":primitive(
      [...frontChannels(["reach-x", "reach-y", "elbow-angle"]), "body-lean-x",
        "cable-load-scale"], 450, 1200, false, 0.9, requiresStableSupport, params => {
        const side = normalizedSide(params.side);
        const sign = sideSign(side);
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {...sideFrontTargets(params, {"reach-x":-sign * (2 + intensity * 5),
          "reach-y":intensity * 3, "elbow-angle":sign * intensity * 0.7}),
          "body-lean-x":-sign * intensity * 4,
          "cable-load-scale":0.7 + clamp01(finiteParam(params, "grip", 0.5)) * 0.5};
      }),
    "settle":primitive(
      ["body-lean-x", "body-lift", "spine-pelvis-angle", "spine-lower-angle",
        "chest-expand", "cable-tension"], 500, 1400, true, 0.65, always, () =>
        ({"body-lean-x":0, "body-lift":0, "spine-pelvis-angle":0,
          "spine-lower-angle":0, "chest-expand":0.08, "cable-tension":1})),
    "stretch":primitive(
      ["body-lift", "spine-mid-angle", "spine-upper-angle",
        ...frontChannels(["reach-x", "reach-y"])],
      800, 1900, false, 0.85, requiresStableSupport, params => {
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"body-lift":3 + intensity * 5, "spine-mid-angle":-0.12 - intensity * 0.2,
          "spine-upper-angle":0.1 + intensity * 0.16,
          "front-left-reach-x":-4 - intensity * 5, "front-left-reach-y":-2 - intensity * 4,
          "front-right-reach-x":4 + intensity * 5, "front-right-reach-y":-2 - intensity * 4};
      }),
    "scratch":primitive(
      [...frontChannels(["shoulder-angle", "elbow-angle", "wrist-angle"]),
        "fur-head-cheek-left", "fur-head-cheek-right"],
      700, 1700, false, 0.9, requiresFreeSide, params => {
        const side = normalizedSide(params.side);
        const sign = sideSign(side);
        return {...sideFrontTargets(params, {"shoulder-angle":sign * 0.85,
          "elbow-angle":-sign * 1.1, "wrist-angle":sign * 0.5}),
          [`fur-head-cheek-${side}`]:clamp01(finiteParam(params, "intensity", 0.5)) * 0.6};
      }),
    "groom":primitive(
      [...frontChannels(["wrist-angle", "palm-angle"]), "face-turn", "nose-twitch"],
      550, 1300, true, 0.7, requiresFreeSide, params => {
        const sign = sideSign(normalizedSide(params.side));
        return {...sideFrontTargets(params, {"wrist-angle":sign * 0.45,
          "palm-angle":-sign * 0.32}), "face-turn":-sign * 0.25, "nose-twitch":0.35};
      }),
    "yawn":primitive(
      ["jaw-open", "muzzle-lift", "lid-left-upper", "lid-right-upper", "chest-expand"],
      900, 2100, false, 0.9, always, params => {
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"jaw-open":0.55 + intensity * 0.45, "muzzle-lift":-0.25 * intensity,
          "lid-left-upper":0.6 + intensity * 0.3, "lid-right-upper":0.6 + intensity * 0.3,
          "chest-expand":0.35 + intensity * 0.4};
      }),
    "doze":primitive(
      ["lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower",
        "head-pitch", "chest-expand"], 1400, 3200, true, 0.55, always, params => {
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"lid-left-upper":0.85, "lid-left-lower":0.2,
          "lid-right-upper":0.85, "lid-right-lower":0.2,
          "head-pitch":0.18 + intensity * 0.25, "chest-expand":0.08};
      }),
    "wake":primitive(
      ["lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower",
        "head-pitch", "brow-left-lift", "brow-right-lift"],
      260, 700, true, 0.75, always, params => {
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"lid-left-upper":0, "lid-left-lower":0, "lid-right-upper":0,
          "lid-right-lower":0, "head-pitch":0,
          "brow-left-lift":0.2 + intensity * 0.25, "brow-right-lift":0.2 + intensity * 0.25};
      }),
    "inspect":primitive(
      ["head-pitch", "head-yaw", "eye-left-look-x", "eye-left-look-y",
        "eye-right-look-x", "eye-right-look-y", "prop-visor-open", "light-visor-glow"],
      500, 1500, true, 0.7, always, params => {
        const sign = sideSign(normalizedSide(params.side));
        const distance = clamp01(finiteParam(params, "distance", 0.5));
        return {"head-pitch":0.12 + distance * 0.22, "head-yaw":sign * distance * 0.18,
          "eye-left-look-x":sign * distance * 0.45, "eye-left-look-y":distance * 0.35,
          "eye-right-look-x":sign * distance * 0.45, "eye-right-look-y":distance * 0.35,
          "prop-visor-open":0.35 + distance * 0.6, "light-visor-glow":0.3 + distance * 0.6};
      }),
    "point":primitive(
      [...frontChannels(["shoulder-angle", "elbow-angle", "wrist-angle", "reach-x",
        "reach-y"]), ...clawChannels("curl"), ...clawChannels("spread")],
      500, 1300, false, 0.9, requiresFreeSide, params => {
        const side = normalizedSide(params.side);
        const sign = sideSign(side);
        const distance = clamp01(finiteParam(params, "distance", 0.5));
        const targets = {...sideFrontTargets(params, {"shoulder-angle":sign * (0.3 + distance * 0.5),
          "elbow-angle":-sign * (0.25 + distance * 0.45), "wrist-angle":sign * 0.12,
          "reach-x":sign * (5 + distance * 10), "reach-y":-2 - distance * 4})};
        for(let claw = 1; claw <= 3; claw += 1){
          targets[`claw-front-${side}-${claw}-curl`] = claw === 1 ? 0.08 : 0.68;
          targets[`claw-front-${side}-${claw}-spread`] = claw === 1 ? sign * 0.2 : -sign * 0.15;
        }
        return targets;
      }),
    "recoil":primitive(
      ["body-lean-x", "body-lift", "head-pitch", "fur-head-crest",
        "fur-neck-ruff-left", "fur-neck-ruff-right"],
      260, 680, false, 0.95, requiresStableSupport, params => {
        const sign = sideSign(normalizedSide(params.side));
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        const follow = clamp01(finiteParam(params, "furFollowThrough", 0.5));
        return {"body-lean-x":-sign * intensity * 5, "body-lift":intensity * 3,
          "head-pitch":-intensity * 0.3, "fur-head-crest":intensity * follow,
          "fur-neck-ruff-left":intensity * follow * 0.6,
          "fur-neck-ruff-right":intensity * follow * 0.6};
      }),
    "celebrate":primitive(
      ["body-lift", "spine-mid-angle", "light-loaded-pulse",
        ...frontChannels(["lift", "reach-x"])],
      650, 1600, false, 0.9, requiresStableSupport, params => {
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"body-lift":3 + intensity * 6, "spine-mid-angle":-0.12 * intensity,
          "light-loaded-pulse":0.4 + intensity * 0.6,
          "front-left-lift":3 + intensity * 6, "front-left-reach-x":-4 - intensity * 5,
          "front-right-lift":3 + intensity * 6, "front-right-reach-x":4 + intensity * 5};
      }),
    "comfort-cable":primitive(
      [...frontChannels(["reach-x", "reach-y", "wrist-angle"]), ...clawChannels("curl"),
        "cable-pulse", "cable-damping"], 650, 1500, true, 0.7, requiresFreeSide,
      params => ({...sideFrontTargets(params, {
        "reach-x":sideSign(normalizedSide(params.side)) * 5, "reach-y":2,
        "wrist-angle":-sideSign(normalizedSide(params.side)) * 0.18}),
        ...sideClawTargets(params, 0.38), "cable-pulse":0.15 + clamp01(
          finiteParam(params, "intensity", 0.5)) * 0.35, "cable-damping":0.998})),
    "slip":primitive(
      [...frontChannels(["reach-x", "reach-y", "lift"]), "body-lift", "cable-sway"],
      180, 480, false, 1, requiresFreeSide, params => {
        const sign = sideSign(normalizedSide(params.side));
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {...sideFrontTargets(params, {"reach-x":sign * intensity * 7,
          "reach-y":intensity * 5, "lift":-intensity * 4}),
          "body-lift":-intensity * 2, "cable-sway":sign * intensity * 5};
      }),
    "recover":primitive(
      ["body-lean-x", "body-lift", "spine-pelvis-angle", "cable-tension",
        "fur-back-shoulder"], 420, 1100, false, 1, requiresStableSupport, params => {
        const sign = sideSign(normalizedSide(params.side));
        const intensity = clamp01(finiteParam(params, "intensity", 0.5));
        return {"body-lean-x":-sign * intensity, "body-lift":1 + intensity * 2,
          "spine-pelvis-angle":sign * intensity * 0.08, "cable-tension":1,
          "fur-back-shoulder":intensity * 0.35};
      }),
    "neutral":primitive(
      ["body-lean-x", "body-lift", "head-yaw", "head-pitch", "head-roll",
        "face-turn", "light-searching-pulse"], 350, 900, true, 0.55, always, () =>
        ({"body-lean-x":0, "body-lift":0, "head-yaw":0, "head-pitch":0,
          "head-roll":0, "face-turn":0, "light-searching-pulse":0})),
  };

  Object.freeze(PRIMITIVES);

  const IDLE_TEMPLATES = Object.freeze([
    ["observe", ["perceive", "orient-gaze", "blink", "refocus"]],
    ["head-turn", ["perceive", "turn-head", "blink"]],
    ["breathing", ["breathe", "neutral"]],
    ["weight-shift", ["brace", "shift-weight", "settle"]],
    ["reach-touch", ["brace", "reach", "touch", "settle"], true],
    ["search", ["perceive", "search", "refocus"]],
    ["cable-comfort", ["reach", "touch", "comfort-cable", "settle"], true],
    ["stretch", ["brace", "stretch", "settle"]],
    ["scratch", ["brace", "scratch", "groom", "settle"], true],
    ["groom", ["groom", "settle"], true],
    ["yawn", ["breathe", "yawn", "blink", "settle"]],
    ["doze", ["settle", "doze", "wake"]],
    ["swing", ["brace", "open-grip", "swing", "close-grip", "settle"], true],
    ["inspect", ["perceive", "orient-gaze", "inspect", "blink"]],
    ["startle", ["perceive", "recoil", "recover", "settle"]],
    ["slip-recover", ["slip", "recover", "settle"], true],
    ["pull-cable", ["brace", "close-grip", "pull", "settle"]],
    ["neutral-reset", ["neutral", "breathe"]],
    ["small-celebration", ["celebrate", "settle"]],
  ].map(template => Object.freeze([template[0], Object.freeze(template[1]), !!template[2]])));

  const STATE_TEMPLATES = Object.freeze({
    working:Object.freeze([
      Object.freeze(["careful-advance", Object.freeze(["perceive", "brace", "shift-weight",
        "reach", "inspect", "close-grip", "pull", "settle"]), true]),
      Object.freeze(["packet-inspect", Object.freeze(["perceive", "brace", "shift-weight",
        "orient-gaze", "inspect", "blink", "settle"]), false]),
      Object.freeze(["packet-refocus", Object.freeze(["turn-head", "brace", "shift-weight",
        "inspect", "refocus", "breathe", "settle"]), false]),
    ]),
    waiting:Object.freeze([
      Object.freeze(["notice-point", Object.freeze(["perceive", "orient-gaze", "point",
        "blink"]), true]),
      Object.freeze(["notice-indicate", Object.freeze(["turn-head", "point", "refocus"]), true]),
    ]),
    done:Object.freeze([
      Object.freeze(["task-settle", Object.freeze(["release", "breathe", "settle"]), true]),
      Object.freeze(["task-satisfied", Object.freeze(["celebrate", "breathe", "settle"]), false]),
      Object.freeze(["task-relax", Object.freeze(["neutral", "breathe", "settle"]), false]),
    ]),
    dead:Object.freeze([
      Object.freeze(["signal-safe-curl", Object.freeze(["perceive", "orient-gaze",
        "close-grip", "settle"]), false]),
      Object.freeze(["signal-check-curl", Object.freeze(["perceive", "turn-head",
        "close-grip", "breathe", "settle"]), false]),
    ]),
    interaction:Object.freeze([
      Object.freeze(["interaction-orient", Object.freeze(["perceive", "orient-gaze",
        "blink", "refocus"]), false]),
      Object.freeze(["interaction-touch", Object.freeze(["perceive", "reach", "touch",
        "settle"]), true]),
      Object.freeze(["interaction-recoil", Object.freeze(["perceive", "recoil", "recover",
        "settle"]), false]),
    ]),
  });

  const SAFE_TEMPLATES = Object.freeze({
    idle:Object.freeze([
      Object.freeze(["idle-safe-breathe", Object.freeze(["neutral", "breathe"]), false]),
      Object.freeze(["idle-safe-observe", Object.freeze(["perceive", "blink", "neutral"]), false]),
    ]),
    working:Object.freeze([
      Object.freeze(["packet-safe-inspect", Object.freeze(["perceive", "inspect", "breathe"]), false]),
      Object.freeze(["packet-safe-refocus", Object.freeze(["inspect", "refocus", "settle"]), false]),
    ]),
    waiting:Object.freeze([
      Object.freeze(["notice-safe-observe", Object.freeze(["perceive", "orient-gaze", "blink"]), false]),
      Object.freeze(["notice-safe-refocus", Object.freeze(["turn-head", "refocus", "breathe"]), false]),
    ]),
    done:Object.freeze([
      Object.freeze(["done-safe-settle", Object.freeze(["breathe", "settle"]), false]),
      Object.freeze(["done-safe-neutral", Object.freeze(["neutral", "settle"]), false]),
    ]),
    dead:Object.freeze([
      Object.freeze(["signal-safe-observe", Object.freeze(["perceive", "orient-gaze", "settle"]), false]),
      Object.freeze(["signal-safe-neutral", Object.freeze(["perceive", "breathe", "neutral"]), false]),
    ]),
    interaction:Object.freeze([
      Object.freeze(["interaction-safe-orient", Object.freeze(["perceive", "orient-gaze", "blink"]), false]),
      Object.freeze(["interaction-safe-refocus", Object.freeze(["turn-head", "refocus", "breathe"]), false]),
    ]),
  });

  function createPersonality(rng){
    const personalityRng = rng.fork("personality");
    return Object.freeze({
      preferredSide:personalityRng.next() < 0.5 ? "left" : "right",
      blinkMs:Math.round(personalityRng.range(2800, 6100)),
      curiosity:personalityRng.range(0.25, 0.85),
      sleepBias:personalityRng.range(0.2, 0.9),
      gripCaution:personalityRng.range(0.55, 0.95),
    });
  }

  function createRing(){
    return Object.seal({values:Object.seal(new Array(MEMORY_SIZE).fill(null)), cursor:0, count:0});
  }

  function ringPush(ring, value){
    ring.values[ring.cursor] = value;
    ring.cursor = (ring.cursor + 1) % MEMORY_SIZE;
    if(ring.count < MEMORY_SIZE) ring.count += 1;
  }

  function ringValues(ring){
    const values = [];
    const start = ring.count === MEMORY_SIZE ? ring.cursor : 0;
    for(let index = 0; index < ring.count; index += 1){
      values.push(ring.values[(start + index) % MEMORY_SIZE]);
    }
    return Object.freeze(values);
  }

  function cleanString(value, fallback, maxLength){
    const string = value === undefined || value === null ? fallback : String(value);
    const trimmed = string.trim().toLowerCase();
    return (trimmed || fallback).slice(0, maxLength);
  }

  function normalizeSupport(value){
    const record = value && typeof value === "object" ? value : {mode:value};
    const mode = record.mode === "loaded" ? "loaded" : "released";
    const load = mode === "loaded" ? clamp01(Number(record.load === undefined ? 1 : record.load)) : 0;
    return Object.freeze({mode, load});
  }

  function normalizeContext(context){
    context = context && typeof context === "object" ? context : {};
    const rawStatus = cleanString(context.status, "idle", 16);
    const rawPressure = cleanString(context.contextPressure, "low", 16);
    const inputSupports = context.supports && typeof context.supports === "object" ?
      context.supports : {};
    const supports = {};
    for(const limb of LIMBS) supports[limb] = normalizeSupport(inputSupports[limb]);
    Object.freeze(supports);
    return Object.freeze({
      sessionId:String(context.sessionId === undefined || context.sessionId === null ? "" :
        context.sessionId).slice(0, 128),
      status:STATUSES.includes(rawStatus) ? rawStatus : "idle",
      role:cleanString(context.role, "daily", 32),
      costume:context.costume === undefined || context.costume === null ? "" :
        String(context.costume).trim().toLowerCase().slice(0, 64),
      contextPressure:PRESSURES.includes(rawPressure) ? rawPressure : "low",
      theme:cleanString(context.theme, "noche", 64),
      expanded:context.expanded === true || context.expanded === 1,
      supports,
    });
  }

  function createDrives(personality){
    return Object.seal({
      sleepiness:clamp01(0.12 + personality.sleepBias * 0.18),
      curiosity:clamp01(personality.curiosity),
      attention:0.62,
      gripConfidence:clamp01(personality.gripCaution),
      fatigue:0.12,
      comfort:0.72,
      boredom:0.24,
      satisfaction:0.5,
      alertness:0.7,
      habituation:0,
    });
  }

  function drivesSnapshot(drives){
    const snapshot = {};
    for(const name of DRIVE_NAMES) snapshot[name] = drives[name];
    return Object.freeze(snapshot);
  }

  function cooldownSnapshot(cooldowns){
    return Object.freeze({scratch:cooldowns.scratch, yawn:cooldowns.yawn,
      doze:cooldowns.doze, "slip-recover":cooldowns["slip-recover"]});
  }

  function memorySnapshot(internal){
    return Object.freeze({families:ringValues(internal.families),
      sides:ringValues(internal.sides), targets:ringValues(internal.targets)});
  }

  function pendingInteractionCount(internal){
    let count = 0;
    for(const slot of internal.interactions){
      if(slot && !slot.consumed && interactionIsPending(slot.event,
        internal.visibleTimeMs)) count += 1;
    }
    return count;
  }

  function createDirector(seed){
    const rootRng = Core.createRng(Core.hashSeed(seed));
    const personality = createPersonality(rootRng);
    const internal = {
      seed:String(seed),
      personality,
      actionRng:rootRng.fork("actions"),
      context:normalizeContext({}),
      drives:createDrives(personality),
      visibleTimeMs:0,
      lastDriveTimeMs:0,
      families:createRing(),
      sides:createRing(),
      targets:createRing(),
      interactions:Object.seal(new Array(INTERACTION_CAPACITY).fill(null)),
      interactionCursor:0,
      cooldowns:Object.seal({scratch:0, yawn:0, doze:0, "slip-recover":0}),
      active:null,
      lastSelectedFamily:null,
      lastInteractionTarget:null,
      lastInteractionAtMs:-INTERACTION_LIFETIME_MS,
      completions:0,
    };
    Object.seal(internal);
    const director = {};
    Object.defineProperties(director, {
      seed:{value:internal.seed, enumerable:true},
      personality:{value:personality, enumerable:true},
      context:{enumerable:true, get(){ return internal.context; }},
      drives:{enumerable:true, get(){ return drivesSnapshot(internal.drives); }},
      memory:{enumerable:true, get(){ return memorySnapshot(internal); }},
      cooldowns:{enumerable:true, get(){ return cooldownSnapshot(internal.cooldowns); }},
      visibleTimeMs:{enumerable:true, get(){ return internal.visibleTimeMs; }},
      pendingInteractions:{enumerable:true, get(){ return pendingInteractionCount(internal); }},
      completions:{enumerable:true, get(){ return internal.completions; }},
    });
    Object.freeze(director);
    DIRECTORS.set(director, internal);
    return director;
  }

  function requireDirector(director){
    const internal = DIRECTORS.get(director);
    if(!internal) throw new TypeError("invalid PerezOS behavior director");
    return internal;
  }

  function evolveDrives(internal, elapsedMs){
    const hours = Math.min(MAX_DRIVE_STEP_MS, elapsedMs) / 3600000;
    if(hours <= 0) return;
    const drives = internal.drives;
    const working = internal.context.status === "working";
    const idle = internal.context.status === "idle";
    drives.sleepiness += hours * (0.08 + internal.personality.sleepBias * 0.12);
    drives.curiosity += hours * (idle ? 0.06 : -0.025);
    drives.attention += hours * (working ? -0.08 : 0.03);
    drives.gripConfidence += hours * (working ? -0.035 : 0.018);
    drives.fatigue += hours * (working ? 0.16 : 0.07);
    drives.comfort += hours * (idle ? 0.035 : -0.025);
    drives.boredom += hours * (idle ? 0.14 : -0.09);
    drives.satisfaction += hours * (working ? -0.035 : -0.012);
    drives.alertness += hours * (working ? 0.045 : -0.055);
    drives.habituation -= hours * 0.22;
    for(const name of DRIVE_NAMES) drives[name] = clamp01(drives[name]);
  }

  function advanceClock(internal, nowMs){
    if(!Number.isFinite(nowMs) || nowMs < 0) return internal.visibleTimeMs;
    const normalized = Math.min(MAX_SAFE_TIME, Math.floor(nowMs));
    if(normalized <= internal.visibleTimeMs) return internal.visibleTimeMs;
    evolveDrives(internal, normalized - internal.lastDriveTimeMs);
    internal.visibleTimeMs = normalized;
    internal.lastDriveTimeMs = normalized;
    return normalized;
  }

  function updateContext(director, context, nowMs){
    const internal = requireDirector(director);
    advanceClock(internal, nowMs);
    internal.context = normalizeContext(context);
    return internal.context;
  }

  function safeAddTime(nowMs, deltaMs){
    return Math.min(MAX_SAFE_TIME, nowMs + deltaMs);
  }

  function deadlineIsPending(deadlineMs, nowMs){
    return deadlineMs > nowMs || (deadlineMs === MAX_SAFE_TIME && nowMs === MAX_SAFE_TIME);
  }

  function interactionIsPending(event, nowMs){
    return nowMs - event.createdAtMs < INTERACTION_LIFETIME_MS;
  }

  function enqueueInteraction(internal, event){
    internal.interactions[internal.interactionCursor] = Object.seal({event, consumed:false});
    internal.interactionCursor = (internal.interactionCursor + 1) % INTERACTION_CAPACITY;
  }

  function notify(director, event, nowMs){
    const internal = requireDirector(director);
    const now = advanceClock(internal, nowMs);
    if(!event || typeof event !== "object") return false;
    const type = cleanString(event.type, "", 32);
    if(type === "completion" || type === "complete"){
      return completePerformance(director, event.performance, now);
    }
    if(!["interaction", "pointer", "tap", "hover", "click", "focus"].includes(type)){
      return false;
    }
    const target = cleanString(event.target, "viewer", 64);
    const side = event.side === "left" || event.side === "right" ? event.side : null;
    const intensity = clamp01(Number.isFinite(event.intensity) ? event.intensity : 0.65);
    const repeated = target === internal.lastInteractionTarget &&
      now - internal.lastInteractionAtMs < INTERACTION_LIFETIME_MS;
    internal.drives.habituation = clamp01(internal.drives.habituation +
      intensity * (repeated ? 0.12 : 0.04));
    internal.drives.attention = clamp01(internal.drives.attention + intensity * 0.14);
    internal.drives.alertness = clamp01(internal.drives.alertness + intensity * 0.1);
    internal.lastInteractionTarget = target;
    internal.lastInteractionAtMs = now;
    enqueueInteraction(internal, Object.freeze({type, target, side, intensity,
      createdAtMs:now, expiresAtMs:safeAddTime(now, INTERACTION_LIFETIME_MS)}));
    return true;
  }

  function newestInteraction(internal){
    for(let offset = 1; offset <= INTERACTION_CAPACITY; offset += 1){
      const index = (internal.interactionCursor - offset + INTERACTION_CAPACITY) %
        INTERACTION_CAPACITY;
      const slot = internal.interactions[index];
      if(!slot || slot.consumed) continue;
      if(!interactionIsPending(slot.event, internal.visibleTimeMs)){
        slot.consumed = true;
        continue;
      }
      return slot;
    }
    return null;
  }

  function freeSides(context){
    return SIDES.filter(side => supportMode(context.supports[`front-${side}`]) !== "loaded");
  }

  function chooseSide(internal, needsFreeSide, interaction){
    const rng = internal.actionRng;
    let choices = needsFreeSide ? freeSides(internal.context) : SIDES.slice();
    if(!choices.length) choices = SIDES.slice();
    if(interaction && interaction.side && choices.includes(interaction.side)) return interaction.side;
    const recent = ringValues(internal.sides);
    const last = recent.length ? recent[recent.length - 1] : null;
    const scores = choices.map(side => {
      let score = rng.next();
      if(side === internal.personality.preferredSide) score += 0.25;
      if(side !== last) score += 0.18;
      return score;
    });
    let selected = 0;
    for(let index = 1; index < scores.length; index += 1){
      if(scores[index] > scores[selected]) selected = index;
    }
    return choices[selected];
  }

  function idleTemplateAllowed(internal, template, nowMs){
    const family = template[0];
    if(template[2] && freeSides(internal.context).length === 0) return false;
    if(RARE_COOLDOWNS[family] && deadlineIsPending(internal.cooldowns[family], nowMs)) return false;
    if(family === "doze" && internal.drives.sleepiness < 0.3) return false;
    if(family === "slip-recover" && internal.drives.fatigue < 0.35) return false;
    return true;
  }

  function rejectRecentFamilies(internal, candidates){
    const recent = ringValues(internal.families);
    for(let keep = recent.length; keep >= 0; keep -= 1){
      const firstBlocked = recent.length - keep;
      const allowed = candidates.filter(template => {
        for(let index = firstBlocked; index < recent.length; index += 1){
          if(template[0] === recent[index]) return false;
        }
        return true;
      });
      if(allowed.length) return allowed;
    }
    return candidates;
  }

  function eligibleStateTemplates(internal, state, nowMs){
    const source = state === "idle" ? IDLE_TEMPLATES : STATE_TEMPLATES[state];
    return source.filter(template => {
      if(template[0] === internal.lastSelectedFamily) return false;
      if(state === "idle") return idleTemplateAllowed(internal, template, nowMs);
      if(template[2] && freeSides(internal.context).length === 0) return false;
      return true;
    });
  }

  function safeFallbackTemplate(internal, state, params){
    const variants = SAFE_TEMPLATES[state] || SAFE_TEMPLATES.idle;
    const candidates = variants.filter(template =>
      template[0] !== internal.lastSelectedFamily && templateIsValid(internal, template, params));
    const allowed = rejectRecentFamilies(internal, candidates);
    return allowed[0] || null;
  }

  function targetForState(internal, state, interaction){
    if(state === "working") return "command-packet";
    if(state === "waiting") return "notice";
    if(state === "done") return "completed-command";
    if(state === "dead") return "signal";
    if(state === "interaction") return interaction ? interaction.target : "viewer";
    const targets = ["cable", "horizon", "hands", "viewer", "shadow",
      internal.context.theme || "noche"];
    return targets[internal.actionRng.int(0, targets.length - 1)];
  }

  function templateEnvironment(internal){
    return {context:internal.context, supports:internal.context.supports,
      drives:internal.drives, personality:internal.personality};
  }

  function templateIsValid(internal, template, params){
    const environment = templateEnvironment(internal);
    for(const id of template[1]){
      if(!PRIMITIVES[id].precondition(environment, params)) return false;
    }
    return true;
  }

  function chooseValidTemplate(internal, state, nowMs, interaction, side){
    const probe = {side};
    let candidates = eligibleStateTemplates(internal, state, nowMs).filter(template =>
      templateIsValid(internal, template, probe));
    candidates = rejectRecentFamilies(internal, candidates);
    if(!candidates.length) return safeFallbackTemplate(internal, state, probe);
    return candidates[internal.actionRng.int(0, candidates.length - 1)];
  }

  function determineState(internal, interactionSlot){
    const status = internal.context.status;
    if(interactionSlot && PRIORITIES.interaction > PRIORITIES[status]) return "interaction";
    return status;
  }

  function phaseParams(internal, side, target, interaction){
    const rng = internal.actionRng;
    const rawIntensity = interaction ? interaction.intensity : rng.range(0.22, 0.96);
    const habituated = interaction ? rawIntensity * (1 - internal.drives.habituation * 0.65) :
      rawIntensity;
    return {
      side,
      target,
      distance:round3(rng.range(0.18, 0.96)),
      grip:round3(Core.clamp(rng.range(0.3, 0.94) * internal.personality.gripCaution,
        0.2, 0.95)),
      intensity:round3(Core.clamp(habituated, 0.08, 1)),
      headLead:round3(rng.range(0.15, 0.92)),
      furFollowThrough:round3(rng.range(0.12, 0.9)),
    };
  }

  function composePerformance(internal, state, nowMs, interactionSlot){
    const interaction = interactionSlot ? interactionSlot.event : null;
    const templates = eligibleStateTemplates(internal, state, nowMs);
    const needsFree = templates.some(template => template[2]);
    const probeSide = chooseSide(internal, needsFree, interaction);
    const target = targetForState(internal, state, interaction);
    let template = chooseValidTemplate(internal, state, nowMs, interaction, probeSide);
    if(template[2] && !freeSides(internal.context).includes(probeSide)){
      const free = freeSides(internal.context);
      if(free.length) template = chooseValidTemplate(internal, state, nowMs, interaction, free[0]);
    }
    const finalSide = template[2] ?
      (freeSides(internal.context).includes(probeSide) ? probeSide :
        freeSides(internal.context)[0] || probeSide) :
      chooseSide(internal, false, interaction);
    const params = phaseParams(internal, finalSide, target, interaction);
    if(!template || !templateIsValid(internal, template, params)){
      template = safeFallbackTemplate(internal, state, params);
    }
    if(!template || !templateIsValid(internal, template, params)){
      throw new Error(`PerezOS has no safe ${state} behavior template`);
    }
    const family = template[0];
    const rng = internal.actionRng;
    const durationScale = rng.range(0.72, 1.32);
    const cooldownMs = RARE_COOLDOWNS[family] || 0;
    if(cooldownMs){
      internal.cooldowns[family] = safeAddTime(nowMs, cooldownMs);
    }
    const phases = [];
    let offsetMs = 0;
    let totalPauseMs = 0;
    for(let index = 0; index < template[1].length; index += 1){
      const id = template[1][index];
      const metadata = PRIMITIVES[id];
      const actionMs = Math.round(Core.clamp(rng.range(metadata.duration.minMs,
        metadata.duration.maxMs) * durationScale, metadata.duration.minMs,
        metadata.duration.maxMs));
      const pauseAfterMs = index === template[1].length - 1 ? 0 : rng.int(0, 420);
      const durationMs = actionMs + pauseAfterMs;
      const phaseValues = Object.freeze({...params, phaseIndex:index});
      const targets = metadata.targets(phaseValues);
      const safeEnd = Math.max(1, Math.min(durationMs, Math.round(actionMs * metadata.safeEnd)));
      phases.push(Object.freeze({
        primitive:id,
        params:phaseValues,
        targets,
        actionMs,
        pauseAfterMs,
        durationMs,
        safeEnd,
        interruptible:metadata.interruptible,
        startMs:offsetMs,
        endMs:offsetMs + durationMs,
      }));
      offsetMs += durationMs;
      totalPauseMs += pauseAfterMs;
    }
    const performance = Object.freeze({
      state,
      family,
      priority:PRIORITIES[state],
      deadlineMs:DEADLINES[state] || null,
      side:finalSide,
      target,
      distance:params.distance,
      grip:params.grip,
      intensity:params.intensity,
      headLead:params.headLead,
      furFollowThrough:params.furFollowThrough,
      pauseMs:totalPauseMs,
      cooldownMs,
      durationMs:offsetMs,
      createdAtMs:nowMs,
      sessionId:internal.context.sessionId,
      phases:Object.freeze(phases),
    });
    internal.lastSelectedFamily = family;
    if(interactionSlot) interactionSlot.consumed = true;
    return performance;
  }

  function nextPerformance(director, nowMs){
    const internal = requireDirector(director);
    const now = advanceClock(internal, nowMs);
    const interactionSlot = newestInteraction(internal);
    const state = determineState(internal, interactionSlot);
    const priority = PRIORITIES[state];
    if(internal.active && internal.active.priority >= priority) return internal.active;
    internal.active = composePerformance(internal, state, now,
      state === "interaction" ? interactionSlot : null);
    return internal.active;
  }

  function applyCompletionDrives(internal, performance){
    const drives = internal.drives;
    drives.satisfaction = clamp01(drives.satisfaction + 0.025 + performance.intensity * 0.025);
    drives.boredom = clamp01(drives.boredom - 0.04 - performance.intensity * 0.025);
    drives.attention = clamp01(drives.attention - 0.012);
    drives.fatigue = clamp01(drives.fatigue + Math.min(0.025, performance.durationMs / 300000));
    if(performance.family === "doze"){
      drives.sleepiness = clamp01(drives.sleepiness - 0.28);
      drives.fatigue = clamp01(drives.fatigue - 0.12);
      drives.alertness = clamp01(drives.alertness + 0.12);
    }else if(performance.family === "yawn"){
      drives.sleepiness = clamp01(drives.sleepiness - 0.06);
    }else if(performance.family === "slip-recover"){
      drives.gripConfidence = clamp01(drives.gripConfidence + 0.1);
      drives.alertness = clamp01(drives.alertness + 0.14);
    }else if(performance.family === "cable-comfort"){
      drives.comfort = clamp01(drives.comfort + 0.08);
    }
  }

  function completePerformance(director, performance, nowMs){
    const internal = requireDirector(director);
    advanceClock(internal, nowMs);
    if(!performance || internal.active !== performance) return false;
    internal.active = null;
    ringPush(internal.families, performance.family);
    ringPush(internal.sides, performance.side);
    ringPush(internal.targets, performance.target);
    applyCompletionDrives(internal, performance);
    internal.completions += 1;
    return true;
  }

  function signatureNumber(value){
    if(!Number.isFinite(value)) return "invalid";
    return Number(value).toString();
  }

  function targetSignature(targets){
    if(!targets || typeof targets !== "object") return "";
    return Object.keys(targets).sort().map(key => `${key}:${signatureNumber(targets[key])}`).join(",");
  }

  function performanceSignature(performance){
    if(!performance || typeof performance !== "object" || !Array.isArray(performance.phases)){
      return "invalid-performance";
    }
    const header = ["perezos-v1", performance.state, performance.family,
      signatureNumber(performance.priority), performance.deadlineMs === null ||
        performance.deadlineMs === undefined ? "-" : signatureNumber(performance.deadlineMs),
      performance.side, performance.target, signatureNumber(performance.distance),
      signatureNumber(performance.grip), signatureNumber(performance.intensity),
      signatureNumber(performance.headLead), signatureNumber(performance.furFollowThrough),
      signatureNumber(performance.pauseMs)].join("|");
    const phases = performance.phases.map(phase => [phase.primitive,
      signatureNumber(phase.actionMs), signatureNumber(phase.pauseAfterMs),
      signatureNumber(phase.durationMs), signatureNumber(phase.safeEnd),
      phase.interruptible ? "1" : "0", targetSignature(phase.targets)].join("~")).join("/");
    return `${header}|${phases}`;
  }

  NS.Behaviors = Object.freeze({PRIMITIVES, createDirector, updateContext, notify,
    nextPerformance, completePerformance, performanceSignature});
})(typeof window !== "undefined" ? window : globalThis);
