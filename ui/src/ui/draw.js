// Drawing engine for the overlay: turns SHAPE / DRAW / WRITE visuals into strokes and renders them
// progressively like a marker, so Badi's cursor can ride the pen tip.
(function () {
  const BLUE = '#2b8aff';
  const HAND_FONT = '"Ink Free", "Segoe Print", "Segoe Script", "Comic Sans MS", cursive';

  const dist = (a, b) => Math.hypot(b.x - a.x, b.y - a.y);
  const lerp = (a, b, t) => ({ x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t });

  function sampleEllipse(cx, cy, rx, ry, n = 72) {
    const pts = [];
    for (let i = 0; i <= n; i++) {
      const a = -Math.PI / 2 + (i / n) * Math.PI * 2;
      pts.push({ x: cx + rx * Math.cos(a), y: cy + ry * Math.sin(a) });
    }
    return pts;
  }

  // Catmull-Rom through the given points, for smooth freehand strokes.
  function smooth(points, steps = 8) {
    if (points.length < 3) return points;
    const out = [points[0]];
    for (let i = 0; i < points.length - 1; i++) {
      const p0 = points[i - 1] || points[i];
      const p1 = points[i];
      const p2 = points[i + 1];
      const p3 = points[i + 2] || p2;
      for (let s = 1; s <= steps; s++) {
        const t = s / steps;
        const t2 = t * t;
        const t3 = t2 * t;
        out.push({
          x: 0.5 * (2 * p1.x + (-p0.x + p2.x) * t + (2 * p0.x - 5 * p1.x + 4 * p2.x - p3.x) * t2 + (-p0.x + 3 * p1.x - 3 * p2.x + p3.x) * t3),
          y: 0.5 * (2 * p1.y + (-p0.y + p2.y) * t + (2 * p0.y - 5 * p1.y + 4 * p2.y - p3.y) * t2 + (-p0.y + 3 * p1.y - 3 * p2.y + p3.y) * t3),
        });
      }
    }
    return out;
  }

  function box(pts) {
    if (pts.length === 1) return { x1: pts[0].x - 30, y1: pts[0].y - 30, x2: pts[0].x + 30, y2: pts[0].y + 30 };
    return {
      x1: Math.min(pts[0].x, pts[1].x),
      y1: Math.min(pts[0].y, pts[1].y),
      x2: Math.max(pts[0].x, pts[1].x),
      y2: Math.max(pts[0].y, pts[1].y),
    };
  }

  function arrowHead(from, to, size = 16) {
    const ang = Math.atan2(to.y - from.y, to.x - from.x);
    return [
      [{ x: to.x - size * Math.cos(ang - 0.5), y: to.y - size * Math.sin(ang - 0.5) }, to],
      [{ x: to.x - size * Math.cos(ang + 0.5), y: to.y - size * Math.sin(ang + 0.5) }, to],
    ];
  }

  // Returns { strokes: [[pt...]...], closed: bool }
  function geometry(v) {
    const p = v.points;
    switch (v.shape) {
      case 'line':
        return { strokes: [p.length > 2 ? p : [p[0], p[1] || p[0]]] };
      case 'arrow': {
        const main = p.length >= 2 ? p : [p[0], { x: p[0].x + 60, y: p[0].y }];
        return { strokes: [main, ...arrowHead(main[main.length - 2], main[main.length - 1])] };
      }
      case 'curve': {
        const a = p[0];
        const b = p[p.length - 1];
        const c = p.length >= 3 ? p[1] : { x: (a.x + b.x) / 2 - (b.y - a.y) * 0.3, y: (a.y + b.y) / 2 + (b.x - a.x) * 0.3 };
        const pts = [];
        for (let i = 0; i <= 40; i++) {
          const t = i / 40;
          pts.push({ x: (1 - t) ** 2 * a.x + 2 * (1 - t) * t * c.x + t * t * b.x, y: (1 - t) ** 2 * a.y + 2 * (1 - t) * t * c.y + t * t * b.y });
        }
        return { strokes: [pts, ...arrowHead(pts[pts.length - 3], b)] };
      }
      case 'circle': {
        const r = p.length >= 2 ? dist(p[0], p[1]) : 40;
        return { strokes: [sampleEllipse(p[0].x, p[0].y, r, r)], closed: true };
      }
      case 'ellipse': {
        const b = box(p);
        return { strokes: [sampleEllipse((b.x1 + b.x2) / 2, (b.y1 + b.y2) / 2, (b.x2 - b.x1) / 2, (b.y2 - b.y1) / 2)], closed: true };
      }
      case 'rect': {
        const b = box(p);
        return { strokes: [[{ x: b.x1, y: b.y1 }, { x: b.x2, y: b.y1 }, { x: b.x2, y: b.y2 }, { x: b.x1, y: b.y2 }, { x: b.x1, y: b.y1 }]], closed: true };
      }
      case 'triangle': {
        let t = p;
        if (p.length < 3) {
          const b = box(p);
          t = [{ x: (b.x1 + b.x2) / 2, y: b.y1 }, { x: b.x2, y: b.y2 }, { x: b.x1, y: b.y2 }];
        }
        return { strokes: [[t[0], t[1], t[2], t[0]]], closed: true };
      }
      case 'polygon':
        return { strokes: [p.length > 2 ? [...p, p[0]] : p], closed: p.length > 2 };
      case 'star': {
        const c = p[0];
        const R = p.length >= 2 ? dist(p[0], p[1]) : 40;
        const pts = [];
        for (let i = 0; i <= 10; i++) {
          const a = -Math.PI / 2 + (i * Math.PI) / 5;
          const r = i % 2 ? R * 0.45 : R;
          pts.push({ x: c.x + r * Math.cos(a), y: c.y + r * Math.sin(a) });
        }
        return { strokes: [pts], closed: true };
      }
      case 'check': {
        const b = box(p);
        const w = b.x2 - b.x1;
        const h = b.y2 - b.y1;
        return { strokes: [[{ x: b.x1, y: b.y1 + h * 0.55 }, { x: b.x1 + w * 0.38, y: b.y2 }, { x: b.x2, y: b.y1 }]] };
      }
      case 'cross': {
        const b = box(p);
        return { strokes: [[{ x: b.x1, y: b.y1 }, { x: b.x2, y: b.y2 }], [{ x: b.x2, y: b.y1 }, { x: b.x1, y: b.y2 }]] };
      }
      default:
        return { strokes: [p] };
    }
  }

  function strokeLength(s) {
    let l = 0;
    for (let i = 1; i < s.length; i++) l += dist(s[i - 1], s[i]);
    return l;
  }

  let measureCtx = null;
  function measure(text, size) {
    measureCtx = measureCtx || document.createElement('canvas').getContext('2d');
    measureCtx.font = `${size}px ${HAND_FONT}`;
    return measureCtx.measureText(text).width;
  }

  // Precomputes strokes, length and duration. Coordinates stay global.
  function prepare(v) {
    v.color = v.color || BLUE;
    if (v.type === 'write') {
      v.lines = v.text.split(/\\n|\n/);
      v.lineWidths = v.lines.map((l) => measure(l, v.size));
      const chars = v.lines.join('').length;
      v.dur = Math.min(4000, 250 + chars * 75);
      v.start = { x: v.x, y: v.y };
      return v;
    }
    const g = v.type === 'draw' ? { strokes: [smooth(v.points)] } : geometry(v);
    v.strokes = g.strokes.filter((s) => s.length >= 2);
    v.closed = Boolean(g.closed);
    v.lengths = v.strokes.map(strokeLength);
    v.len = v.lengths.reduce((a, b) => a + b, 0) || 1;
    v.dur = Math.min(2600, 300 + v.len * 1.1);
    v.start = v.strokes[0]?.[0] || v.points[0];
    return v;
  }

  // Draws v at progress t (0..1). Returns the pen tip (global coords) while drawing.
  function render(g, v, t, local) {
    g.save();
    g.lineCap = 'round';
    g.lineJoin = 'round';
    g.strokeStyle = v.color;
    g.fillStyle = v.color;
    let tip = null;

    if (v.type === 'write') {
      const size = v.size;
      g.font = `${size}px ${HAND_FONT}`;
      g.textBaseline = 'top';
      const total = v.lineWidths.reduce((a, b) => a + b, 0) || 1;
      let budget = total * t;
      v.lines.forEach((line, i) => {
        const w = v.lineWidths[i];
        const shown = Math.max(0, Math.min(w, budget));
        budget -= w;
        if (shown <= 0) return;
        const p = local({ x: v.x, y: v.y + i * size * 1.25 });
        g.save();
        g.beginPath();
        g.rect(p.x - size, p.y - size, shown + size, size * 3);
        g.clip();
        g.lineWidth = Math.max(3, size * 0.14);
        g.strokeStyle = 'rgba(255,255,255,.9)';
        g.strokeText(line, p.x, p.y);
        g.fillText(line, p.x, p.y);
        g.restore();
        if (shown < w) tip = { x: v.x + shown, y: v.y + i * size * 1.25 + size * 0.75 };
      });
      g.restore();
      return t < 1 ? tip : null;
    }

    const width = v.width || (v.type === 'draw' ? 4 : 3.5);
    let budget = v.len * t;
    // White halo first so strokes stay readable on dark and light backgrounds.
    for (const pass of ['halo', 'ink']) {
      let b = budget;
      g.lineWidth = pass === 'halo' ? width + 3 : width;
      g.strokeStyle = pass === 'halo' ? 'rgba(255,255,255,.75)' : v.color;
      v.strokes.forEach((s, si) => {
        if (b <= 0) return;
        g.beginPath();
        const p0 = local(s[0]);
        g.moveTo(p0.x, p0.y);
        let walked = 0;
        for (let i = 1; i < s.length; i++) {
          const seg = dist(s[i - 1], s[i]);
          if (walked + seg >= b) {
            const end = lerp(s[i - 1], s[i], (b - walked) / (seg || 1));
            const pe = local(end);
            g.lineTo(pe.x, pe.y);
            if (pass === 'ink') tip = end;
            walked = b;
            break;
          }
          walked += seg;
          const pi = local(s[i]);
          g.lineTo(pi.x, pi.y);
        }
        g.stroke();
        b -= v.lengths[si];
      });
    }

    if (t >= 1 && v.closed && v.fill) {
      // Small filled shapes (eyes, dots, bullets) read as solid; large ones get a light tint.
      const xs = v.strokes[0].map((q) => q.x);
      const ys = v.strokes[0].map((q) => q.y);
      const size = Math.max(Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys));
      g.globalAlpha = size < 48 ? 1 : 0.22;
      g.beginPath();
      v.strokes[0].forEach((pt, i) => {
        const q = local(pt);
        i ? g.lineTo(q.x, q.y) : g.moveTo(q.x, q.y);
      });
      g.closePath();
      g.fill();
    }
    g.restore();
    return t < 1 ? tip : null;
  }

  window.Sketch = { prepare, render, BLUE };
})();
