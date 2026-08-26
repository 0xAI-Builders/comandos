"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

global.window = global;
require("../../dash/perezos/core.js");
require("../../dash/perezos/art.js");
require("../../dash/perezos/rig.js");

const rendererPath = path.resolve(__dirname, "../../dash/perezos/renderer.js");
if(fs.existsSync(rendererPath)) require(rendererPath);

const NS = global.ComandOSPerezOS;
const A = NS.Art;
const R = NS.Rig;
const D = NS.Renderer;

test("renderer module exposes the exact public contract", () => {
  assert.ok(D, "ComandOSPerezOS.Renderer is undefined");
  assert.deepEqual(Object.keys(D).sort(), [
    "QUALITY", "createRenderer", "destroyRenderer", "markDirty", "render",
    "rendererDiagnostics", "setTheme", "setViewport",
  ]);
  assert.deepEqual(D.QUALITY,
    {FULL:"full", BALANCED:"balanced", ECONOMY:"economy", STATIC:"static"});
  assert.equal(Object.isFrozen(D), true);
  assert.equal(Object.isFrozen(D.QUALITY), true);
});

if(D){
  function recordingContext(){
    const operations = [];
    const draws = [];
    const fills = [];
    const transforms = [];
    let smoothing = true;
    let fillStyle = "";
    return {
      operations,
      draws,
      fills,
      transforms,
      clearRect(...args){ operations.push(["clearRect", ...args]); },
      fillRect(...args){
        const call = {style:fillStyle, rectangle:args};
        fills.push(call);
        operations.push(["fillRect", call]);
      },
      drawImage(image, ...args){
        const call = {
          image,
          source:args.length === 8 ? args.slice(0, 4) : [],
          destination:args.length === 8 ? args.slice(4, 8) : args.slice(0, 4),
        };
        draws.push(call);
        operations.push(["drawImage", call]);
      },
      setTransform(...args){
        transforms.push(args);
        operations.push(["setTransform", ...args]);
      },
      get fillStyle(){ return fillStyle; },
      set fillStyle(value){
        fillStyle = value;
        operations.push(["fillStyle", value]);
      },
      get imageSmoothingEnabled(){ return smoothing; },
      set imageSmoothingEnabled(value){
        smoothing = value;
        operations.push(["smoothing", value]);
      },
      reset(){
        operations.length = 0;
        draws.length = 0;
        fills.length = 0;
        transforms.length = 0;
      },
    };
  }

  function canvasWithContext(width, height, context){
    return {
      width,
      height,
      getContext(kind){ return kind === "2d" ? context : null; },
    };
  }

  function fakeCanvas(width = 512, height = 416){
    const context = recordingContext();
    const canvas = canvasWithContext(width, height, context);
    const offscreen = [];
    const factory = (factoryWidth, factoryHeight) => {
      const offscreenContext = recordingContext();
      const created = canvasWithContext(factoryWidth, factoryHeight, offscreenContext);
      offscreen.push({canvas:created, context:offscreenContext});
      return created;
    };
    return {canvas, context, draws:context.draws, fills:context.fills,
      operations:context.operations, transforms:context.transforms, factory, offscreen};
  }

  const atlasStubContext = {
    imageSmoothingEnabled:true,
    fillStyle:"",
    clearRect(){},
    fillRect(){},
  };
  const atlasFixture = A.buildAtlas((width, height) =>
    canvasWithContext(width, height, atlasStubContext), "dark");
  const atlasEntries = Object.entries(atlasFixture.rects);

  function atlasKeyForDraw(call){
    if(!call.image || call.image.width !== 1024 || call.image.height !== 1024) return null;
    for(const [key, rect] of atlasEntries){
      if(call.source[0] === rect.x && call.source[1] === rect.y &&
         call.source[2] === rect.width && call.source[3] === rect.height) return key;
    }
    return null;
  }

  function atlasDraws(fake){
    return fake.draws.map(call => [atlasKeyForDraw(call), call])
      .filter(entry => entry[0] !== null);
  }

  function destinationMap(fake){
    return new Map(atlasDraws(fake).map(([key, call]) => [baseId(key), call.destination]));
  }

  function baseId(key){ return key.includes("@") ? key.slice(0, key.indexOf("@")) : key; }

  function fixtureContext(overrides = {}){
    return {
      sessionId:"session-name",
      status:"idle",
      role:"daily",
      costume:"corona",
      contextPressure:"low",
      theme:"noche",
      expanded:false,
      timestamp:0,
      colors:{brand:"#8B7CFF", panel:"#121722", line:"#222A3A"},
      ...overrides,
    };
  }

  function fixture(width = 256, height = 208, dpr = 2){
    const fake = fakeCanvas(width * dpr, height * dpr);
    const renderer = D.createRenderer(fake.canvas, {canvasFactory:fake.factory});
    assert.equal(D.setViewport(renderer, width, height, dpr), true);
    return {fake, renderer, rig:R.createRig("renderer-fixture")};
  }

  const OCCLUSION_GROUPS = [
    "rear-cable-shadow", "rear-limbs", "axial-deep-fur", "torso",
    "front-limbs", "face", "claws-contact-masks", "medium-fine-fur",
    "props", "front-cable", "state-rim-light",
  ];
  const REAR_LIMBS = [
    "arm-fr-upper", "arm-fr-fore", "wrist-fr", "palm-fr",
    "leg-rr-upper", "leg-rr-lower", "ankle-rr", "palm-rr",
  ];
  const TORSO = ["pelvis", "abdomen", "ribcage", "neck-lower", "neck-mid", "neck-upper"];
  const FRONT_LIMBS = [
    "arm-fl-upper", "arm-fl-fore", "wrist-fl", "palm-fl",
    "leg-rl-upper", "leg-rl-lower", "ankle-rl", "palm-rl",
  ];
  const FACE = [
    "skull", "face-mask", "muzzle", "jaw", "nose", "eye-left", "eye-right",
    "lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower",
  ];
  const CLAWS = A.BODY_IDS.filter(id => id.startsWith("claw-"));

  test("renderer uses one cached Art atlas build until the palette changes", () => {
    const {fake, renderer, rig} = fixture();
    const atlasBuilds = () => fake.offscreen.filter(item =>
      item.canvas.width === 1024 && item.canvas.height === 1024).length;
    assert.equal(atlasBuilds(), 1);
    assert.equal(D.render(renderer, rig, fixtureContext(), "full"), true);
    D.markDirty(renderer, "test");
    assert.equal(D.render(renderer, rig, fixtureContext(), "full"), true);
    assert.equal(atlasBuilds(), 1);
    assert.equal(D.setTheme(renderer, "light"), true);
    assert.equal(atlasBuilds(), 2);
    assert.equal(D.setTheme(renderer, "light"), false);
    assert.equal(D.setTheme(renderer, "dark"), true);
    assert.equal(atlasBuilds(), 2, "returning to a cached palette must not rebuild it");
    assert.equal(D.rendererDiagnostics(renderer).atlasBuilds, 2);
  });

  test("renderer uses only integer nearest-neighbor source, destination, and pivot transforms", () => {
    const {fake, renderer, rig} = fixture();
    assert.equal(D.render(renderer, rig, fixtureContext(), "full"), true);
    assert.equal(fake.context.imageSmoothingEnabled, false);
    assert.ok(fake.draws.length > 40);
    for(const call of fake.draws){
      assert.ok(call.source.every(Number.isInteger), JSON.stringify(call.source));
      assert.ok(call.destination.every(Number.isInteger), JSON.stringify(call.destination));
    }
    for(const call of fake.fills){
      assert.ok(call.rectangle.every(Number.isInteger), JSON.stringify(call.rectangle));
    }
    for(const transform of fake.transforms){
      assert.ok(transform.every(Number.isInteger), JSON.stringify(transform));
    }
  });

  test("renderer follows the exact deterministic occlusion groups", () => {
    const {fake, renderer, rig} = fixture();
    assert.equal(D.render(renderer, rig, fixtureContext(), "full"), true);
    const ids = atlasDraws(fake).map(entry => baseId(entry[0]));
    const expected = [
      ...REAR_LIMBS, "fur-back", ...TORSO, ...FRONT_LIMBS, ...FACE,
      ...CLAWS, "fur-belly", "fur-head", "prop:corona",
    ];
    assert.deepEqual(ids, expected);
    const diagnostics = D.rendererDiagnostics(renderer);
    assert.deepEqual(diagnostics.occlusionGroups, OCCLUSION_GROUPS);
    assert.deepEqual(diagnostics.lastGroupDrawCounts, [2,8,1,6,8,11,14,3,1,1,1]);

    const operationIndex = id => fake.operations.findIndex(operation => {
      if(operation[0] !== "drawImage") return false;
      const key = atlasKeyForDraw(operation[1]);
      return key !== null && baseId(key) === id;
    });
    const rearStart = operationIndex(REAR_LIMBS[0]);
    const lastClaw = operationIndex(CLAWS[CLAWS.length - 1]);
    const furStart = operationIndex("fur-belly");
    const furEnd = operationIndex("fur-head");
    const prop = operationIndex("prop:corona");
    assert.ok(fake.operations.slice(0, rearStart).some(operation =>
      operation[0] === "fillRect" && operation[1].style === A.PALETTE[0]), "shadow order");
    assert.ok(fake.operations.slice(0, rearStart).some(operation =>
      operation[0] === "fillRect" && operation[1].style === A.PALETTE[18]), "rear cable order");
    assert.ok(fake.operations.slice(lastClaw + 1, furStart).some(operation =>
      operation[0] === "fillRect" && operation[1].style === A.PALETTE[0]), "contact-mask order");
    assert.ok(fake.operations.slice(furEnd + 1, prop).some(operation =>
      operation[0] === "drawImage" && operation[1].image.width === A.WORLD.width),
    "fine-detail order");
    assert.ok(fake.operations.slice(prop + 1).some(operation =>
      operation[0] === "fillRect" && operation[1].style === A.PALETTE[18]), "front cable order");
    const lastCable = fake.operations.findLastIndex(operation =>
      operation[0] === "fillRect" && operation[1].style === A.PALETTE[18]);
    assert.ok(fake.operations.slice(lastCable + 1).some(operation =>
      operation[0] === "fillRect" && operation[1].style === A.PALETTE[15]), "state rim order");
  });

  test("authored alternates are selected from joint and load bands without rotation", () => {
    const {fake, renderer, rig} = fixture();
    R.setChannelTarget(rig, "head-yaw", 0.7);
    R.setChannelTarget(rig, "front-right-reach-y", -14);
    for(let step = 0; step < 90; step += 1) assert.equal(R.solveRig(rig, 1 / 120), true);
    rig.supports["front-left"].load = 0.8;
    rig.supports["rear-right"].load = 0.2;
    assert.equal(D.render(renderer, rig, fixtureContext({costume:""}), "full"), true);
    const keys = atlasDraws(fake).map(entry => entry[0]);
    assert.ok(keys.includes("skull@turned"));
    assert.ok(keys.includes("arm-fr-upper@turned"));
    assert.ok(keys.includes("arm-fl-upper@loaded"));
    assert.ok(keys.includes("palm-fl@loaded"));
    assert.ok(keys.includes("claw-front-left-1@loaded"));
    assert.equal(fake.operations.some(operation => operation[0] === "rotate"), false);
  });

  test("loaded claw modes select authored contact silhouettes at partial load", () => {
    const {fake, renderer, rig} = fixture();
    assert.equal(rig.supports["front-left"].load, 0.58);
    assert.equal(D.render(renderer, rig, fixtureContext({costume:""}), "economy"), true);
    const keys = atlasDraws(fake).map(entry => entry[0]);
    assert.ok(keys.includes("claw-front-left-1@loaded"));
    assert.ok(keys.includes("claw-rear-right-1@loaded"));
    assert.ok(keys.includes("claw-front-right-1"));
    assert.equal(keys.includes("claw-front-right-1@loaded"), false);
  });

  test("body lean and lift move torso, face, fur, and attached props coherently", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext();
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const before = destinationMap(fake);
    fake.context.reset();
    rig.values[R.channelIndex("body-lean-x")] = 5;
    rig.values[R.channelIndex("body-lift")] = 3;
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const after = destinationMap(fake);
    for(const id of ["pelvis", "ribcage", "neck-upper", "skull", "face-mask",
                       "fur-back", "fur-head", "prop:corona"]){
      assert.equal(after.get(id)[0] - before.get(id)[0], 10, `${id} lean`);
      assert.equal(after.get(id)[1] - before.get(id)[1], -6, `${id} lift`);
    }
  });

  test("axial parent motion propagates through every visual descendant", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:""});
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const before = destinationMap(fake);

    fake.context.reset();
    rig.values[R.channelIndex("spine-pelvis-x")] = 6;
    rig.values[R.channelIndex("spine-pelvis-y")] = -3;
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const pelvisAfter = destinationMap(fake);
    for(const id of ["pelvis", "abdomen", "ribcage", "neck-lower", "neck-mid",
                       "neck-upper", "skull", "face-mask", "fur-back", "fur-belly",
                       "fur-head"]){
      assert.deepEqual([
        pelvisAfter.get(id)[0] - before.get(id)[0],
        pelvisAfter.get(id)[1] - before.get(id)[1],
      ], [12, -6], `${id} must inherit pelvis translation`);
    }

    fake.context.reset();
    rig.values[R.channelIndex("neck-lower-angle")] = 0.75;
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const neckAfter = destinationMap(fake);
    for(const id of ["neck-mid", "neck-upper", "skull", "face-mask", "muzzle",
                       "eye-left", "eye-right", "fur-head"]){
      assert.notDeepEqual(neckAfter.get(id).slice(0, 2),
        pelvisAfter.get(id).slice(0, 2), `${id} must inherit lower-neck motion`);
    }
  });

  test("solved joints and endpoints drive lower limb, palm, and claw destinations", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:""});
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const before = destinationMap(fake);
    fake.context.reset();
    R.setChannelTarget(rig, "front-right-reach-x", -16);
    R.setChannelTarget(rig, "front-right-reach-y", 12);
    R.setChannelTarget(rig, "front-right-lift", 8);
    for(let step = 0; step < 45; step += 1){
      assert.equal(R.solveRig(rig, 1 / 120), true);
    }
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const after = destinationMap(fake);
    for(const id of ["arm-fr-fore", "wrist-fr", "palm-fr",
                       "claw-front-right-1", "claw-front-right-2", "claw-front-right-3"]){
      assert.notDeepEqual(after.get(id).slice(0, 2), before.get(id).slice(0, 2), id);
    }
    assert.deepEqual(after.get("pelvis"), before.get("pelvis"),
      "unrelated torso geometry must remain stable");
  });

  test("palm-authored props follow the solved attachment instead of static Art bounds", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:"huevo"});
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const before = destinationMap(fake);
    fake.context.reset();
    R.setChannelTarget(rig, "front-right-reach-x", -16);
    R.setChannelTarget(rig, "front-right-reach-y", 12);
    R.setChannelTarget(rig, "front-right-lift", 8);
    for(let step = 0; step < 45; step += 1) assert.equal(R.solveRig(rig, 1 / 120), true);
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const after = destinationMap(fake);
    const palmDelta = [after.get("palm-fr")[0] - before.get("palm-fr")[0],
      after.get("palm-fr")[1] - before.get("palm-fr")[1]];
    const propDelta = [after.get("prop:huevo")[0] - before.get("prop:huevo")[0],
      after.get("prop:huevo")[1] - before.get("prop:huevo")[1]];
    assert.deepEqual(propDelta, palmDelta);
  });

  test("jaw, eye, and lid channels visibly offset integer face pieces", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:""});
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const before = destinationMap(fake);
    fake.context.reset();
    rig.values[R.channelIndex("jaw-open")] = 0.9;
    rig.values[R.channelIndex("eye-left-look-x")] = 0.8;
    rig.values[R.channelIndex("eye-left-look-y")] = -0.8;
    rig.values[R.channelIndex("lid-left-upper")] = 0.9;
    rig.values[R.channelIndex("lid-left-lower")] = 0.7;
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const after = destinationMap(fake);
    assert.notDeepEqual(after.get("jaw").slice(0, 2), before.get("jaw").slice(0, 2));
    assert.notDeepEqual(after.get("eye-left").slice(0, 2), before.get("eye-left").slice(0, 2));
    assert.notDeepEqual(after.get("lid-left-upper").slice(0, 2),
      before.get("lid-left-upper").slice(0, 2));
    assert.notDeepEqual(after.get("lid-left-lower").slice(0, 2),
      before.get("lid-left-lower").slice(0, 2));
    assert.deepEqual(after.get("eye-right"), before.get("eye-right"));
  });

  test("fur, brow, cheek, claw, prop, and lighting channels change visible geometry", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:"bufanda"});
    assert.equal(D.render(renderer, rig, context, "full"), true);
    const before = destinationMap(fake);
    const beforeRim = fake.fills.filter(call => call.style === A.PALETTE[15])
      .map(call => call.rectangle);
    for(const [channel, value] of [
      ["fur-head-crest", 1], ["fur-back-shoulder", 1], ["fur-belly-flank", 1],
      ["brow-left-lift", 1], ["cheek-right-puff", 1],
      ["claw-front-left-1-curl", 1], ["claw-front-left-1-spread", 1],
      ["prop-bufanda-sway", 1], ["light-rim-intensity", 1],
    ]) assert.equal(R.setChannelTarget(rig, channel, value), true, channel);
    for(let step = 0; step < 120; step += 1) assert.equal(R.solveRig(rig, 1 / 120), true);

    fake.context.reset();
    assert.equal(D.render(renderer, rig, context, "full"), true);
    const after = destinationMap(fake);
    for(const id of ["fur-head", "fur-back", "fur-belly", "lid-left-upper",
                       "muzzle", "claw-front-left-1", "prop:bufanda"]){
      assert.notDeepEqual(after.get(id).slice(0, 2), before.get(id).slice(0, 2), id);
    }
    const afterRim = fake.fills.filter(call => call.style === A.PALETTE[15])
      .map(call => call.rectangle);
    assert.notDeepEqual(afterRim, beforeRim,
      "lighting channels must change bounded state-rim geometry");
  });

  test("authored Full dither and state rim remain attached to the moving torso", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:""});
    assert.equal(D.render(renderer, rig, context, "full"), true);
    const beforeDither = fake.fills.filter(call => call.style === A.PALETTE[7])
      .map(call => call.rectangle);
    const beforeRim = fake.fills.filter(call => call.style === A.PALETTE[15])
      .map(call => call.rectangle);
    assert.ok(beforeDither.length > 0);
    assert.ok(beforeRim.length > 0);

    fake.context.reset();
    rig.values[R.channelIndex("spine-pelvis-x")] = 4;
    rig.values[R.channelIndex("spine-pelvis-y")] = -2;
    assert.equal(D.render(renderer, rig, context, "full"), true);
    const afterDither = fake.fills.filter(call => call.style === A.PALETTE[7])
      .map(call => call.rectangle);
    const afterRim = fake.fills.filter(call => call.style === A.PALETTE[15])
      .map(call => call.rectangle);
    assert.deepEqual(afterDither.map((rectangle, index) =>
      [rectangle[0] - beforeDither[index][0], rectangle[1] - beforeDither[index][1]]),
    afterDither.map(() => [8, -4]));
    assert.deepEqual(afterRim.map((rectangle, index) =>
      [rectangle[0] - beforeRim[index][0], rectangle[1] - beforeRim[index][1]]),
    afterRim.map(() => [8, -4]));
  });

  test("behavior-range pelvis inclination visibly cascades through axial descendants", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:""});
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const before = destinationMap(fake);
    fake.context.reset();
    rig.values[R.channelIndex("spine-pelvis-angle")] = 0.08;
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const after = destinationMap(fake);
    const deltaX = id => after.get(id)[0] - before.get(id)[0];
    assert.equal(deltaX("pelvis"), 0);
    assert.ok(deltaX("abdomen") > 0);
    assert.ok(deltaX("ribcage") >= deltaX("abdomen"));
    assert.ok(deltaX("skull") > deltaX("ribcage"));
  });

  test("inspect visor glow channel produces face-attached light pixels", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:"visor"});
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const before = fake.fills.filter(call => call.style === A.PALETTE[12]).length;
    fake.context.reset();
    rig.values[R.channelIndex("light-visor-glow")] = 0.9;
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const after = fake.fills.filter(call => call.style === A.PALETTE[12]).length;
    assert.ok(after > before, "light-visor-glow must emit bounded face light pixels");
  });

  test("retained fine-detail cells remain attached to their anatomical parents", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:""});
    assert.equal(D.render(renderer, rig, context, "balanced"), true);
    const before = fake.draws.filter(call =>
      call.image && call.image.width === A.WORLD.width && call.image.height === A.WORLD.height)
      .map(call => call.destination);
    assert.equal(before.length, 3);
    fake.context.reset();
    rig.values[R.channelIndex("spine-pelvis-x")] = 4;
    rig.values[R.channelIndex("spine-pelvis-y")] = -2;
    assert.equal(D.render(renderer, rig, context, "balanced"), true);
    const after = fake.draws.filter(call =>
      call.image && call.image.width === A.WORLD.width && call.image.height === A.WORLD.height)
      .map(call => call.destination);
    assert.equal(after.length, 3);
    for(let index = 0; index < before.length; index += 1){
      assert.deepEqual([after[index][0] - before[index][0], after[index][1] - before[index][1]],
        [8, -4], `detail cell ${index} detached from parent`);
    }
  });

  test("cable pixels and contact masks follow actual Rig nodes and support points", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext({costume:""});
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const cableBefore = fake.fills.filter(call => call.style === A.PALETTE[18])
      .map(call => call.rectangle);
    const contactBefore = fake.fills.filter(call => call.style === A.PALETTE[0])
      .map(call => call.rectangle);
    fake.context.reset();
    rig.cable[6] += 7;
    rig.cable[7] += 3;
    rig.supports["front-left"].point.x += 4;
    rig.supports["front-left"].point.y += 2;
    rig.claws["claw-front-left-1"].point.x += 4;
    rig.claws["claw-front-left-1"].point.y += 2;
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    const cableAfter = fake.fills.filter(call => call.style === A.PALETTE[18])
      .map(call => call.rectangle);
    const contactAfter = fake.fills.filter(call => call.style === A.PALETTE[0])
      .map(call => call.rectangle);
    assert.notDeepEqual(cableAfter, cableBefore, "cable ignored its typed node buffer");
    assert.notDeepEqual(contactAfter, contactBefore, "contact mask ignored its support point");
  });

  test("Full, Balanced, and Economy preserve anatomy while applying exact detail policy", () => {
    for(const quality of ["full", "balanced", "economy"]){
      const {fake, renderer, rig} = fixture();
      assert.equal(D.render(renderer, rig, fixtureContext(), quality), true);
      const keys = atlasDraws(fake).map(entry => baseId(entry[0]));
      for(const id of [...FACE, ...CLAWS, "fur-back", "fur-belly", "fur-head", "prop:corona"]){
        assert.ok(keys.includes(id), `${quality} dropped ${id}`);
      }
      const detailDraws = fake.draws.filter(call =>
        call.image && call.image.width === A.WORLD.width && call.image.height === A.WORLD.height);
      const diagnostics = D.rendererDiagnostics(renderer);
      if(quality === "full"){
        assert.equal(detailDraws.length, 3);
        assert.ok(diagnostics.dynamicDitherPixels > 0);
        assert.equal(diagnostics.detailPolicy, "all");
      }else if(quality === "balanced"){
        assert.equal(detailDraws.length, 3);
        assert.equal(diagnostics.dynamicDitherPixels, 0);
        assert.equal(diagnostics.detailPolicy, "merged-fine");
      }else{
        assert.equal(detailDraws.length, 0);
        assert.equal(diagnostics.dynamicDitherPixels, 0);
        assert.equal(diagnostics.detailPolicy, "medium-key");
      }
    }
  });

  test("face, contacts, silhouette, and props survive Economy rendering", () => {
    const {fake, renderer, rig} = fixture();
    assert.equal(D.render(renderer, rig, fixtureContext({status:"working"}), "economy"), true);
    const ids = atlasDraws(fake).map(entry => baseId(entry[0]));
    assert.ok(FACE.every(id => ids.includes(id)));
    assert.ok(CLAWS.every(id => ids.includes(id)));
    assert.ok(ids.includes("prop:corona"));
    assert.ok(fake.fills.length > 0, "contact masks, cable, shadow, and rim must remain");
    assert.equal(D.rendererDiagnostics(renderer).contactMasks > 0, true);
  });

  test("authored contact masks are selected from loaded supports and state", () => {
    const {renderer, rig} = fixture();
    let context = fixtureContext({costume:""});
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    assert.equal(D.rendererDiagnostics(renderer).contactMasks, 2,
      "safe diagonal selects front-left and rear contact masks");
    assert.equal(R.requestGrip(rig, "front-right", "loaded", 0.72), true);
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    assert.equal(D.rendererDiagnostics(renderer).contactMasks, 3);
    context = fixtureContext({status:"dead", costume:""});
    assert.equal(D.render(renderer, rig, context, "economy"), true);
    assert.equal(D.rendererDiagnostics(renderer).contactMasks, 4,
      "dead safe curl adds the authored belly mask");
  });

  test("compact camera is authored, bounded, and never fractionally scaled", () => {
    const {renderer, rig} = fixture(100, 80, 1);
    assert.equal(D.render(renderer, rig, fixtureContext(), "economy"), true);
    const diagnostics = D.rendererDiagnostics(renderer);
    assert.deepEqual(diagnostics.camera,
      {x:0, y:0, sourceX:62, sourceY:42, scale:1, width:100, height:80});
    assert.ok(diagnostics.camera.width <= 180);
    assert.ok(diagnostics.camera.height <= 148);
    assert.ok(Object.values(diagnostics.camera).every(Number.isInteger));
  });

  test("compact camera selection happens in logical pixels before DPR scaling", () => {
    const {renderer, rig} = fixture(180, 148, 2);
    assert.equal(D.render(renderer, rig, fixtureContext(), "economy"), true);
    assert.deepEqual(D.rendererDiagnostics(renderer).camera,
      {x:0, y:0, sourceX:22, sourceY:8, scale:2, width:180, height:148});
  });

  test("fractional DPR allocates only an integer effective pixel scale", () => {
    for(const dpr of [1.25, 1.5]){
      const {fake, renderer, rig} = fixture(256, 208, dpr);
      assert.equal(D.render(renderer, rig, fixtureContext(), "economy"), true);
      const camera = D.rendererDiagnostics(renderer).camera;
      assert.equal(camera.scale, 1, `${dpr} DPR must use one physical pixel per logical pixel`);
      assert.equal(fake.canvas.width, 256, `${dpr} DPR retained unusable horizontal pixels`);
      assert.equal(fake.canvas.height, 208, `${dpr} DPR retained unusable vertical pixels`);
      for(const call of fake.draws){
        assert.ok(call.destination.every(Number.isInteger), JSON.stringify(call.destination));
      }
    }
  });

  test("clean rendering touches no Canvas 2D state and ignores nonvisual timestamps", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext();
    assert.equal(D.render(renderer, rig, context, "full"), true);
    fake.context.reset();
    assert.equal(D.render(renderer, rig, context, "full"), false);
    assert.deepEqual(fake.operations, []);
    assert.equal(D.render(renderer, rig, fixtureContext({timestamp:999}), "full"), false);
    assert.deepEqual(fake.operations, []);
    const diagnostics = D.rendererDiagnostics(renderer);
    assert.equal(diagnostics.renders, 1);
    assert.equal(diagnostics.skippedClean, 2);
  });

  test("pose, palette, prop, camera, contact, quality, and manual revisions redraw once", () => {
    const {fake, renderer, rig} = fixture();
    let context = fixtureContext();
    assert.equal(D.render(renderer, rig, context, "full"), true);

    R.setChannelTarget(rig, "jaw-open", 0.9);
    assert.equal(R.solveRig(rig, 1 / 60), true);
    assert.equal(D.render(renderer, rig, context, "full"), true, "pose");
    assert.equal(D.render(renderer, rig, context, "full"), false);

    assert.equal(D.setTheme(renderer, "light"), true);
    assert.equal(D.render(renderer, rig, context, "full"), true, "palette");
    context = fixtureContext({costume:"visor"});
    assert.equal(D.render(renderer, rig, context, "full"), true, "prop");
    assert.equal(D.setViewport(renderer, 180, 148, 1), true);
    assert.equal(D.render(renderer, rig, context, "full"), true, "camera");
    assert.equal(R.requestGrip(rig, "front-right", "loaded", 0.72), true);
    assert.equal(D.render(renderer, rig, context, "full"), true, "contact mask");
    assert.equal(D.render(renderer, rig, context, "balanced"), true, "quality");
    assert.equal(D.markDirty(renderer, "external-state"), true);
    assert.equal(D.render(renderer, rig, context, "balanced"), true, "manual");
    assert.equal(D.render(renderer, rig, context, "balanced"), false);
    assert.ok(fake.draws.length > 0);
  });

  test("theme cache replacement preserves the exact rig pose and destinations", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext();
    const beforeHash = R.poseHash(rig);
    assert.equal(D.render(renderer, rig, context, "full"), true);
    const before = new Map(atlasDraws(fake).map(([key, call]) => [baseId(key), call.destination]));
    fake.context.reset();
    assert.equal(D.setTheme(renderer, "light"), true);
    assert.equal(R.poseHash(rig), beforeHash);
    assert.equal(D.render(renderer, rig, context, "full"), true);
    const after = new Map(atlasDraws(fake).map(([key, call]) => [baseId(key), call.destination]));
    assert.deepEqual(after, before);
    assert.equal(R.poseHash(rig), beforeHash);
  });

  test("Static ignores live pose revisions and redraws an authored safe pose only for visual events", () => {
    const {fake, renderer, rig} = fixture();
    let context = fixtureContext({costume:""});
    assert.equal(D.render(renderer, rig, context, "static"), true);
    const before = atlasDraws(fake).map(([key, call]) => [baseId(key), call.destination]);
    const clawDraw = atlasDraws(fake).find(([key]) => baseId(key) === "claw-front-right-1");
    const clawRect = atlasFixture.rects[clawDraw[0]];
    const scale = D.rendererDiagnostics(renderer).camera.scale;
    const clawAnchor = [clawDraw[1].destination[0] + clawRect.pivotX * scale,
      clawDraw[1].destination[1] + clawRect.pivotY * scale];
    const cablePixels = fake.fills.filter(call => call.style === A.PALETTE[18]);
    assert.ok(cablePixels.some(call => Math.hypot(call.rectangle[0] - clawAnchor[0],
      call.rectangle[1] - clawAnchor[1]) <= 3 * scale),
    "Static safe pose must visibly attach a front claw to the execution cable");
    fake.context.reset();
    rig.values[R.channelIndex("head-yaw")] = 0.8;
    rig.cable[4] += 5;
    rig.diagnostics.steps += 1;
    assert.equal(D.render(renderer, rig, context, "static"), false);
    assert.deepEqual(fake.operations, []);

    context = fixtureContext({status:"waiting", costume:""});
    assert.equal(D.render(renderer, rig, context, "static"), true);
    const after = atlasDraws(fake).map(([key, call]) => [baseId(key), call.destination]);
    assert.deepEqual(after, before);
    fake.context.reset();
    assert.equal(D.setViewport(renderer, 180, 148, 1), true);
    assert.equal(D.render(renderer, rig, context, "static"), true);
  });

  test("decoded memory accounts for atlas pages, typed arrays, and retained caches below 16 MiB", () => {
    const {renderer, rig} = fixture();
    D.render(renderer, rig, fixtureContext(), "full");
    D.setTheme(renderer, "light");
    const diagnostics = D.rendererDiagnostics(renderer);
    assert.ok(diagnostics.atlasBytes >= 2 * 1024 * 1024 * 4);
    assert.ok(diagnostics.typedArrayBytes > 0);
    assert.ok(diagnostics.retainedCacheBytes > 0);
    assert.equal(diagnostics.decodedBytes,
      diagnostics.atlasBytes + diagnostics.typedArrayBytes + diagnostics.retainedCacheBytes);
    assert.ok(diagnostics.decodedBytes < 16 * 1024 * 1024,
      `${diagnostics.decodedBytes} decoded bytes`);
  });

  test("renderer hot path retains allocation shape across stress redraws", () => {
    const {fake, renderer, rig} = fixture();
    const context = fixtureContext();
    D.render(renderer, rig, context, "full");
    const before = D.rendererDiagnostics(renderer);
    for(let frame = 0; frame < 1000; frame += 1){
      fake.context.reset();
      assert.equal(D.markDirty(renderer, "stress"), true);
      assert.equal(D.render(renderer, rig, context, "full"), true);
    }
    const after = D.rendererDiagnostics(renderer);
    assert.equal(after.atlasBuilds, before.atlasBuilds);
    assert.equal(after.decodedBytes, before.decodedBytes);
    const source = fs.readFileSync(rendererPath, "utf8");
    const start = source.indexOf("function scanPose(");
    const end = source.indexOf("function markDirty(", start);
    assert.ok(start >= 0 && end > start);
    const hotPath = source.slice(start, end);
    assert.doesNotMatch(hotPath, /\bnew\s+(?:Array|Float\d+Array|Int\d+Array|Map|Set)\b/);
    assert.doesNotMatch(hotPath, /Object\.(?:keys|values|entries)\s*\(/);
    assert.doesNotMatch(hotPath, /\.map\s*\(/);
    assert.doesNotMatch(hotPath, /(?:const|let|var)\s+\w+\s*=\s*\[/,
      "steady render helpers must not create temporary arrays");
  });

  test("public methods are total for hostile input and destruction is idempotent", () => {
    assert.throws(() => D.createRenderer(null), TypeError);
    assert.throws(() => D.createRenderer({getContext(){ return null; }}), /2d canvas context/);
    const {fake, renderer, rig} = fixture();
    const before = fake.operations.length;
    for(const invoke of [
      () => D.setViewport(renderer, NaN, 10, 1),
      () => D.setViewport(renderer, 10, -1, 1),
      () => D.setViewport(renderer, 10, 10, Infinity),
      () => D.setTheme(renderer, "sepia"),
      () => D.render(renderer, null, fixtureContext(), "full"),
      () => D.render(renderer, rig, null, "full"),
      () => D.render(renderer, rig, fixtureContext(), "ultra"),
      () => D.markDirty(null, "bad"),
      () => D.destroyRenderer(null),
    ]) assert.doesNotThrow(() => assert.equal(invoke(), false));
    assert.equal(fake.operations.length, before);
    assert.equal(D.rendererDiagnostics(null), null);

    assert.equal(D.render(renderer, rig, fixtureContext(), "full"), true);
    assert.equal(D.destroyRenderer(renderer), true);
    const afterDestroy = fake.operations.length;
    assert.equal(D.destroyRenderer(renderer), false);
    assert.equal(D.render(renderer, rig, fixtureContext(), "full"), false);
    assert.equal(D.setTheme(renderer, "light"), false);
    assert.equal(D.markDirty(renderer, "late"), false);
    assert.equal(fake.operations.length, afterDestroy);
    const diagnostics = D.rendererDiagnostics(renderer);
    assert.equal(diagnostics.destroyed, true);
    assert.equal(diagnostics.decodedBytes, 0);
  });

  test("viewport mutation rolls width back when a hostile height setter throws", () => {
    const context = recordingContext();
    let width = 64;
    let height = 64;
    const canvas = {
      get width(){ return width; },
      set width(value){ width = value; },
      get height(){ return height; },
      set height(value){
        if(value !== height) throw new Error("hostile height");
        height = value;
      },
      getContext(kind){ return kind === "2d" ? context : null; },
    };
    const fake = fakeCanvas();
    const renderer = D.createRenderer(canvas, {canvasFactory:fake.factory});
    assert.equal(D.setViewport(renderer, 256, 208, 1), false);
    assert.equal(width, 64);
    assert.equal(height, 64);
    assert.equal(D.rendererDiagnostics(renderer).camera, null);
  });
}
