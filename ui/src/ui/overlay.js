// Full-screen, click-through overlay for one display. All positions arrive in global DIP
// coordinates; this window subtracts its own origin.
const params = new URLSearchParams(location.search);
const origin = { x: +params.get('x') || 0, y: +params.get('y') || 0 };

const canvas = document.getElementById('c');
const g = canvas.getContext('2d');

const BLUE = '#2b8aff';
const S = {
  phase: 'idle',
  mode: 'companion',
  mouse: null,
  aina: null, // where aina's cursor is, {x,y} global
  flight: null, // {from,to,start,dur}
  parked: false,
  level: 0,
  visuals: [],
  text: '',
  transcript: '',
  error: '',
  fadeAt: 0,
  alpha: 0,
  drawEnd: 0, // when the queued drawings finish (performance.now time)
  pendingAnchor: null,
  penTip: null,
  stepEnds: new Map(), // stepId -> time its last drawing finishes
};

function resize() {
  canvas.width = innerWidth * devicePixelRatio;
  canvas.height = innerHeight * devicePixelRatio;
  canvas.style.width = innerWidth + 'px';
  canvas.style.height = innerHeight + 'px';
}
addEventListener('resize', resize);
resize();

const local = (p) => ({ x: p.x - origin.x, y: p.y - origin.y });
const TRAIL = 14; // aina's cursor rides this far below-right of the mouse
const ease = (t) => (t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2);

function flyTo(target) {
  const from = S.aina || (S.mouse ? { x: S.mouse.x + TRAIL, y: S.mouse.y + TRAIL } : target);
  const dist = Math.hypot(target.x - from.x, target.y - from.y);
  S.flight = { from, to: target, start: performance.now(), dur: Math.min(900, 300 + dist * 0.5) };
  S.parked = true;
}

// The frame loop sleeps while nothing is on screen (aina idles in the background for hours);
// every event from main wakes it.
let raf = 0;
function wake() {
  if (!raf) raf = requestAnimationFrame(frame);
}
const on = (channel, fn) =>
  aina.on(channel, (payload) => {
    fn(payload);
    wake();
  });

// ---- events from main ----
on('phase', (p) => {
  S.phase = p;
  // The scan lasts while aina looks and thinks; the answer (or a new question) ends it.
  if (p !== 'looking' && p !== 'thinking' && p !== 'transcribing') S.scan = null;
  // The user's mark has done its job once aina answers or acts: it shouldn't keep covering the screen.
  if (p === 'speaking' || p === 'guiding' || p === 'acting') {
    S.ink = [];
    S.gestures = [];
    S.dwell = null;
  }
  if (p === 'listening') {
    S.visuals = [];
    S.drawEnd = 0;
    S.pendingAnchor = null;
    S.text = '';
    S.error = '';
    S.transcript = '';
    S.parked = false;
    S.flight = null;
  }
  if (p !== 'idle') {
    S.fadeAt = 0;
    S.alpha = 1;
  } else hideIfEmpty();
});
// Back to idle with nothing left to show or fade (Esc, a cancelled turn): aina's cursor goes away
// instead of standing still where the mouse last was.
function hideIfEmpty() {
  if (S.phase !== 'idle' || S.fadeAt || S.visuals.length || S.text || S.error) return;
  S.aina = null;
  S.alpha = 0;
  S.parked = false;
  S.flight = null;
}
on('mode', (m) => (S.mode = m));
on('mouse', (p) => {
  S.mouse = p;
  if (!S.aina) S.aina = { x: p.x + TRAIL, y: p.y + TRAIL };
});
on('level', (l) => (S.level = S.level * 0.5 + l * 0.5));
on('transcript', (t) => {
  S.transcript = t;
});
on('text', (t) => {
  S.text = t;
  S.alpha = 1;
  S.fadeAt = 0;
});
on('error', (e) => {
  S.error = e;
  S.alpha = 1;
});
const isDrawing = (v) => v.type === 'shape' || v.type === 'draw' || v.type === 'write';

// Drawings are queued one after another; the cursor flies to the first stroke, then rides the pen.
on('visual', (v) => {
  const now = performance.now();
  v.born = now;
  S.alpha = 1;
  S.fadeAt = 0;
  if (isDrawing(v)) {
    Sketch.prepare(v);
    if (S.drawEnd > now) {
      v.begin = S.drawEnd + 120;
    } else {
      flyTo(v.start);
      v.begin = now + S.flight.dur;
    }
    S.drawEnd = v.begin + v.dur;
    if (v.stepId) S.stepEnds.set(v.stepId, Math.max(S.stepEnds.get(v.stepId) || 0, S.drawEnd));
    S.visuals.push(v);
    return;
  }
  S.visuals.push(v);
  const anchor = v.type === 'highlight' ? { x: v.x + v.w / 2, y: v.y + v.h / 2 } : { x: v.x, y: v.y };
  // Don't yank the cursor off a drawing in progress; fly there once the pen is done.
  if (S.drawEnd > now) S.pendingAnchor = anchor;
  else flyTo(anchor);
});
on('clear', () => {
  S.visuals = [];
  S.ink = [];
  S.gestures = [];
  S.dwell = null;
  S.scan = null;
  S.drawEnd = 0;
  S.pendingAnchor = null;
  S.text = '';
  S.error = '';
  S.transcript = '';
  S.parked = false;
  S.flight = null;
  hideIfEmpty();
});
on('linger', (ms) => (S.fadeAt = performance.now() + ms));

// ---- marking while talking (no click) ----
// Main streams the cursor trail while the user talks: it is drawn as ink that follows the cursor at
// once. When main recognizes a gesture, the ink becomes a clean shape: a springy ellipse for a circle,
// a line for an underline, a dot for pointing (after a ring fills while the cursor holds still).
// Orange is the user's color. While aina then looks and thinks, its cursor flies there and scans it.
const ORANGE = '#ff8a3d';
const INK_KEEP = 160; // trail points drawn
S.ink = [];
S.gestures = [];
S.dwell = null; // { x, y, start, ms } ring filling while the cursor holds still
S.scan = null; // { x1, y1, x2, y2, born } area being looked at
on('ink', (p) => {
  if (p.reset) {
    S.ink = [];
    S.gestures = [];
    S.dwell = null;
    S.scan = null;
    return;
  }
  if ('dwell' in p) {
    S.dwell = p.dwell ? { ...p.dwell, start: performance.now() } : null;
    return;
  }
  S.alpha = 1;
  S.ink.push({ x: p.x, y: p.y });
  if (S.ink.length > INK_KEEP) S.ink.shift();
});
on('gesture', (g) => {
  S.alpha = 1;
  S.dwell = null;
  S.gestures = [{ ...g, born: performance.now() }]; // the newest mark replaces the previous one
  if (g.type !== 'point') S.ink = [];
});
on('scan', (box) => {
  S.scan = box ? { ...box, born: performance.now() } : null;
  if (box) flyTo({ x: (box.x1 + box.x2) / 2, y: (box.y1 + box.y2) / 2 });
});

const springBack = (t) => (t >= 1 ? 1 : 1 + 0.12 * Math.exp(-6 * t) * Math.sin(t * 14) * (1 - t) - 0.1 * (1 - t) ** 3);

function drawInk(alpha) {
  const pts = S.ink;
  if (pts.length < 2) return;
  g.save();
  g.strokeStyle = ORANGE;
  g.lineCap = 'round';
  g.lineJoin = 'round';
  for (let i = 1; i < pts.length; i++) {
    const a = local(pts[i - 1]);
    const b = local(pts[i]);
    g.globalAlpha = alpha * (0.15 + 0.85 * (i / pts.length)); // older ink fades
    g.lineWidth = 2 + 3 * (i / pts.length);
    g.beginPath();
    g.moveTo(a.x, a.y);
    g.lineTo(b.x, b.y);
    g.stroke();
  }
  g.restore();
}

function drawGesture(m, now, alpha) {
  const t = Math.min(1, (now - m.born) / 380);
  g.save();
  g.strokeStyle = ORANGE;
  g.fillStyle = ORANGE;
  g.lineWidth = 4;
  g.lineCap = 'round';
  if (m.type === 'circle') {
    const c = local({ x: (m.x1 + m.x2) / 2, y: (m.y1 + m.y2) / 2 });
    const k = springBack(t);
    const rx = ((m.x2 - m.x1) / 2 + 6) * k;
    const ry = ((m.y2 - m.y1) / 2 + 6) * k;
    g.globalAlpha = alpha;
    g.beginPath();
    g.ellipse(c.x, c.y, Math.max(1, rx), Math.max(1, ry), 0, 0, Math.PI * 2);
    g.stroke();
    g.globalAlpha = alpha * 0.1;
    g.fill();
  } else if (m.type === 'underline') {
    const a = local({ x: m.x1, y: m.y2 + 4 });
    const b = local({ x: m.x1 + (m.x2 - m.x1) * Math.min(1, t * 1.6), y: m.y2 + 4 });
    g.globalAlpha = alpha;
    g.beginPath();
    g.moveTo(a.x, a.y);
    g.lineTo(b.x, b.y);
    g.stroke();
  } else if (m.type === 'point') {
    const p = local(m);
    g.globalAlpha = alpha;
    g.beginPath();
    g.arc(p.x, p.y, 5 + 2 * springBack(t), 0, Math.PI * 2);
    g.fill();
    g.globalAlpha = alpha * (1 - t) * 0.6;
    g.lineWidth = 3;
    g.beginPath();
    g.arc(p.x, p.y, 8 + 22 * t, 0, Math.PI * 2);
    g.stroke();
  }
  g.restore();
}

// The ring that fills while the cursor holds still; full = pointing.
function drawDwell(now) {
  const d = S.dwell;
  if (!d) return;
  const p = Math.min(1, (now - d.start) / Math.max(50, d.ms));
  const q = local(d);
  g.save();
  g.strokeStyle = ORANGE;
  g.lineWidth = 3;
  g.globalAlpha = 0.9;
  g.beginPath();
  g.arc(q.x, q.y, 14, -Math.PI / 2, -Math.PI / 2 + p * Math.PI * 2);
  g.stroke();
  g.restore();
}

// aina looking at the marked area: a soft outline with a band of light sweeping down it.
function drawScan(now) {
  const s = S.scan;
  if (!s) return;
  const a = local({ x: s.x1 - 8, y: s.y1 - 8 });
  const w = s.x2 - s.x1 + 16;
  const h = s.y2 - s.y1 + 16;
  const u = ((now - s.born) % 1200) / 1200;
  g.save();
  g.strokeStyle = BLUE;
  g.globalAlpha = 0.6;
  g.lineWidth = 2;
  g.setLineDash([6, 6]);
  g.lineDashOffset = -now / 30;
  g.strokeRect(a.x, a.y, w, h);
  g.setLineDash([]);
  const y = a.y + u * h;
  const band = g.createLinearGradient(0, y - 18, 0, y + 18);
  band.addColorStop(0, 'rgba(43,138,255,0)');
  band.addColorStop(0.5, 'rgba(43,138,255,0.35)');
  band.addColorStop(1, 'rgba(43,138,255,0)');
  g.globalAlpha = 1;
  g.fillStyle = band;
  g.fillRect(a.x, Math.max(a.y, y - 18), w, Math.min(36, a.y + h - Math.max(a.y, y - 18)));
  g.restore();
}

// While aina operates the PC, its cursor flies to each spot it acts on and leaves a ripple.
const ripples = [];
on('actionAt', (a) => {
  S.alpha = 1;
  S.fadeAt = 0;
  flyTo({ x: a.x, y: a.y });
  if (a.kind !== 'move') ripples.push({ x: a.x, y: a.y, born: performance.now() + Math.min(220, S.flight?.dur || 0) });
});

function drawRipples(now) {
  for (let i = ripples.length - 1; i >= 0; i--) {
    const r = ripples[i];
    const t = (now - r.born) / 650;
    if (t > 1) {
      ripples.splice(i, 1);
      continue;
    }
    if (t < 0) continue;
    const p = local(r);
    g.save();
    g.strokeStyle = BLUE;
    g.lineWidth = 3 * (1 - t) + 1;
    g.globalAlpha = 0.9 * (1 - t);
    g.beginPath();
    g.arc(p.x, p.y, 8 + 26 * t, 0, Math.PI * 2);
    g.stroke();
    g.restore();
  }
}

// ---- drawing ----
// aina's pointer is drawn smaller than the real one (about 9×13 px), so the two are never confused.
const POINTER_SCALE = 0.55;
function drawPointer(p, a) {
  g.save();
  g.translate(p.x, p.y);
  g.scale(POINTER_SCALE, POINTER_SCALE);
  g.globalAlpha = a;
  g.shadowColor = 'rgba(43,138,255,.8)';
  g.shadowBlur = 9;
  g.beginPath();
  g.moveTo(0, 0);
  g.lineTo(0, 20);
  g.lineTo(5.5, 15);
  g.lineTo(9.5, 23.5);
  g.lineTo(13, 22);
  g.lineTo(9, 13.5);
  g.lineTo(16, 13.5);
  g.closePath();
  g.fillStyle = BLUE;
  g.fill();
  g.shadowBlur = 0;
  g.lineWidth = 1.5;
  g.strokeStyle = '#fff';
  g.stroke();
  g.restore();
}

function label(text, x, y) {
  if (!text) return;
  g.save();
  g.font = '600 12px Urbanist, "Segoe UI", sans-serif';
  const w = g.measureText(text).width + 14;
  g.fillStyle = BLUE;
  g.beginPath();
  g.roundRect(x, y, w, 22, 11);
  g.fill();
  g.fillStyle = '#fff';
  g.fillText(text, x + 7, y + 15);
  g.restore();
}

// aina's pointing pulse shows where to look while that line is said, then fades instead of staying
// over the screen until the whole reply has lingered out.
const POINT_KEEP = 3500;
const POINT_FADE = 600;

function drawVisual(v, now) {
  const t = Math.min(1, (now - v.born) / 450);
  const pulse = 0.5 + 0.5 * Math.sin(now / 260);
  g.save();
  g.strokeStyle = BLUE;
  g.fillStyle = BLUE;
  g.lineWidth = 3;
  g.lineCap = 'round';
  g.lineJoin = 'round';
  if (v.type === 'highlight') {
    const p = local(v);
    g.globalAlpha = 0.12 * t;
    g.fillRect(p.x, p.y, v.w, v.h);
    g.globalAlpha = t;
    g.setLineDash([10, 6]);
    g.lineDashOffset = -now / 40;
    g.beginPath();
    g.roundRect(p.x - 4, p.y - 4, v.w + 8, v.h + 8, 10);
    g.stroke();
    g.setLineDash([]);
    label(v.label, p.x, p.y - 30);
  } else if (v.type === 'target' || v.type === 'hover') {
    const p = local(v);
    g.globalAlpha = 0.25 + 0.35 * pulse;
    g.beginPath();
    g.arc(p.x, p.y, v.r + 6 * pulse, 0, Math.PI * 2);
    g.stroke();
    g.globalAlpha = 0.12;
    g.beginPath();
    g.arc(p.x, p.y, v.r, 0, Math.PI * 2);
    g.fill();
  } else if (v.type === 'point') {
    const p = local(v);
    const left = 1 - Math.max(0, (now - v.born - POINT_KEEP) / POINT_FADE);
    if (left <= 0) {
      g.restore();
      return;
    }
    g.globalAlpha = 0.3 * pulse * t * left;
    g.beginPath();
    g.arc(p.x, p.y, 16 + 8 * pulse, 0, Math.PI * 2);
    g.fill();
  } else if (isDrawing(v)) {
    if (now < v.begin) {
      g.restore();
      return;
    }
    const p = Math.min(1, (now - v.begin) / v.dur);
    g.restore();
    const tip = Sketch.render(g, v, p, local);
    if (tip) S.penTip = tip;
    if (p >= 1 && v.label && v.type !== 'write') {
      const q = local(v.start);
      label(v.label, q.x + 10, q.y - 30);
    }
    return;
  }
  g.restore();
}

function frame(now) {
  raf = 0;
  g.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
  g.clearRect(0, 0, innerWidth, innerHeight);

  if (S.fadeAt && now > S.fadeAt && S.phase === 'idle') {
    S.alpha = Math.max(0, S.alpha - 0.04);
    if (S.alpha === 0) {
      S.fadeAt = 0;
      S.visuals = [];
      S.drawEnd = 0;
      S.text = '';
      S.error = '';
      S.transcript = '';
      S.parked = false;
      S.aina = null;
      S.ink = [];
      S.gestures = [];
      S.dwell = null;
      S.scan = null;
    }
  }
  if (S.visuals.some((v) => v.type === 'point' && now - v.born > POINT_KEEP + POINT_FADE)) S.visuals = S.visuals.filter((v) => v.type !== 'point' || now - v.born <= POINT_KEEP + POINT_FADE);
  const active = S.phase !== 'idle' || S.alpha > 0 || S.visuals.length || ripples.length || S.ink.length || S.gestures.length || S.dwell || S.scan;
  if (!active) return; // the canvas was just cleared; sleep until the next event
  wake();
  drawRipples(now);
  const markAlpha = S.alpha || 1;
  drawInk(markAlpha);
  for (const m of S.gestures) drawGesture(m, now, markAlpha);
  drawDwell(now);
  drawScan(now);

  // Draw first so the pen tip for this frame is known.
  S.penTip = null;
  g.globalAlpha = S.alpha || 1;
  for (const v of S.visuals) drawVisual(v, now);
  g.globalAlpha = 1;

  // Tell main when a step's drawing is complete so the next sentence can start.
  for (const [id, end] of S.stepEnds) {
    if (now >= end) {
      S.stepEnds.delete(id);
      aina.send('overlay:stepDone', id);
    }
  }

  // aina position: pen tip > flight > parked > trailing the mouse.
  if (S.pendingAnchor && now > S.drawEnd) {
    flyTo(S.pendingAnchor);
    S.pendingAnchor = null;
  }
  if (S.penTip && !S.flight) {
    S.aina = S.penTip;
    S.parked = true;
  } else if (S.flight) {
    const f = S.flight;
    const t = Math.min(1, (now - f.start) / f.dur);
    const e = ease(t);
    const lift = Math.sin(Math.PI * e) * Math.min(120, Math.hypot(f.to.x - f.from.x, f.to.y - f.from.y) * 0.25);
    S.aina = { x: f.from.x + (f.to.x - f.from.x) * e, y: f.from.y + (f.to.y - f.from.y) * e - lift };
    if (t >= 1) S.flight = null;
  } else if (!S.parked && S.mouse) {
    const want = { x: S.mouse.x + TRAIL, y: S.mouse.y + TRAIL };
    S.aina = S.aina ? { x: S.aina.x + (want.x - S.aina.x) * 0.35, y: S.aina.y + (want.y - S.aina.y) * 0.35 } : want;
  }

  if (S.aina) {
    const p = local(S.aina);
    const parkedOnPoint = S.parked && !S.flight;
    const pointVisual = S.visuals.find((v) => !isDrawing(v) && v.type !== 'highlight');
    drawPointer(p, S.alpha || 1);
    if (parkedOnPoint && pointVisual && pointVisual.label) label(pointVisual.label, p.x + 12, p.y + 16);
  }
}
wake();
