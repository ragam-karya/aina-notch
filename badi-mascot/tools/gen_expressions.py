"""
Generates the BADI expression system (Phase 3) as RML.

Writes:
  data/badi_data.rml            Expression enum, BADI and Pose view models
  components/face.rml           Eyes / Brows / Mouth components with every pose
  components/character.rml      Character component (body + face + effects + shadow)
  artboards/expanded.rml        Expanded host (360x360)
  artboards/compact.rml         Compact host (128x128)
  artboards/expression_sheet.rml  All 11 expressions side by side
and patches components/effects.rml (view model, state machine, pose timelines).

The RML it writes is the editable source; rerunning overwrites hand edits in
the files above. Run from the project root:  python tools/gen_expressions.py
"""
import math
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

EXPR = ["idle", "listening", "thinking", "speaking", "success", "error",
        "alert", "guiding", "sleeping", "annoyed", "dizzy"]
ENUM_ID = "0:2000"
EV = {k: f"0:{2001 + i}" for i, k in enumerate(EXPR)}

VM_BADI, P_STATE, I_BADI = "0:2100", "0:2101", "0:2102"
VM_POSE, P_EXPR, I_POSE = "0:2200", "0:2201", "0:2202"
P_AUDIO_BADI, P_AUDIO = "0:2103", "0:2203"     # audioLevel, 0..1
AUDIO_TO_BLEND = "0:2300"                       # range mapper 0..1 -> 0..100 for blend states

MODES = ["compact", "expanded"]
MODE_ENUM = "0:2020"
MV = {k: f"0:{2021 + i}" for i, k in enumerate(MODES)}
INTERS = ["none", "hover", "pressed"]
INTER_ENUM = "0:2030"
IV = {k: f"0:{2031 + i}" for i, k in enumerate(INTERS)}
# BADI (host app) inputs
(P_MODE_BADI, P_IS_LISTENING, P_IS_THINKING, P_IS_SPEAKING,
 P_INTERACTION, P_IS_ANNOYED, P_IS_DIZZY) = (f"0:{n}" for n in range(2104, 2111))
# Pose (component) inputs
P_INTER, P_MODE = "0:2204", "0:2205"

INK = "FF12152B"          # brows, mouth lines
SPIRAL = "FF3B2FA8"       # dizzy spiral eyes


class Ids:
    def __init__(self, start):
        self.n = start

    def __call__(self):
        self.n += 1
        return f"0:{self.n}"


def f(v):
    return f"{v:.4f}".rstrip("0").rstrip(".") if isinstance(v, float) else str(v)


# ---------------------------------------------------------------- state machine

def condition(vm_path, value_id, ind):
    return (f'{ind}<TransitionViewModelCondition opValue="equal">\n'
            f'{ind}    <TransitionPropertyViewModelComparator>\n'
            f'{ind}        <BindablePropertyEnum>\n'
            f'{ind}            <DataBindContext sourcePathIds="{vm_path}" propertyKey="637"/>\n'
            f'{ind}        </BindablePropertyEnum>\n'
            f'{ind}    </TransitionPropertyViewModelComparator>\n'
            f'{ind}    <TransitionValueEnumComparator value="{value_id}"/>\n'
            f'{ind}</TransitionViewModelCondition>\n')


def expression_layer(name, layer_id, anim_ids, ids, vm_path, ind, blend_ms=220):
    """One layer: Entry -> idle, and AnyState -> each expression when the enum matches."""
    states = {k: ids() for k in EXPR}
    i2 = ind + "    "
    out = [f'{ind}<StateMachineLayer name="{name}" id="{layer_id}">\n']
    out.append(f'{i2}<AnyState x="0" y="-220">\n')
    for k in EXPR:
        out.append(f'{i2}    <StateTransition stateToId="{states[k]}" duration="{blend_ms}" interpolationType="cubic">\n')
        out.append(f'{i2}        <CubicEaseInterpolator x1="0.4" y1="0" x2="0.2" y2="1"/>\n')
        out.append(condition(vm_path, EV[k], i2 + "        "))
        out.append(f'{i2}    </StateTransition>\n')
    out.append(f'{i2}</AnyState>\n')
    out.append(f'{i2}<ExitState x="1300" y="-220"/>\n')
    out.append(f'{i2}<EntryState x="-260" y="0">\n{i2}    <StateTransition stateToId="{states["idle"]}"/>\n{i2}</EntryState>\n')
    for n, k in enumerate(EXPR):
        x, y = -100 + (n % 6) * 240, 160 + (n // 6) * 200
        out.append(f'{i2}<AnimationState x="{x}" y="{y}" animationId="{anim_ids[k]}" id="{states[k]}"/>\n')
    out.append(f'{ind}</StateMachineLayer>\n')
    return "".join(out)


def state_machine(name, sm_id, layer_id, anim_ids, ids, vm_path, ind, extra_layers=""):
    return (f'{ind}<StateMachine name="{name}" id="{sm_id}">\n'
            + expression_layer("Expression", layer_id, anim_ids, ids, vm_path, ind + "    ")
            + extra_layers
            + f'{ind}</StateMachine>\n')


def pose_anim(name, anim_id, keys, ind):
    """keys: list of (objectId, property, value). One-frame hold pose."""
    by_obj = {}
    for obj, prop, val in keys:
        by_obj.setdefault(obj, []).append((prop, val))
    out = [f'{ind}<LinearAnimation duration="1" name="{name}" id="{anim_id}">\n']
    for obj, props in by_obj.items():
        out.append(f'{ind}    <KeyedObject objectId="{obj}">\n')
        for prop, val in props:
            out.append(f'{ind}        <KeyedProperty property="{prop}"><KeyFrameDouble value="{f(val)}" frame="0" interpolationType="hold"/></KeyedProperty>\n')
        out.append(f'{ind}    </KeyedObject>\n')
    out.append(f'{ind}</LinearAnimation>\n')
    return "".join(out)


def pose_anims(prefix, anim_ids, keys_for, ind):
    return "".join(pose_anim(f"{prefix} {k.capitalize()}", anim_ids[k], keys_for(k), ind) for k in EXPR)


# ---------------------------------------------------------------- Phase 4 helpers

EASE = '<CubicEaseInterpolator x1="0.42" y1="0" x2="0.58" y2="1"/>'


def anim(name, anim_id, duration, tracks, ind, loop="loop", linear=()):
    """tracks: {(objectId, property): [(frame, value), ...]}. Eased in-out between keys,
    except tracks listed in `linear`."""
    by_obj = {}
    for (obj, prop), keys in tracks.items():
        by_obj.setdefault(obj, []).append((prop, keys))
    lv = f' loopValue="{loop}"' if loop != "oneShot" else ""
    out = [f'{ind}<LinearAnimation{lv} duration="{duration}" name="{name}" id="{anim_id}">\n']
    for obj, props in by_obj.items():
        out.append(f'{ind}    <KeyedObject objectId="{obj}">\n')
        for prop, keys in props:
            out.append(f'{ind}        <KeyedProperty property="{prop}">\n')
            for n, (fr, val) in enumerate(keys):
                if n == len(keys) - 1:
                    out.append(f'{ind}            <KeyFrameDouble value="{f(val)}" frame="{fr}"/>\n')
                elif (obj, prop) in linear:
                    out.append(f'{ind}            <KeyFrameDouble value="{f(val)}" frame="{fr}" interpolationType="linear"/>\n')
                else:
                    out.append(f'{ind}            <KeyFrameDouble value="{f(val)}" frame="{fr}" interpolationType="cubic">{EASE}</KeyFrameDouble>\n')
            out.append(f'{ind}        </KeyedProperty>\n')
        out.append(f'{ind}    </KeyedObject>\n')
    out.append(f'{ind}</LinearAnimation>\n')
    return "".join(out)


def tracks_for(obj_props, spec, neutral):
    """Every (obj, prop) in obj_props gets keys: spec value if given, else a single neutral key."""
    return {op: spec.get(op, [(0, neutral[op[1]])]) for op in obj_props}


def c_enum(path, value, op=""):
    return ("TransitionViewModelCondition", op, f'''<TransitionPropertyViewModelComparator>
    <BindablePropertyEnum><DataBindContext sourcePathIds="{path}" propertyKey="637"/></BindablePropertyEnum>
</TransitionPropertyViewModelComparator>
<TransitionValueEnumComparator value="{value}"/>''')


def c_num(path, op, value):
    return ("TransitionViewModelCondition", op, f'''<TransitionPropertyViewModelComparator>
    <BindablePropertyNumber><DataBindContext sourcePathIds="{path}" propertyKey="636"/></BindablePropertyNumber>
</TransitionPropertyViewModelComparator>
<TransitionValueNumberComparator value="{f(value)}"/>''')


def trans(to, ind, conds=(), duration=0, exit_pct=None):
    attrs = f' duration="{duration}"' if duration else ""
    if exit_pct is not None:
        attrs += f' enableExitTime="true" exitTimeIsPercetange="true" exitTime="{exit_pct}"'
    out = [f'{ind}<StateTransition stateToId="{to}"{attrs}>\n']
    for tag, op, body in conds:
        opa = f' opValue="{op}"' if op else ' opValue="equal"'
        out.append(f'{ind}    <{tag}{opa}>\n')
        out += [f'{ind}        {line}\n' for line in body.split("\n")]
        out.append(f'{ind}    </{tag}>\n')
    out.append(f'{ind}</StateTransition>\n')
    return "".join(out)


def layer(name, layer_id, ind, any_trans, entry_to, states):
    """states: list of (stateId, animationId or raw blend markup, [transition markup], x, y)."""
    i2 = ind + "    "
    out = [f'{ind}<StateMachineLayer name="{name}" id="{layer_id}">\n',
           f'{i2}<AnyState x="0" y="-220">\n', *any_trans, f'{i2}</AnyState>\n',
           f'{i2}<ExitState x="1300" y="-220"/>\n',
           f'{i2}<EntryState x="-260" y="0">\n{i2}    <StateTransition stateToId="{entry_to}"/>\n{i2}</EntryState>\n']
    for st in states:
        sid, a, ts, x, y = st[:5]
        extra = st[5] if len(st) > 5 else ""
        if a.lstrip().startswith("<"):
            out.append(a)
        elif ts:
            out.append(f'{i2}<AnimationState x="{x}" y="{y}"{extra} animationId="{a}" id="{sid}">\n{"".join(ts)}{i2}</AnimationState>\n')
        else:
            out.append(f'{i2}<AnimationState x="{x}" y="{y}"{extra} animationId="{a}" id="{sid}"/>\n')
    out.append(f'{ind}</StateMachineLayer>\n')
    return "".join(out)


def state_machine_layers(name, sm_id, layers, ind):
    return f'{ind}<StateMachine name="{name}" id="{sm_id}">\n' + "".join(layers) + f'{ind}</StateMachine>\n'


# ---------------------------------------------------------------- shared art

def stroke_shape(name, sid, verts, ind, thick=3.4, color=INK, x=0, y=0, closed=False):
    out = [f'{ind}<Shape x="{f(x)}" y="{f(y)}" name="{name}" id="{sid}">\n',
           f'{ind}    <PointsPath isClosed="{"true" if closed else "false"}" name="Path">\n']
    out += [f'{ind}        {v}\n' for v in verts]
    out.append(f'{ind}    </PointsPath>\n')
    out.append(f'{ind}    <Stroke thickness="{thick}" cap="round" join="round" name="Stroke"><SolidColor colorValue="{color}" name="Color"/></Stroke>\n')
    out.append(f'{ind}</Shape>\n')
    return "".join(out)


def sv(x, y, r=None):
    return f'<StraightVertex x="{f(x)}" y="{f(y)}"' + (f' radius="{f(r)}"' if r else "") + "/>"


def cv(x, y, rot, dist):
    return f'<CubicMirroredVertex x="{f(x)}" y="{f(y)}" rotation="{f(rot)}" distance="{f(dist)}"/>'


def spiral_verts(mirror, rmax=15.5, turns=2.25):
    """Archimedean spiral r = b*theta as mirrored cubic vertices every quarter turn."""
    tmax = turns * 2 * math.pi
    b = rmax / tmax
    verts = []
    step = math.pi / 2
    t = 0.5
    while t <= tmax + 1e-6:
        r = b * t
        x, y = r * math.cos(t), r * math.sin(t)
        dx = b * math.cos(t) - r * math.sin(t)
        dy = b * math.sin(t) + r * math.cos(t)
        if mirror:
            x, dx = -x, -dx
        speed = math.hypot(dx, dy)
        verts.append(cv(round(x, 2), round(y, 2), round(math.atan2(dy, dx), 4), round(speed * step / 3 * 0.92, 2)))
        t += step
    return verts


GLOSS = ('<RadialGradient startX="-3" startY="-6" endX="16" endY="10" name="Gloss">'
         '<GradientStop colorValue="FF2C3152" position="0"/><GradientStop colorValue="FF0C0F1F" position="1"/>'
         '</RadialGradient>')


# ================================================================= EYES

EYE_POSES = ["open", "happy", "closed", "wink", "angry", "spiral"]
SPIRAL_SHAPES = {}
EYE_L, EYE_R = "0:1110", "0:1120"
# Expressions whose eyes are open enough to blink.
BLINKERS = {"idle", "thinking", "speaking", "error", "alert", "guiding", "annoyed"}

# expression -> per eye (L, R): (pose, (dx, dy, sx, sy) of the Open pose)
EYE_TABLE = {
    "idle":      (("open", (0, 0, 1, 1)),         ("open", (0, 0, 1, 1))),
    "listening": (("happy", (0, 0, 1, 1)),        ("happy", (0, 0, 1, 1))),
    "thinking":  (("open", (1, -1, 1, 1)),        ("open", (3, -6, 0.8, 0.8))),
    "speaking":  (("open", (0, 0, 1, 1)),         ("open", (0, 0, 1, 1))),
    "success":   (("happy", (0, 0, 1, 1)),        ("happy", (0, 0, 1, 1))),
    "error":     (("open", (0, 3, 0.94, 0.88)),   ("open", (0, 3, 0.94, 0.88))),
    "alert":     (("open", (0, -2, 1.08, 1.12)),  ("open", (0, -2, 1.08, 1.12))),
    "guiding":   (("open", (0, 0, 1, 1)),         ("wink", (0, 0, 1, 1))),
    "sleeping":  (("closed", (0, 0, 1, 1)),       ("closed", (0, 0, 1, 1))),
    "annoyed":   (("angry", (0, 0, 1, 1)),        ("angry", (0, 0, 1, 1))),
    "dizzy":     (("spiral", (0, 0, 1, 1)),       ("spiral", (0, 0, 1, 1))),
}


def eye_art(side, ids, ind):
    """Returns (markup, {pose: nodeId}, openNodeId). side: 'L' or 'R'."""
    m = -1 if side == "R" else 1          # +1 = left eye; inner side is +x for L, -x for R
    base = {"L": (1110, 1111, 1112, 1113), "R": (1120, 1121, 1122, 1123)}[side]
    eye_id, open_id, glint_id, pupil_id = (f"0:{n}" for n in base)
    nodes = {"open": open_id}
    i2, i3 = ind + "    ", ind + "        "
    x = -44 if side == "L" else 44
    out = [f'{ind}<Node x="{x}" y="0" name="Eye {side}" id="{eye_id}">\n']

    # Open (default): glossy black eye with catch-light.
    out.append(f'{i2}<Node name="Open" id="{open_id}">\n'
               f'{i3}<Shape x="-4" y="-8" name="Glint" id="{glint_id}">\n'
               f'{i3}    <Ellipse width="7" height="9" name="Path"/>\n'
               f'{i3}    <Fill name="Fill"><SolidColor colorValue="F2FFFFFF" name="Color"/></Fill>\n'
               f'{i3}</Shape>\n'
               f'{i3}<Shape name="Pupil" id="{pupil_id}">\n'
               f'{i3}    <Ellipse width="26" height="34" name="Path"/>\n'
               f'{i3}    <Fill name="Fill">{GLOSS}</Fill>\n'
               f'{i3}</Shape>\n'
               f'{i2}</Node>\n')

    def pose_node(pose, label, body):
        nid = ids()
        nodes[pose] = nid
        out.append(f'{i2}<Node opacity="0" name="{label}" id="{nid}">\n{body}{i2}</Node>\n')

    # Happy: upward arc (Listening, Success).
    pose_node("happy", "Happy", stroke_shape("Arc", ids(), [sv(-11, 5), cv(0, -6, 0, 7), sv(11, 5)], i3))
    # Closed: relaxed downward lid (Sleeping).
    pose_node("closed", "Closed", stroke_shape("Lid", ids(), [sv(-11, -2), cv(0, 6, 0, 7), sv(11, -2)], i3))
    # Wink: chevron pointing to the face centre (Guiding).
    pose_node("wink", "Wink", stroke_shape("Chevron", ids(), [sv(-7 * m, -8), sv(6 * m, 0, 2), sv(-7 * m, 8)], i3))
    # Angry: eye with a slanted lid, inner corner lower (Annoyed).
    angry = (f'{i3}<Shape x="-3" y="-2" name="Glint" id="{ids()}">\n'
             f'{i3}    <Ellipse width="6" height="7" name="Path"/>\n'
             f'{i3}    <Fill name="Fill"><SolidColor colorValue="F2FFFFFF" name="Color"/></Fill>\n'
             f'{i3}</Shape>\n'
             f'{i3}<Shape y="2" name="Lidded Eye" id="{ids()}">\n'
             f'{i3}    <PointsPath isClosed="true" isClockwise="{"true" if m == 1 else "false"}" name="Path">\n'
             f'{i3}        {sv(-14 * m, -10, 3)}\n'
             f'{i3}        {sv(14 * m, -1, 3)}\n'
             f'{i3}        {cv(0, 17, math.pi if m == 1 else 0, 11)}\n'
             f'{i3}    </PointsPath>\n'
             f'{i3}    <Fill name="Fill">{GLOSS}</Fill>\n'
             f'{i3}</Shape>\n')
    pose_node("angry", "Angry", angry)
    # Spiral (Dizzy).
    SPIRAL_SHAPES[side] = ids()
    pose_node("spiral", "Spiral", stroke_shape("Spiral", SPIRAL_SHAPES[side], spiral_verts(mirror=(side == "R")), i3, thick=3.4, color=SPIRAL))

    out.append(f'{ind}</Node>\n')
    return "".join(out), nodes, open_id


# ================================================================= BROWS

BROW_L, BROW_R = "0:1210", "0:1220"
# expression -> ((dx, dy, rot, opacity) for L, same for R); R mirrors dx/rot by hand.
BROW_TABLE = {
    "idle":      ((0, 0, 0, 1),           (0, 0, 0, 1)),
    "listening": ((0, 0, 0, 0),           (0, 0, 0, 0)),
    "thinking":  ((0, 2, 0, 0),           (2, -8, 0.12, 1)),
    "speaking":  ((0, -3, 0, 1),          (0, -3, 0, 1)),
    "success":   ((2, -6, -0.1, 1),       (0, 0, 0, 0)),
    "error":     ((7, 7, -0.38, 1),       (-7, 7, 0.38, 1)),
    "alert":     ((-2, -10, -0.08, 1),    (2, -10, 0.08, 1)),
    "guiding":   ((0, 0, 0, 0),           (0, 0, 0, 0)),
    "sleeping":  ((0, 0, 0, 0),           (0, 0, 0, 0)),
    "annoyed":   ((9, 13, 0.95, 1),       (-9, 13, -0.95, 1)),
    "dizzy":     ((0, 0, 0, 0),           (0, 0, 0, 0)),
}


def brow_keys(k):
    keys = []
    for bid, base_x, (dx, dy, rot, op) in ((BROW_L, -58, BROW_TABLE[k][0]), (BROW_R, 58, BROW_TABLE[k][1])):
        keys += [(bid, "x", base_x + dx), (bid, "y", dy), (bid, "rotation", rot), (bid, "opacity", op)]
    return keys


# ================================================================= MOUTH

MOUTH_SMILE, MOUTH_TALK = "0:1310", "0:1320"
# expression -> (visible part, sx, sy, rotation)
MOUTH_TABLE = {
    "idle":      ("smile", 1, 1, 0),
    "listening": ("smile", 0.82, 0.9, 0),
    "thinking":  ("smirk", 1, 1, 0),
    "speaking":  ("talk", 1, 1, 0),
    "success":   ("talk", 1.3, 1.15, 0),
    "error":     ("frown", 1, 1, 0),
    "alert":     ("oh", 1, 1, 0),
    "guiding":   ("smile", 0.9, 0.95, -0.06),
    "sleeping":  ("smile", 0.55, 0.5, 0),
    "annoyed":   ("frown", 1.15, 0.6, 0),
    "dizzy":     ("wavy", 1, 1, 0),
}


def build_face():
    ids = Ids(3000)
    eye_markup, eye_nodes = {}, {}
    for side in "LR":
        eye_markup[side], eye_nodes[side], _ = eye_art(side, ids, "        ")

    eye_anims = {k: ids() for k in EXPR}

    def eye_keys(k):
        keys = []
        for n, side in enumerate("LR"):
            pose, (dx, dy, sx, sy) = EYE_TABLE[k][n]
            for p, nid in eye_nodes[side].items():
                keys.append((nid, "opacity", 1 if p == pose else 0))
            o = eye_nodes[side]["open"]
            keys += [(o, "x", dx), (o, "y", dy), (o, "scaleX", sx), (o, "scaleY", sy)]
        return keys

    path = f"{VM_POSE}-{P_EXPR}"
    L, T = "            ", "                    "
    blink_s, still_s, spin_s = ids(), ids(), ids()
    blink_a, still_a, spin_a = ids(), ids(), ids()
    inter = f"{VM_POSE}-{P_INTER}"
    not_pressed = c_enum(inter, IV["pressed"], "notEqual")
    squeeze_s, squeeze_a = ids(), ids()
    blink_layer = layer("Blink", ids(), L,
                        [trans(squeeze_s, T, [c_enum(inter, IV["pressed"])], duration=80)]
                        + [trans(blink_s if k in BLINKERS else still_s, T, [c_enum(path, EV[k]), not_pressed], duration=120)
                           for k in EXPR],
                        blink_s, [(blink_s, blink_a, [], 160, 160), (still_s, still_a, [], 400, 160),
                                  (squeeze_s, squeeze_a, [], 640, 160)])
    spin_layer = layer("Spiral Spin", ids(), L, [], spin_s, [(spin_s, spin_a, [], 160, 160)])
    eyes_sm = state_machine("Eyes SM", "0:1190", "0:1191", eye_anims, ids, path, "        ",
                            blink_layer + spin_layer)
    # Natural blink: a single blink at 2.5s, a double blink at ~4.4s, 5s cycle.
    blink_keys = [(0, 1), (146, 1), (150, 0.08), (156, 1), (258, 1), (261, 0.08), (266, 1),
                  (274, 1), (277, 0.08), (282, 1), (300, 1)]
    eyes_motion = (
        anim("Blink", blink_a, 300, {(EYE_L, "scaleY"): blink_keys, (EYE_R, "scaleY"): blink_keys}, "        ")
        + anim("Eyes Still", still_a, 1, {(EYE_L, "scaleY"): [(0, 1)], (EYE_R, "scaleY"): [(0, 1)]}, "        ", loop="oneShot")
        + anim("Eyes Squeeze", squeeze_a, 1, {(EYE_L, "scaleY"): [(0, 0.18)], (EYE_R, "scaleY"): [(0, 0.18)]}, "        ", loop="oneShot")
        + anim("Spiral Spin", spin_a, 90,
               {(SPIRAL_SHAPES["L"], "rotation"): [(0, 0), (90, -6.2831853)],
                (SPIRAL_SHAPES["R"], "rotation"): [(0, 0), (90, -6.2831853)]}, "        ",
               linear={(SPIRAL_SHAPES["L"], "rotation"), (SPIRAL_SHAPES["R"], "rotation")}))

    # ---- brows
    ids_b = Ids(3200)
    brow_anims = {k: ids_b() for k in EXPR}
    bpath, mpath = f"{VM_POSE}-{P_EXPR}", f"{VM_POSE}-{P_MODE}"
    show_s, hide_s, show_a, hide_a = ids_b(), ids_b(), ids_b(), ids_b()
    TB = "                    "
    brows_mode_layer = layer("Mode", ids_b(), "            ",
                             [trans(hide_s, TB, [c_enum(mpath, MV["compact"]), c_enum(bpath, EV["idle"])], duration=200),
                              trans(show_s, TB, [c_enum(mpath, MV["expanded"])], duration=200),
                              trans(show_s, TB, [c_enum(bpath, EV["idle"], "notEqual")], duration=200)],
                             show_s, [(show_s, show_a, [], 160, 160), (hide_s, hide_a, [], 400, 160)])
    brows_sm = state_machine("Brows SM", "0:1290", "0:1291", brow_anims, ids_b, bpath, "        ", brows_mode_layer)
    brows_mode_anims = (anim("Brows Show", show_a, 1, {("0:1230", "opacity"): [(0, 1)]}, "        ", loop="oneShot")
                        + anim("Brows Hide", hide_a, 1, {("0:1230", "opacity"): [(0, 0)]}, "        ", loop="oneShot"))

    # ---- mouth
    ids_m = Ids(3400)
    i2, i3 = "        ", "            "
    mouth_nodes = {"smile": MOUTH_SMILE, "talk": MOUTH_TALK}
    extra = []

    def mouth_node(part, label, body):
        nid = ids_m()
        mouth_nodes[part] = nid
        extra.append(f'{i2}<Node opacity="0" name="{label}" id="{nid}">\n{body}{i2}</Node>\n')

    mouth_node("smirk", "Smirk", stroke_shape("Line", ids_m(), [sv(-7, 3), cv(2, 1, -0.35, 4), sv(10, -3)], i3, x=4, y=0))
    mouth_node("frown", "Frown", stroke_shape("Line", ids_m(), [sv(-11, 5), cv(0, -3, 0, 7), sv(11, 5)], i3))
    oh_cav, oh_tongue = ids_m(), ids_m()
    mouth_node("oh", "Oh",
               f'{i3}<Shape y="9" name="Tongue" id="{oh_tongue}">\n'
               f'{i3}    <Ellipse width="14" height="10" name="Path"/>\n'
               f'{i3}    <Fill name="Fill"><SolidColor colorValue="FFF0577A" name="Color"/></Fill>\n'
               f'{i3}    <ClippingShape sourceId="{oh_cav}" name="Clip to Mouth"/>\n'
               f'{i3}</Shape>\n'
               f'{i3}<Shape y="3" name="Cavity" id="{oh_cav}">\n'
               f'{i3}    <Ellipse width="17" height="21" name="Path"/>\n'
               f'{i3}    <Fill name="Fill"><SolidColor colorValue="FF1E2140" name="Color"/></Fill>\n'
               f'{i3}    <Stroke thickness="2.4" name="Stroke"><SolidColor colorValue="{INK}" name="Color"/></Stroke>\n'
               f'{i3}</Shape>\n')
    mouth_node("wavy", "Wavy", stroke_shape("Line", ids_m(),
               [sv(-13, 1), cv(-6.5, -3, 0, 3), cv(0, 2, 0, 3), cv(6.5, -3, 0, 3), sv(13, 1)], i3, thick=3))

    mouth_anims = {k: ids_m() for k in EXPR}

    def mouth_keys(k):
        part, sx, sy, rot = MOUTH_TABLE[k]
        keys = [(nid, "opacity", 1 if p == part else 0) for p, nid in mouth_nodes.items()]
        for p in ("smile", "talk", "frown"):
            nid = mouth_nodes[p]
            on = p == part
            keys += [(nid, "scaleX", sx if on else 1), (nid, "scaleY", sy if on else 1), (nid, "rotation", rot if on else 0)]
        return keys

    path = f"{VM_POSE}-{P_EXPR}"
    audio = f"{VM_POSE}-{P_AUDIO}"
    L, T, T2 = "            ", "                    ", "                    "
    rest_s, auto_s, sync_s = ids_m(), ids_m(), ids_m()
    rest_a, closed_a, open_a = ids_m(), ids_m(), ids_m()
    speaking = c_enum(path, EV["speaking"])
    quiet, loud = c_num(audio, "lessThanOrEqual", 0.001), c_num(audio, "greaterThan", 0.001)
    sync_state = (f'{L}    <BlendState1DViewModel x="560" y="160" id="{sync_s}">\n'
                  f'{L}        <BindablePropertyNumber>\n'
                  f'{L}            <DataBindContext sourcePathIds="{audio}" propertyKey="636" converterId="{AUDIO_TO_BLEND}"/>\n'
                  f'{L}        </BindablePropertyNumber>\n'
                  f'{L}        <BlendAnimation1D animationId="{closed_a}" value="0"/>\n'
                  f'{L}        <BlendAnimation1D animationId="{open_a}" value="100"/>\n'
                  f'{L}    </BlendState1DViewModel>\n')
    lipsync_layer = layer(
        "Lipsync", ids_m(), L,
        [trans(rest_s, T, [c_enum(path, EV[k])], duration=80) for k in EXPR if k != "speaking"],
        rest_s,
        [(rest_s, rest_a, [trans(auto_s, T2, [speaking, quiet], duration=80),
                           trans(sync_s, T2, [speaking, loud], duration=80)], 160, 160),
         (auto_s, "0:1381", [trans(sync_s, T2, [loud], duration=80)], 360, 320),
         (sync_s, sync_state, [], 560, 160)])
    mouth_sm = state_machine("Mouth SM", "0:1390", "0:1391", mouth_anims, ids_m, path, "        ", lipsync_layer)

    jaw = "0:1325"
    jaw_props = [(jaw, "scaleX"), (jaw, "scaleY")]
    talk_loop = (
        "\n        <!-- Lipsync. Jaw sits inside Talk, so the pose layer and lipsync never key the same node. -->\n"
        + anim("Jaw Rest", rest_a, 1, {(jaw, "scaleX"): [(0, 1)], (jaw, "scaleY"): [(0, 1)]}, "        ", loop="oneShot")
        + anim("Jaw Closed", closed_a, 1, {(jaw, "scaleX"): [(0, 1.1)], (jaw, "scaleY"): [(0, 0.22)]}, "        ", loop="oneShot")
        + anim("Jaw Open", open_a, 1, {(jaw, "scaleX"): [(0, 0.94)], (jaw, "scaleY"): [(0, 1.12)]}, "        ", loop="oneShot")
        + "        <!-- Talking: speech-like flapping used when no audioLevel is supplied. -->\n"
        + anim("Talking", "0:1381", 48,
               {(jaw, "scaleY"): [(0, 1), (7, 0.35), (14, 0.85), (20, 0.5), (29, 1.05), (38, 0.3), (48, 1)],
                (jaw, "scaleX"): [(0, 1), (7, 1.12), (14, 0.95), (20, 1.08), (29, 0.92), (38, 1.14), (48, 1)]},
               "        "))


    vm_attrs = f'viewModelId="{VM_POSE}" viewModelInstanceId="{I_POSE}"'
    doc = f'''<Rive version="1" kind="fragment">
    <!--
        Face components: Eyes (pair), Brows (pair), Mouth.
        GENERATED by tools/gen_expressions.py (Phase 3).
        Each is centred on its origin (originX/Y 0.5) and bound to the Pose view
        model; its state machine shows the pose for Pose.expression.
        Pose nodes are crossfaded by opacity, so every pose stays editable art.
    -->

    <!-- ================= EYES ================= -->
    <Artboard styleId="0:1195" isComponent="true" originX="0.5" originY="0.5" defaultStateMachineId="0:1190"
              {vm_attrs} x="320" y="600" width="130" height="56" name="Eyes" id="0:1100">
        <LayoutComponentStyle name="Artboard Style" id="0:1195"/>

{eyes_sm}
{eye_markup["L"]}
{eye_markup["R"]}
{pose_anims("Eyes", eye_anims, eye_keys, "        ")}{eyes_motion}    </Artboard>

    <!-- ================= BROWS ================= -->
    <Artboard styleId="0:1295" isComponent="true" originX="0.5" originY="0.5" defaultStateMachineId="0:1290"
              {vm_attrs} x="500" y="600" width="150" height="56" name="Brows" id="0:1200">
        <LayoutComponentStyle name="Artboard Style" id="0:1295"/>

{brows_sm}
        <!-- Brow Set: faded by the Mode layer (compact Idle has no brows). -->
        <Node name="Brow Set" id="0:1230">
        <Shape x="-58" y="0" name="Brow L" id="{BROW_L}">
            <PointsPath isClosed="false" name="Path">
                <StraightVertex x="-8" y="7"/>
                <CubicDetachedVertex x="8" y="-4" inRotation="2.6" inDistance="9" outRotation="0" outDistance="0"/>
            </PointsPath>
            <Stroke thickness="3.2" cap="round" join="round" name="Stroke">
                <SolidColor colorValue="{INK}" name="Color"/>
            </Stroke>
        </Shape>
        <Shape x="58" y="0" name="Brow R" id="{BROW_R}">
            <PointsPath isClosed="false" name="Path">
                <StraightVertex x="8" y="7"/>
                <CubicDetachedVertex x="-8" y="-4" inRotation="0.54" inDistance="9" outRotation="0" outDistance="0"/>
            </PointsPath>
            <Stroke thickness="3.2" cap="round" join="round" name="Stroke">
                <SolidColor colorValue="{INK}" name="Color"/>
            </Stroke>
        </Shape>
        </Node>

{pose_anims("Brows", brow_anims, brow_keys, "        ")}{brows_mode_anims}    </Artboard>

    <!-- ================= MOUTH ================= -->
    <Artboard styleId="0:1395" isComponent="true" originX="0.5" originY="0.5" defaultStateMachineId="0:1390"
              {vm_attrs} x="700" y="600" width="80" height="56" name="Mouth" id="0:1300">
        <LayoutComponentStyle name="Artboard Style" id="0:1395"/>

{mouth_sm}
        <Shape name="Smile" id="{MOUTH_SMILE}">
            <PointsPath isClosed="false" name="Path">
                <StraightVertex x="-26" y="-4"/>
                <CubicMirroredVertex x="0" y="8" rotation="0" distance="14"/>
                <StraightVertex x="26" y="-4"/>
            </PointsPath>
            <Stroke thickness="3.4" cap="round" join="round" name="Stroke">
                <SolidColor colorValue="{INK}" name="Color"/>
            </Stroke>
        </Shape>

        <!-- Talk: open "D" mouth with a pink tongue. Pivot on the top lip, so scaleY opens it downward. -->
        <Node x="0" y="-5" opacity="0" name="Talk" id="{MOUTH_TALK}">
          <Node name="Jaw" id="0:1325">
            <Shape x="0" y="14" name="Tongue" id="0:1321">
                <Ellipse width="18" height="11" name="Path"/>
                <Fill name="Fill"><SolidColor colorValue="FFF0577A" name="Color"/></Fill>
                <ClippingShape sourceId="0:1322" name="Clip to Mouth"/>
            </Shape>
            <Shape name="Cavity" id="0:1322">
                <PointsPath isClosed="true" name="Path">
                    <StraightVertex x="-15" y="0" radius="4"/>
                    <StraightVertex x="15" y="0" radius="4"/>
                    <CubicMirroredVertex x="0" y="19" rotation="3.1416" distance="10"/>
                </PointsPath>
                <Fill name="Fill"><SolidColor colorValue="FF1E2140" name="Color"/></Fill>
                <Stroke thickness="2.6" cap="round" join="round" name="Stroke">
                    <SolidColor colorValue="{INK}" name="Color"/>
                </Stroke>
            </Shape>
          </Node>
        </Node>

{"".join(extra)}
{pose_anims("Mouth", mouth_anims, mouth_keys, "        ")}{talk_loop}    </Artboard>

    <ComponentAsset artboardId="0:1100" name="Eyes"/>
    <ComponentAsset artboardId="0:1200" name="Brows"/>
    <ComponentAsset artboardId="0:1300" name="Mouth"/>
</Rive>
'''
    (ROOT / "components/face.rml").write_text(doc, encoding="utf-8")


# ================================================================= EFFECTS (patch)

FX = {"listening": "0:1610", "speaking": "0:1620", "success": "0:1630", "alert": "0:1640",
      "guiding": "0:1650", "sleeping": "0:1660", "annoyed": "0:1670", "dizzy": "0:1680"}
FX_ALL = list(FX.values())   # press ticks are driven by interaction, on their own layer


def patch_effects():
    p = ROOT / "components/effects.rml"
    s = p.read_text(encoding="utf-8")
    ids = Ids(3600)
    anims = {k: ids() for k in EXPR}
    fx_s, fx_a = ids(), ids()
    fx_layer = layer("FX Loop", ids(), "            ", [], fx_s, [(fx_s, fx_a, [], 160, 160)])
    inter = f"{VM_POSE}-{P_INTER}"
    pon_s, poff_s, pon_a, poff_a = ids(), ids(), ids(), ids()
    TP = "                    "
    press_layer = layer("Press", ids(), "            ",
                        [trans(pon_s, TP, [c_enum(inter, IV["pressed"])], duration=60),
                         trans(poff_s, TP, [c_enum(inter, IV["pressed"], "notEqual")], duration=160)],
                        poff_s, [(poff_s, poff_a, [], 160, 160), (pon_s, pon_a, [], 400, 160)])
    press_anims = (anim("Press Off", poff_a, 1, {("0:1696", "opacity"): [(0, 0)], ("0:1697", "opacity"): [(0, 0)]}, "        ", loop="oneShot")
                   + anim("Press On", pon_a, 1, {("0:1696", "opacity"): [(0, 1)], ("0:1697", "opacity"): [(0, 1)]}, "        ", loop="oneShot"))
    sm = state_machine("Effects SM", "0:1690", "0:1691", anims, ids, f"{VM_POSE}-{P_EXPR}", "        ", fx_layer + press_layer)
    # Press Ticks: the group stays visible; its two shapes are what the Press layer fades.
    s = s.replace('opacity="0" name="Press Ticks"', 'name="Press Ticks"')
    for side, sid in (("Left", "0:1696"), ("Right", "0:1697")):
        s = re.sub(r'(<Shape x="-?118" y="-46")( name="%s" id="%s">)' % (side, sid), r'\1 opacity="0"\2', s)

    def pulse(lo, hi, period, total=120):
        return [(fr, lo if (fr // (period // 2)) % 2 == 0 else hi) for fr in range(0, total + 1, period // 2)]

    # Stars orbit the halo ellipse (a=75, b=14); dimmer on the far (upper) side.
    def orbit(phase):
        xs, ys, ops = [], [], []
        for i in range(9):
            a = phase + i * 2 * math.pi / 8
            fr = i * 15
            xs.append((fr, round(75 * math.cos(a), 2)))
            ys.append((fr, round(14 * math.sin(a), 2)))
            ops.append((fr, round(0.7 + 0.3 * math.sin(a), 3)))
        return xs, ys, ops

    bx, by, bo = orbit(0.0)
    sx_, sy_, so = orbit(math.pi)
    fx_tracks = {
        ("0:1611", "opacity"): pulse(0.35, 1, 60),
        ("0:1612", "opacity"): pulse(1, 0.35, 60),
        ("0:1621", "scaleX"): pulse(0.9, 1.12, 30), ("0:1621", "scaleY"): pulse(0.9, 1.12, 30),
        ("0:1631", "scaleX"): pulse(0.85, 1.15, 60), ("0:1631", "scaleY"): pulse(0.85, 1.15, 60),
        ("0:1631", "rotation"): pulse(-0.06, 0.06, 60),
        ("0:1641", "scaleX"): pulse(0.92, 1.15, 40), ("0:1641", "scaleY"): pulse(0.92, 1.15, 40),
        ("0:1651", "x"): pulse(0, 3, 60),
        ("0:1660", "y"): [(0, -112), (60, -118), (120, -112)],
        ("0:1661", "opacity"): [(0, 0.25), (20, 1), (60, 1), (80, 0.25), (120, 0.25)],
        ("0:1662", "opacity"): [(0, 0.25), (40, 1), (80, 1), (100, 0.25), (120, 0.25)],
        ("0:1663", "opacity"): [(0, 0.25), (20, 0.25), (60, 1), (100, 1), (120, 0.25)],
        ("0:1671", "scaleX"): [(0, 1), (8, 1.25), (16, 1), (24, 1.18), (32, 1), (60, 1), (68, 1.25), (76, 1), (84, 1.18), (92, 1), (120, 1)],
        ("0:1671", "scaleY"): [(0, 1), (8, 1.25), (16, 1), (24, 1.18), (32, 1), (60, 1), (68, 1.25), (76, 1), (84, 1.18), (92, 1), (120, 1)],
        ("0:1681", "x"): bx, ("0:1681", "y"): by, ("0:1681", "opacity"): bo,
        ("0:1681", "rotation"): [(0, 0), (120, 6.2831853)],
        ("0:1682", "x"): sx_, ("0:1682", "y"): sy_, ("0:1682", "opacity"): so,
        ("0:1683", "rotation"): [(0, -0.04), (60, 0.04), (120, -0.04)],
    }
    linear = {k for k in fx_tracks if k[0] in ("0:1681", "0:1682")}
    s = re.sub(r'        <StateMachine name="Effects SM".*?</StateMachine>\n', lambda m: sm, s, flags=re.S)
    if "viewModelId=" not in s.split('name="Effects"')[0].rsplit("<Artboard", 1)[1]:
        s = s.replace('name="Effects" id="0:1600">', f'viewModelId="{VM_POSE}" viewModelInstanceId="{I_POSE}" name="Effects" id="0:1600">', 1)

    def keys(k):
        return [(g, "opacity", 1 if FX.get(k) == g else 0) for g in FX_ALL]

    block = ("        <!-- @gen:pose-anims -->\n" + pose_anims("Effects", anims, keys, "        ")
             + "        <!-- FX Loop: every effect's idle motion; only the visible one is seen. -->\n"
             + anim("FX Loop", fx_a, 120, fx_tracks, "        ", linear=linear)
             + "        <!-- Press: side ticks while BADI is pressed. -->\n" + press_anims
             + "        <!-- @end:pose-anims -->\n")
    if "<!-- @gen:pose-anims -->" in s:
        s = re.sub(r"        <!-- @gen:pose-anims -->.*?<!-- @end:pose-anims -->\n", lambda m: block, s, flags=re.S)
    else:
        s = s.replace("    </Artboard>\n\n    <ComponentAsset artboardId=\"0:1600\"", block + "    </Artboard>\n\n    <ComponentAsset artboardId=\"0:1600\"", 1)
    p.write_text(s, encoding="utf-8")


# ================================================================= CHARACTER

PIVOT, RIG, SHADOW = "0:3801", "0:3802", "0:3803"
MOTION, SHADOW_MOTION, REACT = "0:3805", "0:3806", "0:3807"
NEUTRAL = {"x": 0, "y": 0, "rotation": 0, "scaleX": 1, "scaleY": 1, "opacity": 1}
MOTION_PROPS = [(MOTION, "x"), (MOTION, "y"), (MOTION, "rotation"), (MOTION, "scaleX"), (MOTION, "scaleY"),
                (SHADOW_MOTION, "scaleX"), (SHADOW_MOTION, "opacity")]
REACT_PROPS = [(REACT, "x"), (REACT, "y"), (REACT, "rotation"), (REACT, "scaleX"), (REACT, "scaleY")]

# Looping body motion per expression: (frames, {(node, prop): keys}). Pivot is the body's base.
M, SH = MOTION, SHADOW_MOTION
MOTION_TABLE = {
    # breathing
    "idle": (180, {(M, "scaleY"): [(0, 1), (90, 1.025), (180, 1)], (M, "scaleX"): [(0, 1), (90, 0.99), (180, 1)],
                   (M, "y"): [(0, 0), (90, -2), (180, 0)], (SH, "scaleX"): [(0, 1), (90, 0.96), (180, 1)]}),
    # leaning in toward the sound, attentive bob
    "listening": (120, {(M, "rotation"): [(0, 0), (60, 0.035), (120, 0)], (M, "y"): [(0, 0), (60, -3), (120, 0)],
                        (M, "scaleY"): [(0, 1), (30, 1.015), (60, 1), (90, 1.015), (120, 1)]}),
    # slow ponder sway with a little float
    "thinking": (240, {(M, "rotation"): [(0, -0.03), (120, 0.03), (240, -0.03)], (M, "x"): [(0, -3), (120, 3), (240, -3)],
                       (M, "y"): [(0, 0), (60, -3), (120, 0), (180, -3), (240, 0)]}),
    # quick talking bob
    "speaking": (30, {(M, "scaleY"): [(0, 1), (15, 1.018), (30, 1)], (M, "scaleX"): [(0, 1), (15, 0.992), (30, 1)],
                      (M, "y"): [(0, 0), (15, -1.5), (30, 0)]}),
    # happy hop: anticipation, stretch up, squash on landing, settle
    "success": (90, {(M, "y"): [(0, 0), (8, 0), (22, -16), (30, -18), (42, 0), (90, 0)],
                     (M, "scaleY"): [(0, 1), (8, 0.9), (20, 1.08), (30, 1), (42, 0.88), (52, 1.03), (60, 1), (90, 1)],
                     (M, "scaleX"): [(0, 1), (8, 1.08), (20, 0.94), (30, 1), (42, 1.1), (52, 0.98), (60, 1), (90, 1)],
                     (SH, "scaleX"): [(0, 1), (8, 1.04), (30, 0.7), (42, 1.04), (52, 1), (90, 1)],
                     (SH, "opacity"): [(0, 1), (30, 0.55), (42, 1), (90, 1)]}),
    # little "no" shake, then a sad droop
    "error": (180, {(M, "rotation"): [(0, 0), (10, 0.035), (20, -0.035), (30, 0.02), (40, 0), (180, 0)],
                    (M, "y"): [(0, 0), (90, 2), (180, 0)], (M, "scaleY"): [(0, 1), (90, 0.98), (180, 1)]}),
    # nervous tremble
    "alert": (120, {(M, "x"): [(0, 0), (4, 1.5), (8, -1.5), (12, 1), (16, -1), (20, 0), (120, 0)],
                    (M, "scaleY"): [(0, 1), (60, 1.01), (120, 1)]}),
    # leaning and nodding toward the thing it points at
    "guiding": (120, {(M, "rotation"): [(0, 0), (30, 0.05), (60, 0.02), (90, 0.05), (120, 0)],
                      (M, "y"): [(0, 0), (30, -2), (60, 0), (90, -2), (120, 0)]}),
    # slow deep sleep breathing
    "sleeping": (240, {(M, "scaleY"): [(0, 1), (120, 1.035), (240, 1)], (M, "scaleX"): [(0, 1), (120, 0.985), (240, 1)],
                       (M, "y"): [(0, 0), (120, 1), (240, 0)], (SH, "scaleX"): [(0, 1), (120, 1.03), (240, 1)]}),
    # huffing and quivering
    "annoyed": (90, {(M, "scaleX"): [(0, 1), (10, 1.05), (20, 1), (90, 1)], (M, "scaleY"): [(0, 1), (10, 0.95), (20, 1), (90, 1)],
                     (M, "x"): [(0, 0), (24, 1), (26, -1), (28, 1), (30, -1), (32, 0), (90, 0)]}),
    # woozy wobble
    "dizzy": (120, {(M, "rotation"): [(0, 0), (30, 0.08), (90, -0.08), (120, 0)], (M, "x"): [(0, 0), (30, 5), (90, -5), (120, 0)],
                    (SH, "scaleX"): [(0, 1), (30, 0.95), (60, 1), (90, 0.95), (120, 1)]}),
}

R = REACT
# One-shot reactions: (frames, tracks). Recovery plays when leaving Dizzy / Annoyed for Idle.
REACT_TABLE = {
    "neutral": (1, {}),
    "listen_perk": (30, {(R, "scaleY"): [(0, 1), (6, 0.94), (15, 1.05), (30, 1)], (R, "scaleX"): [(0, 1), (6, 1.04), (15, 0.97), (30, 1)],
                         (R, "y"): [(0, 0), (15, -8), (30, 0)], (R, "rotation"): [(0, 0), (15, 0.05), (30, 0)]}),
    "alert_jolt": (30, {(R, "y"): [(0, 0), (6, -14), (18, 0), (30, 0)], (R, "scaleY"): [(0, 1), (6, 1.08), (18, 0.94), (30, 1)],
                        (R, "scaleX"): [(0, 1), (6, 0.95), (18, 1.05), (30, 1)]}),
    "annoyed_huff": (36, {(R, "scaleX"): [(0, 1), (8, 1.08), (20, 0.98), (36, 1)], (R, "scaleY"): [(0, 1), (8, 0.92), (20, 1.02), (36, 1)]}),
    "dizzy_stumble": (40, {(R, "rotation"): [(0, 0), (10, 0.15), (22, -0.1), (40, 0)], (R, "x"): [(0, 0), (10, 8), (22, -6), (40, 0)]}),
    # shake it off, then a little re-centering bounce
    "recover_shake": (60, {(R, "rotation"): [(0, 0), (6, 0.1), (12, -0.1), (18, 0.08), (24, -0.06), (30, 0.03), (40, 0), (60, 0)],
                           (R, "scaleY"): [(0, 1), (36, 0.94), (46, 1.04), (60, 1)], (R, "scaleX"): [(0, 1), (36, 1.05), (46, 0.98), (60, 1)],
                           (R, "y"): [(0, 0), (36, 0), (46, -6), (60, 0)]}),
    # deep calming breath out
    "recover_sigh": (60, {(R, "scaleY"): [(0, 1), (20, 1.04), (40, 0.95), (60, 1)], (R, "scaleX"): [(0, 1), (20, 0.98), (40, 1.04), (60, 1)],
                          (R, "y"): [(0, 0), (20, -3), (60, 0)]}),
}
# expression -> (rotation, scaleX, scaleY, lift, shadowScale)
BODY_TABLE = {
    "idle":      (0, 1, 1, 0, 1),
    "listening": (0.06, 1, 1, 0, 1),
    "thinking":  (-0.07, 1, 1, -2, 1),
    "speaking":  (0, 1, 1.02, 0, 1),
    "success":   (0, 1.02, 1.04, -8, 0.9),
    "error":     (0, 1.04, 0.95, 0, 1.03),
    "alert":     (0, 0.97, 1.05, -4, 0.95),
    "guiding":   (0.07, 1, 1, 0, 1),
    "sleeping":  (-0.03, 1.05, 0.94, 0, 1.04),
    "annoyed":   (-0.03, 1.02, 0.98, 0, 1),
    "dizzy":     (0.09, 1, 1, 0, 1),
}

COMPONENT_SM = {"0:1000": "0:1090", "0:1100": "0:1190", "0:1200": "0:1290", "0:1300": "0:1390",
                "0:1400": "0:1490", "0:1500": "0:1590", "0:1600": "0:1690", "0:3800": "0:3890"}


def pv(expr=None, fixed=None, expr_id=None, audio=None, inter=None, mode=None, mode_fixed=None):
    """Values for a placement's own Pose instance: (propId, name, kind, default, bindPath, explicitId)."""
    vals = [(P_EXPR, "expression", "enum", fixed or EV["idle"], expr, expr_id)]
    if audio:
        vals.append((P_AUDIO, "audioLevel", "num", 0, audio, None))
    if inter:
        vals.append((P_INTER, "interaction", "enum", IV["none"], inter, None))
    if mode or mode_fixed:
        vals.append((P_MODE, "mode", "enum", mode_fixed or MV["expanded"], mode, None))
    return vals


def placement(artboard, name, nid, ind, ids, attrs="", values=None):
    """A nested component. With `values` it is stateful and owns a Pose instance whose
    values either follow a bind path or hold a literal (or are keyed by the host)."""
    i2, i3 = ind + "    ", ind + "        "
    sf = ' isStateful="true"' if values is not None else ""
    out = [f'{ind}<NestedArtboard artboardId="{artboard}"{sf}{attrs} name="{name}" id="{nid}">\n']
    if values is not None:
        out.append(f'{i2}<ViewModelInstance viewModelId="{VM_POSE}" exports="true" name="{name} Pose" id="{ids()}">\n')
        for pid, pname, kind, default, path, explicit in values:
            tag, key = ("ViewModelInstanceEnum", 560) if kind == "enum" else ("ViewModelInstanceNumber", 575)
            idattr = f' id="{explicit}"' if explicit else ""
            if path:
                out.append(f'{i3}<{tag} propertyValue="{default}" viewModelPropertyId="{pid}" name="{pname}"{idattr}>\n'
                           f'{i3}    <DataBindContext sourcePathIds="{path}" propertyKey="{key}"/>\n'
                           f'{i3}</{tag}>\n')
            else:
                out.append(f'{i3}<{tag} propertyValue="{default}" viewModelPropertyId="{pid}" name="{pname}"{idattr}/>\n')
        out.append(f'{i2}</ViewModelInstance>\n')
    out.append(f'{i2}<NestedStateMachine animationId="{COMPONENT_SM[artboard]}" name="SM"/>\n')
    out.append(f'{ind}</NestedArtboard>\n')
    return "".join(out)


TOUCH = "0:3808"
LIMBS, ARM_L, ARM_R, LEG_L, LEG_R = "0:3804", "0:3840", "0:3841", "0:3842", "0:3843"
TOUCH_PROPS = [(TOUCH, "x"), (TOUCH, "y"), (TOUCH, "rotation"), (TOUCH, "scaleX"), (TOUCH, "scaleY")]
# interaction -> Touch node pose
TOUCH_TABLE = {
    "none":    {},
    "hover":   {(TOUCH, "y"): [(0, -6)], (TOUCH, "rotation"): [(0, 0.04)], (TOUCH, "scaleX"): [(0, 1.01)], (TOUCH, "scaleY"): [(0, 1.02)]},
    "pressed": {(TOUCH, "scaleX"): [(0, 1.08)], (TOUCH, "scaleY"): [(0, 0.9)]},
}
# limb gesture -> (limbs opacity, arm L (x, y, rot, op), arm R (x, y, rot, op), legs opacity)
LIMB_TABLE = {
    "hidden": (0, (-100, 18, 0.55, 1), (98, 6, -0.55, 1), 1),
    "point":  (1, (-100, 18, 0.55, 0), (100, -4, -1.9, 1), 0),   # Guiding: right arm points the way
    "cheer":  (1, (-98, 8, 2.5, 1), (96, -2, -2.5, 1), 1),       # Success: both arms up, feet out
}


def build_character():
    ids = Ids(5000)
    anims = {k: ids() for k in EXPR}
    pose_path = f"{VM_POSE}-{P_EXPR}"
    inter_path, mode_path = f"{VM_POSE}-{P_INTER}", f"{VM_POSE}-{P_MODE}"
    L, T, T2 = "            ", "                    ", "                    "
    # --- Motion layer: a loop per expression
    m_states = {k: ids() for k in EXPR}
    m_anims = {k: ids() for k in EXPR}
    motion_layer = layer("Motion", ids(), L,
                         [trans(m_states[k], T, [c_enum(pose_path, EV[k])], duration=300) for k in EXPR],
                         m_states["idle"],
                         [(m_states[k], m_anims[k], [], -100 + (n % 6) * 240, 160 + (n // 6) * 200) for n, k in enumerate(EXPR)])
    # --- Reaction layer: one-shots on entering an expression, and recovery back to idle
    r_states = {k: ids() for k in REACT_TABLE}
    r_anims = {k: ids() for k in REACT_TABLE}
    idle_c = [c_enum(pose_path, EV["idle"])]
    target = {"listening": "listen_perk", "alert": "alert_jolt", "annoyed": "annoyed_huff", "dizzy": "dizzy_stumble"}
    react_any = [trans(r_states[target.get(k, "neutral")], T, [c_enum(pose_path, EV[k])], duration=100)
                 for k in EXPR if k != "idle"]
    rs = r_states
    react_layer = layer("Reaction", ids(), L, react_any, rs["neutral"], [
        (rs["neutral"], r_anims["neutral"], [], 160, 160),
        (rs["listen_perk"], r_anims["listen_perk"], [trans(rs["neutral"], T2, idle_c, duration=150)], 400, 0, ' reset="true"'),
        (rs["alert_jolt"], r_anims["alert_jolt"], [trans(rs["neutral"], T2, idle_c, duration=150)], 640, 0, ' reset="true"'),
        (rs["annoyed_huff"], r_anims["annoyed_huff"], [trans(rs["recover_sigh"], T2, idle_c, duration=100)], 400, 320, ' reset="true"'),
        (rs["dizzy_stumble"], r_anims["dizzy_stumble"], [trans(rs["recover_shake"], T2, idle_c, duration=100)], 640, 320, ' reset="true"'),
        (rs["recover_sigh"], r_anims["recover_sigh"], [trans(rs["neutral"], T2, (), exit_pct=100)], 400, 480, ' reset="true"'),
        (rs["recover_shake"], r_anims["recover_shake"], [trans(rs["neutral"], T2, (), exit_pct=100)], 640, 480, ' reset="true"'),
    ])
    # --- Interaction layer: hover lifts and leans, pressed squashes
    t_states = {k: ids() for k in TOUCH_TABLE}
    t_anims = {k: ids() for k in TOUCH_TABLE}
    touch_layer = layer("Interaction", ids(), L,
                        [trans(t_states[k], T, [c_enum(inter_path, IV[k])], duration=90 if k == "pressed" else 180)
                         for k in TOUCH_TABLE],
                        t_states["none"],
                        [(t_states[k], t_anims[k], [], 160 + n * 240, 160) for n, k in enumerate(TOUCH_TABLE)])
    # --- Limbs layer (expanded mode only): arms point for Guiding, cheer for Success
    l_states = {k: ids() for k in LIMB_TABLE}
    l_anims = {k: ids() for k in LIMB_TABLE}
    expanded = c_enum(mode_path, MV["expanded"])
    limb_any = [trans(l_states["point"], T, [expanded, c_enum(pose_path, EV["guiding"])], duration=260),
                trans(l_states["cheer"], T, [expanded, c_enum(pose_path, EV["success"])], duration=260),
                trans(l_states["hidden"], T, [c_enum(mode_path, MV["compact"])], duration=200)]
    limb_any += [trans(l_states["hidden"], T, [c_enum(pose_path, EV[k])], duration=200)
                 for k in EXPR if k not in ("guiding", "success")]
    limbs_layer = layer("Limbs", ids(), L, limb_any, l_states["hidden"],
                        [(l_states[k], l_anims[k], [], 160 + n * 240, 160) for n, k in enumerate(LIMB_TABLE)])

    sm = state_machine("Character SM", "0:3890", "0:3891", anims, ids, pose_path, "        ",
                       motion_layer + react_layer + touch_layer + limbs_layer)
    motion_anims = "".join(
        anim(f"Motion {k.capitalize()}", m_anims[k], MOTION_TABLE[k][0],
             tracks_for(MOTION_PROPS, MOTION_TABLE[k][1], NEUTRAL), "        ") for k in EXPR)
    react_anims = "".join(
        anim("React " + k.replace("_", " ").title(), r_anims[k], REACT_TABLE[k][0],
             tracks_for(REACT_PROPS, REACT_TABLE[k][1], NEUTRAL), "        ", loop="oneShot") for k in REACT_TABLE)
    touch_anims = "".join(
        anim(f"Touch {k.capitalize()}", t_anims[k], 1, tracks_for(TOUCH_PROPS, TOUCH_TABLE[k], NEUTRAL),
             "        ", loop="oneShot") for k in TOUCH_TABLE)

    def limb_tracks(k):
        lop, (alx, aly, alr, alo), (arx, ary, arr, aro), legs = LIMB_TABLE[k]
        return {(LIMBS, "opacity"): [(0, lop)],
                (ARM_L, "x"): [(0, alx)], (ARM_L, "y"): [(0, aly)], (ARM_L, "rotation"): [(0, alr)], (ARM_L, "opacity"): [(0, alo)],
                (ARM_R, "x"): [(0, arx)], (ARM_R, "y"): [(0, ary)], (ARM_R, "rotation"): [(0, arr)], (ARM_R, "opacity"): [(0, aro)],
                (LEG_L, "opacity"): [(0, legs)], (LEG_R, "opacity"): [(0, legs)]}
    limb_anims = "".join(anim(f"Limbs {k.capitalize()}", l_anims[k], 1, limb_tracks(k), "        ", loop="oneShot")
                         for k in LIMB_TABLE)

    I = "                    "   # inside Rig
    rig = "".join([
        placement("0:1600", "Effects", ids(), I, ids, values=pv(expr=pose_path, inter=inter_path)),
        f'{I}<Node x="0" y="-4" name="Face" id="{ids()}">\n',
        placement("0:1200", "Brows", ids(), I + "    ", ids, ' x="0" y="-38"', values=pv(expr=pose_path, mode=mode_path)),
        placement("0:1100", "Eyes", ids(), I + "    ", ids, ' x="0" y="-2"', values=pv(expr=pose_path, inter=inter_path)),
        placement("0:1300", "Mouth", ids(), I + "    ", ids, ' x="0" y="26"',
                  values=pv(expr=pose_path, audio=f"{VM_POSE}-{P_AUDIO}")),
        f'{I}</Node>\n',
        placement("0:1000", "Body", ids(), I, ids),
        f'{I}<!-- Limbs tuck behind the body. Hidden unless a gesture (expanded mode) calls for them. -->\n',
        f'{I}<Node opacity="0" name="Limbs" id="{LIMBS}">\n',
        placement("0:1400", "Arm L", ARM_L, I + "    ", ids, ' x="-100" y="18" rotation="0.55"'),
        placement("0:1400", "Arm R", ARM_R, I + "    ", ids, ' x="98" y="6" rotation="-0.55" scaleX="-1"'),
        placement("0:1500", "Leg L", LEG_L, I + "    ", ids, ' x="-36" y="96"'),
        placement("0:1500", "Leg R", LEG_R, I + "    ", ids, ' x="34" y="98" scaleX="-1"'),
        f'{I}</Node>\n',
    ])

    def keys(k):
        rot, sx, sy, lift, sh = BODY_TABLE[k]
        return [(PIVOT, "rotation", rot), (PIVOT, "scaleX", sx), (PIVOT, "scaleY", sy),
                (RIG, "y", -112 + lift), (SHADOW, "scaleX", sh)]

    doc = f'''<Rive version="1" kind="fragment">
    <!--
        Character component: the whole BADI, driven by the Pose view model
        (expression, audioLevel, interaction, mode).
        GENERATED by tools/gen_expressions.py.
        Origin (0,0) is the artboard centre. The body's centre sits at (0,-12);
        Pivot is the body's base, so tilt and squash keep BADI on the ground.
    -->
    <Artboard styleId="0:3895" isComponent="true" originX="0.5" originY="0.5" defaultStateMachineId="0:3890"
              viewModelId="{VM_POSE}" viewModelInstanceId="{I_POSE}"
              x="1400" y="600" width="360" height="360" name="Character" id="0:3800">
        <LayoutComponentStyle name="Artboard Style" id="0:3895"/>

{sm}
        <!-- Pivot (expression pose) > Motion (loop) > React (one-shots) > Touch (hover/press) > Rig (the art). -->
        <Node x="0" y="100" name="Pivot" id="{PIVOT}">
          <Node name="Motion" id="{MOTION}">
           <Node name="React" id="{REACT}">
            <Node name="Touch" id="{TOUCH}">
             <Node x="0" y="-112" name="Rig" id="{RIG}">
{rig}             </Node>
            </Node>
           </Node>
          </Node>
        </Node>

        <!-- Ambient shadow: lavender, low opacity, soft. Stays on the ground. -->
        <Node x="0" y="136" name="Shadow" id="{SHADOW}">
          <Node name="Shadow Motion" id="{SHADOW_MOTION}">
            <Shape name="Shadow Shape" id="{ids()}">
                <Ellipse width="168" height="20" name="Path"/>
                <Fill fillRule="clockwise" name="Fill">
                    <SolidColor colorValue="8C9EA6E6" name="Color"/>
                    <Feather strength="9" name="Feather"/>
                </Fill>
            </Shape>
          </Node>
        </Node>

{pose_anims("Body", anims, keys, "        ")}{motion_anims}{react_anims}{touch_anims}{limb_anims}    </Artboard>

    <ComponentAsset artboardId="0:3800" name="Character"/>
</Rive>
'''
    (ROOT / "components/character.rml").write_text(doc, encoding="utf-8")


# ================================================================= HOSTS: BADIController

# Flags override `state`, highest priority first.
FLAGS = [("dizzy", P_IS_DIZZY), ("annoyed", P_IS_ANNOYED), ("speaking", P_IS_SPEAKING),
         ("listening", P_IS_LISTENING), ("thinking", P_IS_THINKING)]


def c_bool(path, value):
    return ("TransitionViewModelCondition", "", f'''<TransitionPropertyViewModelComparator>
    <BindablePropertyBoolean><DataBindContext sourcePathIds="{path}" propertyKey="634"/></BindablePropertyBoolean>
</TransitionPropertyViewModelComparator>
<TransitionValueBooleanComparator value="{"true" if value else "false"}"/>''')


def set_interaction(value_key, ind):
    return (f'{ind}<ListenerViewModelChange>\n'
            f'{ind}    <BindablePropertyEnum propertyValue="{IV[value_key]}">\n'
            f'{ind}        <DataBindContext sourcePathIds="{VM_BADI}-{P_INTERACTION}" propertyKey="637" direction="true"/>\n'
            f'{ind}    </BindablePropertyEnum>\n'
            f'{ind}</ListenerViewModelChange>\n')


def host(name, ab_id, style_id, sm_id, layer_id, x, w, h, ids, char_attrs, hit, mode_bound, comment):
    B = VM_BADI
    expr_id = ids()
    L, T = "            ", "                    "
    # Resolver: flags (by priority) override state; each state keys the Character's expression.
    r_states = {k: ids() for k in EXPR}
    r_anims = {k: ids() for k in EXPR}
    any_t = []
    for n, (k, flag) in enumerate(FLAGS):
        conds = [c_bool(f"{B}-{flag}", True)] + [c_bool(f"{B}-{hf}", False) for _, hf in FLAGS[:n]]
        any_t.append(trans(r_states[k], T, conds))
    no_flags = [c_bool(f"{B}-{fl}", False) for _, fl in FLAGS]
    for k in EXPR:
        any_t.append(trans(r_states[k], T, no_flags + [c_enum(f"{B}-{P_STATE}", EV[k])]))
    resolver = layer("Resolver", layer_id, L, any_t, r_states["idle"],
                     [(r_states[k], r_anims[k], [], -100 + (n % 6) * 240, 160 + (n // 6) * 200) for n, k in enumerate(EXPR)])
    resolve_anims = "".join(
        f'        <LinearAnimation duration="1" name="Resolve {k.capitalize()}" id="{r_anims[k]}">\n'
        f'            <KeyedObject objectId="{expr_id}"><KeyedProperty property="propertyValue">'
        f'<KeyFrameId value="{EV[k]}" frame="0" interpolationType="hold"/></KeyedProperty></KeyedObject>\n'
        f'        </LinearAnimation>\n' for k in EXPR)

    hit_id = ids()
    I = "            "
    listeners = "".join(
        f'{I}<StateMachineListenerSingle targetId="{hit_id}" listenerTypeValue="{ev}" name="Pointer {ev.capitalize()}" id="{ids()}">\n'
        + set_interaction(val, I + "    ") + f'{I}</StateMachineListenerSingle>\n'
        for ev, val in (("enter", "hover"), ("exit", "none"), ("down", "pressed"), ("up", "hover")))

    mode_kw = {"mode": f"{B}-{P_MODE_BADI}"} if mode_bound else {"mode_fixed": MV["compact"]}
    char = placement("0:3800", "BADI", ids(), "        ", ids, char_attrs,
                     values=pv(expr_id=expr_id, audio=f"{B}-{P_AUDIO_BADI}", inter=f"{B}-{P_INTERACTION}", **mode_kw))
    hx, hy, hw, hh = hit
    return f'''<Rive version="1" kind="fragment">
    <!--
        {comment}
        GENERATED by tools/gen_expressions.py.
        BADIController: bound to the BADI view model.
          Resolver layer  - isDizzy > isAnnoyed > isSpeaking > isListening > isThinking > state
                            picks the expression and keys it onto the Character.
          Listeners       - pointer enter/exit/down/up over BADI write BADI.interaction.
    -->
    <Artboard defaultStateMachineId="{sm_id}" styleId="{style_id}" viewModelId="{VM_BADI}" viewModelInstanceId="{I_BADI}"
              x="{x}" y="0" width="{w}" height="{h}" name="{name}" id="{ab_id}">
        <LayoutComponentStyle name="Artboard Style" id="{style_id}"/>

        <!-- No background: the hosts are transparent so BADI sits on whatever the app draws. -->

        <StateMachine name="BADIController" id="{sm_id}">
{listeners}{resolver}        </StateMachine>

{char}
        <!-- Pointer hit area over the body (transparent, still hit-testable). -->
        <Shape x="{hx}" y="{hy}" name="Hit Area" id="{hit_id}">
            <Ellipse width="{hw}" height="{hh}" name="Path"/>
            <Fill name="Fill"><SolidColor colorValue="00FFFFFF" name="Color"/></Fill>
        </Shape>

{resolve_anims}    </Artboard>
</Rive>
'''


def build_hosts():
    (ROOT / "artboards/expanded.rml").write_text(
        host("Expanded", "0:2", "0:5", "0:7", "0:8", 0, 360, 360, Ids(4000), ' x="180" y="180"',
             (180, 168, 236, 244), True,
             "BADI — Expanded (Full Body), 360 x 360. Main window / interaction. mode follows BADI.mode."),
        encoding="utf-8")
    (ROOT / "artboards/compact.rml").write_text(
        host("Compact", "0:300", "0:305", "0:307", "0:308", 400, 128, 128, Ids(4200), ' x="64" y="69" scaleX="0.39" scaleY="0.39"',
             (64, 64, 96, 98), False,
             "BADI — Compact (Face Only), 128 x 128. Notch / tray / small UI. Always compact mode."),
        encoding="utf-8")


def build_sheet():
    ids = Ids(4400)
    cells = []
    for n, k in enumerate(EXPR):
        col, row = n % 6, n // 6
        x = 100 + col * 200 + (100 if row else 0)
        y = 130 + row * 220
        cells.append(f'        <!-- {n + 1}. {k.capitalize()} -->\n' +
                     placement("0:3800", k.capitalize(), ids(), "        ", ids,
                               f' x="{x}" y="{y}" scaleX="0.5" scaleY="0.5"', values=pv(fixed=EV[k], mode_fixed=MV["expanded"])))
    doc = f'''<Rive version="1" kind="fragment">
    <!--
        Expression sheet: all 11 expressions, each a Character placement with a
        fixed Pose.expression (expanded mode). Reference only. GENERATED by tools/gen_expressions.py.
    -->
    <Artboard styleId="0:4399" defaultStateMachineId="0:4397" x="0" y="1100" width="1300" height="480" name="ExpressionSheet" id="0:4390">
        <LayoutComponentStyle name="Artboard Style" id="0:4399"/>
        <Fill name="Background"><SolidColor colorValue="FFF5F6FB" name="Color"/></Fill>

        <StateMachine name="Sheet SM" id="0:4397">
            <StateMachineLayer name="Base" id="0:4398">
                <AnyState x="400" y="0"/>
                <ExitState x="560" y="0"/>
                <EntryState x="0" y="0"/>
            </StateMachineLayer>
        </StateMachine>

{"".join(cells)}    </Artboard>
</Rive>
'''
    (ROOT / "artboards/expression_sheet.rml").write_text(doc, encoding="utf-8")


def build_data():
    def enum(name, eid, keys, ids_):
        vals = "".join(f'        <DataEnumValue key="{k}" value="{k.capitalize()}" id="{ids_[k]}"/>\n' for k in keys)
        return f'    <DataEnumCustom name="{name}" id="{eid}">\n{vals}    </DataEnumCustom>\n'

    doc = f'''<Rive version="1" kind="fragment">
    <!--
        Data. GENERATED by tools/gen_expressions.py.
          BADI - the view model the host app drives (BADIController's inputs).
          Pose - what every component reads; Character passes its values down.
    -->
{enum("Expression", ENUM_ID, EXPR, EV)}
{enum("Mode", MODE_ENUM, MODES, MV)}
{enum("Interaction", INTER_ENUM, INTERS, IV)}
    <ViewModel defaultInstanceId="{I_BADI}" name="BADI" id="{VM_BADI}">
        <ViewModelPropertyEnumCustom enumId="{MODE_ENUM}" name="mode" id="{P_MODE_BADI}"/>
        <ViewModelPropertyEnumCustom enumId="{ENUM_ID}" name="state" id="{P_STATE}"/>
        <ViewModelPropertyBoolean name="isListening" id="{P_IS_LISTENING}"/>
        <ViewModelPropertyBoolean name="isThinking" id="{P_IS_THINKING}"/>
        <ViewModelPropertyBoolean name="isSpeaking" id="{P_IS_SPEAKING}"/>
        <ViewModelPropertyNumber name="audioLevel" id="{P_AUDIO_BADI}"/>
        <ViewModelPropertyEnumCustom enumId="{INTER_ENUM}" name="interaction" id="{P_INTERACTION}"/>
        <ViewModelPropertyBoolean name="isAnnoyed" id="{P_IS_ANNOYED}"/>
        <ViewModelPropertyBoolean name="isDizzy" id="{P_IS_DIZZY}"/>
        <ViewModelInstance exports="true" name="Default" id="{I_BADI}">
            <ViewModelInstanceEnum propertyValue="{MV["expanded"]}" viewModelPropertyId="{P_MODE_BADI}"/>
            <ViewModelInstanceEnum propertyValue="{EV["idle"]}" viewModelPropertyId="{P_STATE}"/>
            <ViewModelInstanceBoolean propertyValue="false" viewModelPropertyId="{P_IS_LISTENING}"/>
            <ViewModelInstanceBoolean propertyValue="false" viewModelPropertyId="{P_IS_THINKING}"/>
            <ViewModelInstanceBoolean propertyValue="false" viewModelPropertyId="{P_IS_SPEAKING}"/>
            <ViewModelInstanceNumber propertyValue="0" viewModelPropertyId="{P_AUDIO_BADI}"/>
            <ViewModelInstanceEnum propertyValue="{IV["none"]}" viewModelPropertyId="{P_INTERACTION}"/>
            <ViewModelInstanceBoolean propertyValue="false" viewModelPropertyId="{P_IS_ANNOYED}"/>
            <ViewModelInstanceBoolean propertyValue="false" viewModelPropertyId="{P_IS_DIZZY}"/>
        </ViewModelInstance>
    </ViewModel>

    <ViewModel defaultInstanceId="{I_POSE}" name="Pose" id="{VM_POSE}">
        <ViewModelPropertyEnumCustom componentProps="1" enumId="{ENUM_ID}" name="expression" id="{P_EXPR}"/>
        <ViewModelPropertyNumber componentProps="1" name="audioLevel" id="{P_AUDIO}"/>
        <ViewModelPropertyEnumCustom componentProps="1" enumId="{INTER_ENUM}" name="interaction" id="{P_INTER}"/>
        <ViewModelPropertyEnumCustom componentProps="1" enumId="{MODE_ENUM}" name="mode" id="{P_MODE}"/>
        <ViewModelInstance exports="true" name="Default" id="{I_POSE}">
            <ViewModelInstanceEnum propertyValue="{EV["idle"]}" viewModelPropertyId="{P_EXPR}"/>
            <ViewModelInstanceNumber propertyValue="0" viewModelPropertyId="{P_AUDIO}"/>
            <ViewModelInstanceEnum propertyValue="{IV["none"]}" viewModelPropertyId="{P_INTER}"/>
            <ViewModelInstanceEnum propertyValue="{MV["expanded"]}" viewModelPropertyId="{P_MODE}"/>
        </ViewModelInstance>
    </ViewModel>

    <!-- audioLevel arrives as 0..1; blend states run 0..100. -->
    <DataConverterRangeMapper minInput="0" maxInput="1" minOutput="0" maxOutput="100"
                              clampLower="true" clampUpper="true" name="Audio To Blend" id="{AUDIO_TO_BLEND}"/>
</Rive>
'''
    (ROOT / "data").mkdir(exist_ok=True)
    (ROOT / "data/badi_data.rml").write_text(doc, encoding="utf-8")


if __name__ == "__main__":
    build_data()
    build_face()
    patch_effects()
    build_character()
    build_hosts()
    build_sheet()
    print("generated")
