"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");

global.window = global;
require("../../dash/perezos/core.js");
require("../../dash/perezos/art.js");

const A = global.ComandOSPerezOS.Art;

const BODY_IDS = [
  "pelvis", "abdomen", "ribcage", "neck-lower", "neck-mid", "neck-upper",
  "skull", "face-mask", "muzzle", "jaw", "nose", "eye-left", "eye-right",
  "lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower",
  "arm-fl-upper", "arm-fl-fore", "wrist-fl", "palm-fl",
  "arm-fr-upper", "arm-fr-fore", "wrist-fr", "palm-fr",
  "leg-rl-upper", "leg-rl-lower", "ankle-rl", "palm-rl",
  "leg-rr-upper", "leg-rr-lower", "ankle-rr", "palm-rr",
  "claw-front-left-1", "claw-front-left-2", "claw-front-left-3",
  "claw-front-right-1", "claw-front-right-2", "claw-front-right-3",
  "claw-rear-left-1", "claw-rear-left-2", "claw-rear-left-3",
  "claw-rear-right-1", "claw-rear-right-2", "claw-rear-right-3",
  "fur-back", "fur-belly", "fur-head",
];

const REQUIRED_STATE_KEYS = [
  "pelvis@turned", "abdomen@loaded", "ribcage@loaded", "neck-mid@searching",
  "skull@searching", "skull@turned", "face-mask@searching", "muzzle@turned",
  "jaw@loaded", "nose@searching", "eye-left@loaded", "eye-left@searching",
  "eye-right@loaded", "eye-right@searching", "lid-left-upper@turned",
  "lid-right-upper@turned", "arm-fl-upper@loaded", "palm-fl@loaded",
  "arm-fr-upper@loaded", "arm-fr-upper@turned", "arm-fr-fore@turned",
  "wrist-fr@turned", "palm-fr@loaded", "palm-fr@turned",
  "leg-rr-upper@turned", "leg-rr-lower@turned", "ankle-rr@turned",
  "palm-rr@turned",
  "claw-front-left-1@loaded", "claw-front-left-2@loaded", "claw-front-left-3@loaded",
  "claw-front-right-1@loaded", "claw-front-right-2@loaded", "claw-front-right-3@loaded",
  "claw-rear-left-1@loaded", "claw-rear-left-2@loaded", "claw-rear-left-3@loaded",
  "claw-rear-right-1@loaded", "claw-rear-right-2@loaded", "claw-rear-right-3@loaded",
  "fur-belly@loaded", "fur-head@searching",
];

const PROP_ATLAS_KEYS = [
  "prop:corona", "prop:casco", "prop:visor", "prop:fuego",
  "prop:hamster", "prop:gordo", "prop:huevo", "prop:bufanda",
];

function makeCanvasFactory(){
  const calls = [];
  const changes = [];
  let fillStyle = "";
  let smoothing = true;
  const ctx = {
    clearRect(...args){ calls.push(["clearRect", ...args]); },
    fillRect(...args){ calls.push(["fillRect", fillStyle, ...args]); },
    get fillStyle(){ return fillStyle; },
    set fillStyle(value){ fillStyle = value; changes.push(["fillStyle", value]); },
    get imageSmoothingEnabled(){ return smoothing; },
    set imageSmoothingEnabled(value){ smoothing = value; changes.push(["smoothing", value]); },
    set shadowBlur(value){ changes.push(["shadowBlur", value]); },
    set filter(value){ changes.push(["filter", value]); },
    set globalAlpha(value){ changes.push(["globalAlpha", value]); },
  };
  const factory = (width, height) => ({
    width,
    height,
    getContext(kind, options){
      calls.push(["getContext", kind, options]);
      return ctx;
    },
  });
  return {factory, calls, changes, ctx};
}

function paletteIndexes(commands){
  return commands.map(command => command[1]);
}

function pixelsInRect(atlasHarness, rect, includeColor = true){
  const pixels = new Map();
  for(const call of atlasHarness.calls){
    if(call[0] !== "fillRect") continue;
    const [, color, x, y, width, height] = call;
    for(let py = y; py < y + height; py += 1){
      for(let px = x; px < x + width; px += 1){
        if(px >= rect.x && py >= rect.y && px < rect.x + rect.width && py < rect.y + rect.height){
          const key = `${px - rect.x},${py - rect.y}`;
          pixels.set(key, includeColor ? color : true);
        }
      }
    }
  }
  return pixels;
}

function pixelDifference(a, b){
  const keys = new Set([...a.keys(), ...b.keys()]);
  let count = 0;
  for(const key of keys) if(a.get(key) !== b.get(key)) count += 1;
  return count;
}

function shapeDifference(harness, atlas, left, right){
  return pixelDifference(pixelsInRect(harness, atlas.rects[left], false),
                         pixelsInRect(harness, atlas.rects[right], false));
}

function manifestFixture(overrides = {}){
  return {
    BODY_IDS:[...A.BODY_IDS],
    PARTS:A.PARTS.map(part => ({...part, pivot:[...part.pivot], bounds:[...part.bounds],
      commands:part.commands.map(command => [...command]),
      states:Object.fromEntries(Object.entries(part.states).map(([name, commands]) =>
        [name, commands.map(command => [...command])]))})),
    PROPS:Object.fromEntries(Object.entries(A.PROPS).map(([name, prop]) => [name, {...prop,
      pivot:[...prop.pivot], bounds:[...prop.bounds], commands:prop.commands.map(command => [...command])}])),
    MASKS:Object.fromEntries(Object.entries(A.MASKS).map(([name, mask]) => [name, {...mask,
      bounds:[...mask.bounds], commands:mask.commands.map(command => [...command])}])),
    PALETTE:[...A.PALETTE],
    THEMES:{dark:[...A.THEMES.dark], light:[...A.THEMES.light]},
    PALETTE_ROLES:{stableIndices:[...A.PALETTE_ROLES.stableIndices],
      variableIndices:[...A.PALETTE_ROLES.variableIndices], roleByIndex:[...A.PALETTE_ROLES.roleByIndex]},
    CAMERAS:Object.fromEntries(Object.entries(A.CAMERAS).map(([name, camera]) => [name, {...camera}])),
    ...overrides,
  };
}

function goldenSignature(harness, atlas, ids){
  let text = "";
  for(const id of ids){
    const rect = atlas.rects[id];
    const pixels = pixelsInRect(harness, rect);
    text += `${id}:${rect.width}x${rect.height}:`;
    for(let y = 0; y < rect.height; y += 1){
      for(let x = 0; x < rect.width; x += 1){
        const color = pixels.get(`${x},${y}`);
        text += color === undefined ? "." : A.PALETTE.indexOf(color).toString(36);
      }
      text += "|";
    }
  }
  let hash = 0x811c9dc5;
  for(let index = 0; index < text.length; index += 1){
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

test("manifest provides the exact authored PerezOS body hierarchy", () => {
  assert.deepEqual(A.validateManifest(), []);
  assert.deepEqual(A.WORLD, {width:224, height:192});
  assert.deepEqual(A.PARTS.map(part => part.id), BODY_IDS);
  assert.equal(A.PARTS.length, 48);
  assert.equal(A.PARTS.filter(part =>
    /^claw-(?:front-left|front-right|rear-left|rear-right)-[123]$/.test(part.id)).length, 12);

  const knownIds = new Set(BODY_IDS);
  for(const part of A.PARTS){
    assert.equal(typeof part.parent, "string", `${part.id} needs an explicit parent`);
    assert.ok(part.parent === "world" || knownIds.has(part.parent), `${part.id} has unknown parent`);
    assert.equal(typeof part.z, "number", `${part.id} needs a numeric z group`);
    assert.equal(Object.isFrozen(part), true, `${part.id} must be frozen`);
  }
});

test("every piece contains authored indexed clusters inside integer bounds", () => {
  for(const part of A.PARTS){
    assert.equal(part.pivot.length, 2, `${part.id} pivot`);
    assert.equal(part.bounds.length, 4, `${part.id} bounds`);
    assert.ok(part.bounds[2] > 0 && part.bounds[3] > 0, `${part.id} has empty bounds`);
    assert.ok([...part.pivot, ...part.bounds].every(Number.isInteger), `${part.id} geometry must be integer`);
    assert.ok(part.pivot[0] >= 0 && part.pivot[0] < part.bounds[2], `${part.id} pivot x`);
    assert.ok(part.pivot[1] >= 0 && part.pivot[1] < part.bounds[3], `${part.id} pivot y`);
    assert.ok(part.commands.length >= 2, `${part.id} has no authored clusters`);

    for(const command of part.commands){
      assert.ok(["px", "run", "rect", "poly"].includes(command[0]), `${part.id} command kind`);
      assert.ok(Number.isInteger(command[1]) && command[1] >= 0 && command[1] < A.PALETTE.length,
        `${part.id} palette index`);
      assert.ok(command.slice(2).every(Number.isInteger), `${part.id} command geometry`);
    }
  }
});

test("warm palette and state variants are explicit indexed artwork", () => {
  assert.ok(A.PALETTE.length >= 12);
  assert.ok(A.PALETTE.every(color => /^#[0-9a-f]{6}$/i.test(color)));
  assert.equal(new Set(A.PALETTE).size, A.PALETTE.length);

  const variantNames = new Set();
  for(const part of A.PARTS){
    for(const [name, commands] of Object.entries(part.states || {})){
      variantNames.add(name);
      assert.ok(commands.length >= 1, `${part.id}.${name}`);
      assert.ok(paletteIndexes(commands).every(index => index >= 0 && index < A.PALETTE.length));
    }
  }
  assert.deepEqual([...variantNames].sort(), ["loaded", "searching", "turned"]);
  assert.deepEqual(A.PARTS.flatMap(part => Object.keys(part.states).map(state => `${part.id}@${state}`)),
                   REQUIRED_STATE_KEYS);
});

test("every input prop and contact mask is authored and immutable", () => {
  assert.deepEqual(Object.keys(A.PROPS).sort(),
    ["bufanda", "casco", "corona", "fuego", "gordo", "hamster", "huevo", "visor"]);
  for(const [name, prop] of Object.entries(A.PROPS)){
    assert.equal(typeof prop.parent, "string", `${name} parent`);
    assert.equal(typeof prop.z, "number", `${name} z`);
    assert.ok(prop.commands.length >= 2, `${name} authored clusters`);
    assert.ok(prop.commands.every(command => paletteIndexes([command])[0] < A.PALETTE.length));
    assert.equal(Object.isFrozen(prop), true, `${name} must be frozen`);
  }
  assert.ok(Object.keys(A.MASKS).includes("contact-belly"));
  for(const [name, mask] of Object.entries(A.MASKS)){
    assert.ok(mask.commands.length >= 1, `${name} commands`);
    assert.ok(mask.bounds.every(Number.isInteger), `${name} bounds`);
    assert.equal(Object.isFrozen(mask), true, `${name} must be frozen`);
  }
});

test("compact camera preserves integer pixel scaling and world framing", () => {
  assert.equal(Object.isFrozen(A.CAMERAS), true);
  assert.ok(Object.keys(A.CAMERAS).length >= 2);
  assert.deepEqual(A.compactCamera(224, 192),
    {x:0, y:0, sourceX:0, sourceY:0, scale:1, width:224, height:192});
  assert.deepEqual(A.compactCamera(500, 400),
    {x:26, y:8, sourceX:0, sourceY:0, scale:2, width:224, height:192});
  assert.deepEqual(A.compactCamera(180, 148),
    {x:0, y:0, sourceX:22, sourceY:8, scale:1, width:180, height:148});
  assert.deepEqual(A.compactCamera(100, 80),
    {x:0, y:0, sourceX:62, sourceY:42, scale:1, width:100, height:80});

  for(const [width, height] of [[180,148], [100,80], [500,400]]){
    const camera = A.compactCamera(width, height);
    assert.ok(camera.x + camera.width * camera.scale <= width);
    assert.ok(camera.y + camera.height * camera.scale <= height);
    assert.ok(Object.values(camera).every(Number.isInteger));
  }
});

test("atlas caches exact base, complete state, and prop sprites", () => {
  const light = makeCanvasFactory();
  const dark = makeCanvasFactory();
  const lightAtlas = A.buildAtlas(light.factory, "light");
  const darkAtlas = A.buildAtlas(dark.factory, "dark");

  const expectedKeys = [...BODY_IDS, ...REQUIRED_STATE_KEYS, ...PROP_ATLAS_KEYS];

  assert.equal(lightAtlas.canvas.width, 1024);
  assert.equal(lightAtlas.canvas.height, 1024);
  assert.equal(light.ctx.imageSmoothingEnabled, false);
  assert.equal(Object.isFrozen(lightAtlas), true);
  assert.equal(Object.isFrozen(lightAtlas.rects), true);
  assert.deepEqual(Object.keys(lightAtlas.rects), expectedKeys);
  assert.equal(lightAtlas.keys, A.ATLAS_KEYS);
  assert.deepEqual(lightAtlas.keys, expectedKeys);
  assert.equal(Object.keys(lightAtlas.rects).length, 98);
  assert.notDeepEqual(lightAtlas.palette, darkAtlas.palette);
  assert.deepEqual(lightAtlas.rects, darkAtlas.rects);
  assert.deepEqual(light.calls.filter(call => call[0] === "fillRect"),
                   makeRasterCalls("light"));

  const forbidden = light.changes.filter(change =>
    ["shadowBlur", "filter", "globalAlpha"].includes(change[0]));
  assert.deepEqual(forbidden, []);
  assert.ok(light.calls.filter(call => call[0] === "fillRect")
    .every(call => call.slice(2).every(Number.isInteger)));

  for(const key of expectedKeys){
    assert.ok(pixelsInRect(light, lightAtlas.rects[key]).size > 0, `${key} has no cached raster`);
    assert.equal(Object.isFrozen(lightAtlas.rects[key]), true, `${key} rect is mutable`);
  }
});

test("state atlas cells are complete and silhouette variants are substantial", () => {
  const harness = makeCanvasFactory();
  const atlas = A.buildAtlas(harness.factory, "dark");
  for(const key of REQUIRED_STATE_KEYS){
    const baseKey = key.slice(0, key.indexOf("@"));
    const base = pixelsInRect(harness, atlas.rects[baseKey]);
    const state = pixelsInRect(harness, atlas.rects[key]);
    assert.ok(state.size >= Math.floor(base.size * 0.6), `${key} is only a sparse overlay`);
    assert.ok(pixelDifference(base, state) >= 8, `${key} barely differs from base`);
  }

  for(const key of ["skull@searching", "arm-fr-fore@turned", "leg-rr-lower@turned",
                     "claw-front-left-1@loaded", "claw-rear-right-1@loaded"]){
    const baseKey = key.slice(0, key.indexOf("@"));
    const baseShape = pixelsInRect(harness, atlas.rects[baseKey], false);
    const stateShape = pixelsInRect(harness, atlas.rects[key], false);
    assert.ok(pixelDifference(baseShape, stateShape) >= 12, `${key} has no meaningful silhouette change`);
  }
});

test("near/far limb chains and claw families have materially distinct authored silhouettes", () => {
  const harness = makeCanvasFactory();
  const atlas = A.buildAtlas(harness.factory, "dark");
  const opposingPairs = [
    ["arm-fl-upper", "arm-fr-upper"], ["arm-fl-fore", "arm-fr-fore"],
    ["wrist-fl", "wrist-fr"], ["palm-fl", "palm-fr"],
    ["leg-rl-upper", "leg-rr-upper"], ["leg-rl-lower", "leg-rr-lower"],
    ["ankle-rl", "ankle-rr"], ["palm-rl", "palm-rr"],
  ];
  for(const [near, far] of opposingPairs){
    assert.ok(shapeDifference(harness, atlas, near, far) >= 24,
      `${near} and ${far} still share a template silhouette`);
  }

  for(const [left, right] of [
    ["claw-front-left-1", "claw-front-right-1"],
    ["claw-front-left-2", "claw-front-right-2"],
    ["claw-rear-left-1", "claw-rear-right-1"],
    ["claw-rear-left-2", "claw-rear-right-2"],
    ["claw-front-left-1", "claw-rear-left-1"],
  ]){
    assert.ok(shapeDifference(harness, atlas, left, right) >= 16,
      `${left} and ${right} are not anatomically distinct`);
  }

  for(const id of ["arm-fl-upper", "arm-fl-fore", "arm-fr-upper", "arm-fr-fore",
                   "leg-rl-upper", "leg-rl-lower", "leg-rr-upper", "leg-rr-lower"]){
    const commands = A.PARTS.find(part => part.id === id).commands;
    assert.ok(commands.some(command => command[0] === "poly"), `${id} lacks an irregular contour`);
    assert.ok(commands.filter(command => command[0] === "px" || command[0] === "run").length >= 3,
      `${id} lacks authored highlight/dither clusters`);
  }
});

test("representative anatomical raster families match authored golden signatures", () => {
  const harness = makeCanvasFactory();
  const atlas = A.buildAtlas(harness.factory, "dark");
  const groups = {
    head:["skull", "face-mask", "muzzle", "jaw", "nose", "fur-head"],
    torso:["pelvis", "abdomen", "ribcage", "fur-back", "fur-belly"],
    frontLeft:["arm-fl-upper", "arm-fl-fore", "wrist-fl", "palm-fl"],
    frontRight:["arm-fr-upper", "arm-fr-fore", "wrist-fr", "palm-fr"],
    rearLeft:["leg-rl-upper", "leg-rl-lower", "ankle-rl", "palm-rl"],
    rearRight:["leg-rr-upper", "leg-rr-lower", "ankle-rr", "palm-rr"],
    clawFrontLeft:["claw-front-left-1", "claw-front-left-2", "claw-front-left-3"],
    clawFrontRight:["claw-front-right-1", "claw-front-right-2", "claw-front-right-3"],
    clawRearLeft:["claw-rear-left-1", "claw-rear-left-2", "claw-rear-left-3"],
    clawRearRight:["claw-rear-right-1", "claw-rear-right-2", "claw-rear-right-3"],
  };
  assert.deepEqual(Object.fromEntries(Object.entries(groups).map(([name, ids]) =>
    [name, goldenSignature(harness, atlas, ids)])), {
    head:"e9fc3490", torso:"7a092361", frontLeft:"24333438", frontRight:"daa2c55e",
    rearLeft:"14b2a305", rearRight:"644ed50e", clawFrontLeft:"d5297881",
    clawFrontRight:"6d2d63b8", clawRearLeft:"ed1c4d1e", clawRearRight:"d727ecfb",
  });
});

function makeRasterCalls(theme){
  const replay = makeCanvasFactory();
  A.buildAtlas(replay.factory, theme);
  return replay.calls.filter(call => call[0] === "fillRect");
}

test("atlas rectangles are stable, non-overlapping, and bounded", () => {
  const first = A.buildAtlas(makeCanvasFactory().factory, "dark");
  const second = A.buildAtlas(makeCanvasFactory().factory, "dark");
  assert.deepEqual(first.rects, second.rects);

  const rects = Object.values(first.rects);
  for(let i = 0; i < rects.length; i += 1){
    const a = rects[i];
    assert.ok(a.x >= 0 && a.y >= 0 && a.x + a.width <= 1024 && a.y + a.height <= 1024);
    assert.ok(Object.values(a).every(Number.isInteger));
    for(let j = i + 1; j < rects.length; j += 1){
      const b = rects[j];
      const overlaps = a.x < b.x + b.width && a.x + a.width > b.x &&
                       a.y < b.y + b.height && a.y + a.height > b.y;
      assert.equal(overlaps, false);
    }
  }
});

test("themes preserve body identity roles and vary only declared sensitive roles", () => {
  const stable = A.PALETTE_ROLES.stableIndices;
  const variable = A.PALETTE_ROLES.variableIndices;
  assert.deepEqual(stable, [1,2,3,4,5,6,7,8,9,10,11,20]);
  assert.deepEqual(variable, [0,12,13,14,15,16,17,18,19]);
  assert.deepEqual([...stable, ...variable].sort((a,b) => a - b),
                   Array.from({length:A.PALETTE.length}, (_, index) => index));

  const light = A.buildAtlas(makeCanvasFactory().factory, "light").palette;
  const dark = A.buildAtlas(makeCanvasFactory().factory, "dark").palette;
  for(const index of stable) assert.equal(light[index], dark[index], `identity index ${index}`);
  for(const index of variable) assert.notEqual(light[index], dark[index], `sensitive index ${index}`);
});

test("variable palette usage is restricted to shadows, states, and prop-sensitive artwork", () => {
  const variable = new Set(A.PALETTE_ROLES.variableIndices);
  const stateRoles = {
    loaded:new Set([0,12,15]),
    searching:new Set([0,13,17]),
    turned:new Set([0,13,14]),
  };
  for(const part of A.PARTS){
    for(const command of part.commands){
      if(variable.has(command[1])) assert.equal(command[1], 0, `${part.id} varies identity index ${command[1]}`);
    }
    for(const [state, commands] of Object.entries(part.states)){
      for(const command of commands){
        if(variable.has(command[1])) assert.ok(stateRoles[state].has(command[1]),
          `${part.id}@${state} misuses variable index ${command[1]}`);
      }
    }
  }

  const darkHarness = makeCanvasFactory();
  const lightHarness = makeCanvasFactory();
  const darkAtlas = A.buildAtlas(darkHarness.factory, "dark");
  const lightAtlas = A.buildAtlas(lightHarness.factory, "light");
  for(const id of BODY_IDS){
    const dark = pixelsInRect(darkHarness, darkAtlas.rects[id]);
    const light = pixelsInRect(lightHarness, lightAtlas.rects[id]);
    assert.deepEqual([...dark.keys()], [...light.keys()], `${id} silhouette changes by theme`);
    for(const [pixel, darkColor] of dark){
      const paletteIndex = A.PALETTE.indexOf(darkColor);
      if(paletteIndex !== 0) assert.equal(light.get(pixel), darkColor,
        `${id} identity pixel ${pixel} changes at index ${paletteIndex}`);
    }
  }
});

test("manifest validation is total and reports malformed fixtures", () => {
  for(const malformed of [null, {}, {PARTS:null}, {PARTS:[null]}]){
    assert.doesNotThrow(() => A.validateManifest(malformed));
    assert.ok(A.validateManifest(malformed).length > 0);
  }

  const cases = [
    ["pivot", fixture => { fixture.PARTS[0].pivot = null; }],
    ["world bounds", fixture => { fixture.PARTS[0].bounds = [220,116,36,28]; }],
    ["mask", fixture => { fixture.MASKS["contact-belly"].commands = [["wat",0,0]]; }],
    ["world bounds", fixture => { fixture.MASKS["contact-belly"].bounds = [220,103,27,17]; }],
    ["cycle", fixture => {
      fixture.PARTS[0].parent = fixture.PARTS[1].id;
      fixture.PARTS[1].parent = fixture.PARTS[0].id;
    }],
    ["prop", fixture => { delete fixture.PROPS.bufanda; }],
    ["prop key", fixture => { fixture.PROPS.bufanda.id = "wrong-prop-id"; }],
    ["contact-belly", fixture => { delete fixture.MASKS["contact-belly"]; }],
    ["mask key", fixture => { fixture.MASKS["contact-belly"].id = "wrong-mask-id"; }],
    ["state", fixture => { fixture.PARTS[0].states = {dancing:fixture.PARTS[0].commands}; }],
    ["palette", fixture => { fixture.PARTS[0].commands[0][1] = 999; }],
    ["theme", fixture => { fixture.THEMES.light[3] = "#ffffff"; }],
    ["theme color", fixture => { fixture.THEMES.light[12] = "not-a-color"; }],
    ["theme dark", fixture => {
      fixture.THEMES.dark[2] = "#abcdef";
      fixture.THEMES.light[2] = "#abcdef";
    }],
    ["palette roles", fixture => {
      fixture.PALETTE_ROLES.stableIndices = [1,2,3,4,5,6,7,8,9,10,12];
      fixture.PALETTE_ROLES.variableIndices = [0,11,13,14,15,16,17,18,19,20];
    }],
    ["camera", fixture => { fixture.CAMERAS.compact.width = 181; }],
  ];
  for(const [needle, mutate] of cases){
    const fixture = manifestFixture();
    mutate(fixture);
    const errors = A.validateManifest(fixture);
    assert.ok(errors.some(error => error.includes(needle)), `${needle}: ${errors.join("; ")}`);
  }


  for(const hostileIndex of [Symbol("role"), 1n, NaN, "1"]){
    const fixture = manifestFixture();
    fixture.PALETTE_ROLES.stableIndices[0] = hostileIndex;
    assert.doesNotThrow(() => A.validateManifest(fixture));
    assert.ok(A.validateManifest(fixture).some(error => error.includes("palette roles")));
  }
});

test("triangle scan conversion uses one pixel-center half-open coverage rule", () => {
  const cases = [
    [["poly",0,1,1,5,1,3,5], [[1,1,4],[2,2,2],[2,3,2]]],
    [["poly",0,3,1,1,5,5,5], [[2,2,2],[2,3,2],[1,4,4]]],
    [["poly",0,1,1,2,6,3,1], [[1,1,2],[1,2,2],[1,3,1]]],
    [["poly",0,1,1,6,3,2,7], [[1,1,1],[1,2,4],[1,3,4],[2,4,2],[2,5,1]]],
  ];
  for(const [command, expected] of cases){
    const actual = A.rasterTriangle(command);
    assert.deepEqual(actual, expected);
    assert.equal(Object.isFrozen(actual), true);
    assert.ok(actual.every(run => Object.isFrozen(run) && run.every(Number.isInteger)));
  }
});

test("public triangle rasterization rejects malformed commands predictably", () => {
  for(const command of [null, [], ["rect",0,1,1,5,5], ["poly",0,1,1,5,1,3],
                         ["poly",0,1,1,5,1,3,2.5], ["poly",0,1,1,5,1,3,Symbol("y")]]){
    assert.throws(() => A.rasterTriangle(command),
      error => error instanceof TypeError && error.message === "rasterTriangle requires an integer poly command");
  }
  assert.throws(() => A.rasterTriangle(["poly",999,1,1,5,1,3,5]),
    error => error instanceof RangeError && error.message === "rasterTriangle palette index is out of range");
  assert.deepEqual(A.rasterTriangle(["poly",0,1,2,3,2,5,2]), []);
});

test("public triangle rasterization is bounded to atlas-safe coordinates and spans", () => {
  assert.equal(A.RASTER_LIMIT, 1024);
  const unsafe = [
    ["poly",0,0,0,1025,0,0,1],
    ["poly",0,-1025,0,0,0,0,1],
    ["poly",0,-512,0,513,0,0,1],
    ["poly",0,0,-512,1,513,2,0],
  ];
  for(const command of unsafe){
    assert.throws(() => A.rasterTriangle(command), error =>
      error instanceof RangeError &&
      error.message === "rasterTriangle coordinates exceed atlas-safe limit 1024");
  }

  const boundary = A.rasterTriangle(["poly",0,0,0,1,1024,2,0]);
  assert.ok(boundary.length <= A.RASTER_LIMIT);
});
