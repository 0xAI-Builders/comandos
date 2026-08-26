(function(root){
  "use strict";

  const NS = root.ComandOSPerezOS = root.ComandOSPerezOS || {};
  if(!NS.Core) throw new Error("ComandOSPerezOS.Core must load before Motion");
  if(!NS.Rig) throw new Error("ComandOSPerezOS.Rig must load before Motion");
  if(!NS.Behaviors) throw new Error("ComandOSPerezOS.Behaviors must load before Motion");

  const Core = NS.Core;
  const Rig = NS.Rig;
  const Behaviors = NS.Behaviors;
  const CHANNEL_COUNT = Rig.CHANNELS.length;
  const QUEUE_CAPACITY = 8;
  const COMPLETION_CAPACITY = 16;
  const UNOWNED = -1;
  const TRANSITION_OWNER = -2;
  const MAX_ACCELERATION = 50;
  const MAX_SAFE_TIME = Number.MAX_SAFE_INTEGER;
  const MAX_SCHEDULE_EVENTS = 512;
  const MAX_STEP_DEPTH = 32;
  const MOTIONS = new WeakMap();
  const LIMBS = Object.freeze([
    "front-left", "front-right", "rear-left", "rear-right",
  ]);
  const FRONT_LIMBS = Object.freeze(["front-left", "front-right"]);
  const CONTACT_T = Object.freeze({"front-left":0.34, "front-right":0.72,
    "rear-left":0.34, "rear-right":0.72});

  const BRACE_CHANNELS = Object.freeze([
    "body-lean-x", "body-lift", "cable-tension", "spine-pelvis-angle",
  ]);
  const BRACE_VALUES = Object.freeze([0, 1, 1, 0]);
  const SETTLE_CHANNELS = Object.freeze([
    "body-lean-x", "body-lift", "spine-pelvis-angle", "spine-lower-angle",
    "chest-expand", "cable-tension",
  ]);
  const SETTLE_VALUES = Object.freeze([0, 0, 0, 0, 0.08, 1]);
  const TRANSITION_NAMES = Object.freeze(["brace", "settle"]);
  const TRANSITION_DURATIONS = Object.freeze([160, 160]);
  const TRANSITION_TOTAL_MS = TRANSITION_DURATIONS[0] + TRANSITION_DURATIONS[1];

  const SECONDARY_SPECS = Object.freeze([
    Object.freeze(["fur-head-crest", "head-yaw", 0.004, 58, 15, null]),
    Object.freeze(["fur-head-cheek-left", "head-roll", 0.006, 62, 16, null]),
    Object.freeze(["fur-head-cheek-right", "head-roll", -0.006, 62, 16, null]),
    Object.freeze(["fur-head-nape", "neck-upper-angle", 0.005, 55, 14, null]),
    Object.freeze(["fur-neck-ruff-left", "neck-upper-angle", 0.008, 50, 13, null]),
    Object.freeze(["fur-neck-ruff-right", "neck-upper-angle", -0.008, 50, 13, null]),
    Object.freeze(["fur-back-shoulder", "spine-upper-angle", 0.008, 48, 13, null]),
    Object.freeze(["fur-back-mid", "spine-mid-angle", 0.009, 45, 12, null]),
    Object.freeze(["fur-back-rump", "spine-pelvis-angle", 0.01, 43, 12, null]),
    Object.freeze(["fur-belly-chest", "chest-expand", 0.02, 42, 12, null]),
    Object.freeze(["fur-belly-mid", "belly-compress", 0.02, 40, 11, null]),
    Object.freeze(["fur-belly-flank", "spine-lower-angle", 0.008, 45, 12, null]),
    Object.freeze(["jaw-open", "head-pitch", 0.008, 72, 18, null]),
    Object.freeze(["belly-compress", "chest-expand", -0.015, 46, 13, null]),
    Object.freeze(["front-left-wrist-angle", "front-left-elbow-angle", 0.006,
      68, 17, "front-left"]),
    Object.freeze(["front-right-wrist-angle", "front-right-elbow-angle", 0.006,
      68, 17, "front-right"]),
    Object.freeze(["rear-left-wrist-angle", "rear-left-elbow-angle", 0.006,
      68, 17, "rear-left"]),
    Object.freeze(["rear-right-wrist-angle", "rear-right-elbow-angle", 0.006,
      68, 17, "rear-right"]),
    Object.freeze(["cable-load-scale", "body-lift", 0.015, 52, 14, null]),
  ]);

  function safeTime(value){
    return Math.min(MAX_SAFE_TIME, Math.max(0, value));
  }

  function monotonicTime(internal, value){
    const normalized = safeTime(value);
    if(normalized > internal.lastNowMs) internal.lastNowMs = normalized;
    return internal.lastNowMs;
  }

  function performanceTerminalDeadline(performance, nowMs){
    if(performance.state !== "dead" || performance.deadlineMs === null ||
       performance.deadlineMs === undefined) return MAX_SAFE_TIME;
    return performance.deadlineMs >= MAX_SAFE_TIME - nowMs ?
      MAX_SAFE_TIME : nowMs + performance.deadlineMs;
  }

  function owns(record, key){
    return Object.prototype.hasOwnProperty.call(record, key);
  }

  function targetIsDeclared(metadata, channel){
    for(let index = 0; index < metadata.channels.length; index += 1){
      if(metadata.channels[index] === channel) return true;
    }
    return false;
  }

  function validatePerformance(performance){
    if(!performance || typeof performance !== "object" ||
       typeof performance.family !== "string" || !performance.family ||
       typeof performance.state !== "string" || !performance.state ||
       !Number.isFinite(performance.priority) || !Array.isArray(performance.phases) ||
       performance.phases.length === 0) return false;
    if(performance.deadlineMs !== null && performance.deadlineMs !== undefined &&
       (!Number.isFinite(performance.deadlineMs) || performance.deadlineMs < 0)) return false;
    for(let phaseIndex = 0; phaseIndex < performance.phases.length; phaseIndex += 1){
      const phase = performance.phases[phaseIndex];
      const metadata = phase && Behaviors.PRIMITIVES[phase.primitive];
      if(!phase || !metadata || !phase.targets || typeof phase.targets !== "object" ||
         !Number.isFinite(phase.durationMs) || phase.durationMs <= 0 ||
         !Number.isFinite(phase.safeEnd) || phase.safeEnd < 0 ||
         phase.safeEnd > phase.durationMs || typeof phase.interruptible !== "boolean"){
        return false;
      }
      let targetCount = 0;
      for(const channel in phase.targets){
        if(!owns(phase.targets, channel)) continue;
        const value = phase.targets[channel];
        if(Rig.channelIndex(channel) < 0 || !targetIsDeclared(metadata, channel) ||
           !Number.isFinite(value)) return false;
        targetCount += 1;
      }
      if(targetCount === 0) return false;
    }
    return true;
  }

  function createSecondaryLinks(rig){
    const links = new Array(SECONDARY_SPECS.length);
    for(let index = 0; index < SECONDARY_SPECS.length; index += 1){
      const spec = SECONDARY_SPECS[index];
      const childIndex = Rig.channelIndex(spec[0]);
      const spring = Core.createSpring(rig.targets[childIndex], spec[3], spec[4]);
      Object.seal(spring);
      links[index] = Object.seal({childIndex, parentIndex:Rig.channelIndex(spec[1]),
        follow:spec[2], spring, freeLimb:spec[5]});
    }
    return Object.freeze(links);
  }

  function completionSnapshot(internal){
    const snapshot = new Array(internal.completionSize);
    const start = internal.completionSize === COMPLETION_CAPACITY ?
      internal.completionCursor : 0;
    for(let index = 0; index < internal.completionSize; index += 1){
      snapshot[index] = internal.completionRing[
        (start + index) % COMPLETION_CAPACITY];
    }
    return Object.freeze(snapshot);
  }

  function createMotion(rig){
    if(!rig || Rig.validatePose(rig).length !== 0){
      throw new TypeError("createMotion requires a valid PerezOS Rig");
    }
    const owners = new Int16Array(CHANNEL_COUNT);
    owners.fill(UNOWNED);
    const channelTargets = new Float64Array(CHANNEL_COUNT);
    const phaseStarts = new Float64Array(CHANNEL_COUNT);
    const baseTargets = new Float64Array(CHANNEL_COUNT);
    const previousVelocities = new Float64Array(CHANNEL_COUNT);
    const accelerations = new Float64Array(CHANNEL_COUNT);
    const queueTerminalDeadlines = new Float64Array(QUEUE_CAPACITY);
    channelTargets.set(rig.targets);
    phaseStarts.set(rig.targets);
    baseTargets.set(rig.targets);
    previousVelocities.set(rig.velocities);
    queueTerminalDeadlines.fill(MAX_SAFE_TIME);
    for(const buffer of [owners, channelTargets, phaseStarts, baseTargets,
      previousVelocities, accelerations, queueTerminalDeadlines]){
      Object.preventExtensions(buffer);
    }
    const secondaryLinks = createSecondaryLinks(rig);
    const internal = {
      rig,
      owners,
      channelTargets,
      phaseStarts,
      baseTargets,
      previousVelocities,
      accelerations,
      secondaryLinks,
      current:null,
      phaseIndex:-1,
      phaseElapsedMs:0,
      completedPhases:0,
      startedAtMs:0,
      lastNowMs:0,
      previousDt:1 / 60,
      queue:Object.seal(new Array(QUEUE_CAPACITY).fill(null)),
      queueTerminalDeadlines,
      queueHead:0,
      queueSize:0,
      shiftedTerminalDeadlineMs:MAX_SAFE_TIME,
      pendingInterrupt:null,
      interruptDeadlineMs:MAX_SAFE_TIME,
      pendingTerminalDeadlineMs:MAX_SAFE_TIME,
      currentTerminalDeadlineMs:MAX_SAFE_TIME,
      transitionStage:-1,
      transitionElapsedMs:0,
      completionRing:Object.seal(new Array(COMPLETION_CAPACITY).fill(null)),
      completionCursor:0,
      completionSize:0,
      completionCount:0,
      stepDepth:0,
      transactionDurationMs:0,
      transactionCoveredEndMs:0,
      scheduledThroughMs:0,
      activeCoverageEndMs:0,
      publicMotion:null,
    };
    Object.seal(internal);
    const motion = {};
    Object.defineProperties(motion, {
      rig:{value:rig, enumerable:true},
      owners:{value:owners, enumerable:true},
      channelTargets:{value:channelTargets, enumerable:true},
      phaseStarts:{value:phaseStarts, enumerable:true},
      baseTargets:{value:baseTargets, enumerable:true},
      previousVelocities:{value:previousVelocities, enumerable:true},
      accelerations:{value:accelerations, enumerable:true},
      queueTerminalDeadlines:{value:queueTerminalDeadlines, enumerable:true},
      secondaryLinks:{value:secondaryLinks, enumerable:true},
      onComplete:{value:null, enumerable:true, writable:true},
      current:{enumerable:true, get(){ return internal.current; }},
      phaseIndex:{enumerable:true, get(){ return internal.phaseIndex; }},
      phase:{enumerable:true, get(){
        return internal.current && internal.phaseIndex >= 0 ?
          internal.current.phases[internal.phaseIndex] : null;
      }},
      pendingInterrupt:{enumerable:true, get(){ return internal.pendingInterrupt; }},
      transitionPhase:{enumerable:true, get(){
        return internal.transitionStage < 0 ? null :
          TRANSITION_NAMES[internal.transitionStage];
      }},
      queued:{enumerable:true, get(){ return internal.queueSize; }},
      completions:{enumerable:true, get(){ return completionSnapshot(internal); }},
      completionCount:{enumerable:true, get(){ return internal.completionCount; }},
    });
    Object.seal(motion);
    internal.publicMotion = motion;
    MOTIONS.set(motion, internal);
    return motion;
  }

  function requireMotion(motion){
    return MOTIONS.get(motion) || null;
  }

  function clearOwners(internal){
    for(let index = 0; index < CHANNEL_COUNT; index += 1){
      internal.owners[index] = UNOWNED;
    }
  }

  function enterPhase(internal, phaseIndex){
    clearOwners(internal);
    internal.phaseIndex = phaseIndex;
    internal.phaseElapsedMs = 0;
    const phase = internal.current.phases[phaseIndex];
    for(const channel in phase.targets){
      if(!owns(phase.targets, channel)) continue;
      const index = Rig.channelIndex(channel);
      const limit = Rig.LIMITS[channel];
      internal.owners[index] = phaseIndex;
      internal.phaseStarts[index] = internal.baseTargets[index];
      internal.channelTargets[index] = Core.clamp(phase.targets[channel],
        limit.min, limit.max);
    }
  }

  function setupTransition(internal, stage){
    clearOwners(internal);
    internal.transitionStage = stage;
    internal.transitionElapsedMs = 0;
    const channels = stage === 0 ? BRACE_CHANNELS : SETTLE_CHANNELS;
    const values = stage === 0 ? BRACE_VALUES : SETTLE_VALUES;
    for(let item = 0; item < channels.length; item += 1){
      const index = Rig.channelIndex(channels[item]);
      internal.owners[index] = TRANSITION_OWNER;
      internal.phaseStarts[index] = internal.baseTargets[index];
      internal.channelTargets[index] = values[item];
    }
  }

  function startPerformance(internal, performance, nowMs, withTransition,
      terminalDeadlineMs){
    internal.current = performance;
    internal.startedAtMs = nowMs;
    internal.completedPhases = 0;
    internal.currentTerminalDeadlineMs = terminalDeadlineMs === undefined ?
      performanceTerminalDeadline(performance, nowMs) : terminalDeadlineMs;
    if(withTransition){
      internal.phaseIndex = -1;
      setupTransition(internal, 0);
    }else{
      internal.transitionStage = -1;
      enterPhase(internal, 0);
    }
  }

  function queuePush(internal, performance, terminalDeadlineMs){
    if(internal.queueSize >= QUEUE_CAPACITY) return false;
    const index = (internal.queueHead + internal.queueSize) % QUEUE_CAPACITY;
    internal.queue[index] = performance;
    internal.queueTerminalDeadlines[index] = terminalDeadlineMs;
    internal.queueSize += 1;
    return true;
  }

  function queueShift(internal){
    if(internal.queueSize === 0) return null;
    const performance = internal.queue[internal.queueHead];
    internal.shiftedTerminalDeadlineMs =
      internal.queueTerminalDeadlines[internal.queueHead];
    internal.queue[internal.queueHead] = null;
    internal.queueTerminalDeadlines[internal.queueHead] = MAX_SAFE_TIME;
    internal.queueHead = (internal.queueHead + 1) % QUEUE_CAPACITY;
    internal.queueSize -= 1;
    return performance;
  }

  function queueRemove(internal, offset){
    const selected = (internal.queueHead + offset) % QUEUE_CAPACITY;
    const performance = internal.queue[selected];
    internal.shiftedTerminalDeadlineMs =
      internal.queueTerminalDeadlines[selected];
    for(let position = offset; position + 1 < internal.queueSize; position += 1){
      const target = (internal.queueHead + position) % QUEUE_CAPACITY;
      const source = (target + 1) % QUEUE_CAPACITY;
      internal.queue[target] = internal.queue[source];
      internal.queueTerminalDeadlines[target] =
        internal.queueTerminalDeadlines[source];
    }
    const tail = (internal.queueHead + internal.queueSize - 1) % QUEUE_CAPACITY;
    internal.queue[tail] = null;
    internal.queueTerminalDeadlines[tail] = MAX_SAFE_TIME;
    internal.queueSize -= 1;
    return performance;
  }

  function interruptCutDeadline(performance, nowMs, terminalDeadlineMs){
    if(performance.deadlineMs === null || performance.deadlineMs === undefined){
      return MAX_SAFE_TIME;
    }
    if(performance.state === "dead" && terminalDeadlineMs < MAX_SAFE_TIME){
      let poseTimeMs = TRANSITION_TOTAL_MS;
      for(let index = 0; index < performance.phases.length; index += 1){
        poseTimeMs += performance.phases[index].durationMs;
      }
      return Math.max(0, terminalDeadlineMs - poseTimeMs);
    }
    return performance.deadlineMs >= MAX_SAFE_TIME - nowMs ?
      MAX_SAFE_TIME : nowMs + performance.deadlineMs;
  }

  function promoteQueuedDead(internal, nowMs){
    if(!internal.current || internal.queueSize === 0) return;
    const threshold = internal.pendingInterrupt ? internal.pendingInterrupt.priority :
      internal.current.priority;
    let selectedOffset = -1;
    let selectedDeadline = MAX_SAFE_TIME;
    for(let offset = 0; offset < internal.queueSize; offset += 1){
      const index = (internal.queueHead + offset) % QUEUE_CAPACITY;
      const performance = internal.queue[index];
      const deadline = internal.queueTerminalDeadlines[index];
      if(performance && performance.state === "dead" &&
         performance.priority > internal.current.priority &&
         performance.priority > threshold && deadline < selectedDeadline){
        selectedOffset = offset;
        selectedDeadline = deadline;
      }
    }
    if(selectedOffset < 0) return;
    const performance = queueRemove(internal, selectedOffset);
    internal.pendingInterrupt = performance;
    internal.pendingTerminalDeadlineMs = selectedDeadline;
    internal.interruptDeadlineMs = interruptCutDeadline(performance, nowMs,
      selectedDeadline);
  }

  function enqueue(motion, performance, nowMs){
    const internal = requireMotion(motion);
    if(!internal || !validatePerformance(performance) || !Number.isFinite(nowMs) || nowMs < 0){
      return false;
    }
    const now = monotonicTime(internal, nowMs);
    const terminalDeadlineMs = performanceTerminalDeadline(performance, now);
    if(!internal.current && internal.transitionStage < 0){
      startPerformance(internal, performance, now, false, terminalDeadlineMs);
      return true;
    }
    return queuePush(internal, performance, terminalDeadlineMs);
  }

  function requestInterrupt(motion, performance, nowMs){
    const internal = requireMotion(motion);
    if(!internal || !validatePerformance(performance) || !Number.isFinite(nowMs) || nowMs < 0){
      return false;
    }
    const now = monotonicTime(internal, nowMs);
    const terminalDeadlineMs = performanceTerminalDeadline(performance, now);
    if(!internal.current){
      startPerformance(internal, performance, now, false, terminalDeadlineMs);
      return true;
    }
    const threshold = internal.pendingInterrupt ? internal.pendingInterrupt.priority :
      internal.current.priority;
    if(performance.priority <= internal.current.priority || performance.priority <= threshold){
      return false;
    }
    internal.pendingInterrupt = performance;
    internal.pendingTerminalDeadlineMs = terminalDeadlineMs;
    internal.interruptDeadlineMs = interruptCutDeadline(performance, now,
      terminalDeadlineMs);
    return true;
  }

  function phaseNeedsBridge(phase){
    if(!phase) return false;
    if(phase.primitive === "open-grip" || phase.primitive === "release" ||
       phase.primitive === "close-grip") return true;
    for(const channel in phase.targets){
      if(!owns(phase.targets, channel)) continue;
      if(channel.startsWith("front-") || channel.startsWith("rear-") ||
         channel.startsWith("body-") || channel.startsWith("spine-") ||
         channel.startsWith("cable-")) return true;
    }
    return false;
  }

  function makeCompletionRecord(internal, status, nowMs, interruptedBy){
    return Object.freeze({
      status,
      state:internal.current.state,
      family:internal.current.family,
      startedAtMs:internal.startedAtMs,
      endedAtMs:nowMs,
      phaseCount:internal.current.phases.length,
      completedPhases:internal.completedPhases,
      interruptedBy:interruptedBy || null,
    });
  }

  function recordCompletion(internal, status, nowMs, interruptedBy){
    const record = makeCompletionRecord(internal, status, nowMs, interruptedBy);
    internal.completionRing[internal.completionCursor] = record;
    internal.completionCursor = (internal.completionCursor + 1) % COMPLETION_CAPACITY;
    if(internal.completionSize < COMPLETION_CAPACITY) internal.completionSize += 1;
    internal.completionCount += 1;
    if(nowMs > internal.lastNowMs) internal.lastNowMs = nowMs;
    if(nowMs > internal.scheduledThroughMs){
      internal.scheduledThroughMs = nowMs;
    }
    return record;
  }

  function dispatchCompletion(internal, record){
    const callback = internal.publicMotion.onComplete;
    if(typeof callback === "function"){
      try{ callback(record); }catch(error){ /* user callbacks cannot corrupt motion */ }
    }
  }

  function beginNext(internal, nowMs){
    if(internal.pendingInterrupt){
      const pending = internal.pendingInterrupt;
      const terminalDeadlineMs = internal.pendingTerminalDeadlineMs;
      internal.pendingInterrupt = null;
      internal.interruptDeadlineMs = MAX_SAFE_TIME;
      internal.pendingTerminalDeadlineMs = MAX_SAFE_TIME;
      startPerformance(internal, pending, nowMs, false, terminalDeadlineMs);
      return;
    }
    const queued = queueShift(internal);
    if(queued){
      startPerformance(internal, queued, nowMs, false,
        internal.shiftedTerminalDeadlineMs);
      return;
    }
    internal.current = null;
    internal.phaseIndex = -1;
    internal.phaseElapsedMs = 0;
    internal.transitionStage = -1;
    internal.currentTerminalDeadlineMs = MAX_SAFE_TIME;
    clearOwners(internal);
  }

  function finishCurrent(internal, nowMs){
    const record = recordCompletion(internal, "completed", nowMs, null);
    beginNext(internal, nowMs);
    dispatchCompletion(internal, record);
  }

  function cutToInterrupt(internal, nowMs){
    const pending = internal.pendingInterrupt;
    const terminalDeadlineMs = internal.pendingTerminalDeadlineMs;
    const oldPhase = internal.phaseIndex >= 0 ?
      internal.current.phases[internal.phaseIndex] : null;
    const bridge = phaseNeedsBridge(oldPhase);
    internal.pendingInterrupt = null;
    internal.interruptDeadlineMs = MAX_SAFE_TIME;
    internal.pendingTerminalDeadlineMs = MAX_SAFE_TIME;
    const record = recordCompletion(internal, "interrupted", nowMs, pending.family);
    startPerformance(internal, pending, nowMs, bridge, terminalDeadlineMs);
    dispatchCompletion(internal, record);
  }

  function transferCandidate(internal, releasingLimb){
    let best = null;
    let bestLoad = -1;
    for(let index = 0; index < LIMBS.length; index += 1){
      const name = LIMBS[index];
      if(name === releasingLimb) continue;
      const support = internal.rig.supports[name];
      if(support.mode === "loaded" && support.load > bestLoad){
        best = support;
        bestLoad = support.load;
      }
    }
    return best;
  }

  function processContact(internal, phase){
    const primitive = phase.primitive;
    if(primitive !== "close-grip" && primitive !== "open-grip" &&
       primitive !== "release") return true;
    const limb = FRONT_LIMBS[internal.current.side === "right" ? 1 : 0];
    const support = internal.rig.supports[limb];
    if(primitive === "close-grip"){
      if(support.mode === "loaded") return true;
      return Rig.requestGrip(internal.rig, limb, "loaded", CONTACT_T[limb]);
    }
    if(support.mode !== "loaded") return true;

    const candidate = transferCandidate(internal, limb);
    if(candidate && candidate.load >= 0.95){
      return Rig.requestGrip(internal.rig, limb, "release", support.cableT);
    }
    if(candidate){
      const candidateIndex = LIMBS.indexOf(candidate.limb);
      if(internal.rig.transferGoal !== candidateIndex){
        Rig.requestGrip(internal.rig, candidate.limb, "loaded", candidate.cableT);
      }
      return false;
    }

    for(let index = 0; index < LIMBS.length; index += 1){
      const name = LIMBS[index];
      if(name === limb || internal.rig.supports[name].mode === "loaded") continue;
      if(Rig.requestGrip(internal.rig, name, "loaded", CONTACT_T[name])) return false;
    }
    return false;
  }

  function applyActiveTargets(internal){
    const phase = internal.current.phases[internal.phaseIndex];
    const progress = Core.smoothstep(internal.phaseElapsedMs / phase.durationMs);
    for(let index = 0; index < CHANNEL_COUNT; index += 1){
      if(internal.owners[index] !== internal.phaseIndex) continue;
      const target = Core.lerp(internal.phaseStarts[index],
        internal.channelTargets[index], progress);
      internal.baseTargets[index] = target;
      internal.rig.targets[index] = target;
    }
  }

  function applyTransitionTargets(internal){
    const duration = TRANSITION_DURATIONS[internal.transitionStage];
    const progress = Core.smoothstep(internal.transitionElapsedMs / duration);
    for(let index = 0; index < CHANNEL_COUNT; index += 1){
      if(internal.owners[index] !== TRANSITION_OWNER) continue;
      const target = Core.lerp(internal.phaseStarts[index],
        internal.channelTargets[index], progress);
      internal.baseTargets[index] = target;
      internal.rig.targets[index] = target;
    }
  }

  function computeAccelerations(internal, dt){
    if(dt <= 0){
      for(let index = 0; index < CHANNEL_COUNT; index += 1){
        internal.accelerations[index] = 0;
      }
      return;
    }
    const inverseDt = 1 / internal.previousDt;
    for(let index = 0; index < CHANNEL_COUNT; index += 1){
      const acceleration = (internal.rig.velocities[index] -
        internal.previousVelocities[index]) * inverseDt;
      internal.accelerations[index] = Core.clamp(acceleration,
        -MAX_ACCELERATION, MAX_ACCELERATION);
    }
  }

  function updateSecondary(internal, dt){
    for(let index = 0; index < internal.secondaryLinks.length; index += 1){
      const link = internal.secondaryLinks[index];
      let follow = link.follow;
      if(link.freeLimb && internal.rig.supports[link.freeLimb].mode === "loaded") follow = 0;
      const channel = Rig.CHANNELS[link.childIndex];
      const limit = Rig.LIMITS[channel];
      const desired = Core.clamp(internal.baseTargets[link.childIndex] +
        internal.accelerations[link.parentIndex] * follow, limit.min, limit.max);
      let target = Core.stepSpring(link.spring, desired, dt);
      if(target < limit.min || target > limit.max){
        target = Core.clamp(target, limit.min, limit.max);
        link.spring.value = target;
        link.spring.velocity = 0;
      }
      internal.rig.targets[link.childIndex] = target;
    }
  }

  function copyVelocities(internal, dt){
    for(let index = 0; index < CHANNEL_COUNT; index += 1){
      internal.previousVelocities[index] = internal.rig.velocities[index];
    }
    if(dt > 0) internal.previousDt = dt;
  }

  function resyncAfterRigRecovery(internal){
    const phaseElapsedMs = internal.phaseElapsedMs;
    const transitionElapsedMs = internal.transitionElapsedMs;
    for(let index = 0; index < CHANNEL_COUNT; index += 1){
      const target = internal.rig.targets[index];
      internal.baseTargets[index] = target;
      internal.phaseStarts[index] = target;
      internal.channelTargets[index] = target;
      internal.previousVelocities[index] = internal.rig.velocities[index];
      internal.accelerations[index] = 0;
    }
    for(let index = 0; index < internal.secondaryLinks.length; index += 1){
      const link = internal.secondaryLinks[index];
      link.spring.value = internal.rig.targets[link.childIndex];
      link.spring.velocity = 0;
    }
    internal.previousDt = internal.rig.lastDt > 0 ? internal.rig.lastDt : 1 / 60;
    if(internal.transitionStage >= 0){
      setupTransition(internal, internal.transitionStage);
      internal.transitionElapsedMs = transitionElapsedMs;
    }else if(internal.current && internal.phaseIndex >= 0){
      enterPhase(internal, internal.phaseIndex);
      internal.phaseElapsedMs = phaseElapsedMs;
    }else{
      clearOwners(internal);
    }
  }

  function advanceTransitionSegment(internal, dtMs){
    internal.transitionElapsedMs += dtMs;
    const duration = TRANSITION_DURATIONS[internal.transitionStage];
    if(internal.transitionElapsedMs > duration) internal.transitionElapsedMs = duration;
    applyTransitionTargets(internal);
  }

  function completeTransitionStage(internal){
    if(internal.transitionStage === 0){
      setupTransition(internal, 1);
      return;
    }
    internal.transitionStage = -1;
    enterPhase(internal, 0);
  }

  function advanceActiveSegment(internal, dtMs){
    const phase = internal.current.phases[internal.phaseIndex];
    internal.phaseElapsedMs += dtMs;
    if(internal.phaseElapsedMs > phase.durationMs){
      internal.phaseElapsedMs = phase.durationMs;
    }
    applyActiveTargets(internal);
  }

  function completeActivePhase(internal, nowMs){
    internal.completedPhases += 1;
    if(internal.phaseIndex + 1 < internal.current.phases.length){
      enterPhase(internal, internal.phaseIndex + 1);
      return;
    }
    finishCurrent(internal, nowMs);
  }

  function resolvePendingBoundary(internal, nowMs){
    const atPhaseEnd = internal.phaseElapsedMs >=
      internal.current.phases[internal.phaseIndex].durationMs;
    if(atPhaseEnd && internal.phaseIndex + 1 >= internal.current.phases.length){
      completeActivePhase(internal, nowMs);
      return;
    }
    if(atPhaseEnd) internal.completedPhases += 1;
    cutToInterrupt(internal, nowMs);
  }

  function completeTerminalFallback(internal, nowMs){
    clearOwners(internal);
    for(let index = 0; index < CHANNEL_COUNT; index += 1){
      let target = internal.rig.targets[index];
      if(!Number.isFinite(target)) target = internal.rig.lastValidTargets[index];
      internal.rig.targets[index] = target;
      internal.baseTargets[index] = target;
      internal.phaseStarts[index] = target;
      internal.channelTargets[index] = target;
    }
    for(let item = 0; item < SETTLE_CHANNELS.length; item += 1){
      const channel = SETTLE_CHANNELS[item];
      const index = Rig.channelIndex(channel);
      const limit = Rig.LIMITS[channel];
      const target = Core.clamp(SETTLE_VALUES[item], limit.min, limit.max);
      internal.rig.targets[index] = target;
      internal.baseTargets[index] = target;
      internal.channelTargets[index] = target;
    }
    for(let index = 0; index < internal.secondaryLinks.length; index += 1){
      const link = internal.secondaryLinks[index];
      link.spring.value = internal.rig.targets[link.childIndex];
      link.spring.velocity = 0;
    }
    internal.completedPhases = internal.current.phases.length;
    internal.currentTerminalDeadlineMs = MAX_SAFE_TIME;
    finishCurrent(internal, nowMs);
  }

  function advanceSchedule(internal, dtMs, nowMs){
    let remainingMs = dtMs;
    let cursorMs = nowMs;

    for(let eventCount = 0; eventCount < MAX_SCHEDULE_EVENTS; eventCount += 1){
      if(internal.lastNowMs > cursorMs){
        const reentrantMs = internal.lastNowMs - cursorMs;
        if(reentrantMs >= remainingMs) return;
        cursorMs += reentrantMs;
        remainingMs -= reentrantMs;
      }
      promoteQueuedDead(internal, cursorMs);
      if(!internal.current) return;
      if(internal.currentTerminalDeadlineMs <= cursorMs){
        completeTerminalFallback(internal, cursorMs);
        if(remainingMs <= 0) return;
        continue;
      }

      if(internal.transitionStage >= 0){
        const stageDuration = TRANSITION_DURATIONS[internal.transitionStage];
        let segmentMs = stageDuration - internal.transitionElapsedMs;
        let eventKind = 0;
        if(internal.pendingInterrupt){
          const deadlineMs = Math.max(0, internal.interruptDeadlineMs - cursorMs);
          if(deadlineMs <= segmentMs && deadlineMs <= remainingMs){
            segmentMs = deadlineMs;
            eventKind = 1;
          }
        }
        const terminalMs = Math.max(0,
          internal.currentTerminalDeadlineMs - cursorMs);
        if(terminalMs <= segmentMs && terminalMs <= remainingMs){
          segmentMs = terminalMs;
          eventKind = 2;
        }
        if(segmentMs > remainingMs) segmentMs = remainingMs;
        if(segmentMs > 0){
          advanceTransitionSegment(internal, segmentMs);
          cursorMs += segmentMs;
          remainingMs -= segmentMs;
        }
        if(eventKind === 2){
          completeTerminalFallback(internal, cursorMs);
          if(remainingMs <= 0) return;
          continue;
        }
        if(eventKind === 1){
          cutToInterrupt(internal, cursorMs);
          if(remainingMs <= 0) return;
          continue;
        }
        if(internal.transitionElapsedMs >= stageDuration){
          completeTransitionStage(internal);
          if(remainingMs <= 0) return;
          continue;
        }
        return;
      }

      const phase = internal.current.phases[internal.phaseIndex];
      if(internal.pendingInterrupt &&
         (internal.phaseElapsedMs >= phase.safeEnd ||
          cursorMs >= internal.interruptDeadlineMs)){
        resolvePendingBoundary(internal, cursorMs);
        if(remainingMs <= 0) return;
        continue;
      }

      if(!processContact(internal, phase)){
        let eventMs = remainingMs + 1;
        let eventKind = 0;
        if(internal.pendingInterrupt){
          let deadlineMs = internal.interruptDeadlineMs - cursorMs;
          if(deadlineMs < 0) deadlineMs = 0;
          if(deadlineMs <= remainingMs){
            eventMs = deadlineMs;
            eventKind = 1;
          }
        }
        let terminalMs = internal.currentTerminalDeadlineMs - cursorMs;
        if(terminalMs < 0) terminalMs = 0;
        if(terminalMs <= eventMs && terminalMs <= remainingMs){
          eventMs = terminalMs;
          eventKind = 2;
        }
        if(eventKind === 0) return;
        cursorMs += eventMs;
        remainingMs -= eventMs;
        if(eventKind === 2){
          completeTerminalFallback(internal, cursorMs);
        }else{
          cutToInterrupt(internal, cursorMs);
        }
        if(remainingMs <= 0) return;
        continue;
      }

      if(internal.pendingInterrupt){
        let safeMs = phase.safeEnd - internal.phaseElapsedMs;
        let deadlineMs = internal.interruptDeadlineMs - cursorMs;
        if(safeMs < 0) safeMs = 0;
        if(deadlineMs < 0) deadlineMs = 0;
        let segmentMs = safeMs < deadlineMs ? safeMs : deadlineMs;
        let terminalEvent = false;
        let terminalMs = internal.currentTerminalDeadlineMs - cursorMs;
        if(terminalMs < 0) terminalMs = 0;
        if(terminalMs <= segmentMs){
          segmentMs = terminalMs;
          terminalEvent = true;
        }
        if(segmentMs <= remainingMs){
          if(segmentMs > 0){
            advanceActiveSegment(internal, segmentMs);
            cursorMs += segmentMs;
            remainingMs -= segmentMs;
          }
          if(terminalEvent){
            completeTerminalFallback(internal, cursorMs);
          }else{
            resolvePendingBoundary(internal, cursorMs);
          }
          if(remainingMs <= 0) return;
          continue;
        }
      }

      let segmentMs = phase.durationMs - internal.phaseElapsedMs;
      let terminalEvent = false;
      const terminalMs = internal.currentTerminalDeadlineMs - cursorMs;
      if(terminalMs < segmentMs){
        segmentMs = terminalMs;
        terminalEvent = true;
      }
      if(segmentMs > remainingMs) segmentMs = remainingMs;
      if(segmentMs > 0){
        advanceActiveSegment(internal, segmentMs);
        cursorMs += segmentMs;
        remainingMs -= segmentMs;
      }
      if(terminalEvent && cursorMs >= internal.currentTerminalDeadlineMs){
        completeTerminalFallback(internal, cursorMs);
        if(remainingMs <= 0) return;
        continue;
      }
      if(internal.phaseElapsedMs >= phase.durationMs){
        completeActivePhase(internal, cursorMs);
        if(remainingMs <= 0) return;
        continue;
      }
      return;
    }
    const frameEndMs = safeTime(nowMs + dtMs);
    if(internal.current && internal.currentTerminalDeadlineMs <= frameEndMs){
      completeTerminalFallback(internal, internal.currentTerminalDeadlineMs);
    }
  }

  function includeStepInterval(internal, startMs, endMs){
    let addedMs = 0;
    if(startMs >= internal.transactionCoveredEndMs){
      addedMs = endMs - startMs;
    }else if(endMs > internal.transactionCoveredEndMs){
      addedMs = endMs - internal.transactionCoveredEndMs;
    }
    if(addedMs > 0){
      internal.transactionDurationMs = safeTime(
        internal.transactionDurationMs + addedMs);
    }
    if(endMs > internal.transactionCoveredEndMs){
      internal.transactionCoveredEndMs = endMs;
    }
  }

  function runScheduleWindow(internal, startMs, endMs){
    const previousCoverageEndMs = internal.activeCoverageEndMs;
    if(startMs > internal.scheduledThroughMs){
      internal.scheduledThroughMs = startMs;
    }
    internal.activeCoverageEndMs = Math.max(previousCoverageEndMs, endMs);
    advanceSchedule(internal, endMs - startMs, startMs);
    if(endMs > internal.scheduledThroughMs){
      internal.scheduledThroughMs = endMs;
    }
    if(endMs > internal.lastNowMs) internal.lastNowMs = endMs;
    internal.activeCoverageEndMs = previousCoverageEndMs;
  }

  function stepMotion(motion, dt, nowMs){
    const internal = requireMotion(motion);
    if(!internal || !Number.isFinite(dt) || dt < 0 ||
       !Number.isFinite(nowMs) || nowMs < 0) return false;
    if(internal.stepDepth >= MAX_STEP_DEPTH) return false;
    const outermost = internal.stepDepth === 0;
    const now = outermost ? monotonicTime(internal, nowMs) :
      Math.max(internal.lastNowMs, safeTime(nowMs));
    const dtMs = Math.min(MAX_SAFE_TIME, dt * 1000);
    const frameEndMs = safeTime(now + dtMs);
    if(outermost){
      internal.transactionDurationMs = 0;
      internal.transactionCoveredEndMs = now;
    }
    includeStepInterval(internal, now, frameEndMs);
    internal.stepDepth += 1;
    if(outermost){
      runScheduleWindow(internal, now, frameEndMs);
    }else{
      let cursorMs = internal.scheduledThroughMs;
      const enclosingEndMs = internal.activeCoverageEndMs;
      if(now <= enclosingEndMs){
        if(frameEndMs >= cursorMs){
          runScheduleWindow(internal, cursorMs, frameEndMs);
        }
      }else{
        if(cursorMs < enclosingEndMs){
          runScheduleWindow(internal, cursorMs, enclosingEndMs);
        }
        cursorMs = Math.max(now, internal.scheduledThroughMs);
        if(cursorMs <= frameEndMs){
          runScheduleWindow(internal, cursorMs, frameEndMs);
        }
      }
    }
    internal.stepDepth -= 1;
    if(!outermost) return true;
    const effectiveDt = internal.transactionDurationMs / 1000;
    computeAccelerations(internal, effectiveDt);
    updateSecondary(internal, effectiveDt);
    copyVelocities(internal, effectiveDt);
    const solved = Rig.solveRig(internal.rig, effectiveDt);
    if(!solved){
      resyncAfterRigRecovery(internal);
      return false;
    }
    return true;
  }

  function isIdle(motion){
    const internal = requireMotion(motion);
    return !!internal && !internal.current && internal.queueSize === 0 &&
      !internal.pendingInterrupt && internal.transitionStage < 0;
  }

  NS.Motion = Object.freeze({createMotion, enqueue, requestInterrupt, stepMotion, isIdle});
})(typeof window !== "undefined" ? window : globalThis);
