# PerezOS Living Engine Design

**Date:** 2026-08-25
**Status:** Approved

## Objective

Replace the current axolotl sticker-like mascot with PerezOS: an original,
full-body, hand-authored pixel-art sloth that physically inhabits the selected
session's Control Center. PerezOS must appear biologically alive through
coordinated anatomical motion, contact physics, secondary fur motion, short-term
behavioral memory, and reactions to real ComandOS state. It must create a very
large variety of coherent performances without storing or continuously playing
thousands of complete frame sequences.

The result must remain a quiet part of the product, not a decorative animation
layer floating above it. It must preserve dashboard responsiveness, stop work
when it is not visible, respect reduced-motion preferences, and degrade detail
automatically on slow devices.

## Confirmed Problem

The current mascot in `dash/index.html` is a 64 by 68 indexed pixel map rendered
as a large colored `<pre>`. Its body is one indivisible rectangle. Breathing and
the nine random actions (`boing`, `hop`, `spin`, `tilt`, `shake`, `flip`,
`stretch`, `peek`, and `wiggle`) transform that rectangle as a whole.

This creates four visible failures:

1. There is no anatomical cause and effect. An arm, eye, gill, or torso cannot
   move without transforming the whole sprite.
2. The mascot sits in an isolated 250 by 196 px radial halo, so it reads as an
   inserted sticker instead of part of the Control Center.
3. Random chained transforms provide novelty but not believable behavior.
4. The same code contains an unused procedural canvas axolotl and the active
   ANSI `<pre>` axolotl, leaving two incompatible rendering approaches in the
   main dashboard file.

PerezOS removes both axolotl implementations and replaces them with one bounded
canvas animation subsystem.

## Product Decision

PerezOS is a three-toed sloth called the **ComandOS operator**. It hangs from a
pixel-art execution cable whose anchor is drawn as part of the Control Center.
The cable, body, contact shadows, and panel edge form one composition. Body
parts may pass behind or in front of the panel edge so the character occupies
the same visual space as the software.

The character is deliberately slow. Realism comes from dense internal detail,
continuity, and small causal reactions rather than constant large movements.
The default idle should be interesting when watched for several minutes but
must not compete with session information during ordinary use.

## Scope

### Included

- Replace all axolotl art, naming, preferences copy, click responses, CSS, and
  dead procedural renderer code with PerezOS equivalents.
- Render one full-body mascot for the currently selected session in one visible
  `<canvas>`.
- Add a hand-authored indexed-color sprite atlas with layered body parts,
  silhouette variants, facial features, fur clusters, contact masks, props, and
  cable pieces.
- Add a hierarchical anatomical rig, constraint solver, stylized contact
  physics, behavior director, deterministic random source, pixel renderer, and
  adaptive performance governor.
- React to the existing selected-session signals without adding network calls:
  session identity, status, agent/model role, costume, context pressure, panel
  expansion, theme, pointer proximity, direct activation, page visibility, and
  reduced-motion preference.
- Preserve the existing local show/hide preference. Migrate its storage key
  from the axolotl-specific name without turning a previously hidden mascot
  back on.
- Support desktop and mobile Control Center layouts.
- Add deterministic behavioral, solver, lifecycle, accessibility, visual, and
  performance tests.

### Excluded

- WebGL, third-party animation libraries, a build pipeline, network-loaded art,
  sound, speech, telemetry, or server-side behavior generation.
- A separately simulated mascot for every session. Exactly one PerezOS instance
  represents the selected session and changes context without leaking timers.
- Free roaming over buttons, terminal text, or session rows. PerezOS remains in
  its Control Center stage and never blocks a functional control.
- Unbounded particle systems, per-frame DOM updates, or arbitrary per-pixel
  noise.
- Photorealistic antialiasing. The art remains crisp indexed-color pixel art;
  “hyper-realistic” describes movement and physical coherence.

## Visual Composition

### Stage

The mascot stage replaces the current `.aqua` halo. It is a bounded part of
`.cx-hero`, approximately 256 by 208 CSS pixels on desktop and 180 by 148 CSS
pixels on narrow screens. The exact CSS size may flex with the current layout,
but the renderer always uses an integer pixel scale and letterboxes rather than
distorting the character.

The execution cable enters from the upper Control Center border, sags under the
sloth, and reconnects visually with a short terminal-style node. A low-contrast
contact shadow and limited state-colored rim light connect PerezOS to the
active theme. The panel owns the cable anchor; it must not look like a branch
or an unrelated floating prop.

The canvas has a transparent background. The existing panel background remains
visible through it. The stage may use CSS pseudo-elements for the fixed anchor
only; all deforming cable, body, fur, lighting, and contact shadows are drawn in
the single canvas.

### Pixel Art

- Authoring resolution: 224 by 192 logical pixels.
- Display scaling: nearest-neighbor only, snapped to positive integer scale.
- Base identity: warm brown and cream fur with a recognizable dark three-toed
  sloth face mask.
- Theme response: only rim light, deep shadow bias, cable node, and small props
  inherit theme colors. The body does not recolor wholesale.
- Palette: indexed ramps for outline, deep fur, mid fur, light fur, face mask,
  skin, claws, eyes, highlights, cable, and state light.
- Shading: hand-placed clusters and restrained dithering. Dither patterns may
  switch only through authored masks; they must not shimmer randomly.
- Perspective: a stable three-quarter full-body pose with authored alternate
  silhouettes for foreshortened limbs and head turns. Runtime rotation alone
  must not create diagonal mush or holes between pieces.

## Anatomical Model

PerezOS uses a hierarchical rig of approximately 40 to 50 drawable pieces and
90 to 120 control channels. The exact count is allowed to vary inside those
ranges when authored silhouettes combine adjacent regions, but the following
independent behaviors are required.

### Axial Body

- Skull, jaw, muzzle, and nose volume.
- Three-segment neck with limited twist and extension.
- Upper, middle, and lower spine segments.
- Rib cage expansion, asymmetric breath, abdomen compression, and pelvis tilt.
- Dynamic center of mass derived from the current support contacts.

### Face

- Left and right gaze direction with convergence limits.
- Independent upper and lower eyelids.
- Single and double blinks, partial sleepy blinks, and brief refocus movements.
- Brow-like fur groups, cheek compression, nostril breath, jaw opening, and
  mouth corners.
- Eye and head motion use separate timing so gaze normally anticipates the head.

### Limbs and Contact

- Sliding shoulder blades and independent upper arm, elbow, forearm, and wrist
  controls on both front limbs.
- Pelvis-connected thigh, knee, lower leg, and ankle controls on both rear
  limbs.
- Three independently closing claws on every limb, for twelve claws total.
- Authored open, searching, contacting, loaded, sliding, and released claw
  silhouettes.
- Each supporting limb reports contact position, normal, grip amount, and load.
- Moving a loaded limb first transfers enough weight to another valid support.

### Fur and Soft Tissue

- Deep silhouette fur attached to each anatomical mass.
- Medium fur clusters on head, neck, shoulders, spine, abdomen, elbows, hips,
  knees, and ankles.
- Fine accent tufts that inherit delayed movement from their parent cluster.
- Compression masks at cable/body and limb/body contact points.
- Stretch masks around reaching shoulders, abdomen, and hips.
- Secondary motion uses bounded springs with region-specific stiffness and
  damping. It is driven by body acceleration, never by independent white noise.

## Contact Physics

The simulation is stylized, bounded, and deterministic. It is not a general
rigid-body engine.

- The cable is a short constrained curve with fixed endpoints, gravity, body
  load, tension, and damping.
- The rig solver maintains claw-to-cable contact using two-dimensional inverse
  kinematics and authored joint limits.
- The center of mass produces a limited pendulum response around the current
  support set.
- Head, free limbs, abdomen, and fur receive decreasing delayed inertia.
- A grip may slide a bounded distance before it must recover or settle.
- Body and cable cannot intersect outside authored overlap zones.
- Failed pose requests fall back to the last valid support pose and schedule a
  neutral settle; they never leave a dislocated or floating frame onscreen.
- Every large action follows anticipation, effort, contact, absorption, and
  secondary-settle phases.

## Living Behavior Model

### Drives and Memory

The behavior director maintains bounded values for sleepiness, curiosity,
attention, grip confidence, muscular fatigue, comfort, boredom, satisfaction,
and alertness. Drives change slowly from elapsed visible time and app events;
they are not wall-clock background jobs.

Each session identity maps to a deterministic seed. The seed establishes small
personality biases such as preferred supporting arm, average blink rhythm,
curiosity, and sleep posture. Returning to a session recreates its recognizable
personality. No additional user data is persisted.

Short-term memory records the last behavior family, side used, gaze target,
support set, reaction, and completion time. The director rejects immediate
repetition and strongly reduces recently used variants.

### Behavior Grammar

Complete performances are assembled from reusable primitives rather than
selected from a flat list of full animations. Required primitives include:

- perceive, orient gaze, refocus, blink, turn head, breathe, brace, shift
  weight, reach, search, open grip, release, swing, touch, close grip, pull,
  settle, stretch, scratch, groom, yawn, doze, wake, inspect, point, recoil,
  celebrate, comfort cable, slip, recover, and return to neutral.

Every primitive declares preconditions, occupied body channels, duration range,
interruptibility, completion conditions, and possible successors. A composed
performance may be interrupted only at a declared safe boundary. Urgent
`waiting` and `dead` events preempt low-priority idle actions through a settle
or brace transition rather than snapping directly to a new pose.

Parameter variation includes body side, gaze target, reach distance, grip,
intensity, phase duration, pause placement, head lead, and fur follow-through.
The combination of primitives, parameters, drive state, session personality,
and recent-memory exclusions must yield at least 10,000 reachable valid idle
performances without storing 10,000 frame sequences.

### Time Scales

- 80–250 ms: eye refocus and the smallest facial adjustments.
- 1–4 s: blink, breath phase, nostril movement, and facial reaction.
- 4–15 s: gaze change, head adjustment, grip tension, or small fur settle.
- 15–60 s: support change, posture change, stretch, or brief grooming.
- 1–5 min of visible time: yawn, scratch, larger cable traversal, or deep doze.
- Rare actions use deterministic cooldowns long enough that slip/recovery,
  cable hug, and full-body repositioning cannot dominate a normal session.

Idle always maintains breath and valid contact, but it may hold a comfortable
pose for long periods. “Alive” does not mean “always moving.”

## ComandOS State Mapping

The selected session context is perception input, not a command to play one
fixed clip.

| Input | PerezOS response |
| --- | --- |
| `working` | Becomes attentive, braces, and occasionally advances along the cable while inspecting small command packets. |
| `waiting` | Safely interrupts idle, looks toward the user-facing notice, and extends one free claw without covering controls. |
| `done` | Releases effort, shows a restrained satisfied expression, settles cable oscillation, and returns to comfort. |
| `idle` | Runs the full natural behavior director with low visual urgency. |
| `dead` | Checks the lost signal, curls safely around the cable, and enters deep sleep; no X eyes or violent fall. |
| High context pressure | Briefly inspects a denser packet cluster and appears more careful; it does not become frantic. |
| Role/model change | Inspects and adjusts the small harness badge associated with the role. |
| Theme change | Rebuilds palette-dependent light caches without resetting pose or behavior. |
| Control Center “More” opened | Looks toward the expanding content, then habituates instead of staring indefinitely. |

Existing role hats and costume identifiers remain valid input contracts but are
reinterpreted as restrained authored props attached to the harness, head, or
cable. Props participate in occlusion and secondary motion but never replace
the sloth's body or change its hit target.

## Direct Interaction

- Pointer proximity is sampled at no more than 10 Hz and only inside the mascot
  stage. Eyes may notice the pointer after a variable delay; the head does not
  chase it continuously.
- Direct click, Enter, or Space activation schedules one safe, cooldown-limited
  acknowledgement and shows a localized PerezOS phrase through the existing
  toast system.
- Activation never cancels `waiting`, `dead`, or an unsafe contact phase.
- Pointer capture is not used. The canvas cannot intercept neighboring Control
  Center actions.
- Repeated activation increases habituation, producing smaller reactions until
  the user leaves it alone.

## Frontend Architecture

The engine is a focused no-build JavaScript subsystem loaded by
`dash/index.html`. It exposes only `window.ComandOSPerezOS` to match the
existing classic-script dashboard; internal files do not add general dashboard
globals.

### Public Controller

`createPerezOS(canvas, options)` returns a controller with:

- `setContext(context)` — update selected-session and UI perception.
- `setVisible(visible)` — start or stop all scheduling.
- `setReducedMotion(reduced)` — switch behavior and renderer policy.
- `setViewport(width, height, dpr)` — select integer scale and quality ceiling.
- `notifyInteraction(kind, x, y)` — enqueue bounded pointer or activation input.
- `destroy()` — cancel scheduled work, detach observers, and release caches.
- `getDiagnostics()` — return deterministic counters and timing summaries for
  tests and local debugging without sending data anywhere.

The context contains stable primitive values only: session id, status, role,
costume, context pressure band, theme palette inputs, expanded state, and
timestamp. `setContext` compares fields and emits perception events; it does
not recreate the canvas or rig.

### Internal Units

1. **Art manifest:** indexed palettes, atlas rectangles, pivots, occlusion
   groups, authored silhouette variants, contact masks, and prop attachment
   points.
2. **Rig and solver:** hierarchical transforms, joint limits, support contacts,
   center of mass, inverse kinematics, cable curve, and last-valid-pose fallback.
3. **Motion synthesizer:** primitive phases, channel blending, spring-based
   secondary motion, and safe interrupt boundaries.
4. **Behavior director:** drives, personality seed, short-term memory,
   precondition filtering, cooldowns, and performance composition.
5. **Pixel renderer:** offscreen atlas/cache management, integer-snapped draw
   order, authored mask selection, palette application, dirty tracking, and the
   single visible canvas.
6. **Performance governor:** visible-time scheduler, rolling update/render
   timings, quality policy, page/intersection state, and diagnostics.

Each unit accepts an explicit deterministic clock and random source. Behavior,
motion, and solving can therefore run headlessly in tests without a browser
canvas.

### Data Flow

1. `renderCentro()` keeps the canvas and controller alive while the selected
   session exists and calls `setContext()` when its meaningful signature
   changes.
2. Context changes become perception events in the behavior director.
3. The director selects a valid behavior phrase and issues phase targets to the
   motion synthesizer.
4. The synthesizer blends anatomical channels and advances secondary springs.
5. The rig solver enforces joint, support, center-of-mass, and cable constraints.
6. The renderer redraws only when the solved visual state or palette is dirty.
7. The performance governor records cost and selects the next permitted update
   time.

## Scheduling and Performance

The visible canvas uses `requestAnimationFrame` as the wake-up mechanism but
does not redraw at display refresh rate unless an active gesture needs it.

### Quality Levels

| Level | Behavior/physics | Render ceiling | Detail |
| --- | --- | --- | --- |
| Full | 30 Hz during actions, 12 Hz during quiet idle | 30 fps action, 15 fps idle | All anatomy, facial channels, fur groups, cable, masks, and lighting |
| Balanced | 20 Hz action, 8 Hz idle | 20 fps action, 10 fps idle | Fine fur merged; all contacts, face, and silhouette preserved |
| Economy | 12 Hz action, 4 Hz idle | 12 fps action, 6 fps idle | Medium fur and authored key silhouettes; no dynamic dither changes |
| Static | Event-driven only | One frame per state/contact-safe pose | Breath represented by rare pose changes; no continuous secondary motion |

The governor enters a lower level only after a sustained rolling over-budget
window and upgrades only after a longer stable under-budget window. This
hysteresis prevents visible quality flapping. A quality change preserves
current pose, contacts, behavior phase, and deterministic random sequence.

### Required Budgets

- One visible canvas and zero per-body-part DOM elements.
- No JavaScript allocation in the steady-state physics/render loop after warmup.
- Average combined engine update and render time below 1.0 ms per produced
  frame on the project's desktop browser test machine.
- 95th percentile below 2.0 ms outside first decode, resize, and theme-cache
  rebuild.
- Engine plus decoded atlas memory below 16 MiB.
- Shipped compressed art and engine payload below 750 KiB.
- No engine work while `document.hidden`, while the stage is not intersecting,
  or while the mascot preference is off.
- On resume, advance drives from bounded visible elapsed time; never replay a
  backlog of missed frames or behaviors.
- Device-pixel-ratio is capped by the integer scale needed for the stage. Extra
  backing resolution that cannot reveal another logical pixel is not allocated.

If the Full level cannot meet the timing budget, the system must degrade before
the dashboard develops input latency. Biological correctness, contact safety,
facial readability, and silhouette take priority over fine fur and lighting.

## Responsive Behavior

- Desktop uses the full composition and may show the complete cable sag.
- Narrow layout keeps the same logical character and behavior but chooses a
  hand-authored camera crop no larger than 180 by 148 logical pixels at 1x; it
  does not substitute a different cartoon or fractionally scale the full atlas.
- Resize preserves behavior, pose, support contacts, session personality, and
  short-term memory.
- If the stage is too small for a positive integer scale, the Economy renderer
  uses a hand-authored compact camera composition rather than fractional blur.
- The mascot stage must not increase the Control Center's minimum width or
  create horizontal page scrolling.

## Accessibility

- The interactive stage is keyboard reachable and uses button semantics with a
  localized accessible name such as “PerezOS, mascota de la sesión seleccionada.”
- State changes update a quiet description but microactions are never announced
  to assistive technology.
- `prefers-reduced-motion: reduce` selects Static immediately and prevents
  continuous eye tracking, cable swing, breathing loops, and fur motion.
- The existing show/hide control is renamed “Mascota PerezOS” and retains a
  truthful `aria-checked` value.
- Disabling the mascot destroys scheduling and hides the stage without hiding
  the selected session's name or status.
- PerezOS never communicates state by motion or color alone; existing status
  text remains authoritative.

## Lifecycle and Error Handling

- Art decode begins once and may be shared across controller context changes.
- Before art is ready, the stage reserves its final dimensions and displays no
  broken-image icon or fallback emoji.
- Decode failure produces a compact static pixel fallback and disables further
  retries for the page lifetime. Session controls remain fully usable.
- Invalid art manifest entries fail closed to the last valid body piece and are
  reported only in local diagnostics.
- A solver invariant failure restores the last valid pose, clears unsafe queued
  actions, schedules neutral settle, and increments a diagnostic counter.
- Changing selected sessions resets behavior drives and memory to the new
  deterministic personality but reuses decoded art and render caches.
- Re-rendering the rest of the Control Center must not register duplicate
  pointer, media-query, visibility, resize, or intersection listeners.
- `destroy()` is idempotent and leaves no animation frame, timeout, observer, or
  event listener alive.

## Preference Migration

The existing `cc-axo` value is read once when the new `cc-mascot` preference is
absent. A value of `0` migrates to hidden; any other or missing value migrates
to visible. After migration, new code reads and writes only `cc-mascot`.

No old axolotl label, aria text, phrase, CSS class, palette, pixel map, canvas
builder, animation keyframe, timer, or comment remains in the active dashboard.

## Testing Strategy

### Deterministic Unit Tests

- Identical session seed, context events, clock samples, and input events
  produce identical behavior phrases and solved pose hashes.
- Joint angles, claw contacts, center of mass, cable stretch, and spring values
  remain within declared bounds over simulated time.
- A loaded grip cannot release before another support accepts the required
  weight.
- Recent-memory and cooldown rules prevent immediate behavior-family repeats.
- State priorities interrupt only at safe boundaries. `waiting` begins its
  acknowledgement within five seconds of the event and `dead` reaches its safe
  curled pose within eight seconds, even when either event arrives during a
  large idle action.
- Quality transitions preserve pose and deterministic sequence.

### Long-Run Simulation

Run at least six hours of synthetic visible time for multiple seeds and every
session status. Assert no non-finite values, invalid contacts, stuck occupied
channels, unbounded cable energy, impossible joint angles, or repeated rare
behavior inside its cooldown. Collect behavior-family coverage and confirm at
least 10,000 distinct valid idle performance signatures across the tested seed
set.

### Browser and Visual Tests

- Capture approved deterministic poses for idle, working, waiting, done, dead,
  click acknowledgement, slip recovery, theme change, and narrow layout.
- Compare silhouette, canvas bounds, transparency, palette indices, cable
  anchor, prop attachment, and panel occlusion.
- Verify mouse, keyboard activation, habituation, show/hide preference,
  preference migration, reduced motion, visibility pause, intersection pause,
  resize continuity, and session switching.
- Verify no mascot interaction triggers neighboring session controls.

### Performance Tests

- Warm the atlas and engine before sampling.
- Use the Playwright Chromium desktop harness at a 1400 by 900 CSS-pixel
  viewport. After a 10-second warmup, collect engine diagnostics for 30 seconds
  per scenario; budget assertions use engine update/render durations rather
  than total browser frame time so unrelated dashboard polling cannot mask a
  mascot regression.
- Measure Full idle, Full action, theme change, resize, and selected-session
  change independently.
- Assert the timing, memory, payload, listener, and hidden-work budgets from
  this specification.
- Hold the page hidden during a synthetic interval and assert zero new engine
  updates/renders and no replay burst after visibility resumes.

## Acceptance Criteria

PerezOS is complete when all of the following are true:

1. The current axolotl is fully removed and the selected-session Control Center
   contains one crisp full-body PerezOS canvas physically attached to its cable.
2. Eyes, eyelids, face, head, axial body, four limbs, twelve claws, contact
   loads, cable, soft tissue, and layered fur visibly participate in coherent
   authored behaviors.
3. The engine can construct at least 10,000 distinct valid idle performance
   signatures through composition without shipping equivalent full-frame clips.
4. Working, waiting, done, idle, and dead produce distinct causal behavior while
   preserving existing textual status and controls.
5. Direct interaction, session personality, memory, rare events, props, theme
   response, and responsive camera behavior work without pose snapping.
6. Reduced motion is static, disabling the preference stops all work, hidden or
   non-intersecting stages perform no work, and lifecycle tests find no leaks.
7. The Full implementation meets the stated payload, memory, average timing,
   and 95th-percentile budgets or automatically selects a lower quality level
   before dashboard interaction is affected.
8. Deterministic unit, six-hour simulation, browser, visual, accessibility, and
   performance suites pass.
