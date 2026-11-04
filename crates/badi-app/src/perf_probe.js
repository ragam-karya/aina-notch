// BADI_DEMO=mascot only: measures the overlay page without touching its code. Injected after the shim
// (windows.rs), so it wraps requestAnimationFrame and the Rive runtime before the page's scripts run.
// Every WINDOW ms it sends one summary over perf:sample; demo.rs writes it to renderer.csv.
//   frames      distinct animation frames (callbacks sharing a timestamp are one frame)
//   int_*       frame interval percentiles (ms) between consecutive frames under 1 s apart
//   work_*      JS time spent inside rAF callbacks per frame (Rive advances and draws there)
//   heap_mb     performance.memory.usedJSHeapSize
//   rive_*      Rive instances created / cleaned up / alive, and their load times (new â†’ onLoad)
//   listeners   addEventListener minus removeEventListener (approximate: signal removals aren't seen)
(() => {
  const WINDOW = 5000;
  const P = { frames: 0, ints: [], works: [], longN: 0, longMs: 0, riveNew: 0, riveLive: 0, riveInit: [], adds: 0, removes: 0, ro: 0 };
  window.__badiPerf = P;

  // Frames: Rive and the island both schedule through the global rAF on every call.
  const raf = window.requestAnimationFrame.bind(window);
  let curT = -1;
  let curWork = 0;
  const close = (t) => {
    if (curT >= 0) {
      P.frames++;
      P.works.push(curWork);
      if (t - curT < 1000) P.ints.push(t - curT);
    }
    curT = t;
    curWork = 0;
  };
  window.requestAnimationFrame = (fn) =>
    raf((t) => {
      if (t !== curT) close(t);
      const s = performance.now();
      try {
        fn(t);
      } finally {
        curWork += performance.now() - s;
      }
    });

  try {
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) {
        P.longN++;
        P.longMs += e.duration;
      }
    }).observe({ type: 'longtask', buffered: true });
  } catch {}

  const add = EventTarget.prototype.addEventListener;
  const remove = EventTarget.prototype.removeEventListener;
  EventTarget.prototype.addEventListener = function (...a) {
    P.adds++;
    return add.apply(this, a);
  };
  EventTarget.prototype.removeEventListener = function (...a) {
    P.removes++;
    return remove.apply(this, a);
  };
  const RO = window.ResizeObserver;
  if (RO) {
    window.ResizeObserver = class extends RO {
      constructor(cb) {
        super(cb);
        P.ro++;
      }
      disconnect() {
        if (!this.__gone) {
          this.__gone = true;
          P.ro--;
        }
        return super.disconnect();
      }
    };
  }

  // Rive: rive.js (UMD) assigns window.rive; its exports are getters, so the page gets a copy with a
  // counting Rive class in place of the original.
  let R;
  Object.defineProperty(window, 'rive', {
    configurable: true,
    get: () => R,
    set: (v) => {
      if (!v || !v.Rive || v.__perf) {
        R = v;
        return;
      }
      const Base = v.Rive;
      class Rive extends Base {
        constructor(p) {
          const t0 = performance.now();
          const onLoad = p && p.onLoad;
          super({
            ...p,
            onLoad: (...a) => {
              P.riveInit.push(performance.now() - t0);
              return onLoad && onLoad(...a);
            },
          });
          P.riveNew++;
          P.riveLive++;
        }
        cleanup() {
          if (!this.__gone) {
            this.__gone = true;
            P.riveLive--;
          }
          return super.cleanup();
        }
      }
      const copy = {};
      for (const k of Object.keys(v)) copy[k] = v[k];
      copy.Rive = Rive;
      copy.__perf = true;
      R = copy;
    },
  });

  const pct = (a, q) => {
    if (!a.length) return '';
    const s = [...a].sort((x, y) => x - y);
    return s[Math.min(s.length - 1, Math.floor(q * s.length))].toFixed(2);
  };
  setInterval(() => {
    const m = performance.memory;
    window.badi?.send('perf:sample', {
      t: Date.now(),
      vis: document.visibilityState,
      frames: P.frames,
      fps: (P.frames / (WINDOW / 1000)).toFixed(1),
      int_p50: pct(P.ints, 0.5),
      int_p95: pct(P.ints, 0.95),
      int_p99: pct(P.ints, 0.99),
      late25: P.ints.filter((x) => x > 25).length, // a frame missed at 60 Hz
      late50: P.ints.filter((x) => x > 50).length,
      work_p50: pct(P.works, 0.5),
      work_p95: pct(P.works, 0.95),
      work_max: P.works.length ? Math.max(...P.works).toFixed(2) : '',
      long_n: P.longN,
      long_ms: P.longMs.toFixed(0),
      heap_mb: m ? (m.usedJSHeapSize / 1048576).toFixed(2) : '',
      dom: document.getElementsByTagName('*').length,
      rive_new: P.riveNew,
      rive_live: P.riveLive,
      rive_init_ms: P.riveInit.map((x) => x.toFixed(0)).join('|'),
      listeners: P.adds - P.removes,
      resize_obs: P.ro,
    });
    P.frames = 0;
    P.ints = [];
    P.works = [];
    P.longN = 0;
    P.longMs = 0;
  }, WINDOW);
})();
