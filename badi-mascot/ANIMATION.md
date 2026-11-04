# BADI — Animation & Integration Guide

BADI is a native Rive project written in RML and built with the Rive CLI.
The source of truth is the `.rml` files; `dist/BADI_Mascot.riv` is the runtime
file built from them.

## Deliverables

| Path | What |
|---|---|
| `dist/BADI_Mascot.riv` | Runtime file (also written to `build/` on every build) |
| `artboards/expanded.rml` | **Expanded** artboard, 360×360: main window / interaction |
| `artboards/compact.rml` | **Compact** artboard, 128×128: notch / tray / small UI |
| `components/*.rml` | Reusable components: Body, Eyes, Brows, Mouth, Arm, Leg, Effects, Character |
| `data/badi_data.rml` | Enums (`Expression`, `Mode`, `Interaction`), view models `BADI` and `Pose` |
| `artboards/expression_sheet.rml` | Review sheet: all 11 expressions |
| `artboards/component_sheet.rml` | Review sheet: every component on its own |
| `tools/gen_expressions.py` | Generator for the expression system, hosts and data |
| `tools/gen_body_jelly.py` | Generator for the body's jelly deformation |

Build and check:

```bash
rive . --verify                     # compile + reload check
rive inspect . --summary            # problems + object counts
rive .                              # live previewer
rive . --artboard=Expanded --screenshot --data=state=thinking --advance=40
```

## Runtime API: `BADIController`

Both host artboards play the `BADIController` state machine and are bound to the
**`BADI`** view model. The app drives BADI by setting these properties on the bound
instance. There are no legacy state machine inputs.

| Property | Type | Default | Meaning |
|---|---|---|---|
| `mode` | enum `compact` \| `expanded` | `expanded` | Detail level. Expanded: arms appear for gestures (Guiding points, Success cheers). Compact: brows hidden at Idle. The Compact artboard is always compact. |
| `state` | enum `Expression` | `idle` | Base expression (see the table below). |
| `isListening` | bool | false | Forces Listening. |
| `isThinking` | bool | false | Forces Thinking. |
| `isSpeaking` | bool | false | Forces Speaking. |
| `audioLevel` | number 0–1 | 0 | Mouth opening while speaking. 0 = closed; if it stays 0, BADI flaps its mouth automatically. |
| `interaction` | enum `none` \| `hover` \| `pressed` | `none` | Written by the built-in pointer listeners (enter/exit/down/up over BADI). The app can also set it. |
| `isAnnoyed` | bool | false | Forces Annoyed. |
| `isDizzy` | bool | false | Forces Dizzy. |

**Priority** (Resolver layer): `isDizzy` > `isAnnoyed` > `isSpeaking` > `isListening` > `isThinking` > `state`.
A typical voice loop is: set `isListening` while the mic is open, then `isThinking`,
then `isSpeaking` with `audioLevel` fed from TTS output. Clearing every flag falls
back to `state`.

Enum values (`Expression`): `idle, listening, thinking, speaking, success, error, alert, guiding, sleeping, annoyed, dizzy`.

## The 11 expressions

| # | Expression | Eyes | Brows | Mouth | Effect | Body motion (loop) | On entry |
|---|---|---|---|---|---|---|---|
| 1 | Idle | open, blinking | neutral | smile | – | slow breathing (3 s) | recovery from Dizzy/Annoyed |
| 2 | Listening | happy ◠ ◠ | hidden | small smile | cyan sound waves, pulsing | lean toward sound, bob | perk-up hop |
| 3 | Thinking | L open, R small and raised | R raised | side smirk | – | slow sway and float (4 s) | – |
| 4 | Speaking | open, blinking | raised | open "D" + tongue, lipsync | cyan ticks | quick talking bob | – |
| 5 | Success | happy ◠ ◠ | L tick | big laugh | gold sparkle | hop: anticipation → stretch → squash landing | arms cheer (expanded) |
| 6 | Error | open, lowered | worried | frown | – | "no" shake, droop | – |
| 7 | Alert | wide | high | "o" | gold burst | nervous tremble | jolt |
| 8 | Guiding | L open, R wink | hidden | smile | cyan dashes | lean and nod | arm points (expanded) |
| 9 | Sleeping | closed ◡ ◡ | hidden | tiny smile | rising Z z z | deep slow breathing | – |
| 10 | Annoyed | slanted-lid angry | angry V | tight frown | throbbing anger mark | huffing quiver | huff |
| 11 | Dizzy | spinning spirals | hidden | wavy | halo with orbiting stars | woozy wobble | stumble |

Expressions crossfade over about 220 ms (cubic ease). Returning to Idle from
**Dizzy** plays a shake-off and re-centering bounce; from **Annoyed** it plays
a calming sigh.

## How it is built

```
Expanded / Compact (host, bound to BADI)
└─ BADIController: Resolver layer + pointer listeners
   └─ Character component (bound to its own Pose instance)
      ├─ Effects  (Pose: expression, interaction)
      ├─ Face: Brows (expression, mode) · Eyes (expression, interaction) · Mouth (expression, audioLevel)
      ├─ Body
      └─ Limbs: Arm L/R, Leg L/R
```

- The host's **Resolver** layer picks the effective expression and *keys* it onto
  the Character placement's Pose instance. Other values (`audioLevel`,
  `interaction`, `mode`) reach the Character by data binding.
- Each component is a stateful nested artboard with its own `Pose` instance, so
  each one reads only its own public properties (`componentProps`).
- Poses are separate art groups, crossfaded by opacity. Every pose stays
  editable art in the editor.

### State machine layers

| Artboard | Layer | Drives | Notes |
|---|---|---|---|
| Expanded/Compact | Resolver | Character expression | 11 states, priority conditions |
| Character | Expression | Pivot tilt/squash, Rig lift, shadow | static pose per expression |
| Character | Motion | Motion node + shadow | looping body motion per expression |
| Character | Reaction | React node | one-shots (perk, jolt, huff, stumble) + recoveries |
| Character | Interaction | Touch node | hover lift/lean, pressed squash |
| Character | Limbs | arms/legs | expanded mode only: point / cheer |
| Body | Jelly | silhouette vertices, washes, sheen | soft 4 s outline morph and liquid drift |
| Eyes | Expression | eye poses | open / happy / closed / wink / angry / spiral |
| Eyes | Blink | eye scaleY | blink at 2.5 s, double blink at ~4.4 s (5 s cycle); squeezed while pressed |
| Eyes | Spiral Spin | spirals | 1.5 s rotation |
| Brows | Expression / Mode | brow transforms / visibility | compact Idle hides brows |
| Mouth | Expression | mouth shapes | smile / smirk / talk / frown / oh / wavy |
| Mouth | Lipsync | Jaw | 1D blend on `audioLevel` (0–1 mapped to 0–100), or the auto `Talking` loop |
| Effects | Expression / FX Loop / Press | effect visibility / motion / press ticks | |

Node stacking in Character, from outside in:
`Pivot` (expression) > `Motion` (loop) > `React` (one-shots) > `Touch` (pointer) > `Rig` (art).
Each layer owns one node, so no two layers key the same property.

## Editing

- **Art** (body, effects, limbs): edit `components/body.rml`, `effects.rml` and
  `limbs.rml` directly.
- **Expressions, motion tables and controller logic**: edit the tables in
  `tools/gen_expressions.py` (`EYE_TABLE`, `BROW_TABLE`, `MOUTH_TABLE`,
  `BODY_TABLE`, `MOTION_TABLE`, `REACT_TABLE`, `TOUCH_TABLE`, `LIMB_TABLE`), then
  run `python tools/gen_expressions.py`. It rewrites `face.rml`, `character.rml`,
  the hosts, the expression sheet and `data/badi_data.rml`, and patches the state
  machine and timelines in `effects.rml`. Hand edits to the rewritten files are lost.
- **Body jelly**: tune `tools/gen_body_jelly.py`, then run it.
- Then run `rive . --verify` and check a screenshot.

## Validation (Phase 6)

- `rive . --verify`: 0 errors, 0 warnings; the built `.riv` re-imports.
- `rive inspect`: 0 problems; every cubic keyframe has its curve.
  12 artboards, 22 state machine layers, 177 transitions, 115 timelines.
- All controller names (`BADIController`, every input, every layer) are present
  in the built `.riv` bytes.
- Behaviour checked with headless captures (`--data`, `--pointer`, `--advance`):
  each expression, flag priority, lipsync at `audioLevel` 1 vs 0.05, blink,
  success hop, recovery from Dizzy, hover/pressed/release, mode gestures, and
  Compact framing in every expression.

## Known limitations

- The watercolor is built from soft gradients and feathered washes. The
  reference's paper-grain texture would need a shader.
- Arms and legs are not in the reference design. They appear only for the
  expanded-mode gestures (Guiding, Success).
- `ExpressionSheet` and `ComponentSheet` are review artboards and ship in the
  `.riv` (a few KB). Remove those two files before a production build if you
  want them gone.
- An editor-openable `.rev` needs a signed-in CLI (`rive login`, then `rive . --rev`).
- No headless test was run on a web runtime. Local previews use the native runtime.
