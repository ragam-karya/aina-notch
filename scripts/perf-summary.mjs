// Summarizes one perf-mascot.ps1 run: process.csv (CPU/memory/GPU by process type), renderer.csv (the
// overlay page's own frames, heap and Rive counters, from perf_probe.js) and markers.csv (when each
// scenario ran). Writes summary.json and summary.md next to them.
//   node scripts/perf-summary.mjs perf/A [perf/B perf/C]   # several runs: one comparison table
import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join, basename } from 'node:path';

const readCsv = (file) => {
  if (!existsSync(file)) return [];
  const [head, ...lines] = readFileSync(file, 'utf8').trim().split(/\r?\n/);
  const keys = head.split(',');
  return lines.filter(Boolean).map((l) => Object.fromEntries(l.split(',').map((v, i) => [keys[i], v])));
};
const num = (v) => (v === '' || v == null ? NaN : Number(v));
const vals = (rows, k) => rows.map((r) => num(r[k])).filter((x) => Number.isFinite(x));
const mean = (a) => (a.length ? a.reduce((s, x) => s + x, 0) / a.length : NaN);
const max = (a) => (a.length ? Math.max(...a) : NaN);
const min = (a) => (a.length ? Math.min(...a) : NaN);
const sum = (a) => a.reduce((s, x) => s + x, 0);
const median = (a) => {
  if (!a.length) return NaN;
  const s = [...a].sort((x, y) => x - y);
  return s[Math.floor(s.length / 2)];
};
// Least-squares slope per minute of (t ms, v) points.
const slope = (pts) => {
  if (pts.length < 3) return NaN;
  const xs = pts.map((p) => p[0] / 60000);
  const ys = pts.map((p) => p[1]);
  const mx = mean(xs);
  const my = mean(ys);
  const d = sum(xs.map((x) => (x - mx) ** 2));
  return d ? sum(xs.map((x, i) => (x - mx) * (ys[i] - my))) / d : NaN;
};
const f = (x, d = 2) => (Number.isFinite(x) ? x.toFixed(d) : '–');

function summarize(dir) {
  const markers = readCsv(join(dir, 'markers.csv'));
  const proc = readCsv(join(dir, 'process.csv'));
  const rend = readCsv(join(dir, 'renderer.csv'));
  const warm = rend.filter((r) => r.scenario === 'warmup');
  const init = [...new Set(warm.map((r) => r.rive_init_ms).filter(Boolean))].join(' / ');
  const out = { run: basename(dir), riveInitMs: init, scenarios: {} };
  for (const m of markers) {
    const a = num(m.start_ms);
    const b = num(m.end_ms);
    const p = proc.filter((r) => num(r.t) >= a && num(r.t) <= b);
    // Totals per sample time across process types.
    const byT = new Map();
    for (const r of p) {
      const t = num(r.t);
      const s = byT.get(t) || { cpu: 0, ws: 0, priv: 0, gpu: 0, gmem: 0 };
      s.cpu += num(r.cpu_pct) || 0;
      s.ws += num(r.ws_mb) || 0;
      s.priv += num(r.private_mb) || 0;
      s.gpu += num(r.gpu3d_pct) || 0;
      s.gmem += num(r.gpu_dedicated_mb) || 0;
      byT.set(t, s);
    }
    const T = [...byT.entries()].sort((x, y) => x[0] - y[0]);
    // The first sample of a scenario still holds CPU from before it; leave it out of the CPU mean.
    const cpuT = T.slice(1).map(([, s]) => s.cpu);
    const types = {};
    for (const type of [...new Set(p.map((r) => r.type))]) {
      const rows = p.filter((r) => r.type === type).slice(1);
      types[type] = { cpu: mean(vals(rows, 'cpu_pct')), cpuMax: max(vals(rows, 'cpu_pct')), ws: mean(vals(rows, 'ws_mb')), gpu: mean(vals(rows, 'gpu3d_pct')) };
    }
    const r = rend.filter((x) => x.scenario === m.scenario);
    const overlays = {};
    for (const label of [...new Set(r.map((x) => x.overlay))]) {
      const rows = r.filter((x) => x.overlay === label);
      const heap = rows.map((x) => [num(x.t), num(x.heap_mb)]).filter((x) => Number.isFinite(x[1]));
      const busy = rows.filter((x) => num(x.frames) > 0);
      overlays[label] = {
        fps: mean(vals(rows, 'fps')),
        fpsBusy: mean(vals(busy, 'fps')),
        intP50: median(vals(busy, 'int_p50')),
        intP95: median(vals(busy, 'int_p95')),
        intP99Max: max(vals(busy, 'int_p99')),
        workP50: median(vals(busy, 'work_p50')),
        workP95: median(vals(busy, 'work_p95')),
        workMax: max(vals(busy, 'work_max')),
        late25: sum(vals(rows, 'late25')),
        late50: sum(vals(rows, 'late50')),
        longTasks: sum(vals(rows, 'long_n')),
        heapFirst: heap[0]?.[1],
        heapLast: heap[heap.length - 1]?.[1],
        heapMax: max(heap.map((x) => x[1])),
        heapSlope: slope(heap),
        domFirst: num(rows[0]?.dom),
        domLast: num(rows[rows.length - 1]?.dom),
        riveNew: num(rows[rows.length - 1]?.rive_new),
        riveLiveMax: max(vals(rows, 'rive_live')),
        listenersFirst: num(rows[0]?.listeners),
        listenersLast: num(rows[rows.length - 1]?.listeners),
        resizeObs: num(rows[rows.length - 1]?.resize_obs),
        hiddenWindows: rows.filter((x) => x.vis === 'hidden').length,
        windows: rows.length,
      };
    }
    out.scenarios[m.scenario] = {
      minutes: (b - a) / 60000,
      cpu: mean(cpuT),
      cpuP95: (() => {
        const s = [...cpuT].sort((x, y) => x - y);
        return s.length ? s[Math.min(s.length - 1, Math.floor(0.95 * s.length))] : NaN;
      })(),
      gpu3d: mean(T.map(([, s]) => s.gpu)),
      gpu3dMax: max(T.map(([, s]) => s.gpu)),
      gpuMem: mean(T.map(([, s]) => s.gmem)),
      wsFirst: T[0]?.[1].ws,
      wsLast: T[T.length - 1]?.[1].ws,
      wsMax: max(T.map(([, s]) => s.ws)),
      wsSlope: slope(T.map(([t, s]) => [t, s.ws])),
      privLast: T[T.length - 1]?.[1].priv,
      privSlope: slope(T.map(([t, s]) => [t, s.priv])),
      types,
      overlays,
    };
  }
  return out;
}

const dirs = process.argv.slice(2);
if (!dirs.length) {
  console.error('usage: node scripts/perf-summary.mjs <run dir> [more run dirs]');
  process.exit(1);
}
const runs = dirs.map(summarize);
const primaryOf = (s) => {
  // The overlay with the notch is the one that drew the most during the run.
  const o = Object.entries(s.overlays);
  return o.sort((x, y) => (y[1].fpsBusy || 0) * (y[1].windows - y[1].hiddenWindows) - (x[1].fpsBusy || 0) * (x[1].windows - x[1].hiddenWindows))[0]?.[1] || {};
};
const md = [];
const names = [...new Set(runs.flatMap((r) => Object.keys(r.scenarios)))].filter((n) => !n.startsWith('cool-'));
md.push(`Runs: ${runs.map((r) => `**${r.run}** (Rive init ms: ${r.riveInitMs || '–'})`).join(', ')}`, '');
for (const n of names) {
  md.push(`### ${n}`, '');
  md.push('| metric | ' + runs.map((r) => r.run).join(' | ') + ' |', '|---|' + runs.map(() => '---:').join('|') + '|');
  const row = (label, get) => md.push(`| ${label} | ${runs.map((r) => (r.scenarios[n] ? get(r.scenarios[n], primaryOf(r.scenarios[n])) : '–')).join(' | ')} |`);
  row('duration (min)', (s) => f(s.minutes, 1));
  row('CPU total % (mean / p95)', (s) => `${f(s.cpu)} / ${f(s.cpuP95)}`);
  row('CPU renderer %', (s) => f(s.types.renderer?.cpu));
  row('CPU gpu-process %', (s) => f(s.types['gpu-process']?.cpu));
  row('CPU app (badi.exe) %', (s) => f(s.types.app?.cpu));
  row('GPU 3D % (mean / max)', (s) => `${f(s.gpu3d)} / ${f(s.gpu3dMax)}`);
  row('GPU dedicated MB', (s) => f(s.gpuMem, 1));
  row('working set MB (first → last, max)', (s) => `${f(s.wsFirst, 0)} → ${f(s.wsLast, 0)} (${f(s.wsMax, 0)})`);
  row('working set slope MB/min', (s) => f(s.wsSlope, 3));
  row('private MB last / slope MB/min', (s) => `${f(s.privLast, 0)} / ${f(s.privSlope, 3)}`);
  row('overlay fps (all / while drawing)', (s, o) => `${f(o.fps, 1)} / ${f(o.fpsBusy, 1)}`);
  row('frame interval ms p50 / p95 / p99 max', (s, o) => `${f(o.intP50)} / ${f(o.intP95)} / ${f(o.intP99Max)}`);
  row('frame JS work ms p50 / p95 / max', (s, o) => `${f(o.workP50)} / ${f(o.workP95)} / ${f(o.workMax)}`);
  row('late frames >25 ms / >50 ms', (s, o) => `${o.late25 ?? '–'} / ${o.late50 ?? '–'}`);
  row('long tasks', (s, o) => `${o.longTasks ?? '–'}`);
  row('JS heap MB (first → last, max)', (s, o) => `${f(o.heapFirst)} → ${f(o.heapLast)} (${f(o.heapMax)})`);
  row('JS heap slope MB/min', (s, o) => f(o.heapSlope, 4));
  row('DOM nodes first → last', (s, o) => `${o.domFirst ?? '–'} → ${o.domLast ?? '–'}`);
  row('Rive instances created (total) / alive max', (s, o) => `${o.riveNew ?? '–'} / ${o.riveLiveMax ?? '–'}`);
  row('listeners first → last / ResizeObservers', (s, o) => `${o.listenersFirst ?? '–'} → ${o.listenersLast ?? '–'} / ${o.resizeObs ?? '–'}`);
  row('probe windows hidden / total', (s, o) => `${o.hiddenWindows ?? '–'} / ${o.windows ?? '–'}`);
  md.push('');
}
const text = md.join('\n');
for (const r of runs) writeFileSync(join(dirs[runs.indexOf(r)], 'summary.json'), JSON.stringify(r, null, 2));
writeFileSync(join(dirs[dirs.length - 1], runs.length > 1 ? 'compare.md' : 'summary.md'), text);
console.log(text);
