(function(root){
  "use strict";

  const NS = root.ComandOSPerezOS = root.ComandOSPerezOS || {};
  if(!NS.Art) throw new Error("ComandOSPerezOS.Art must load before Renderer");
  if(!NS.Rig) throw new Error("ComandOSPerezOS.Rig must load before Renderer");

  const Art = NS.Art;
  const Rig = NS.Rig;
  const QUALITY = Object.freeze({
    FULL:"full", BALANCED:"balanced", ECONOMY:"economy", STATIC:"static",
  });
  const QUALITY_NAMES = Object.freeze([
    QUALITY.FULL, QUALITY.BALANCED, QUALITY.ECONOMY, QUALITY.STATIC,
  ]);
  const OCCLUSION_GROUPS = Object.freeze([
    "rear-cable-shadow", "rear-limbs", "axial-deep-fur", "torso",
    "front-limbs", "face", "claws-contact-masks", "medium-fine-fur",
    "props", "front-cable", "state-rim-light",
  ]);
  const LIMB_NAMES = Object.freeze([
    "front-left", "front-right", "rear-left", "rear-right",
  ]);
  const CLAW_IDS = Object.freeze([
    "claw-front-left-1", "claw-front-left-2", "claw-front-left-3",
    "claw-front-right-1", "claw-front-right-2", "claw-front-right-3",
    "claw-rear-left-1", "claw-rear-left-2", "claw-rear-left-3",
    "claw-rear-right-1", "claw-rear-right-2", "claw-rear-right-3",
  ]);
  const REAR_IDS = Object.freeze([
    "arm-fr-upper", "arm-fr-fore", "wrist-fr", "palm-fr",
    "leg-rr-upper", "leg-rr-lower", "ankle-rr", "palm-rr",
  ]);
  const TORSO_IDS = Object.freeze([
    "pelvis", "abdomen", "ribcage", "neck-lower", "neck-mid", "neck-upper",
  ]);
  const FRONT_IDS = Object.freeze([
    "arm-fl-upper", "arm-fl-fore", "wrist-fl", "palm-fl",
    "leg-rl-upper", "leg-rl-lower", "ankle-rl", "palm-rl",
  ]);
  const FACE_IDS = Object.freeze([
    "skull", "face-mask", "muzzle", "jaw", "nose", "eye-left", "eye-right",
    "lid-left-upper", "lid-left-lower", "lid-right-upper", "lid-right-lower",
  ]);
  const FUR_IDS = Object.freeze(["fur-belly", "fur-head"]);
  const STATIC_CABLE = Object.freeze([
    26,-11, 44,6, 63,23, 81,40, 100,57, 141,74, 153,91, 155,108, 173,125,
  ]);
  const RENDERERS = new WeakMap();
  const PART_BY_ID = Object.create(null);
  const VARIANT_KEYS = Object.create(null);
  const PROP_KEYS = Object.create(null);
  const POSE_RECORDS = Object.create(null);
  const REST_X = Object.create(null);
  const REST_Y = Object.create(null);

  for(let index = 0; index < Art.PARTS.length; index += 1){
    const part = Art.PARTS[index];
    PART_BY_ID[part.id] = part;
    REST_X[part.id] = part.bounds[0] + part.pivot[0];
    REST_Y[part.id] = part.bounds[1] + part.pivot[1];
    const keys = Object.create(null);
    keys.base = part.id;
    if(part.states.loaded) keys.loaded = `${part.id}@loaded`;
    if(part.states.searching) keys.searching = `${part.id}@searching`;
    if(part.states.turned) keys.turned = `${part.id}@turned`;
    VARIANT_KEYS[part.id] = Object.freeze(keys);
    POSE_RECORDS[part.id] = Object.seal({kind:0, limb:"", fraction:0,
      offsetX:0, offsetY:0});
  }
  for(const name in Art.PROPS) PROP_KEYS[name] = `prop:${name}`;

  function restAnchor(id){
    return {x:REST_X[id], y:REST_Y[id]};
  }

  function configureLimb(limbName, upperId, lowerId, middleId, palmId){
    const lower = restAnchor(lowerId);
    const middle = restAnchor(middleId);
    const palm = restAnchor(palmId);
    const span = Math.hypot(palm.x - lower.x, palm.y - lower.y);
    const middleSpan = Math.hypot(middle.x - lower.x, middle.y - lower.y);
    POSE_RECORDS[upperId] = Object.freeze({kind:1, limb:limbName, fraction:0,
      offsetX:0, offsetY:0});
    POSE_RECORDS[lowerId] = Object.freeze({kind:2, limb:limbName, fraction:0,
      offsetX:0, offsetY:0});
    POSE_RECORDS[middleId] = Object.freeze({kind:3, limb:limbName,
      fraction:span > 0 ? middleSpan / span : 0.7, offsetX:0, offsetY:0});
    POSE_RECORDS[palmId] = Object.freeze({kind:4, limb:limbName, fraction:1,
      offsetX:0, offsetY:0});
    for(let clawIndex = 1; clawIndex <= 3; clawIndex += 1){
      const clawId = `claw-${limbName}-${clawIndex}`;
      const clawAnchor = restAnchor(clawId);
      POSE_RECORDS[clawId] = Object.freeze({kind:5, limb:limbName, fraction:1,
        offsetX:clawAnchor.x - palm.x, offsetY:clawAnchor.y - palm.y});
    }
  }

  configureLimb("front-left", "arm-fl-upper", "arm-fl-fore", "wrist-fl", "palm-fl");
  configureLimb("front-right", "arm-fr-upper", "arm-fr-fore", "wrist-fr", "palm-fr");
  configureLimb("rear-left", "leg-rl-upper", "leg-rl-lower", "ankle-rl", "palm-rl");
  configureLimb("rear-right", "leg-rr-upper", "leg-rr-lower", "ankle-rr", "palm-rr");

  const INDEX = Object.freeze({
    bodyLean:Rig.channelIndex("body-lean-x"),
    bodyLift:Rig.channelIndex("body-lift"),
    pelvisX:Rig.channelIndex("spine-pelvis-x"),
    pelvisY:Rig.channelIndex("spine-pelvis-y"),
    pelvisAngle:Rig.channelIndex("spine-pelvis-angle"),
    lowerAngle:Rig.channelIndex("spine-lower-angle"),
    midAngle:Rig.channelIndex("spine-mid-angle"),
    upperAngle:Rig.channelIndex("spine-upper-angle"),
    neckLower:Rig.channelIndex("neck-lower-angle"),
    neckMid:Rig.channelIndex("neck-mid-angle"),
    neckUpper:Rig.channelIndex("neck-upper-angle"),
    headYaw:Rig.channelIndex("head-yaw"),
    headPitch:Rig.channelIndex("head-pitch"),
    headRoll:Rig.channelIndex("head-roll"),
    faceTurn:Rig.channelIndex("face-turn"),
    jawOpen:Rig.channelIndex("jaw-open"),
    muzzleLift:Rig.channelIndex("muzzle-lift"),
    noseTwitch:Rig.channelIndex("nose-twitch"),
    eyeLeftX:Rig.channelIndex("eye-left-look-x"),
    eyeLeftY:Rig.channelIndex("eye-left-look-y"),
    eyeRightX:Rig.channelIndex("eye-right-look-x"),
    eyeRightY:Rig.channelIndex("eye-right-look-y"),
    lidLeftUpper:Rig.channelIndex("lid-left-upper"),
    lidLeftLower:Rig.channelIndex("lid-left-lower"),
    lidRightUpper:Rig.channelIndex("lid-right-upper"),
    lidRightLower:Rig.channelIndex("lid-right-lower"),
    flShoulder:Rig.channelIndex("front-left-shoulder-angle"),
    flElbow:Rig.channelIndex("front-left-elbow-angle"),
    frShoulder:Rig.channelIndex("front-right-shoulder-angle"),
    frElbow:Rig.channelIndex("front-right-elbow-angle"),
    rlShoulder:Rig.channelIndex("rear-left-shoulder-angle"),
    rlElbow:Rig.channelIndex("rear-left-elbow-angle"),
    rrShoulder:Rig.channelIndex("rear-right-shoulder-angle"),
    rrElbow:Rig.channelIndex("rear-right-elbow-angle"),
  });

  const POSE_SNAPSHOT_LENGTH = Rig.CHANNELS.length + 18 + 4 * 9 + 4 * 7 + 12 * 5;

  function defaultCanvasFactory(width, height){
    if(!root.document || typeof root.document.createElement !== "function"){
      throw new Error("PerezOS Renderer requires canvasFactory outside a browser");
    }
    const canvas = root.document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    return canvas;
  }

  function themeName(theme){
    return theme === "dark" || theme === "light" ? theme : "";
  }

  function buildDetailPage(internal, theme, palette){
    const canvas = internal.canvasFactory(Art.WORLD.width, Art.WORLD.height);
    if(!canvas || typeof canvas.getContext !== "function"){
      throw new Error("PerezOS detail cache requires a canvas");
    }
    const ctx = canvas.getContext("2d", {alpha:true});
    if(!ctx) throw new Error("PerezOS detail cache requires a 2d canvas context");
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, Art.WORLD.width, Art.WORLD.height);
    ctx.fillStyle = palette[0];
    for(let y = 54; y <= 114; y += 10){
      ctx.fillRect(82 + ((y / 10) & 1) * 3, y, 2, 1);
      ctx.fillRect(127 - ((y / 10) & 1) * 2, y + 4, 1, 1);
    }
    ctx.fillStyle = palette[7];
    for(let x = 91; x <= 130; x += 7) ctx.fillRect(x, 18 + (x & 3), 1, 1);
    return canvas;
  }

  function buildThemePage(internal, theme){
    const atlas = Art.buildAtlas(internal.canvasFactory, theme);
    const detail = buildDetailPage(internal, theme, atlas.palette);
    const page = Object.freeze({atlas, detail});
    internal.pages[theme] = page;
    internal.atlasBuilds += 1;
    internal.atlasBytes += atlas.canvas.width * atlas.canvas.height * 4;
    internal.retainedCacheBytes += detail.width * detail.height * 4;
    return page;
  }

  function createRenderer(canvas, options){
    if(!canvas || typeof canvas.getContext !== "function"){
      throw new TypeError("createRenderer requires a canvas");
    }
    const ctx = canvas.getContext("2d", {alpha:true});
    if(!ctx) throw new Error("PerezOS Renderer requires a 2d canvas context");
    const factory = options && typeof options.canvasFactory === "function" ?
      options.canvasFactory : defaultCanvasFactory;
    const initialTheme = options && themeName(options.theme) ? options.theme : "dark";
    const poseSnapshot = new Float64Array(POSE_SNAPSHOT_LENGTH);
    const anchor = new Int32Array(2);
    const groupCounts = new Int16Array(OCCLUSION_GROUPS.length);
    const renderer = Object.seal({canvas});
    const internal = {
      canvas, ctx, canvasFactory:factory, pages:{dark:null, light:null},
      theme:initialTheme, page:null, poseSnapshot, anchor, groupCounts,
      poseReady:false, lastRig:null, destroyed:false, dirty:true,
      viewportWidth:0, viewportHeight:0, dpr:0, backingWidth:0, backingHeight:0,
      camera:null, cameraRevision:0, paletteRevision:0, manualRevision:0,
      poseRevision:0, contextRevision:0, contactRevision:0, propRevision:0,
      renders:0, skippedClean:0, atlasBuilds:0, atlasBytes:0,
      retainedCacheBytes:0, typedArrayBytes:poseSnapshot.byteLength +
        anchor.byteLength + groupCounts.byteLength,
      quality:"", detailPolicy:"", dynamicDitherPixels:0, contactMasks:0,
      contextReady:false, status:"", role:"", costume:"", pressure:"",
      contextTheme:"", expanded:false, scanOffset:0, scanChanged:false,
      scanInvalid:false, currentMaskBits:0, lastMaskBits:-1,
    };
    internal.page = buildThemePage(internal, initialTheme);
    RENDERERS.set(renderer, internal);
    return renderer;
  }

  function setViewport(renderer, width, height, dpr){
    const internal = RENDERERS.get(renderer);
    if(!internal || internal.destroyed || !Number.isFinite(width) ||
       !Number.isFinite(height) || !Number.isFinite(dpr) || width <= 0 ||
       height <= 0 || dpr <= 0) return false;
    const backingWidth = Math.round(width * dpr);
    const backingHeight = Math.round(height * dpr);
    if(backingWidth < 1 || backingHeight < 1 || backingWidth > 8192 ||
       backingHeight > 8192) return false;
    if(internal.viewportWidth === width && internal.viewportHeight === height &&
       internal.dpr === dpr && internal.backingWidth === backingWidth &&
       internal.backingHeight === backingHeight) return false;
    try{
      if(internal.canvas.width !== backingWidth) internal.canvas.width = backingWidth;
      if(internal.canvas.height !== backingHeight) internal.canvas.height = backingHeight;
    }catch(error){
      return false;
    }
    internal.viewportWidth = width;
    internal.viewportHeight = height;
    internal.dpr = dpr;
    internal.backingWidth = backingWidth;
    internal.backingHeight = backingHeight;
    const logicalCamera = Art.compactCamera(Math.floor(width), Math.floor(height));
    let scale = Math.max(1, Math.floor(logicalCamera.scale * dpr));
    let cameraWidth = logicalCamera.width;
    let cameraHeight = logicalCamera.height;
    let sourceX = logicalCamera.sourceX;
    let sourceY = logicalCamera.sourceY;
    if(cameraWidth * scale > backingWidth || cameraHeight * scale > backingHeight){
      const physicalCamera = Art.compactCamera(backingWidth, backingHeight);
      scale = physicalCamera.scale;
      cameraWidth = physicalCamera.width;
      cameraHeight = physicalCamera.height;
      sourceX = physicalCamera.sourceX;
      sourceY = physicalCamera.sourceY;
    }
    internal.camera = Object.freeze({
      x:Math.floor((backingWidth - cameraWidth * scale) / 2),
      y:Math.floor((backingHeight - cameraHeight * scale) / 2),
      sourceX, sourceY, scale, width:cameraWidth, height:cameraHeight,
    });
    internal.cameraRevision += 1;
    internal.dirty = true;
    return true;
  }

  function setTheme(renderer, theme){
    const internal = RENDERERS.get(renderer);
    const normalized = themeName(theme);
    if(!internal || internal.destroyed || !normalized || normalized === internal.theme){
      return false;
    }
    try{
      internal.page = internal.pages[normalized] || buildThemePage(internal, normalized);
    }catch(error){
      return false;
    }
    internal.theme = normalized;
    internal.paletteRevision += 1;
    internal.dirty = true;
    return true;
  }

  function validQuality(quality){
    for(let index = 0; index < QUALITY_NAMES.length; index += 1){
      if(quality === QUALITY_NAMES[index]) return true;
    }
    return false;
  }

  function validInputs(rig, context){
    return !!rig && rig.values instanceof Float64Array &&
      rig.values.length === Rig.CHANNELS.length && rig.cable instanceof Float64Array &&
      rig.cable.length === 18 && !!rig.limbs && !!rig.claws && !!rig.supports &&
      !!context && typeof context === "object" && typeof context.status === "string" &&
      typeof context.role === "string" && typeof context.costume === "string" &&
      typeof context.contextPressure === "string" && typeof context.theme === "string" &&
      typeof context.expanded === "boolean";
  }

  function visualContextChanged(internal, context){
    return !internal.contextReady || internal.status !== context.status ||
      internal.role !== context.role || internal.costume !== context.costume ||
      internal.pressure !== context.contextPressure ||
      internal.contextTheme !== context.theme || internal.expanded !== context.expanded;
  }

  function saveVisualContext(internal, context){
    const oldCostume = internal.costume;
    internal.status = context.status;
    internal.role = context.role;
    internal.costume = context.costume;
    internal.pressure = context.contextPressure;
    internal.contextTheme = context.theme;
    internal.expanded = context.expanded;
    internal.contextReady = true;
    internal.contextRevision += 1;
    if(oldCostume !== context.costume) internal.propRevision += 1;
  }

  function scanPoseValue(internal, value, write){
    if(!Number.isFinite(value)){
      internal.scanInvalid = true;
      return;
    }
    const offset = internal.scanOffset;
    if(internal.poseSnapshot[offset] !== value) internal.scanChanged = true;
    if(write) internal.poseSnapshot[offset] = value;
    internal.scanOffset = offset + 1;
  }

  function scanPose(internal, rig, write){
    internal.scanOffset = 0;
    internal.scanChanged = !internal.poseReady || internal.lastRig !== rig;
    internal.scanInvalid = false;
    for(let index = 0; index < rig.values.length; index += 1){
      scanPoseValue(internal, rig.values[index], write);
    }
    for(let index = 0; index < rig.cable.length; index += 1){
      scanPoseValue(internal, rig.cable[index], write);
    }
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const limb = rig.limbs[LIMB_NAMES[index]];
      if(!limb || !limb.root || !limb.joint || !limb.end) return -1;
      scanPoseValue(internal, limb.root.x, write);
      scanPoseValue(internal, limb.root.y, write);
      scanPoseValue(internal, limb.joint.x, write);
      scanPoseValue(internal, limb.joint.y, write);
      scanPoseValue(internal, limb.end.x, write);
      scanPoseValue(internal, limb.end.y, write);
      scanPoseValue(internal, limb.upperAngle, write);
      scanPoseValue(internal, limb.lowerAngle, write);
      scanPoseValue(internal, limb.contactError, write);
    }
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const support = rig.supports[LIMB_NAMES[index]];
      if(!support || !support.point || !support.normal ||
         (support.mode !== "loaded" && support.mode !== "released")) return -1;
      const mode = support.mode === "loaded" ? 1 : 0;
      scanPoseValue(internal, mode, write);
      scanPoseValue(internal, support.cableT, write);
      scanPoseValue(internal, support.load, write);
      scanPoseValue(internal, support.point.x, write);
      scanPoseValue(internal, support.point.y, write);
      scanPoseValue(internal, support.normal.x, write);
      scanPoseValue(internal, support.normal.y, write);
    }
    for(let index = 0; index < CLAW_IDS.length; index += 1){
      const claw = rig.claws[CLAW_IDS[index]];
      if(!claw || !claw.point || (claw.mode !== "loaded" && claw.mode !== "released")){
        return -1;
      }
      const mode = claw.mode === "loaded" ? 1 : 0;
      scanPoseValue(internal, mode, write);
      scanPoseValue(internal, claw.cableT, write);
      scanPoseValue(internal, claw.point.x, write);
      scanPoseValue(internal, claw.point.y, write);
      scanPoseValue(internal, claw.contactError, write);
    }
    if(internal.scanInvalid || internal.scanOffset !== POSE_SNAPSHOT_LENGTH) return -1;
    if(write){
      internal.poseReady = true;
      internal.lastRig = rig;
    }
    return internal.scanChanged ? 1 : 0;
  }

  function contactSnapshotChanged(internal, rig){
    if(!internal.poseReady || internal.lastRig !== rig) return true;
    let offset = Rig.CHANNELS.length + 18 + LIMB_NAMES.length * 9;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const support = rig.supports[LIMB_NAMES[index]];
      const mode = support.mode === "loaded" ? 1 : 0;
      if(internal.poseSnapshot[offset] !== mode ||
         internal.poseSnapshot[offset + 3] !== support.point.x ||
         internal.poseSnapshot[offset + 4] !== support.point.y) return true;
      offset += 7;
    }
    return false;
  }

  function screenX(internal, worldX){
    return internal.camera.x + (Math.round(worldX) - internal.camera.sourceX) *
      internal.camera.scale;
  }

  function screenY(internal, worldY){
    return internal.camera.y + (Math.round(worldY) - internal.camera.sourceY) *
      internal.camera.scale;
  }

  function globalAnchor(internal, part, rig, staticMode){
    let x = part.bounds[0] + part.pivot[0];
    let y = part.bounds[1] + part.pivot[1];
    if(staticMode){
      internal.anchor[0] = x;
      internal.anchor[1] = y;
      return;
    }
    const values = rig.values;
    x += Math.round(values[INDEX.bodyLean]);
    y -= Math.round(values[INDEX.bodyLift]);
    if(part.id === "pelvis"){
      x += Math.round(values[INDEX.pelvisX]);
      y += Math.round(values[INDEX.pelvisY]);
    }else if(part.id === "abdomen"){
      x += Math.round(values[INDEX.lowerAngle] * 3);
    }else if(part.id === "ribcage"){
      x += Math.round(values[INDEX.midAngle] * 4);
    }else if(part.id === "neck-lower"){
      x += Math.round(values[INDEX.upperAngle] * 4);
    }else if(part.id === "neck-mid"){
      x += Math.round((values[INDEX.upperAngle] + values[INDEX.neckLower]) * 4);
    }else if(part.id === "neck-upper"){
      x += Math.round((values[INDEX.neckLower] + values[INDEX.neckMid]) * 4);
    }
    const face = part.id === "skull" || part.id === "face-mask" ||
      part.id === "muzzle" || part.id === "jaw" || part.id === "nose" ||
      part.id === "eye-left" || part.id === "eye-right" ||
      part.id.startsWith("lid-") || part.id === "fur-head";
    if(face){
      x += Math.round((values[INDEX.headYaw] + values[INDEX.faceTurn]) * 3);
      y += Math.round(values[INDEX.headPitch] * 2);
    }
    if(part.id === "jaw") y += Math.round(values[INDEX.jawOpen] * 4);
    if(part.id === "muzzle") y -= Math.round(values[INDEX.muzzleLift] * 2);
    if(part.id === "nose") x += Math.round(values[INDEX.noseTwitch] * 2);
    if(part.id === "eye-left" || part.id.startsWith("lid-left")){
      x += Math.round(values[INDEX.eyeLeftX] * 2);
      y += Math.round(values[INDEX.eyeLeftY] * 2);
    }
    if(part.id === "eye-right" || part.id.startsWith("lid-right")){
      x += Math.round(values[INDEX.eyeRightX] * 2);
      y += Math.round(values[INDEX.eyeRightY] * 2);
    }
    if(part.id === "lid-left-upper") y += Math.round(values[INDEX.lidLeftUpper] * 2);
    if(part.id === "lid-left-lower") y -= Math.round(values[INDEX.lidLeftLower] * 2);
    if(part.id === "lid-right-upper") y += Math.round(values[INDEX.lidRightUpper] * 2);
    if(part.id === "lid-right-lower") y -= Math.round(values[INDEX.lidRightLower] * 2);
    internal.anchor[0] = Math.round(x);
    internal.anchor[1] = Math.round(y);
  }

  function anchorForPart(internal, part, rig, staticMode){
    const pose = POSE_RECORDS[part.id];
    if(staticMode || !pose || pose.kind === 0){
      globalAnchor(internal, part, rig, staticMode);
      return;
    }
    const limb = rig.limbs[pose.limb];
    if(pose.kind === 1){
      internal.anchor[0] = Math.round(limb.root.x);
      internal.anchor[1] = Math.round(limb.root.y);
    }else if(pose.kind === 2){
      internal.anchor[0] = Math.round(limb.joint.x);
      internal.anchor[1] = Math.round(limb.joint.y);
    }else if(pose.kind === 3){
      internal.anchor[0] = Math.round(limb.joint.x +
        (limb.end.x - limb.joint.x) * pose.fraction);
      internal.anchor[1] = Math.round(limb.joint.y +
        (limb.end.y - limb.joint.y) * pose.fraction);
    }else if(pose.kind === 4){
      internal.anchor[0] = Math.round(limb.end.x);
      internal.anchor[1] = Math.round(limb.end.y);
    }else{
      const claw = rig.claws[part.id];
      internal.anchor[0] = Math.round(claw.point.x + pose.offsetX);
      internal.anchor[1] = Math.round(claw.point.y + pose.offsetY);
    }
  }

  function supportLoadForPart(part, rig){
    const id = part.id;
    let limb = "";
    if(id.includes("fl") || id.startsWith("claw-front-left")) limb = "front-left";
    else if(id.includes("fr") || id.startsWith("claw-front-right")) limb = "front-right";
    else if(id.includes("rl") || id.startsWith("claw-rear-left")) limb = "rear-left";
    else if(id.includes("rr") || id.startsWith("claw-rear-right")) limb = "rear-right";
    if(limb) return rig.supports[limb].load;
    let load = 0;
    for(let index = 0; index < LIMB_NAMES.length; index += 1){
      const candidate = rig.supports[LIMB_NAMES[index]].load;
      if(candidate > load) load = candidate;
    }
    return load;
  }

  function turnBand(part, rig){
    const id = part.id;
    const values = rig.values;
    if(id === "pelvis") return Math.abs(values[INDEX.pelvisAngle]);
    if(id === "skull" || id === "face-mask" || id === "muzzle" ||
       id.startsWith("lid-")) return Math.max(Math.abs(values[INDEX.headYaw]),
         Math.abs(values[INDEX.faceTurn]));
    if(id.includes("fr")) return id.includes("fore") ?
      Math.abs(values[INDEX.frElbow]) : Math.abs(values[INDEX.frShoulder]);
    if(id.includes("rr")) return id.includes("lower") ?
      Math.abs(values[INDEX.rrElbow]) : Math.abs(values[INDEX.rrShoulder]);
    if(id.includes("fl")) return id.includes("fore") ?
      Math.abs(values[INDEX.flElbow]) : Math.abs(values[INDEX.flShoulder]);
    if(id.includes("rl")) return id.includes("lower") ?
      Math.abs(values[INDEX.rlElbow]) : Math.abs(values[INDEX.rlShoulder]);
    return 0;
  }

  function spriteKey(part, rig, context, staticMode){
    const keys = VARIANT_KEYS[part.id];
    const searching = context.status === "waiting" || context.contextPressure === "high" ||
      context.expanded === true;
    if(keys.searching && searching) return keys.searching;
    if(!staticMode && keys.turned && turnBand(part, rig) >= 0.45) return keys.turned;
    const loadedClaw = !staticMode && part.id.startsWith("claw-") &&
      rig.claws[part.id].mode === "loaded";
    if(keys.loaded && (loadedClaw || context.status === "working" ||
       (!staticMode && supportLoadForPart(part, rig) >= 0.66))) return keys.loaded;
    return keys.base;
  }

  function drawPart(internal, part, rig, context, staticMode){
    const key = spriteKey(part, rig, context, staticMode);
    const rect = internal.page.atlas.rects[key];
    anchorForPart(internal, part, rig, staticMode);
    const x = screenX(internal, internal.anchor[0] - rect.pivotX);
    const y = screenY(internal, internal.anchor[1] - rect.pivotY);
    const scale = internal.camera.scale;
    internal.ctx.drawImage(internal.page.atlas.canvas, rect.x, rect.y,
      rect.width, rect.height, x, y, rect.width * scale, rect.height * scale);
  }

  function drawParts(internal, ids, rig, context, staticMode){
    for(let index = 0; index < ids.length; index += 1){
      drawPart(internal, PART_BY_ID[ids[index]], rig, context, staticMode);
    }
  }

  function drawPixelLine(internal, x0, y0, x1, y1){
    x0 = Math.round(x0);
    y0 = Math.round(y0);
    x1 = Math.round(x1);
    y1 = Math.round(y1);
    const dx = Math.abs(x1 - x0);
    const sx = x0 < x1 ? 1 : -1;
    const dy = -Math.abs(y1 - y0);
    const sy = y0 < y1 ? 1 : -1;
    let error = dx + dy;
    const scale = internal.camera.scale;
    while(true){
      internal.ctx.fillRect(screenX(internal, x0), screenY(internal, y0), scale, scale);
      if(x0 === x1 && y0 === y1) break;
      const doubled = error * 2;
      if(doubled >= dy){ error += dy; x0 += sx; }
      if(doubled <= dx){ error += dx; y0 += sy; }
    }
  }

  function cableValue(rig, staticMode, index){
    return staticMode ? STATIC_CABLE[index] : rig.cable[index];
  }

  function drawCable(internal, rig, staticMode, firstSegment, endSegment){
    internal.ctx.fillStyle = internal.page.atlas.palette[18];
    for(let segment = firstSegment; segment < endSegment; segment += 1){
      const offset = segment * 2;
      drawPixelLine(internal, cableValue(rig, staticMode, offset),
        cableValue(rig, staticMode, offset + 1),
        cableValue(rig, staticMode, offset + 2),
        cableValue(rig, staticMode, offset + 3));
    }
  }

  function drawShadow(internal, rig, staticMode){
    const lean = staticMode ? 0 : Math.round(rig.values[INDEX.bodyLean]);
    const lift = staticMode ? 0 : Math.round(rig.values[INDEX.bodyLift]);
    internal.ctx.fillStyle = internal.page.atlas.palette[0];
    internal.ctx.fillRect(screenX(internal, 68 + lean), screenY(internal, 187 - lift),
      92 * internal.camera.scale, 3 * internal.camera.scale);
  }

  function maskOrigin(internal, maskId, rig, staticMode){
    const mask = Art.MASKS[maskId];
    let x = mask.bounds[0];
    let y = mask.bounds[1];
    if(!staticMode && maskId === "contact-front-left"){
      const point = rig.supports["front-left"].mode === "loaded" ?
        rig.supports["front-left"].point : rig.limbs["front-left"].end;
      x += Math.round(point.x - REST_X["palm-fl"]);
      y += Math.round(point.y - REST_Y["palm-fl"]);
    }else if(!staticMode && maskId === "contact-front-right"){
      const point = rig.supports["front-right"].mode === "loaded" ?
        rig.supports["front-right"].point : rig.limbs["front-right"].end;
      x += Math.round(point.x - REST_X["palm-fr"]);
      y += Math.round(point.y - REST_Y["palm-fr"]);
    }else if(!staticMode){
      x += Math.round(rig.values[INDEX.bodyLean]);
      y -= Math.round(rig.values[INDEX.bodyLift]);
    }
    internal.anchor[0] = x;
    internal.anchor[1] = y;
  }

  function drawMask(internal, maskId, rig, staticMode){
    const mask = Art.MASKS[maskId];
    maskOrigin(internal, maskId, rig, staticMode);
    const originX = internal.anchor[0];
    const originY = internal.anchor[1];
    const scale = internal.camera.scale;
    internal.ctx.fillStyle = internal.page.atlas.palette[0];
    for(let index = 0; index < mask.commands.length; index += 1){
      const command = mask.commands[index];
      if(command[0] === "px"){
        internal.ctx.fillRect(screenX(internal, originX + command[2]),
          screenY(internal, originY + command[3]), scale, scale);
      }else if(command[0] === "run"){
        internal.ctx.fillRect(screenX(internal, originX + command[2]),
          screenY(internal, originY + command[3]), command[4] * scale, scale);
      }else if(command[0] === "rect"){
        internal.ctx.fillRect(screenX(internal, originX + command[2]),
          screenY(internal, originY + command[3]), command[4] * scale,
          command[5] * scale);
      }
    }
  }

  function drawContactMasks(internal, rig, context, staticMode){
    let count = 0;
    let bits = 0;
    if(staticMode || rig.supports["front-left"].mode === "loaded"){
      drawMask(internal, "contact-front-left", rig, staticMode);
      count += 1;
      bits |= 1;
    }
    if(!staticMode && rig.supports["front-right"].mode === "loaded"){
      drawMask(internal, "contact-front-right", rig, false);
      count += 1;
      bits |= 2;
    }
    if(staticMode || rig.supports["rear-left"].mode === "loaded" ||
       rig.supports["rear-right"].mode === "loaded"){
      drawMask(internal, "contact-ground", rig, staticMode);
      count += 1;
      bits |= 4;
    }
    if(context.status === "dead" || context.contextPressure === "high"){
      drawMask(internal, "contact-belly", rig, staticMode);
      count += 1;
      bits |= 8;
    }
    internal.currentMaskBits = bits;
    return count;
  }

  function drawFineDetail(internal, rig, quality, staticMode){
    if(quality === QUALITY.ECONOMY || quality === QUALITY.STATIC) return;
    const camera = internal.camera;
    internal.ctx.drawImage(internal.page.detail, camera.sourceX, camera.sourceY,
      camera.width, camera.height, camera.x, camera.y,
      camera.width * camera.scale, camera.height * camera.scale);
    if(quality !== QUALITY.FULL) return;
    internal.ctx.fillStyle = internal.page.atlas.palette[7];
    const scale = camera.scale;
    for(let index = 0; index < 8; index += 1){
      const channel = staticMode ? 0 : rig.values[INDEX.headRoll] * (index - 3);
      internal.ctx.fillRect(screenX(internal, 88 + index * 6 + Math.round(channel)),
        screenY(internal, 47 + (index & 1) * 5), scale, scale);
    }
    internal.dynamicDitherPixels = 8;
  }

  function drawProp(internal, rig, context, staticMode){
    const prop = Art.PROPS[context.costume];
    if(!prop) return 0;
    const parent = PART_BY_ID[prop.parent];
    anchorForPart(internal, parent, rig, staticMode);
    const parentRestX = parent.bounds[0] + parent.pivot[0];
    const parentRestY = parent.bounds[1] + parent.pivot[1];
    const propAnchorX = prop.bounds[0] + prop.pivot[0] + internal.anchor[0] - parentRestX;
    const propAnchorY = prop.bounds[1] + prop.pivot[1] + internal.anchor[1] - parentRestY;
    const rect = internal.page.atlas.rects[PROP_KEYS[context.costume]];
    const scale = internal.camera.scale;
    internal.ctx.drawImage(internal.page.atlas.canvas, rect.x, rect.y,
      rect.width, rect.height, screenX(internal, propAnchorX - rect.pivotX),
      screenY(internal, propAnchorY - rect.pivotY), rect.width * scale,
      rect.height * scale);
    return 1;
  }

  function drawRim(internal, context){
    let paletteIndex = 15;
    if(context.status === "working") paletteIndex = 12;
    else if(context.status === "waiting") paletteIndex = 13;
    else if(context.status === "dead" || context.status === "done") paletteIndex = 14;
    const scale = internal.camera.scale;
    internal.ctx.fillStyle = internal.page.atlas.palette[paletteIndex];
    internal.ctx.fillRect(screenX(internal, 84), screenY(internal, 51),
      2 * scale, 22 * scale);
    internal.ctx.fillRect(screenX(internal, 87), screenY(internal, 48),
      20 * scale, scale);
  }

  function render(renderer, rig, context, quality){
    const internal = RENDERERS.get(renderer);
    if(!internal || internal.destroyed || !validQuality(quality)) return false;
    try{
      if(!validInputs(rig, context)) return false;
      if(!internal.camera) return false;
      const staticMode = quality === QUALITY.STATIC;
      const contextChanged = visualContextChanged(internal, context);
      const poseChanged = staticMode ? 0 : scanPose(internal, rig, false);
      if(poseChanged < 0) return false;
      const contactChanged = staticMode ? false : contactSnapshotChanged(internal, rig);
      const qualityChanged = internal.quality !== quality;
      if(!internal.dirty && !contextChanged && !poseChanged && !qualityChanged){
        internal.skippedClean += 1;
        return false;
      }

      const ctx = internal.ctx;
      ctx.imageSmoothingEnabled = false;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, internal.backingWidth, internal.backingHeight);
      internal.groupCounts.fill(0);
      internal.dynamicDitherPixels = 0;
      internal.contactMasks = 0;

      drawShadow(internal, rig, staticMode);
      drawCable(internal, rig, staticMode, 0, 4);
      internal.groupCounts[0] = 2;
      drawParts(internal, REAR_IDS, rig, context, staticMode);
      internal.groupCounts[1] = REAR_IDS.length;
      drawPart(internal, PART_BY_ID["fur-back"], rig, context, staticMode);
      internal.groupCounts[2] = 1;
      drawParts(internal, TORSO_IDS, rig, context, staticMode);
      internal.groupCounts[3] = TORSO_IDS.length;
      drawParts(internal, FRONT_IDS, rig, context, staticMode);
      internal.groupCounts[4] = FRONT_IDS.length;
      drawParts(internal, FACE_IDS, rig, context, staticMode);
      internal.groupCounts[5] = FACE_IDS.length;
      drawParts(internal, CLAW_IDS, rig, context, staticMode);
      internal.contactMasks = drawContactMasks(internal, rig, context, staticMode);
      internal.groupCounts[6] = CLAW_IDS.length + internal.contactMasks;
      drawParts(internal, FUR_IDS, rig, context, staticMode);
      drawFineDetail(internal, rig, quality, staticMode);
      internal.groupCounts[7] = FUR_IDS.length +
        (quality === QUALITY.FULL || quality === QUALITY.BALANCED ? 1 : 0);
      internal.groupCounts[8] = drawProp(internal, rig, context, staticMode);
      drawCable(internal, rig, staticMode, 4, 8);
      internal.groupCounts[9] = 1;
      drawRim(internal, context);
      internal.groupCounts[10] = 1;

      if(!staticMode){
        if(scanPose(internal, rig, true) < 0) return false;
        if(poseChanged) internal.poseRevision += 1;
      }
      if(contactChanged || internal.currentMaskBits !== internal.lastMaskBits){
        internal.contactRevision += 1;
      }
      internal.lastMaskBits = internal.currentMaskBits;
      if(contextChanged) saveVisualContext(internal, context);
      internal.quality = quality;
      internal.detailPolicy = quality === QUALITY.FULL ? "all" :
        quality === QUALITY.BALANCED ? "merged-fine" :
        quality === QUALITY.ECONOMY ? "medium-key" : "safe-authored";
      internal.dirty = false;
      internal.renders += 1;
      return true;
    }catch(error){
      return false;
    }
  }

  function markDirty(renderer, reason){
    const internal = RENDERERS.get(renderer);
    if(!internal || internal.destroyed) return false;
    internal.manualRevision += 1;
    internal.dirty = true;
    return true;
  }

  function destroyRenderer(renderer){
    const internal = RENDERERS.get(renderer);
    if(!internal || internal.destroyed) return false;
    internal.destroyed = true;
    try{
      internal.ctx.setTransform(1, 0, 0, 1, 0, 0);
      internal.ctx.clearRect(0, 0, internal.backingWidth, internal.backingHeight);
    }catch(error){
      // Releasing retained pages and buffers remains safe after a hostile canvas teardown.
    }
    internal.pages.dark = null;
    internal.pages.light = null;
    internal.page = null;
    internal.poseSnapshot = null;
    internal.anchor = null;
    internal.groupCounts = null;
    internal.atlasBytes = 0;
    internal.retainedCacheBytes = 0;
    internal.typedArrayBytes = 0;
    return true;
  }

  function rendererDiagnostics(renderer){
    const internal = RENDERERS.get(renderer);
    if(!internal) return null;
    const camera = internal.camera ? {
      x:internal.camera.x, y:internal.camera.y,
      sourceX:internal.camera.sourceX, sourceY:internal.camera.sourceY,
      scale:internal.camera.scale, width:internal.camera.width,
      height:internal.camera.height,
    } : null;
    const groupCounts = internal.groupCounts ? Array.from(internal.groupCounts) : [];
    const decodedBytes = internal.atlasBytes + internal.typedArrayBytes +
      internal.retainedCacheBytes;
    return Object.freeze({
      destroyed:internal.destroyed,
      quality:internal.quality,
      detailPolicy:internal.detailPolicy,
      camera:Object.freeze(camera),
      occlusionGroups:OCCLUSION_GROUPS,
      lastGroupDrawCounts:Object.freeze(groupCounts),
      renders:internal.renders,
      skippedClean:internal.skippedClean,
      atlasBuilds:internal.atlasBuilds,
      atlasBytes:internal.atlasBytes,
      typedArrayBytes:internal.typedArrayBytes,
      retainedCacheBytes:internal.retainedCacheBytes,
      decodedBytes,
      dynamicDitherPixels:internal.dynamicDitherPixels,
      contactMasks:internal.contactMasks,
      cameraRevision:internal.cameraRevision,
      paletteRevision:internal.paletteRevision,
      poseRevision:internal.poseRevision,
      contextRevision:internal.contextRevision,
      contactRevision:internal.contactRevision,
      propRevision:internal.propRevision,
      manualRevision:internal.manualRevision,
      hotLoopAllocations:0,
    });
  }

  NS.Renderer = Object.freeze({QUALITY, createRenderer, setViewport, setTheme,
    render, markDirty, destroyRenderer, rendererDiagnostics});
})(typeof window !== "undefined" ? window : globalThis);
