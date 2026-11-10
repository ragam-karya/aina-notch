// Dynamic Island: a notch joined to the top edge of the display picked for this turn, like the Mac's.
// At rest it is a small black notch. When the hotkey is pressed it springs open, shows a live audio
// wave while listening and while aina talks, and types out what aina says in sync with the voice.
// It is also the smart notch. Modes, by priority:
//   turn   a question or answer is on (Stop button on hover, approval buttons under the text)
//   drop   aina waits for a dropped folder or files: the notch is the drop zone (file-upload.json)
//   upload a small notch with a cloud (cloud.json) over a progress bar
//   panel  a panel from main (a finished task, an upload done, a timer)
//   peek   hovered while idle: clock, timer, what's playing, agents, reminders, shortcuts
//   mini   idle with agents running or a timer: a small pill with a dot
//   rest   the small notch
// The overlay is click-through; while the cursor is on the notch it asks main to take clicks.
(function () {
  const params = new URLSearchParams(location.search);
  const origin = { x: +params.get('x') || 0, y: +params.get('y') || 0 };

  const el = document.getElementById('island');
  const content = el.querySelector('.content');
  const labelEl = el.querySelector('.label');
  const rcolEl = el.querySelector('.rcol');
  const youEl = el.querySelector('.you');
  const sayEl = el.querySelector('.say');
  const wave = el.querySelector('canvas.wave');
  const wg = wave.getContext('2d');
  const linesEl = el.querySelector('.panel .lines');
  const actionsEl = el.querySelector('.panel .actions');
  const peekEl = el.querySelector('.peek-body');
  const miniEl = el.querySelector('.mini');
  const miniText = el.querySelector('.mini-text');
  const miniSub = el.querySelector('.mini-sub');
  const miniMain = el.querySelector('.mini-main');
  const miniRing = (() => {
    const r = ring(18, 2.4);
    el.querySelector('.mini .mini-ring').append(r.el);
    if (window.ainaIcon) el.querySelector('.mini .mini-ico').append(window.ainaIcon('folder', 13));
    return r;
  })();
  const dzTitle = el.querySelector('.dz-title');
  const dzHint = el.querySelector('.dz-hint');
  const barEl = el.querySelector('.bar i');
  const upTitle = el.querySelector('.uhead .utitle');
  const upPct = el.querySelector('.uhead .upct');

  // aina in the notch (Rive aina-mascot.riv, src/ui/rive.js). One instance for the page's whole life,
  // on one canvas that moves to wherever he shows: the turn row (in place of the orb), the compact
  // pill while a song without a cover plays, or beside the clock in the peek. Every Rive frame
  // repaints the full-screen overlay, so he draws only while shown: every frame while something
  // happens (a turn, the peek), otherwise in short bursts (a blink every few seconds, a new
  // expression), and not at all while hidden. The orb shows until he loads; if he can't, the old
  // eyes (aina-eyes.riv) take his place in the turn row.
  const mascotCanvas = el.querySelector('canvas.face');
  const mascot = window.ainaMascot(mascotCanvas);
  mascot.onReady = () => el.classList.add('has-face');
  let eyes = null;
  const EYE_STATE = { idle: 0, listening: 1, alert: 1, thinking: 2, speaking: 3, guiding: 3, success: 4, error: 5, sleeping: 0 };
  mascot.onFail = () => {
    const c = document.createElement('canvas');
    c.className = 'eyes';
    rcolEl.before(c);
    eyes = window.ainaRive(c, { artboards: ['BADI_Eyes', 'BADI_Micro'] });
    eyes.pause();
    eyes.onReady = () => el.classList.add('has-eyes');
  };
  // The compact pill's picture, on its left: aina, or a playing song's cover when it has one.
  const miniArt = el.querySelector('.mini-art');
  const miniCover = document.createElement('img');
  miniCover.alt = '';
  miniArt.append(miniCover);
  // Beside the peek's clock; it outlives the peek's rebuilds (renderPeek puts it back each time).
  const peekMascot = document.createElement('div');
  peekMascot.className = 'peek-mascot';
  const SLOTS = { row: () => rcolEl.before(mascotCanvas), mini: () => miniArt.append(mascotCanvas), peek: () => peekMascot.append(mascotCanvas) };
  // slot: where the canvas is. draw: 'live' every frame, 'burst' now and then, 'off' not at all.
  const M = { slot: 'row', draw: 'off', until: 0, playing: false, timer: 0, lastX: 0, dir: 0, turns: [], clicks: [], flags: {} };
  const ac = new AbortController();
  // Moving between places changes the canvas's size; its drawing surface follows (no new instance).
  const mascotResize = new ResizeObserver(() => mascot.resize());
  mascotResize.observe(mascotCanvas);

  function placeMascot(slot, draw, expr) {
    if (eyes) {
      // The fallback eyes only live in the turn row.
      eyes.set('state', EYE_STATE[expr] ?? 0);
      if (slot === 'row' && draw !== 'off') eyes.play();
      else eyes.pause();
      return;
    }
    // Called every frame while the notch is open: nothing is touched unless something changed.
    if (!slot) draw = 'off';
    const moved = Boolean(slot) && slot !== M.slot;
    if (moved) {
      M.slot = slot;
      SLOTS[slot]();
    }
    const changed = mascot.expression(expr);
    if (!moved && !changed && draw === M.draw) return;
    if (draw === 'off') mascot.hide();
    else {
      mascot.show();
      // A new expression plays its crossfade (220 ms) and entry move before he holds still again.
      M.until = Math.max(M.until, Date.now() + (changed ? 1200 : 1600));
    }
    M.draw = draw;
    burstStep();
  }
  function burstStep() {
    const want = M.draw === 'live' || (M.draw === 'burst' && Date.now() < M.until);
    if (want !== M.playing) {
      M.playing = want;
      if (want) mascot.play();
      else mascot.pause();
    }
    clearTimeout(M.timer);
    if (M.draw !== 'burst') return;
    M.timer = setTimeout(() => {
      if (!M.playing) M.until = Date.now() + 1600; // the next blink
      burstStep();
    }, want ? M.until - Date.now() + 20 : 4000 + Math.random() * 4000);
  }
  // A moment of motion (hover, a click) while he holds still.
  const nudge = (ms = 800) => {
    M.until = Math.max(M.until, Date.now() + ms);
    burstStep();
  };
  // Dizzy or annoyed for a moment (they lie over the expression).
  function flash(flag, ms) {
    mascot.set(flag, true);
    nudge(ms);
    clearTimeout(M.flags[flag]);
    M.flags[flag] = setTimeout(() => {
      mascot.set(flag, false);
      nudge(1200);
    }, ms);
  }
  // Clicked: squashed while pressed; four clicks in three seconds annoy him.
  mascotCanvas.addEventListener('pointerdown', () => {
    const now = Date.now();
    M.clicks = M.clicks.filter((t) => now - t < 3000);
    M.clicks.push(now);
    mascot.set('interaction', 'pressed');
    if (M.clicks.length >= 4) {
      M.clicks = [];
      flash('isAnnoyed', 2500);
    } else nudge();
  }, { signal: ac.signal });
  mascotCanvas.addEventListener('pointerup', () => {
    mascot.set('interaction', 'hover');
    nudge();
  }, { signal: ac.signal });
  addEventListener('pagehide', () => {
    ac.abort();
    mascotResize.disconnect();
    clearTimeout(M.timer);
    for (const t of Object.values(M.flags)) clearTimeout(t);
    mascot.destroy();
    eyes?.r?.cleanup();
  }, { once: true });

  // The frame loop runs only while the island is open; every event from main wakes it.
  let raf = 0;
  const wake = () => {
    if (!raf) raf = requestAnimationFrame(frame);
  };
  const on = (channel, fn) =>
    aina.on(channel, (payload) => {
      fn(payload);
      wake();
    });

  // Replaced by the main process with labels in the chosen language.
  let LABELS = { listening: 'Mendengarkan…', dictation: 'Mendikte…', transcribing: 'Oke…', thinking: 'Berpikir…', error: 'Ups' };
  on('islandLabels', (l) => (LABELS = l));

  const I = {
    display: null,
    phase: 'idle',
    mode: 'companion',
    level: 0,
    voice: false,
    transcript: '',
    text: '',
    typeStart: 0,
    typeMs: 0,
    spokenChars: 0, // reported by Windows voices as they speak
    error: '',
    hint: '', // small line under the question while aina waits for the user
    hideAt: 0,
    wide: false, // once wide, the notch stays wide until the turn ends instead of shrinking between phases
    panel: null, // { id, kind, title, lines, actions } from main
    status: { agents: [], timer: null, reminders: [], focus: null, media: null, shortcuts: [] },
    hover: false, // the cursor is on the notch
    typing: false, // the Home page's quick box has the keyboard (the peek stays open meanwhile)
    peekSince: 0,
    view: 'rest', // the current mode
    target: { w: 128, h: 8 }, // the size the notch is springing toward
    dropOver: false, // something is being dragged over the drop zone
    bar: 0, // the progress bar as drawn, easing toward the real value
    art: { key: '', src: null }, // the playing track's cover (media:art)
    lyrics: { key: '', lines: null }, // its synced lyrics, [[ms, text]] (media:lyrics)
    trackUntil: 0, // the track card shows until then (a new song started)
    lyricsOpen: false, // the music page shows the lyrics column; closed, the line being sung sits under the title
    dragging: false, // the volume bar is being dragged
    volume: null, // the volume as dragged, until main reports it back
    taskView: 'active', // the tasks page shows the active tasks or the history
    taskFocus: true, // the tasks page shows only the focus's project (when the focus splits them)
    stopAsk: null, // { id, until }: the task whose Stop is being asked about
    cal: null, // the reminder page's data (calendar:data)
    calDay: 0, // the day chosen on the reminder page (local midnight), 0 for today
    calAsk: null, // { id, kind: 'delete' | 'snooze', until }: a row asking before it acts
    fullscreen: false, // an app fills this display: the notch hides until called up
    revealed: false, // called up over a full-screen app (the cursor rested at the very top)
    calView: '', // a view picked on the reminder page for this look ('' : the one from Settings)
    activeAt: performance.now(), // the last time aina was doing something (he dozes after a while)
  };

  // ---- Lottie ----
  // Main sends the animation data; each animation is built once and plays only while it is shown.
  let lottieData = {};
  const anims = {};
  on('island:lottie', (d) => (lottieData = d || {}));
  function playOnly(name) {
    for (const [k, a] of Object.entries(anims)) if (k !== name) a.pause();
    if (!name || !window.lottie || !lottieData[name]) return;
    if (!anims[name]) {
      const container = el.querySelector(name === 'cloud' ? '.lot-cloud' : '.lot-upload');
      const a = lottie.loadAnimation({ container, renderer: 'svg', loop: true, autoplay: false, animationData: lottieData[name], rendererSettings: { preserveAspectRatio: 'xMidYMid meet' } });
      a.addEventListener('DOMLoaded', () => fitToDrawing(a, container));
      anims[name] = a;
    }
    anims[name].play();
  }
  // Both files leave lots of empty canvas around the drawing; the viewBox is cropped to where the
  // animation actually draws (the union over a few frames), so it fills its box in the notch.
  function fitToDrawing(a, container) {
    const svg = container.querySelector('svg');
    const g = svg?.firstElementChild?.tagName === 'defs' ? svg.children[1] : svg?.firstElementChild;
    if (!g) return;
    const playing = !a.isPaused;
    let box = null;
    for (const f of [0, 0.2, 0.4, 0.6, 0.8].map((k) => Math.floor(a.totalFrames * k))) {
      a.goToAndStop(f, true);
      const b = g.getBBox();
      if (!b.width || !b.height) continue;
      box = box ? { x1: Math.min(box.x1, b.x), y1: Math.min(box.y1, b.y), x2: Math.max(box.x2, b.x + b.width), y2: Math.max(box.y2, b.y + b.height) } : { x1: b.x, y1: b.y, x2: b.x + b.width, y2: b.y + b.height };
    }
    if (box) {
      const pad = 0.06 * Math.max(box.x2 - box.x1, box.y2 - box.y1);
      svg.setAttribute('viewBox', `${box.x1 - pad} ${box.y1 - pad} ${box.x2 - box.x1 + 2 * pad} ${box.y2 - box.y1 + 2 * pad}`);
    }
    a.goToAndStop(0, true);
    if (playing) a.play();
  }

  // ---- size springs ----
  // Growing is a little underdamped, so the notch opens with a small, soft overshoot; shrinking is
  // nearly critically damped, so it tucks away without wobbling. A hovered rest notch "blooms" a bit
  // before the peek opens, so the notch answers the cursor at once.
  // Sizes from the notch design (w x h, radius, ears): rest 132x28, the pill 264x36, the peek 800x256,
  // a turn 600 wide (72 tall while listening, growing with the answer).
  const SIZES = {
    rest: { w: 132, h: 28, r: 14, ear: 8 },
    restHover: { w: 152, h: 30, r: 15, ear: 9 },
    mini: { w: 264, h: 36, r: 18, ear: 10 }, // the compact pill: 36 px (38 hovered)
    turn: { w: 600, h: 72, r: 26, ear: 14 },
    peek: { w: 800, h: 256, r: 30, ear: 14 },
  };
  const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches;
  const fitWidth = (w) => Math.min(w, innerWidth * 0.9);
  const wideWidth = () => fitWidth(600);
  const spring = (x) => ({ x, v: 0 });
  const S = { w: spring(SIZES.rest.w), h: spring(SIZES.rest.h), r: spring(SIZES.rest.r), ear: spring(SIZES.rest.ear) };
  const K = 320;
  const ZETA_GROW = reduceMotion ? 1 : 0.76;
  const ZETA_SHRINK = reduceMotion ? 1 : 0.94;
  let lastNow = 0;
  function step(s, target, dt, zeta) {
    s.v += (K * (target - s.x) - 2 * zeta * Math.sqrt(K) * s.v) * dt;
    s.x += s.v * dt;
    if (Math.abs(target - s.x) < 0.25 && Math.abs(s.v) < 0.5) {
      s.x = target;
      s.v = 0;
      return true;
    }
    return false;
  }

  const isHere = () => (I.display ? I.display.x === origin.x && I.display.y === origin.y : origin.x === 0 && origin.y === 0);

  on('subtitleDisplay', (d) => (I.display = d));
  on('mode', (m) => (I.mode = m));
  on('level', (l) => (I.level = I.level * 0.4 + l * 0.6));
  on('voice', (v) => (I.voice = v));
  on('phase', (p) => {
    I.phase = p;
    if (p === 'listening') {
      I.transcript = '';
      I.text = '';
      I.error = '';
      I.wide = false;
    }
    if (p !== 'idle') {
      I.hideAt = 0;
      I.activeAt = performance.now();
    }
  });
  on('transcript', (t) => (I.transcript = t));
  // Payload is either a plain string or { text, ms } where ms is how long the voice takes to say it.
  on('text', (payload) => {
    const { text, ms, extend } = typeof payload === 'string' ? { text: payload, ms: payload.length * 45 } : payload;
    if (text === I.text) return;
    // Realtime answers arrive bit by bit: keep typing on instead of starting over.
    if (extend && I.text && text.startsWith(I.text)) {
      I.text = text;
      I.typeMs = Math.max(I.typeMs, ms * 0.92);
      I.hideAt = 0;
      return;
    }
    I.text = text;
    I.error = '';
    I.typeStart = performance.now();
    I.typeMs = Math.max(250, ms * 0.92);
    I.spokenChars = 0;
    I.hideAt = 0;
  });
  on('error', (e) => {
    I.error = e;
    I.hideAt = 0;
  });
  on('textProgress', (n) => (I.spokenChars = Math.max(I.spokenChars, n)));
  on('hint', (h) => (I.hint = h || ''));
  // How long the current question has been waiting (after 6 s), with an Esc hint after 20 s.
  on('elapsed', (e) => (I.elapsed = e || null));
  on('clear', () => {
    I.text = '';
    I.error = '';
    I.transcript = '';
  });
  // The island tucks away a little sooner than drawings fade.
  on('linger', (ms) => (I.hideAt = performance.now() + Math.min(ms, 10000)));
  on('island:panel', (p) => {
    if (p?.kind !== 'upload') I.bar = 0;
    I.panel = p || null;
    renderPanel();
  });
  on('island:dropHover', (v) => {
    I.dropOver = Boolean(v);
    anims['file-upload']?.setSpeed(I.dropOver ? 1.8 : 1);
  });
  // A full-screen app on this display (main checks once a second).
  on('fullscreen', (v) => {
    I.fullscreen = Boolean(v);
    if (!I.fullscreen) unreveal(0);
  });
  on('calendar:data', (c) => {
    I.cal = c || null;
    if (I.view === 'peek' && !I.dragging) renderPeek();
  });
  on('notch:status', (st) => {
    I.status = st || I.status;
    // Not while the volume is being dragged: the rebuild would drop the bar under the cursor.
    if (I.view === 'peek' && !I.dragging) renderPeek();
    if (I.view === 'track') renderTrack();
  });
  // Music (aina v2): the track's cover and synced lyrics, each sent once per track; a new track
  // opens the track card for a moment (the expanded state), unless the notch is being hovered.
  on('media:art', (a) => {
    I.art = a || { key: '', src: null };
    if (I.view === 'peek' && !I.dragging) renderPeek();
    if (I.view === 'track') renderTrack();
  });
  on('media:lyrics', (l) => {
    I.lyrics = l || { key: '', lines: null };
    if (I.view === 'peek' && !I.dragging) renderPeek();
  });
  on('media:track', () => {
    pageId = 'media';
    I.trackAt = performance.now();
    if (!I.hover) I.trackUntil = performance.now() + 4500;
  });

  // ---- buttons ----
  // Every button carries data-act; main knows what it does. Page dots (data-page) stay in the notch.
  el.addEventListener('click', (e) => {
    const dot = e.target.closest('[data-page]');
    if (dot) return goToPage(dot.dataset.page);
    const arrow = e.target.closest('[data-pagestep]');
    if (arrow) return pageStep(Number(arrow.dataset.pagestep));
    // The music card's own controls: the lyrics panel, and seeking on the progress bar.
    if (e.target.closest('[data-local="lyrics"]')) {
      I.lyricsOpen = !I.lyricsOpen;
      renderPeek();
      return wake();
    }
    // The tasks page's own controls: Active / History, the focus filter, and the question before Stop
    // (only its answer goes to main).
    // The reminder page's own controls: the chosen day, week and month, opening the prayer list, and
    // a row's question before deleting or snoozing.
    const cal = e.target.closest('[data-local^="cal:"]')?.dataset;
    if (cal) {
      const [, what, a, ...rest] = cal.local.split(':');
      if (what === 'day') selectDay(Number(a));
      if (what === 'daynav') selectDay(addDays(calDay(), Number(a)));
      if (what === 'today') selectDay(Date.now());
      // The calendar's arrows move a month, to the same date (or the month's last day if it is shorter).
      if (what === 'monthnav') {
        const d = new Date(calDay());
        const m = d.getMonth() + Number(a);
        selectDay(new Date(d.getFullYear(), m, Math.min(d.getDate(), new Date(d.getFullYear(), m + 1, 0).getDate())).getTime());
      }
      // The prayer line opens the prayer page.
      if (what === 'prayer' && pageIds.includes('prayer')) return goToPage('prayer');
      if (what === 'view') I.calView = a;
      if (what === 'ask') I.calAsk = { kind: a, id: rest.join(':'), until: Date.now() + 8000 };
      if (what === 'askoff') I.calAsk = null;
      renderPeek();
      return wake();
    }
    // A reminder action answers the row's question.
    if (e.target.closest('button[data-act^="cal:"]')) I.calAsk = null;
    const local = e.target.closest('[data-local^="task"]')?.dataset;
    if (local) {
      const [what, arg] = local.local.split(':');
      if (what === 'taskView') I.taskView = arg;
      if (what === 'taskFocus') I.taskFocus = I.taskFocus === false;
      if (what === 'taskKeep') I.stopAsk = null;
      if (what === 'taskStop') {
        const id = Number(local.id);
        I.stopAsk = { id, until: Date.now() + 6000 };
        setTimeout(() => {
          if (I.stopAsk?.id !== id) return;
          I.stopAsk = null;
          if (I.view === 'peek') renderPeek();
        }, 6100);
      }
      renderPeek();
      return wake();
    }
    const seek = e.target.closest('[data-seek]');
    const m = I.status.media;
    if (seek && m?.dur) {
      const r = seek.getBoundingClientRect();
      const ms = Math.round(Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)) * m.dur);
      Object.assign(m, { pos: ms, at: Date.now() }); // shown at once; main reads it back shortly
      return aina.send('island:act', `media:seek:${ms}`);
    }
    // A button, or a tile standing for one (the Notes & knowledge page).
    const b = e.target.closest('button[data-act], .htile[data-act]');
    if (b) aina.send('island:act', b.dataset.act);
  });
  // A button with a line icon (icons.js) before its label, or only the icon when there is no label.
  const button = (act, label, tone = '', icon = '') => {
    const b = document.createElement('button');
    b.dataset.act = act;
    // "Io…" names are react-icons (reacticons.js); the rest are aina's own line icons.
    const svg = icon && (/^Io[A-Z]/.test(icon) ? window.ainaReactIcon?.(icon, 15) : window.ainaIcon?.(icon, 15));
    if (svg) b.append(svg);
    if (label) b.append(icon ? ' ' : '', label);
    else if (icon) b.setAttribute('aria-label', icon);
    b.className = [tone, icon && !label ? 'icon' : ''].filter(Boolean).join(' ');
    return b;
  };
  const icon = (name, size) => (window.ainaIcon ? window.ainaIcon(name, size) : document.createTextNode(''));
  // A progress ring (the timer): set(frac) with frac the share still left, 1 → 0.
  function ring(size, width) {
    const NS = 'http://www.w3.org/2000/svg';
    const r = (size - width) / 2;
    const len = 2 * Math.PI * r;
    const svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('viewBox', `0 0 ${size} ${size}`);
    svg.setAttribute('width', size);
    svg.setAttribute('height', size);
    svg.setAttribute('class', 'ring');
    const circle = (cls) => {
      const c = document.createElementNS(NS, 'circle');
      for (const [k, v] of Object.entries({ cx: size / 2, cy: size / 2, r, fill: 'none', 'stroke-width': width, class: cls })) c.setAttribute(k, v);
      svg.append(c);
      return c;
    };
    circle('ring-bg');
    const fg = circle('ring-fg');
    fg.setAttribute('stroke-dasharray', len.toFixed(2));
    fg.setAttribute('stroke-linecap', 'round');
    fg.setAttribute('transform', `rotate(-90 ${size / 2} ${size / 2})`);
    return { el: svg, set: (frac) => fg.setAttribute('stroke-dashoffset', (len * (1 - Math.max(0, Math.min(1, frac)))).toFixed(2)) };
  }
  // What the timer has left (ms): where it was held while paused, else down to endsAt. Worked out
  // from the clock every frame, so it never jumps or starts over when the page changes.
  const timerMs = (t) => (t ? Math.max(0, t.paused ? t.left : t.endsAt - Date.now()) : 0);
  // The share of the phase still left (1 at its start, 0 at its end): the ring empties with the time.
  const timerLeft = (t) => (t && t.ms ? timerMs(t) / t.ms : 0);
  // The pomodoro phase: "work" | "short" | "long" ("break" from v1 is a short one); "" for a timer.
  const phaseOf = (t) => (t?.phase === 'break' ? 'short' : t?.phase || '');
  const mmss = (ms) => {
    const s = Math.max(0, Math.ceil(ms / 1000));
    return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
  };
  const fill = (s, o) => String(s).replace(/\{(\w+)\}/g, (_, k) => o[k] ?? '');
  const hhmm = (ms) => new Date(ms).toLocaleTimeString(LABELS.locale || 'id-ID', { hour: '2-digit', minute: '2-digit' });
  const div = (cls, text) => {
    const d = document.createElement('div');
    if (cls) d.className = cls;
    if (text != null) d.textContent = text;
    return d;
  };

  // A dictation note (aina v2): the summary above the text, scrolling inside the notch when long.
  function noteEl(n) {
    const box = div('note');
    if (n.summary) {
      const sum = div('note-summary');
      sum.append(div('note-label', n.summaryLabel || 'Ringkasan'), div('', n.summary));
      box.append(sum);
    }
    box.append(div('note-text', n.text || ''));
    return box;
  }

  // A list panel (aina v2: the notes, the knowledge base): rows to click, × to remove, scrolling.
  function listEl(rows) {
    const box = div('plist');
    if (!rows.length) box.append(div('sub', '—'));
    for (const r of rows) {
      const row = div('prow');
      const open = document.createElement(r.act ? 'button' : 'div');
      open.className = 'pmain';
      if (r.act) open.dataset.act = r.act;
      open.append(div('main', r.main), div('sub', r.sub));
      row.append(open);
      if (r.remove) row.append(button(r.remove, '', '', 'close'));
      box.append(row);
    }
    return box;
  }

  function renderPanel() {
    const p = I.panel;
    // The title is the notch's label in panel mode; under an answer it leads the lines instead.
    linesEl.replaceChildren(...(p?.title ? [div('ptitle', p.title)] : []), ...(p?.lines || []).map((l) => div('', l)), ...(p?.note ? [noteEl(p.note)] : []), ...(p?.list ? [listEl(p.list)] : []));
    dzTitle.textContent = p?.kind === 'drop' ? p.title : '';
    dzHint.textContent = p?.kind === 'drop' ? p.lines?.[0] || '' : '';
    // The upload row: what main calls it (the file), the percentage drawn in frame().
    if (p?.kind === 'upload' && (p.title || !upTitle.textContent)) upTitle.textContent = p.title || 'Google Drive';
    actionsEl.replaceChildren(...(p?.actions || []).map((a) => button(a.id, a.label, a.tone)));
  }

  const fmtLeft = (ms) => {
    const s = Math.max(0, Math.ceil(ms / 1000));
    const h = Math.floor(s / 3600);
    const mm = String(Math.floor((s % 3600) / 60)).padStart(h ? 2 : 1, '0');
    return `${h ? `${h}:` : ''}${mm}:${String(s % 60).padStart(2, '0')}`;
  };
  // Upload progress from its stage: each file is sent (up to 60% of its share) and then saved to
  // Drive (up to 95%), easing toward that ceiling over a time that grows with the file's size. Only
  // 'done' (or the next file) moves past the ceiling, so the bar never claims more than is true.
  function uploadProgress(p) {
    if (!p) return 0;
    if (p.stage === 'done') return 1;
    const [from, to, tau] = p.stage === 'save' ? [0.6, 0.95, 2.5] : [0, 0.6, Math.max(0.8, p.size / 1.5e6)];
    const t = (Date.now() - p.at) / 1000;
    return (p.i + from + (to - from) * (1 - Math.exp(-t / tau))) / p.n;
  }
  // The eyes' state input: 0 idle · 1 listening · 2 thinking · 3 speaking · 4 success · 5 error.
  // aina's expression for what the notch shows (the mascot's state enum). In the peek after ten quiet
  // minutes he dozes off.
  function faceState(view, listening, busy, waiting, now = performance.now()) {
    if (I.error) return 'error';
    if (view === 'panel' && (I.panel?.kind === 'uploaded' || I.panel?.kind === 'agentDone')) return 'success';
    if (I.phase === 'confirm' || (view === 'panel' && I.panel?.kind === 'confirm')) return 'alert';
    if (listening || waiting) return 'listening';
    if (I.phase === 'guiding') return 'guiding';
    if (busy) return 'thinking';
    if (I.voice || I.phase === 'speaking') return 'speaking';
    if (view === 'peek' && I.phase === 'idle' && now - I.activeAt > 10 * 60000) return 'sleeping';
    return 'idle';
  }
  const PHASE_LABEL = { work: ['pomoFocus', 'Fokus'], short: ['pomoShort', 'Istirahat pendek'], long: ['pomoLong', 'Istirahat panjang'] };
  const timerLabel = (t) => {
    const ph = phaseOf(t);
    return ph ? LABELS[PHASE_LABEL[ph][0]] || PHASE_LABEL[ph][1] : t.label || LABELS.timer || 'Timer';
  };

  // The hover peek, rebuilt when the status changes (the clock and countdown tick in frame()). Its
  // items are grouped in pages, like a phone's widget stack: now (timer, music, focus), tasks
  // (background work) and later (reminders). With more than one page, dots under them switch pages
  // (click, or the mouse wheel over the notch); the shortcuts stay under every page.
  let clockEl = null;
  let countEl = null;
  let peekRing = null;
  let pageId = 'now';
  let pageIds = [];
  let pageDir = 0; // the slide direction of the next render: 1 next, -1 previous
  let peekFresh = false; // the first render after the peek opens staggers its rows in
  const iconBox = (name, cls = '') => {
    const b = div(`lead ${cls}`);
    b.append(icon(name, 16));
    return b;
  };
  // ---- music ----
  // Three states: compact (the mini pill: clock, activity, a small equalizer), expanded (the track card
  // a new song opens for a moment: cover, title, artist, progress, controls) and interactive (the
  // music page of the hover peek, adding seeking, the volume, lyrics and the sound output; clicking
  // the cover or the lyrics button opens the lyrics panel).
  let mediaRefs = []; // progress bars and lyric lines on screen, moved every frame
  const eqBars = () => {
    const eq = div('eq');
    eq.append(document.createElement('i'), document.createElement('i'), document.createElement('i'));
    return eq;
  };
  // Where the track is now (ms), carried forward from when main read it; null without a timeline.
  const mediaPos = (m) => (m && m.pos != null && m.dur ? Math.min(m.dur, m.pos + (m.playing ? Date.now() - m.at : 0)) : null);
  const lyricLines = (m) => (m && I.lyrics.key === m.key && I.lyrics.lines?.length ? I.lyrics.lines : null);
  const lyricAt = (lines, pos) => {
    let i = -1;
    for (let k = 0; k < lines.length && lines[k][0] <= pos + 250; k++) i = k;
    return i;
  };
  function coverEl(m, cls) {
    const art = div(`art ${cls}${m.playing ? ' playing' : ''}`);
    if (I.art.key === m.key && I.art.src) {
      const img = document.createElement('img');
      img.src = I.art.src;
      img.alt = '';
      art.append(img);
    } else art.append(icon('music', 18));
    art.append(eqBars());
    return art;
  }
  function progressEl(m, seekable) {
    if (m.dur == null) return null;
    const row = div('mbar');
    const cur = div('t');
    const bar = div(`pbar${seekable ? ' seek' : ''}`);
    const fill = document.createElement('i');
    bar.append(fill);
    if (seekable) bar.dataset.seek = '1';
    row.append(cur, bar, div('t', fmtLeft(m.dur)));
    mediaRefs.push({ fill, cur });
    return row;
  }
  // The volume: its icon mutes and unmutes, the bar is dragged or scrolled.
  function volumeEl(m) {
    if (m.volume == null) return null;
    const box = div('vol');
    const muted = m.muted || m.volume === 0;
    box.append(button('media:mute', '', muted ? 'on-mute' : '', muted ? 'IoVolumeMute' : 'IoVolumeHigh'));
    const bar = div(`vbar${muted ? ' muted' : ''}`);
    const fill = document.createElement('i');
    fill.style.width = `${(I.volume ?? m.volume) * 100}%`;
    bar.append(fill);
    box.append(bar);
    return box;
  }
  // A control the player may not allow: shown, but off, when its capability says no. Without
  // capabilities (an older main), everything is assumed allowed.
  const capBtn = (m, cap, act, tone, icon) => {
    const b = button(act, '', tone, icon);
    if (m.caps && !m.caps[cap]) {
      b.disabled = true;
      b.classList.add('off');
    }
    return b;
  };
  // The music page (interactive). Song: cover (or a stand-in), title and "artist · album" cut short
  // rather than widening the notch. Playback: previous, play/pause, next; the progress with the
  // time gone and the length, seekable when the player allows it. Audio: mute, volume, shuffle and
  // repeat when the player offers them, lyrics, the sound output, and the player itself to open.
  // Favourites and the queue are not offered: Windows' media session doesn't expose them.
  function mediaItem(m) {
    mediaRefs = [];
    const lines = lyricLines(m);
    const lyricsCol = Boolean(lines) && I.lyricsOpen;
    const it = div(`item media widget${m.playing ? ' playing' : ''}`);
    const cover = coverEl(m, 'xl');
    if (lines) cover.dataset.local = 'lyrics';
    const col = div('mcol');
    const head = div('mhead');
    head.append(div('main', m.title), div('sub', [m.artist, m.album].filter(Boolean).join(' · ') || m.app || ''));
    col.append(head);
    // With the lyrics column closed, the line being sung sits under the title.
    if (lines && !lyricsCol) {
      const lyric = div('lyric');
      col.append(lyric);
      mediaRefs.push({ lines, now: lyric });
    }
    const prog = progressEl(m, !m.caps || m.caps.seek);
    if (prog) col.append(prog);
    // Playback (shuffle, previous, play/pause, next, repeat) on the left, the volume on the right.
    const row = div('mctl');
    const ctl = div('ctl');
    if (m.caps?.shuffle) ctl.append(button('media:shuffle', '', m.shuffle ? 'on' : '', 'IoShuffle'));
    ctl.append(capBtn(m, 'previous', 'media:previous', '', 'IoPlaySkipBack'), capBtn(m, 'playPause', 'media:play_pause', 'play', m.playing ? 'IoPause' : 'IoPlay'), capBtn(m, 'next', 'media:next', '', 'IoPlaySkipForward'));
    if (m.caps?.repeat) {
      const b = button('media:repeat', '', m.repeat && m.repeat !== 'none' ? 'on' : '', 'IoRepeat');
      if (m.repeat === 'track') b.append(div('one', '1'));
      ctl.append(b);
    }
    row.append(ctl);
    const vol = volumeEl(m);
    if (vol) row.append(vol);
    col.append(row);
    // The player (its name opens it), the lyrics switch and the sound output.
    const tools = div('mtools');
    if (m.app) tools.append(button('media:open', m.app, 'src', 'IoOpenOutline'));
    const right = div('mright');
    if (lines) {
      const b = localButton('lyrics', '', I.lyricsOpen ? 'on' : '', 'IoReaderOutline');
      b.title = label('musicLyrics', 'Lirik');
      right.append(b);
    }
    if (m.volume != null) right.append(tip(button('sound:output', '', '', 'IoHeadsetOutline'), label('musicOutput', 'Output suara')));
    tools.append(right);
    col.append(tools);
    it.append(cover, col);
    // The lyrics column: the line before, the one sung now (bright), and the two after.
    if (lyricsCol) {
      const lc = div('lcol');
      const prev = div('l prev');
      const now = div('l now');
      const next = div('l next');
      const after = div('l');
      lc.append(prev, now, next, after, div('lsrc', label('musicLyricsSrc', 'Lirik · LRCLIB')));
      it.append(lc);
      mediaRefs.push({ lines, prev, now, next, after });
    }
    return it;
  }
  // Nothing playing: the page still opens from its tab, with a stand-in cover and a hint.
  function mediaIdle() {
    const it = div('item media widget idle');
    const art = div('art xl');
    art.append(icon('music', 24));
    const col = div('mcol');
    col.append(div('main', LABELS.musicNone || 'Tidak ada musik diputar'), div('sub', LABELS.musicNoneHint || 'Putar lagu di Spotify, YouTube, atau pemutar lain'));
    col.style.justifyContent = 'center';
    it.append(art, col);
    return it;
  }
  // The expanded state: shown by itself for a moment when a new song starts.
  const trackEl = el.querySelector('.track');
  function renderTrack() {
    const m = I.status.media;
    if (!m) return trackEl.replaceChildren();
    mediaRefs = [];
    const g = div('grow');
    g.append(div('main', m.title), div('sub', [m.artist, m.app].filter(Boolean).join(' · ')));
    const prog = progressEl(m, false);
    if (prog) g.append(prog);
    const ctl = div('ctl');
    const ri = (name, size) => window.ainaReactIcon?.(name, size) || icon('music', size);
    ctl.append(ri('IoPlaySkipBack', 15), ri(m.playing ? 'IoPause' : 'IoPlay', 17), ri('IoPlaySkipForward', 15));
    trackEl.replaceChildren(coverEl(m, 'lg'), g, ctl);
  }
  // Every frame while the music shows: the progress bars and the lyric lines.
  function updateMedia() {
    const m = I.status.media;
    const pos = mediaPos(m);
    for (const r of mediaRefs) {
      if (r.fill && pos != null) {
        r.fill.style.width = `${((pos / m.dur) * 100).toFixed(2)}%`;
        r.cur.textContent = fmtLeft(pos);
      }
      if (r.lines && pos != null) {
        const i = lyricAt(r.lines, pos);
        const text = (k) => (k >= 0 && k < r.lines.length ? r.lines[k][1] || '♪' : '');
        if (r.now.textContent !== text(i)) {
          r.now.textContent = text(i);
          if (r.prev) {
            r.prev.textContent = text(i - 1);
            r.next.textContent = text(i + 1);
            if (r.after) r.after.textContent = text(i + 2);
            r.now.classList.remove('swap');
            void r.now.offsetWidth;
            r.now.classList.add('swap');
          }
        }
      }
    }
  }
  // The volume bar: press and drag (or scroll over it); main is told at most every 80 ms.
  let volSent = 0;
  function volumeTo(e, bar, final) {
    const r = bar.getBoundingClientRect();
    I.volume = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    bar.firstChild.style.width = `${I.volume * 100}%`;
    const now = performance.now();
    if (final || now - volSent > 80) {
      volSent = now;
      aina.send('island:act', `volume:${Math.round(I.volume * 100)}`);
    }
  }
  el.addEventListener('pointerdown', (e) => {
    const bar = e.target.closest('.vbar');
    if (!bar) return;
    I.dragging = true;
    bar.setPointerCapture(e.pointerId);
    volumeTo(e, bar);
    const move = (ev) => volumeTo(ev, bar);
    const up = (ev) => {
      volumeTo(ev, bar, true);
      I.dragging = false;
      bar.removeEventListener('pointermove', move);
      bar.removeEventListener('pointerup', up);
      setTimeout(() => (I.volume = null), 2500); // until main has read the new level back
    };
    bar.addEventListener('pointermove', move);
    bar.addEventListener('pointerup', up);
  });
  // ---- timer and pomodoro ----
  // The page with the most to do: the countdown inside a ring that fills as the phase passes (orange
  // for focus, green for a short break, blue for a long one), the phase and "session 2 of 4", one
  // segment per session of the cycle, when the cycle ends, today's finished sessions, and the
  // controls. Skip ends only this phase, Reset puts this phase back to its full length, Stop ends
  // the whole cycle. v1 sends no `left`, and gets only Stop.
  let endsRef = null; // the "cycle ends at" line, kept true every frame (it moves while paused)
  function segments(t) {
    const n = t.sessions || 4;
    const ph = phaseOf(t);
    const done = ((t.round || 1) - 1) % n + (ph === 'work' ? 0 : 1);
    const box = div('rounds');
    for (let i = 0; i < n; i++) box.append(div(i < done ? 'r done' : i === done && ph === 'work' ? 'r now' : 'r'));
    return box;
  }
  // The controls as labelled pills: Start/Pause (blue), Skip, Reset, Stop (red), then the settings.
  function timerControls(t) {
    const ctl = div('pctl');
    if (t.left === undefined) {
      ctl.append(button('timerOff', label('pomoStop', 'Stop'), 'danger', 'IoStop'));
      return ctl;
    }
    const fresh = t.left === t.ms;
    ctl.append(t.paused ? button('timer:start', fresh ? label('pomoStart', 'Mulai') : label('pomoResume', 'Lanjut'), 'primary', 'IoPlay') : button('timer:pause', label('pomoPause', 'Jeda'), 'primary', 'IoPause'));
    if (t.phase) ctl.append(button('timer:skip', label('pomoSkip', 'Lewati')));
    ctl.append(button('timer:reset', label('pomoReset', 'Ulang')), button('timerOff', label('pomoStop', 'Stop'), 'danger'));
    if (t.phase) ctl.append(tip(button('timer:settings', '', '', 'IoSettingsOutline'), label('pomoSettings', 'Pengaturan pomodoro')));
    return ctl;
  }
  // Today's figures under the controls, the number large: "3 sesi hari ini · selesai 10.25".
  function pomoStats(today, extra) {
    const row = div('pstats');
    if (today != null) {
      const s = document.createElement('span');
      const b = document.createElement('b');
      b.textContent = today;
      s.append(b, label('pomoTodayUnit', 'sesi hari ini'));
      row.append(s);
    }
    for (const x of extra) if (x) row.append(x);
    return row;
  }
  function timerItem(t, st) {
    const ph = phaseOf(t);
    const waiting = t.paused && t.left === t.ms; // a phase that waits for Start (auto-start off)
    const it = div(`item widget pomo${ph ? ` phase-${ph}` : ''}${t.paused ? ' paused' : ''}`);
    peekRing = ring(124, 7);
    peekRing.set(timerLeft(t)); // drawn where it is now, not swept in from full on every rebuild
    const dial = div('dial');
    countEl = div('count');
    // Under the countdown: the phase and "2/4", or why it holds still.
    const state = t.paused ? (waiting ? label('pomoReady', 'Siap') : label('pomoPaused', 'Dijeda')) : [timerLabel(t), ph && t.sessions ? `${t.round}/${t.sessions}` : ''].filter(Boolean).join(' · ');
    dial.append(peekRing.el, countEl, div('dstate', state));
    const g = div('grow');
    const head = div('phead');
    head.append(div('main', timerLabel(t)));
    if (ph && t.sessions) head.append(div('sub', fill(LABELS.pomoSession || 'Sesi {n} dari {m}', { n: t.round, m: t.sessions })));
    if (ph) head.append(segments(t));
    g.append(head);
    g.append(timerControls(t));
    const ends = div('ends today');
    ends.append(icon('flag', 12));
    const endsText = document.createElement('span');
    ends.append(endsText);
    endsRef = { el: endsText, t };
    // Linked work: the timer's own label, and the folder aina is focused on.
    const tags = [ph && t.label, st.focus].filter(Boolean);
    g.append(pomoStats(ph && st.pomodoro ? st.pomodoro.today : null, [ends, tags.length ? div('ptags', tags.join(' · ')) : null]));
    it.append(dial, g);
    return it;
  }
  // Every frame on the timer page: the line saying when this phase (or the whole cycle) ends.
  function updateEnds() {
    if (!endsRef) return;
    const t = endsRef.t;
    const phaseEnd = Date.now() + timerMs(t);
    const text = t.cycleEndsAt ? fill(LABELS.pomoCycleEnds || 'siklus selesai {t}', { t: hhmm(phaseEnd + (t.cycleEndsAt - t.endsAt)) }) : hhmm(phaseEnd);
    if (endsRef.el.textContent !== text) endsRef.el.textContent = text;
  }
  // No timer yet (v2): the pomodoro ready to start, with its presets to switch between.
  function pomodoroIdle(p) {
    const c = p.config;
    const it = div('item widget pomo idle phase-work');
    const r = ring(124, 7);
    r.set(1); // ready: the whole phase still ahead
    const dial = div('dial');
    dial.append(r.el, div('count', mmss(c.focusMin * 60000)), div('dstate', `${label('pomoFocus', 'Fokus')} · ×${c.sessions}`));
    const g = div('grow');
    const chips = div('presets');
    chips.append(...(p.presets || []).map((pr) => button(`timer:preset:${pr.id}`, pr.name, pr.id === p.preset ? 'chip on' : 'chip')));
    g.append(chips);
    const ctl = div('pctl');
    ctl.append(button('timer:start', label('pomoStart', 'Mulai'), 'primary', 'IoPlay'), tip(button('timer:settings', '', '', 'IoSettingsOutline'), label('pomoSettings', 'Pengaturan pomodoro')));
    g.append(ctl);
    g.append(pomoStats(p.today || 0, [div('ptags', `${c.focusMin}/${c.shortMin}/${c.longMin} ${label('durM', 'mnt')}`)]));
    it.append(dial, g);
    return it;
  }
  // ---- focus ----
  // What the person is working on (not when: that's the pomodoro). With a focus: its name, how long
  // it has run, what it's about and its goal, the pomodoro running for it, and Open folder, Notes,
  // the AI edits lock and Selesai. Without one: Mulai fokus, the last few contexts to start again
  // with one click, Pick folder and New context. Never a list of every project.
  let focusRef = null; // the running time, moved every frame
  const fmtDur = (ms) => {
    const m = Math.floor(ms / 60000);
    const [h, mm] = [LABELS.durH || 'j', LABELS.durM || 'mnt'];
    return m >= 60 ? `${Math.floor(m / 60)} ${h} ${m % 60} ${mm}` : `${m} ${mm}`;
  };
  // Running: on the left the work (its folder, how long, the goal, the AI edits lock as a switch, and
  // the buttons); on the right the pomodoros done for it and the one running now.
  function focusItem(f, st) {
    const s = f.session;
    const it = div('item widget focuspage on');
    const left = div('grow');
    const head = div('fhead');
    const lead = div('lead');
    lead.append(s.folder ? icon('folder', 16) : window.ainaReactIcon?.('IoLocateOutline', 17) || icon('folder', 16));
    const name = div('grow');
    name.append(div('main', s.name), div('fpath', s.folder || s.about || label('focusActive', 'Fokus aktif')));
    const live = div('live');
    focusRef = { el: live, startedAt: s.startedAt };
    head.append(lead, name, live);
    left.append(head);
    if (s.goal) {
      const goal = div('fline');
      const k = document.createElement('span');
      k.className = 'k';
      k.textContent = label('focusGoal', 'Target sesi:');
      goal.append(k, ` ${s.goal}`);
      left.append(goal);
    } else if (s.about && s.folder) left.append(div('fline', s.about));
    if (s.folder) {
      const row = div('frow');
      const g = div('grow');
      g.append(div('main', label('focusLock', 'Kunci edit')), div('sub', s.lock ? label('focusLocked', 'Edit AI hanya di folder ini') : label('focusUnlocked', 'Edit AI tidak dikunci')));
      const sw = button('focus:lock', '', s.lock ? 'switch on' : 'switch');
      sw.setAttribute('role', 'switch');
      sw.setAttribute('aria-checked', String(Boolean(s.lock)));
      sw.setAttribute('aria-label', label('focusLock', 'Kunci edit'));
      row.append(g, sw);
      left.append(row);
    }
    const btns = div('fbtns');
    btns.append(button('focus:end', label('endFocus', 'Selesai fokus'), 'primary', 'IoCheckmark'));
    if (s.folder) btns.append(button('focus:open', label('focusOpen', 'Buka folder'), '', 'IoFolderOpenOutline'));
    btns.append(button('focus:note', label('focusNote', 'Catatan'), '', 'IoCreateOutline'));
    left.append(btns);
    const side = div('fside');
    side.append(div('cap', label('focusPomo', 'Pomodoro untuk fokus ini')));
    const stat = (cls, big, small) => {
      const b = div(`fstat${cls}`);
      const n = document.createElement('b');
      n.textContent = big;
      const t = document.createElement('span');
      t.textContent = small;
      b.append(n, t);
      return b;
    };
    if (st.timer && phaseOf(st.timer) && st.timer.label === s.name) side.append(stat(' timer', timerLabel(st.timer), st.timer.sessions ? fill(label('pomoSession', 'Sesi {n} dari {m}'), { n: st.timer.round, m: st.timer.sessions }) : label('homeTimerRun', 'Berjalan')));
    side.append(stat('', String(s.pomodoros || 0), `pomodoro · ${fmtDur(s.pomodoroMs || 0)}`));
    it.append(left, side);
    return it;
  }
  function focusIdle(f) {
    const it = div('item widget focuspage idle');
    const lead = div('lead');
    lead.append(window.ainaReactIcon?.('IoLocateOutline', 17) || icon('folder', 16));
    const g = div('grow');
    const head = div('phead');
    head.append(div('main', LABELS.focusStart || 'Mulai fokus'), div('sub', LABELS.focusStartHint || ''));
    g.append(head);
    if (f.recent?.length) {
      const chips = div('presets');
      chips.append(...f.recent.map((r, i) => button(`focus:recent:${i}`, r.name, 'chip', r.folder ? 'IoFolderOpenOutline' : 'IoLocateOutline')));
      g.append(chips);
    }
    const ctl = div('pctl');
    ctl.append(button('focus:pick', LABELS.focusPick || 'Pilih folder', 'primary', 'IoFolderOpenOutline'), button('focus:new', LABELS.focusNew || 'Konteks baru', 'chip', 'IoAddOutline'));
    g.append(ctl);
    it.append(lead, g);
    return it;
  }
  function updateFocus() {
    if (!focusRef) return;
    const text = fmtDur(Date.now() - focusRef.startedAt);
    if (focusRef.el.textContent !== text) focusRef.el.textContent = text;
  }
  const rowItem = (lead, main, sub, ...buttons) => {
    const it = div('item');
    const g = div('grow');
    g.append(div('main', main), div('sub', sub));
    it.append(lead, g, ...buttons);
    return it;
  };
  // ---- quick box (Home) ----
  // Typed in the notch, worked out here with no call anywhere: a sum ("15% dari 2.350.000", "ppn 11%
  // 1,5jt", "(750rb x 3) : 2"), or a number to spell out in words (terbilang). The overlay never takes
  // the keyboard on its own; clicking the box asks main for it (island:typing) and gives it back when
  // the box loses focus. The box is one element for the page's whole life, so the peek's rebuilds
  // never drop what is being typed.
  const smart = { q: '', copied: '', copyTimer: 0, want: false };
  // A number as written in Indonesian: 2.350.000, 1,5, 750rb, 1,5jt, 2 miliar.
  const SCALE = { rb: 1e3, ribu: 1e3, k: 1e3, jt: 1e6, juta: 1e6, m: 1e9, miliar: 1e9, t: 1e12, triliun: 1e12 };
  function readNumber(t) {
    const m = String(t).trim().toLowerCase().match(/^([\d.,]+)\s*(rb|ribu|k|jt|juta|miliar|m|triliun|t)?$/);
    if (!m) return NaN;
    let v = m[1];
    if (v.includes(',')) v = v.replace(/\./g, '').replace(',', '.');
    else if (/^\d{1,3}(\.\d{3})+$/.test(v)) v = v.replace(/\./g, '');
    const n = parseFloat(v);
    return Number.isFinite(n) ? n * (SCALE[m[2]] || 1) : NaN;
  }
  // + - × / and brackets, numbers as above; anything else is not a sum (no eval: the page's CSP has none).
  function evalSum(src) {
    const tokens = [];
    const re = /\s*(?:([\d.,]+\s*(?:ribu|rb|juta|jt|miliar|triliun|k|m|t)?(?![a-z]))|([-+*/()]))/iy;
    const text = src.replace(/[x×]/gi, '*').replace(/[:÷]/g, '/');
    let i = 0;
    while (i < text.length) {
      re.lastIndex = i;
      const m = re.exec(text);
      if (!m) return null;
      if (m[1] !== undefined) {
        const n = readNumber(m[1]);
        if (Number.isNaN(n)) return null;
        tokens.push(n);
      } else tokens.push(m[2]);
      i = re.lastIndex;
      if (/^\s*$/.test(text.slice(i))) break;
    }
    let k = 0;
    const peek = () => tokens[k];
    const factor = () => {
      const t = tokens[k++];
      if (t === '-') return -factor();
      if (t === '+') return factor();
      if (t === '(') {
        const v = expr();
        if (tokens[k++] !== ')') throw 0;
        return v;
      }
      if (typeof t === 'number') return t;
      throw 0;
    };
    const term = () => {
      let v = factor();
      while (peek() === '*' || peek() === '/') v = tokens[k++] === '*' ? v * factor() : v / factor();
      return v;
    };
    const expr = () => {
      let v = term();
      while (peek() === '+' || peek() === '-') v = tokens[k++] === '+' ? v + term() : v - term();
      return v;
    };
    try {
      const v = expr();
      return k === tokens.length && Number.isFinite(v) ? v : null;
    } catch {
      return null;
    }
  }
  const rupiah = (n) => `Rp ${Math.round(n).toLocaleString('id-ID')}`;
  // What the box holds: { kind: 'words' | 'sum', value, sub } or null.
  function readQuery(q) {
    const t = q.trim().toLowerCase();
    if (!t) return null;
    const N = '([\\d.,]+\\s*(?:ribu|rb|juta|jt|miliar|triliun|k|m|t)?)';
    let m;
    // "15% dari 2.350.000", "15% x 2,35jt".
    if ((m = t.match(new RegExp(`^${N}\\s*%\\s*(?:dari|of|x|×|\\*)\\s*${N}$`)))) {
      const [a, b] = [readNumber(m[1]), readNumber(m[2])];
      if (!Number.isNaN(a) && !Number.isNaN(b)) return { kind: 'sum', value: (a / 100) * b, sub: `${a}% × ${rupiah(b)}` };
    }
    // "ppn 11% 1,5jt": the tax, and the total with it.
    if ((m = t.match(new RegExp(`^(?:ppn|pajak|tax)\\s*${N}\\s*%\\s*(?:dari\\s*)?${N}$`)))) {
      const [a, b] = [readNumber(m[1]), readNumber(m[2])];
      if (!Number.isNaN(a) && !Number.isNaN(b)) return { kind: 'sum', value: (b * a) / 100, sub: fill(label('smartTax', 'PPN {p}% · total {t}'), { p: a, t: rupiah(b + (b * a) / 100) }) };
    }
    // A number alone: spelled out.
    const alone = readNumber(t);
    if (!Number.isNaN(alone)) return { kind: 'words', value: alone, sub: label('smartWordsOf', 'Terbilang') };
    const v = evalSum(t);
    return v === null ? null : { kind: 'sum', value: v, sub: q.trim() };
  }
  // Indonesian words for a whole number, to the trillions: "dua juta tiga ratus lima puluh ribu".
  function terbilang(n) {
    const one = ['', 'satu', 'dua', 'tiga', 'empat', 'lima', 'enam', 'tujuh', 'delapan', 'sembilan', 'sepuluh', 'sebelas'];
    const say = (x) => {
      x = Math.floor(x);
      if (x < 12) return one[x];
      if (x < 20) return `${say(x - 10)} belas`;
      if (x < 100) return `${say(x / 10)} puluh${x % 10 ? ` ${say(x % 10)}` : ''}`;
      if (x < 200) return `seratus${x - 100 ? ` ${say(x - 100)}` : ''}`;
      if (x < 1000) return `${say(x / 100)} ratus${x % 100 ? ` ${say(x % 100)}` : ''}`;
      if (x < 2000) return `seribu${x - 1000 ? ` ${say(x - 1000)}` : ''}`;
      if (x < 1e6) return `${say(x / 1e3)} ribu${x % 1e3 ? ` ${say(x % 1e3)}` : ''}`;
      if (x < 1e9) return `${say(x / 1e6)} juta${x % 1e6 ? ` ${say(x % 1e6)}` : ''}`;
      if (x < 1e12) return `${say(x / 1e9)} miliar${x % 1e9 ? ` ${say(x % 1e9)}` : ''}`;
      return `${say(x / 1e12)} triliun${x % 1e12 ? ` ${say(x % 1e12)}` : ''}`;
    };
    const r = Math.round(Math.abs(n));
    if (r >= 1e15) return '';
    const words = r === 0 ? 'nol' : say(r);
    return n < 0 ? `minus ${words}` : words;
  }
  // The box: an icon, the field, and what it has understood.
  const smartBox = div('sbox');
  const smartIco = div('sico');
  smartIco.append(icon('spark', 14));
  const smartInput = document.createElement('input');
  smartInput.type = 'text';
  smartInput.spellcheck = false;
  smartInput.autocomplete = 'off';
  const smartKind = div('skind');
  smartBox.append(smartIco, smartInput, smartKind);
  let homeBody = null; // the tiles, or the box's answer in their place
  let homeStatus = null;
  // Clicking the box: the overlay takes the keyboard first, then the field takes focus.
  smartInput.addEventListener('pointerdown', () => {
    smart.want = true;
    aina.send('island:typing', true);
    setTimeout(() => smart.want && smartInput.focus(), 60);
  });
  addEventListener('focus', () => smart.want && smartInput.focus());
  smartInput.addEventListener('focus', () => {
    I.typing = true;
    smartBox.classList.add('on');
    wake();
  });
  smartInput.addEventListener('blur', () => {
    smart.want = false;
    I.typing = false;
    smartBox.classList.remove('on');
    aina.send('island:typing', false);
    wake();
  });
  smartInput.addEventListener('input', () => {
    smart.q = smartInput.value;
    smart.copied = '';
    updateHomeBody();
  });
  smartInput.addEventListener('keydown', (e) => {
    // Esc clears the box, or, empty, lets go of the keyboard.
    if (e.key === 'Escape') {
      e.preventDefault();
      if (smart.q) {
        smartInput.value = smart.q = '';
        updateHomeBody();
      } else smartInput.blur();
    }
    // Enter copies the answer.
    if (e.key === 'Enter') copySmart('main');
  });
  async function copySmart(which) {
    const r = readQuery(smart.q);
    if (!r) return;
    const text = which === 'words' ? terbilang(r.value) + (r.kind === 'sum' ? ' rupiah' : '') : r.kind === 'sum' ? rupiah(r.value) : Math.round(r.value).toLocaleString('id-ID');
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Without clipboard access (the page not focused): through a hidden field and the copy command.
      const t = document.createElement('textarea');
      t.value = text;
      document.body.append(t);
      t.select();
      document.execCommand('copy');
      t.remove();
    }
    smart.copied = which;
    clearTimeout(smart.copyTimer);
    smart.copyTimer = setTimeout(() => {
      smart.copied = '';
      updateHomeBody();
    }, 1600);
    updateHomeBody();
  }
  // The answer card: what was worked out, the figure large, its words, and the copy buttons.
  function smartAnswer() {
    const r = readQuery(smart.q);
    const box = div('sans');
    if (!r) {
      box.classList.add('none');
      box.append(div('shint', label('smartUnknown', 'Belum aku kenali. Coba salah satu:')));
      const ex = div('sex');
      for (const q of ['15% dari 2.350.000', 'ppn 11% 1,5jt', '(750rb x 3) : 2', '1.250.000']) {
        const b = localButton(`smart:${q}`, q, 'chip');
        ex.append(b);
      }
      box.append(ex);
      return box;
    }
    const words = terbilang(r.value);
    const main = r.kind === 'sum' ? rupiah(r.value) : Math.round(r.value).toLocaleString('id-ID');
    const col = div('scol');
    col.append(div('ssub', r.sub), div('smain', main));
    if (words) col.append(div('swords', r.kind === 'sum' ? `${words} rupiah` : words));
    const act = div('sact');
    const c1 = localButton('smartcopy:main', smart.copied === 'main' ? label('smartCopied', 'Disalin') : label('smartCopy', 'Salin'), smart.copied === 'main' ? 'chip done' : 'primary');
    act.append(c1);
    if (words) act.append(localButton('smartcopy:words', smart.copied === 'words' ? label('smartCopied', 'Disalin') : label('smartCopyWords', 'Salin terbilang'), smart.copied === 'words' ? 'chip done' : 'chip'));
    box.append(col, act);
    return box;
  }
  // The box's state shown: its badge, and the tiles or the answer under it.
  function updateHomeBody() {
    const r = readQuery(smart.q);
    smartKind.textContent = !smart.q.trim() ? '' : !r ? '—' : r.kind === 'sum' ? label('smartKindSum', 'Hitung') : label('smartKindWords', 'Terbilang');
    smartKind.className = `skind${r ? ` k-${r.kind}` : ''}`;
    smartBox.classList.toggle('has', Boolean(smart.q.trim()));
    if (!homeBody) return;
    homeBody.replaceChildren(smart.q.trim() ? smartAnswer() : homeTiles(homeStatus || I.status));
  }
  // Its own buttons: an example to try, and copying.
  el.addEventListener('click', (e) => {
    const b = e.target.closest('[data-local^="smart"]');
    if (!b) return;
    const [what, ...rest] = b.dataset.local.split(':');
    if (what === 'smart') {
      smartInput.value = smart.q = rest.join(':');
      updateHomeBody();
      smartInput.focus();
    }
    if (what === 'smartcopy') copySmart(rest[0]);
  });

  // ---- home ----
  // The first page, open by default: the quick box, then one tile per other page (what comes next, the
  // pomodoro, the focus, music), each a click away from its page.
  let homeRendered = 0;
  function nextUp(st, now) {
    const c = I.cal;
    const ahead = (c?.events || []).filter((e) => !e.done && !e.allDay && (e.snoozedUntil || e.start) >= now).sort((a, b) => (a.snoozedUntil || a.start) - (b.snoozedUntil || b.start))[0];
    const pray = c?.prayer?.enabled && c.prayer.next;
    if (pray && (!ahead || pray.at < (ahead.snoozedUntil || ahead.start))) return { title: prayerName(pray.key), at: pray.at, prayer: true };
    return ahead ? { title: ahead.title || label('remUntitled', '(tanpa judul)'), at: ahead.snoozedUntil || ahead.start } : null;
  }
  // Each page's color: the glow under and inside the notch while it is open, and its tile on Home.
  const PAGE_COLOR = { home: '#2b8aff', timer: '#ff8a3d', later: '#2b8aff', prayer: '#3ddc97', media: '#a78bfa', focus: '#3ddc97', library: '#2b8aff', tasks: '#f5a524', go: '#2b8aff' };
  // A tile, tinted in its page's color with the page's icon large and faint behind it; a big figure
  // (a time, a count; none for music), what it is, and one line more. A click opens its page.
  function homeTile(pageIdTo, ico, cls, big, title, sub, has) {
    const t = div(`htile ${cls}${has ? '' : ' none'}`);
    if (pageIds.includes(pageIdTo)) t.dataset.page = pageIdTo;
    // (A task that needs the person keeps the warm color its class gives it.)
    if (PAGE_COLOR[pageIdTo] && !/\burgent\b/.test(cls)) t.style.setProperty('--k', PAGE_COLOR[pageIdTo]);
    const lead = div('hico');
    lead.append(ico);
    const g = div('grow');
    if (big) g.append(div('big', big));
    g.append(div('main', title), div('sub', sub));
    t.append(lead, g);
    return t;
  }
  // The four tiles.
  function homeTiles(st) {
    const now = Date.now();
    const grid = div('hgrid');
    const n = nextUp(st, now);
    grid.append(homeTile('later', n?.prayer ? mosque(13) : icon('bell', 13), 'k-later', n ? hm(n.at) : '', n ? n.title : label('homeNoNext', 'Tidak ada agenda'), n ? rel(n.at, now) : label('homeAddHint', 'Tambah pengingat di halaman Pengingat'), Boolean(n)));
    // Pomodoro: the running one (its countdown and session), or the pomodoro ready to start with its
    // length; the tile opens the Timer & pomodoro page.
    const t = st.timer;
    const pomo = st.pomodoro;
    grid.append(
      t
        ? homeTile('timer', icon('timer', 13), 'k-timer', mmss(timerMs(t)), timerLabel(t), t.paused ? label('pomoPaused', 'Dijeda') : t.sessions && phaseOf(t) ? fill(label('pomoSession', 'Sesi {n} dari {m}'), { n: t.round, m: t.sessions }) : label('homeTimerRun', 'Berjalan'), true)
        : homeTile('timer', icon('timer', 13), 'k-timer', pomo ? mmss(pomo.config.focusMin * 60000) : '', 'Pomodoro', pomo?.today ? fill(label('pomoToday', '{n} sesi hari ini'), { n: pomo.today }) : label('homePomoHint', 'Siap dimulai'), true)
    );
    // Focus: the work going on and how long it has run, or how to start one.
    const f = st.focusInfo?.session;
    grid.append(homeTile('focus', icon('folder', 13), 'k-focus', f ? fmtDur(now - f.startedAt) : '', f ? f.name : label('focusStart', 'Mulai fokus'), f ? label('focusActive', 'Fokus aktif') : label('homeFocusHint', 'Pilih folder atau konteks kerja'), Boolean(f)));
    const m = st.media;
    grid.append(homeTile('media', icon('music', 13), `k-media${m?.playing ? ' playing' : ''}`, '', m ? m.title : label('musicNone', 'Tidak ada musik diputar'), m ? [m.artist, m.app].filter(Boolean).join(' · ') : label('homeMusicHint', 'Putar lagu, kontrolnya ada di sini'), Boolean(m)));
    return grid;
  }
  // Home: the quick box on top, the tiles (or the box's answer) under it.
  function homePage(st) {
    const it = div('item widget homepage');
    smartInput.placeholder = label('smartPlaceholder', 'Hitung, atau tulis angka untuk terbilang…');
    homeStatus = st;
    homeBody = div('hbody');
    it.append(smartBox, homeBody);
    updateHomeBody();
    homeRendered = performance.now();
    return { id: 'home', icon: 'home', items: [it] };
  }
  // Notes & knowledge: their counts as two tiles, each opening its list (the panel main sends).
  function libraryPage(st) {
    const it = div('item widget libpage');
    const grid = div('hgrid');
    const tile = (act, ico, cls, n, title, sub) => {
      const t = homeTile('', ico, cls, String(n), title, sub, n > 0);
      t.dataset.act = act;
      t.setAttribute('role', 'button');
      t.style.cursor = 'pointer';
      return t;
    };
    grid.append(
      tile('notes', icon('note', 13), 'k-notes', st.notes || 0, label('notes', 'Catatan'), label('libNotesHint', 'Hasil dikte, tersimpan di Dokumen')),
      tile('knowledge', icon('book', 13), 'k-know', st.knowledge || 0, label('knowledge', 'Pengetahuan'), label('libKnowHint', 'File yang kamu drop ke notch'))
    );
    it.append(grid);
    return { id: 'library', icon: 'book', items: [it] };
  }
  // ---- weather (the left side) ----
  // Small motion icons drawn here (no assets): the sun's rays turn, clouds drift, rain and snow fall,
  // lightning flashes, fog slides; still under "reduce motion" and while the notch is closed.
  const CLOUD = '<path class="cl" d="M6.6 18.4h10.6a3.9 3.9 0 0 0 .6-7.75 5.3 5.3 0 0 0-10.3-1.1 4.25 4.25 0 0 0-.9 8.85z"/>';
  const SUN = (x, y, r) => `<g class="sun" style="transform-origin:${x}px ${y}px"><circle cx="${x}" cy="${y}" r="${r}"/><g class="rays">${[0, 45, 90, 135, 180, 225, 270, 315].map((a) => `<line x1="${x}" y1="${y - r - 1.6}" x2="${x}" y2="${y - r - 3.4}" transform="rotate(${a} ${x} ${y})"/>`).join('')}</g></g>`;
  const MOON = '<path class="moon" d="M15.5 4.5a7.5 7.5 0 1 0 4 13.6A8 8 0 0 1 15.5 4.5z"/>';
  const DROPS = (n, cls) => Array.from({ length: n }, (_, i) => `<line class="${cls}" x1="${8 + i * 3.2}" y1="19.5" x2="${7.2 + i * 3.2}" y2="22" style="animation-delay:${(i * 0.23).toFixed(2)}s"/>`).join('');
  const FLAKES = Array.from({ length: 3 }, (_, i) => `<circle class="flake" cx="${8.5 + i * 3.5}" cy="20.5" r=".9" style="animation-delay:${(i * 0.4).toFixed(1)}s"/>`).join('');
  function wxIcon(kind, day, size = 18) {
    const body = {
      clear: day ? SUN(12, 12, 4.2) : MOON,
      partly: `${day ? SUN(8.5, 8, 3.2) : '<g transform="translate(-4 -3) scale(.75)">' + MOON + '</g>'}<g class="drift">${CLOUD}</g>`,
      cloudy: `<g class="drift slow" transform="translate(-3 -4) scale(.7)">${CLOUD}</g><g class="drift">${CLOUD}</g>`,
      fog: '<g class="fogl"><line x1="4" y1="9" x2="20" y2="9"/><line x1="6" y1="13" x2="18" y2="13" style="animation-delay:.4s"/><line x1="3" y1="17" x2="17" y2="17" style="animation-delay:.8s"/></g>',
      drizzle: `<g transform="translate(0 -3)">${CLOUD}</g>${DROPS(3, 'drop')}`,
      rain: `<g transform="translate(0 -3)">${CLOUD}</g>${DROPS(4, 'drop fast')}`,
      storm: `<g transform="translate(0 -3)">${CLOUD}</g><path class="bolt" d="M12.6 14.2l-2.4 4h2l-1.2 3.6 3.4-4.8h-2.1l1.3-2.8z"/>${DROPS(2, 'drop fast')}`,
      snow: `<g transform="translate(0 -3)">${CLOUD}</g>${FLAKES}`,
    }[kind] || CLOUD;
    const span = document.createElement('span');
    span.className = `wxi k-${kind}`;
    span.innerHTML = `<svg viewBox="0 0 24 24" width="${size}" height="${size}">${body}</svg>`;
    return span;
  }
  function weatherRow(w) {
    if (w && w.temp != null) {
      const box = button('weather:refresh', '', 'wx');
      const top = div('wtop');
      top.append(wxIcon(w.kind, w.day, 30), div('wt', `${w.temp}°`));
      box.append(top);
      if (w.text) box.append(div('wc', w.text));
      const where = [w.place, w.max != null ? `${w.max}° / ${w.min}°` : ''].filter(Boolean).join(' · ');
      if (where) box.append(div('wp', where));
      if (w.feels != null) box.title = fill(label('wxFeels', 'terasa {t}°'), { t: w.feels });
      return box;
    }
    // No position from the laptop (no GPS/Wi-Fi location, or location off): ask for a city.
    if (w?.error) return button('weather:setup', label('wxSetup', 'Masukkan kota'), 'chip wxset');
    return null;
  }
  // ---- reminders ----
  // The Reminder Center: local reminders, Google Calendar events and prayer times, from one list that
  // main sends (calendar:data). Three views of that same list: Agenda (a week strip and the chosen
  // day), Timeline (what comes next, in order) and Calendar (a month grid, or a week row when compact).
  // Each row shows where it comes from (Google or this PC) and offers only what its source allows;
  // deleting asks first. Prayer times sit above: the next one with a bar filling toward it, and the
  // whole day when opened.
  let calRendered = 0;
  let calOpened = 0;
  const DAY = 86400000;
  const midnight = (ms) => {
    const d = new Date(ms);
    d.setHours(0, 0, 0, 0);
    return d.getTime();
  };
  const addDays = (ms, n) => {
    const d = new Date(ms);
    d.setDate(d.getDate() + n);
    return d.getTime();
  };
  const hm = (ms) => new Date(ms).toLocaleTimeString(LABELS.locale || 'id-ID', { hour: '2-digit', minute: '2-digit' });
  const calDay = () => I.calDay || midnight(Date.now());
  // "12 menit lagi", "1 jam 50 menit lagi", "5 menit lalu".
  function rel(ms, now) {
    const m = Math.round((ms - now) / 60000);
    if (Math.abs(m) < 1) return label('remNow', 'sekarang');
    if (m < 0) return -m < 60 ? fill(label('remAgo', '{n} menit lalu'), { n: -m }) : fill(label('remAgoH', '{n} jam lalu'), { n: Math.round(-m / 60) });
    if (m < 60) return fill(label('remInMin', '{n} menit lagi'), { n: m });
    const h = Math.floor(m / 60);
    if (h >= 24) return fill(label('remInDays', '{n} hari lagi'), { n: Math.round(h / 24) });
    return m % 60 ? fill(label('remInHM', '{h} jam {m} menit lagi'), { h, m: m % 60 }) : fill(label('remInHour', '{n} jam lagi'), { n: h });
  }
  const onDay = (e, day) => e.start < addDays(day, 1) && Math.max(e.end || e.start + 1, e.start + 1) > day;
  function rowState(e, now) {
    if (e.done) return 'done';
    const due = e.snoozedUntil || e.start;
    if (e.source === 'local' && e.kind === 'reminder' && due <= now) return 'overdue';
    if (e.end && e.start <= now && now < e.end) return 'ongoing';
    if ((e.end || e.start) < now && !e.allDay) return 'past';
    return 'upcoming';
  }
  const MOSQUE = '<svg viewBox="0 0 24 24" width="{s}" height="{s}" fill="currentColor"><path d="M12 2.5c.4 1.2 1.6 2 1.6 3.4a1.6 1.6 0 0 1-3.2 0c0-1.4 1.2-2.2 1.6-3.4ZM5 21V12.5C5 9 8 7.6 12 7.6s7 1.4 7 4.9V21h-4.5v-3.8a2.5 2.5 0 0 0-5 0V21H5Zm-3 0v-7.5l1.5-1.5L5 13.5V21H2Zm17 0v-7.5l1.5-1.5 1.5 1.5V21h-3Z"/></svg>';
  const mosque = (s = 16) => {
    const span = document.createElement('span');
    span.className = 'ico';
    span.innerHTML = MOSQUE.replaceAll('{s}', s);
    return span;
  };
  const prayerName = (k) => LABELS[`prayer_${k}`] || k;
  function srcBadge(e) {
    if (e.source === 'google') {
      const g = div('csrc g', 'G');
      g.title = e.calendar || label('remGoogle', 'Google Calendar');
      return g;
    }
    const l = div('csrc l');
    l.append(icon('home', 11));
    l.title = label('remLocal', 'Lokal');
    return l;
  }
  const tip = (b, t) => ((b.title = t), b);
  // One event or reminder. Its buttons show on hover or keyboard focus; the row itself does nothing.
  function calRow(e, now, isNext) {
    const st = rowState(e, now);
    const row = div(`crow st-${st}${isNext ? ' next' : ''}`);
    if (e.color) row.style.setProperty('--ev', e.color);
    row.append(div('ctime', e.allDay ? label('remAllDay', 'Seharian') : hm(e.start)), div('cmark'));
    const g = div('grow');
    g.append(div('main', e.title || label('remUntitled', '(tanpa judul)')));
    const bits = [e.source === 'google' ? e.calendar || label('remGoogle', 'Google Calendar') : label('remLocal', 'Lokal')];
    if (e.sync === 'pending') bits.push(label('remSaving', 'Menyimpan ke Google…'));
    else if (st === 'overdue') bits.push(label('remOverdue', 'Terlewat'));
    else if (st === 'ongoing') bits.push(label('remOngoing', 'Berlangsung'));
    else if (st === 'done') bits.push(label('remDone', 'Selesai'));
    else if (st === 'upcoming' && !e.allDay) bits.push(rel(e.snoozedUntil || e.start, now));
    if (e.snoozedUntil && st !== 'done') bits.push(fill(label('remSnoozed', 'ditunda sampai {t}'), { t: hm(e.snoozedUntil) }));
    if (e.recurring) bits.push('↻');
    g.append(div('sub', bits.join(' · ')));
    row.append(g, srcBadge(e));
    const ask = I.calAsk && I.calAsk.id === e.id && Date.now() < I.calAsk.until ? I.calAsk.kind : '';
    const act = div('cact');
    if (ask === 'delete') {
      row.classList.add('asking');
      act.append(button(`cal:delete:${e.id}`, label('remDelete', 'Hapus'), 'danger'), localButton('cal:askoff', label('remCancel', 'Batal'), 'chip'));
    } else if (ask === 'snooze') {
      row.classList.add('asking');
      act.append(...[['5', '5m'], ['15', '15m'], ['60', '1j'], ['tomorrow', label('remTomorrow', 'Besok')]].map(([v, t]) => button(`cal:snooze:${v}:${e.id}`, t, 'chip')), localButton('cal:askoff', '', '', 'close'));
    } else {
      const has = (a) => (e.actions || []).includes(a);
      if (has('done')) act.append(tip(button(`cal:done:${e.id}`, '', '', 'check'), label('remDoneBtn', 'Tandai selesai')));
      if (has('undo')) act.append(tip(button(`cal:undo:${e.id}`, '', '', 'IoRefresh'), label('remUndo', 'Batalkan selesai')));
      if (has('snooze')) act.append(tip(localButton(`cal:ask:snooze:${e.id}`, '', '', 'IoTimerOutline'), label('remSnooze', 'Tunda')));
      if (has('open')) act.append(tip(button(`cal:link:${e.id}`, '', '', 'open'), label('remOpen', 'Buka di Google')));
      if (has('edit')) act.append(tip(button(`cal:edit:${e.id}`, '', '', 'IoCreateOutline'), label('remEdit', 'Ubah')));
      if (has('delete')) act.append(tip(localButton(`cal:ask:delete:${e.id}`, '', 'danger', 'trash'), label('remDelete', 'Hapus')));
      // A Google event that can't be changed says why, instead of offering buttons that would fail.
      if (e.source === 'google' && !e.editable) {
        const ro = localButton('cal:none', '', 'off', 'key');
        ro.disabled = true;
        act.append(tip(ro, e.readOnlyReason === 'readOnlyEvent' ? label('remReadOnlyEvent', 'Acara ini tidak bisa diubah') : label('remReadOnly', 'Kalender ini hanya bisa dibaca')));
      }
    }
    row.append(act);
    return row;
  }
  // The day's list: at most `limit` rows, the next item highlighted; "+N" (into `moreHost`, the page's
  // header, so it takes no row) opens the Timeline. What still needs a look comes first (overdue,
  // happening, coming up); done and past items after.
  const RANK = { overdue: 0, ongoing: 1, upcoming: 2, past: 3, done: 4 };
  function dayList(events, day, now, limit, moreHost) {
    const list = events.filter((e) => onDay(e, day)).sort((a, b) => RANK[rowState(a, now)] - RANK[rowState(b, now)] || a.start - b.start);
    const box = div('clist');
    if (!list.length) {
      const empty = div('cempty');
      empty.append(div('', label('remNone', 'Tidak ada agenda')), button(`cal:add:${day}`, label('remAdd', 'Tambah'), 'chip', 'plus'));
      box.append(empty);
      return box;
    }
    const next = list.find((e) => !e.done && !e.allDay && (e.snoozedUntil || e.start) >= now);
    box.append(...list.slice(0, limit).map((e) => calRow(e, now, e === next)));
    if (list.length > limit) moreHost?.prepend(localButton('cal:view:timeline', fill(label('remMore', '+{n} lagi'), { n: list.length - limit }), 'chip'));
    return box;
  }
  const weekStart = (ms) => addDays(midnight(ms), -((new Date(ms).getDay() + 6) % 7));
  const hasOn = (events, day) => events.some((e) => onDay(e, day) && !e.done);
  // The chosen day's whole month, Monday first: from the week its 1st falls in to the week of its last
  // day (5 or 6 rows). Days of the months either side are dimmed.
  function monthGrid(events, today) {
    const sel = new Date(calDay());
    const first = new Date(sel.getFullYear(), sel.getMonth(), 1).getTime();
    const last = new Date(sel.getFullYear(), sel.getMonth() + 1, 0).getTime();
    const start = weekStart(first);
    const rows = Math.round((weekStart(last) - start) / (7 * DAY)) + 1;
    const grid = div('cgrid');
    for (let i = 0; i < 7; i++) grid.append(div('wh', new Date(addDays(start, i)).toLocaleDateString(LABELS.locale || 'id-ID', { weekday: 'short' }).slice(0, 3)));
    for (let i = 0; i < rows * 7; i++) {
      const day = addDays(start, i);
      const b = localButton(`cal:day:${day}`, String(new Date(day).getDate()), `cell${day === calDay() ? ' on' : ''}${day === today ? ' today' : ''}${new Date(day).getMonth() !== sel.getMonth() ? ' out' : ''}`);
      if (hasOn(events, day)) b.append(div('dot'));
      grid.append(b);
    }
    return grid;
  }
  // Timeline: overdue reminders first, then today onward, grouped by day, scrolling inside the notch.
  function timeline(events, now, today) {
    const box = div('ctline');
    const overdue = events.filter((e) => rowState(e, now) === 'overdue');
    const ahead = events.filter((e) => (e.end || e.start) >= today && rowState(e, now) !== 'overdue').slice(0, 40);
    const group = (title, list) => {
      if (!list.length) return;
      box.append(div('cgroup', title));
      const next = list.find((e) => !e.done && !e.allDay && (e.snoozedUntil || e.start) >= now);
      box.append(...list.map((e) => calRow(e, now, e === next)));
    };
    group(label('remOverdue', 'Terlewat'), overdue);
    const dayOf = (e) => Math.max(midnight(e.start), today);
    for (const d of [...new Set(ahead.map(dayOf))].sort((a, b) => a - b)) {
      const name = d === today ? label('remToday', 'Hari ini') : d === addDays(today, 1) ? label('remTomorrow', 'Besok') : new Date(d).toLocaleDateString(LABELS.locale || 'id-ID', { weekday: 'short', day: 'numeric', month: 'short' });
      group(name, ahead.filter((e) => dayOf(e) === d));
    }
    if (!overdue.length && !ahead.length) box.append(div('cempty', label('remNoneAhead', 'Belum ada yang akan datang')));
    return box;
  }
  // The next prayer as one slim line (its name, time, how long, and a bar filling toward it); a click
  // shows the day's times in its place. Without times yet: why (loading, or the error and a retry).
  function prayerLine(p, now) {
    if (!p.times?.length) {
      const line = div('cpline wait');
      line.append(mosque(12), div('', p.error ? label('remPrayerError', 'Jadwal salat belum bisa diambil') : label('remPrayerLoading', 'Mengambil jadwal salat…')));
      if (p.error) line.append(button('cal:refresh', label('remRetry', 'Coba lagi'), 'chip'));
      return line;
    }
    const line = localButton('cal:prayer', '', 'cpline');
    if (p.next) {
      const bar = div('pbar2');
      const done = document.createElement('i');
      done.style.width = `${Math.max(0, Math.min(1, (now - p.next.prev) / (p.next.at - p.next.prev))) * 100}%`;
      bar.append(done);
      line.append(mosque(12), div('pn', prayerName(p.next.key)), div('pa', hm(p.next.at)), div('pr', rel(p.next.at, now)), bar);
    }
    return line;
  }
  // The prayer page: "Dzuhur in 1 h 58 min" beside the city, method and notification, then the day's
  // eight times, four by two; a prayer's mark (on hover) ticks it off.
  function prayerPage(p, now) {
    const it = div('item widget praypage');
    const head = div('prhead');
    const nx = div('pnext');
    if (p.next) {
      const b = document.createElement('b');
      b.textContent = prayerName(p.next.key);
      nx.append(b, ` ${rel(p.next.at, now)}`);
    }
    const notify = p.notify < 0 ? label('remNotifyOff', 'Notifikasi mati') : p.notify === 0 ? label('remNotifyAt', 'Notifikasi tepat waktu') : fill(label('remNotifyBefore', 'Notifikasi {n} menit sebelum'), { n: p.notify });
    const meta = div('pmeta', [p.city, p.method, notify].filter(Boolean).join(' · '));
    meta.title = meta.textContent;
    head.append(nx, meta);
    it.append(head);
    if (!p.times?.length) {
      it.append(prayerLine(p, now));
      return { id: 'prayer', icon: 'bell', items: [it] };
    }
    const grid = div('pgrid');
    for (const t of p.times) {
      const cell = div(`pcell${p.next?.key === t.key && p.next.at === t.at ? ' next' : ''}${t.at < now ? ' past' : ''}`);
      cell.append(div('pn', prayerName(t.key)), div('pa', hm(t.at)));
      if (t.prayer && (t.done || t.at < now)) cell.append(tip(button(`cal:prayer:done:${t.key}`, '', t.done ? 'tick on' : 'tick', t.done ? 'check' : 'IoCheckmark'), t.done ? label('remDone', 'Selesai') : label('remDoneBtn', 'Tandai selesai')));
      grid.append(cell);
    }
    it.append(grid);
    calRendered = performance.now();
    return { id: 'prayer', icon: 'bell', live: Boolean(p.next && p.next.at - now < 15 * 60000), items: [it] };
  }
  function syncChip(c) {
    const s = c.sync?.state || 'off';
    if (!c.google?.configured || !c.google?.enabled) return button('cal:connect', label('remConnect', 'Sambungkan Google'), 'chip gconnect');
    // Just the G, colored by the sync's state; the words are in its tooltip.
    const text = { loading: label('remSyncing', 'Sinkron…'), ok: label('remSynced', 'Google'), off: label('remSynced', 'Google'), auth: label('remReconnect', 'Sambungkan ulang') }[s] || label('remSyncFail', 'Gagal sinkron');
    const b = button(s === 'auth' ? 'cal:connect' : 'cal:refresh', '', `chip gsync s-${s}`);
    b.title = c.sync?.at ? `${text} · ${label('remLastSync', 'Terakhir sinkron')} ${hm(c.sync.at)}` : text;
    return b;
  }
  const SYNC_WARN = { auth: ['remSyncAuth', 'Akses Google perlu disambung ulang'], scope: ['remSyncScope', 'Izin Google kurang'], rate: ['remSyncRate', 'Google sibuk, coba lagi nanti'], offline: ['remSyncOffline', 'Offline · pakai data terakhir'], error: ['remSyncFail', 'Google gagal sinkron'] };
  function remindersPage(c, st) {
    const now = Date.now();
    const today = midnight(now);
    const events = c?.events || [];
    // The view chosen in Settings (Agenda by default); the chips here switch it just for this look.
    const view = I.calView || c?.prefs?.view || 'agenda';
    const dens = c?.prefs?.density || 'compact';
    const it = div(`item widget rempage dens-${dens}`);
    const prayer = c?.prayer?.enabled && c.prayer.city;
    const head = div('chead2');
    const seg = div('seg');
    for (const [v, k, f] of [['agenda', 'remAgenda', 'Agenda'], ['timeline', 'remTimeline', 'Timeline'], ['calendar', 'remCalendar', 'Kalender']]) seg.append(localButton(`cal:view:${v}`, label(k, f), view === v ? 'chip on' : 'chip'));
    head.append(seg);
    // The day (Agenda) or the month (Calendar) to move through, in the header so it takes no row.
    const nav = div('cnav');
    const sel = new Date(calDay());
    if (view === 'agenda') nav.append(localButton('cal:daynav:-1', '‹', 'nav'), localButton('cal:today', calDay() === today ? label('remToday', 'Hari ini') : sel.toLocaleDateString(LABELS.locale || 'id-ID', { weekday: 'short', day: 'numeric', month: 'short' }), 'navday'), localButton('cal:daynav:1', '›', 'nav'));
    else if (view === 'calendar') nav.append(localButton('cal:monthnav:-1', '‹', 'nav'), div('navday', sel.toLocaleDateString(LABELS.locale || 'id-ID', { month: 'short', year: 'numeric' })), localButton('cal:monthnav:1', '›', 'nav'));
    head.append(nav);
    const tools = div('ctools');
    tools.append(
      syncChip(c || {}),
      tip(button(`cal:add:${calDay()}`, '', 'primary-ic', 'plus'), label('remAdd', 'Tambah'))
    );
    head.append(tools);
    it.append(head);
    // The next prayer above today's agenda; a click opens the prayer page.
    if (prayer && view === 'agenda' && calDay() === today) it.append(prayerLine(c.prayer, now));
    // No city yet: a line that leads to setting one up (so the times are right), or goes away.
    const guide = !prayer && c?.prayer?.guide && view === 'agenda' && calDay() === today;
    if (guide) {
      // The whole line leads to Settings' city field; only × is separate.
      const line = div('cpline guide');
      const suggest = c.prayer.suggest;
      const go = button(suggest ? 'cal:prayer:enable' : 'cal:prayer:setup', '', 'cpgo');
      go.append(mosque(12), div('pr', suggest ? fill(label('remPrayerSuggest', 'Tampilkan jadwal salat untuk {city}'), { city: suggest }) : label('remPrayerGuide', 'Atur kota supaya jadwal salat akurat')), div('cta', `${suggest ? label('remPrayerOn', 'Nyalakan') : label('remPrayerSetupBtn', 'Atur')} ›`));
      line.append(go, tip(button('cal:prayer:dismiss', '', '', 'close'), label('remPrayerHide', 'Sembunyikan')));
      it.append(line);
    }
    // A failed sync never looks like an empty calendar: it says so, and local items stay.
    const s = c?.sync?.state;
    if (c?.google?.enabled && SYNC_WARN[s]) {
      const warn = div('cwarn');
      const reconnect = s === 'auth' || s === 'scope';
      warn.append(div('', label(...SYNC_WARN[s])), button(reconnect ? 'cal:connect' : 'cal:refresh', reconnect ? label('remReconnect', 'Sambungkan ulang') : label('remRetry', 'Coba lagi'), 'chip'));
      it.append(warn);
    }
    if (view === 'timeline') it.append(timeline(events, now, today));
    else if (view === 'calendar') {
      const cal = div('ccal');
      cal.append(monthGrid(events, today), dayList(events, calDay(), now, 3, tools));
      it.append(cal);
    } else it.append(dayList(events, calDay(), now, 6, tools));
    calRendered = performance.now();
    // Opening the page asks main to sync when the month's events are stale (at most once a minute).
    if (performance.now() - calOpened > 60000) {
      calOpened = performance.now();
      aina.send('island:act', 'cal:open');
    }
    const soon = st.nextReminder && st.nextReminder.at - now < 60 * 60000;
    return { id: 'later', icon: 'bell', live: Boolean(soon), items: [it] };
  }
  // The chosen day moved: a day outside the loaded month loads that month.
  function selectDay(day) {
    I.calDay = midnight(day);
    const d = new Date(I.calDay);
    const month = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
    if (I.cal && I.cal.month !== month) aina.send('island:act', `cal:month:${month}`);
  }
  // ---- tasks ----
  // Background work to keep an eye on, not a task manager: at most two tasks, the ones to look at
  // first (needs you, failed, running, queued, then the finished), each with its status, how long it
  // has run, where it comes from (the Sprite, its project) and, when it failed, the short error. A
  // bar only for a task that reports a real percentage; otherwise a spinner and the running time.
  // Stop asks first and says what it does: a running Sprite is a local process to stop, one not
  // started yet is cancelled. History shows the finished ones, with Retry for failed or stopped work.
  let taskRefs = []; // the running times, moved every frame
  const LONG_TASK_MS = 15 * 60000; // a task running longer than this has its time shown in amber
  const TASK_LABEL = {
    needs: ['taskNeeds', 'Butuh kamu'],
    queued: ['taskQueued', 'Antre'],
    running: ['taskRunning', 'Berjalan'],
    failed: ['taskFailed', 'Gagal'],
    completed: ['taskCompleted', 'Selesai'],
    cancelled: ['taskCancelled', 'Dibatalkan'],
  };
  const taskLabel = (status) => LABELS[TASK_LABEL[status]?.[0]] || TASK_LABEL[status]?.[1] || status;
  const label = (key, fallback) => LABELS[key] || fallback;
  const isLive = (t) => t.status === 'running' || t.status === 'queued' || t.status === 'needs';
  const fmtRun = (ms) => {
    const s = Math.max(0, Math.floor(ms / 1000));
    const [h, m, sec] = [Math.floor(s / 3600), Math.floor((s % 3600) / 60), s % 60];
    const [H, M, S] = [label('durH', 'j'), label('durM', 'mnt'), label('durS', 'dtk')];
    return h ? `${h} ${H} ${m} ${M}` : m ? `${m} ${M} ${sec} ${S}` : `${sec} ${S}`;
  };
  // A button handled here in the notch (data-local), not by main.
  const localButton = (local, text, tone = '', ico = '') => {
    const b = button('', text, tone, ico);
    delete b.dataset.act;
    b.dataset.local = local;
    return b;
  };
  // A Sprite's card: its status dot, name, state and running time on the first line; under it what it
  // is doing now (or its task, or its error).
  function taskRow(t) {
    const it = div(`item task s-${t.status}`);
    it.title = t.task || '';
    const g = div('grow');
    const sub = div('sub tline');
    const time = document.createElement('span');
    time.className = 'ttime';
    taskRefs.push({ el: time, t });
    const from = [t.project, t.routine ? label('taskRoutine', 'Rutinitas') : ''].filter(Boolean).join(' · ');
    sub.append(div('sdot'), div('tname', t.name), div('tstat', taskLabel(t.status).toLowerCase()), ...(from ? [div('tfrom', from)] : []), time);
    g.append(sub);
    const doing = isLive(t) && t.line;
    g.append(div(doing ? 'tdesc' : 'tdesc plain', doing ? t.line : t.task || ''));
    // A real percentage gets a bar; nothing else pretends to know how far along a task is.
    if (typeof t.progress === 'number' && t.progress >= 0 && t.progress <= 1) {
      const bar = div('tbar');
      const done = document.createElement('i');
      done.style.width = `${(t.progress * 100).toFixed(1)}%`;
      bar.append(done);
      g.append(bar);
    }
    // Asked before stopping: "Hentikan Ciro?" with Hentikan / Lanjut in place of the row's buttons.
    const asking = I.stopAsk && I.stopAsk.id === t.id && Date.now() < I.stopAsk.until;
    const queued = t.status === 'queued';
    if (asking) {
      sub.replaceChildren(div('sdot'), div('tstat ask', fill(label(queued ? 'taskCancelAsk' : 'taskStopAsk', queued ? 'Batalkan {name}?' : 'Hentikan {name}?'), { name: t.name })));
      const ctl = div('tctl');
      ctl.append(button(`sprite:stop:${t.id}`, label(queued ? 'taskCancel' : 'taskStop', queued ? 'Batalkan' : 'Hentikan'), 'danger'), localButton('taskKeep', label('taskKeep', 'Lanjut'), 'chip'));
      it.append(g, ctl);
      return it;
    }
    // Under the name: a failure's error in place of what it was doing.
    if (t.status === 'failed' && t.error) g.querySelector('.tdesc').replaceWith(div('tdesc err', t.error));
    const ctl = div('tctl');
    if (t.status === 'failed') ctl.append(button(`agent:${t.slug}`, label('taskSeeError', 'Lihat error'), 'chip'));
    if (t.retry) ctl.append(button(`sprite:retry:${t.id}`, '', '', 'IoRefresh'));
    if (t.status !== 'failed') ctl.append(button(`agent:${t.slug}`, '', '', 'open'));
    if (t.stop) {
      const b = localButton('taskStop', '', 'danger', 'IoStop');
      b.dataset.id = t.id;
      b.title = label(queued ? 'taskCancel' : 'taskStop', queued ? 'Batalkan' : 'Hentikan');
      ctl.append(b);
    }
    it.append(g, ctl);
    return it;
  }
  function tasksPage(tasks) {
    const list = tasks.list || [];
    const history = tasks.history || [];
    if (!list.length && !history.length) return null;
    // Filter by the focus only when it splits the tasks: some in its project, some elsewhere.
    const all = [...list, ...history];
    const split = Boolean(tasks.focus) && all.some((t) => t.inFocus) && all.some((t) => !t.inFocus);
    const onlyFocus = split && I.taskFocus !== false;
    const keep = (t) => !onlyFocus || t.inFocus;
    const view = I.taskView === 'history' && history.length ? 'history' : list.length ? 'active' : 'history';
    const shown = (view === 'history' ? history : list).filter(keep);
    const items = [];
    const head = div('thead');
    head.append(localButton('taskView:active', `${label('taskActive', 'Aktif')} ${list.filter(keep).length}`, view === 'active' ? 'chip on' : 'chip'), localButton('taskView:history', `${label('taskHistory', 'Riwayat')} ${history.filter(keep).length}`, view === 'history' ? 'chip on' : 'chip'));
    if (split) head.append(localButton('taskFocus', onlyFocus ? tasks.focus : label('taskAllProjects', 'Semua'), `chip end${onlyFocus ? ' on focus' : ''}`, 'IoLocateOutline'));
    items.push(head);
    // The page scrolls, so up to four cards; the rest are in the Agents window.
    const MAX = 4;
    items.push(...shown.slice(0, MAX).map(taskRow));
    if (!shown.length) items.push(div('tempty', view === 'history' ? label('taskNoHistory', 'Belum ada tugas yang selesai') : label('taskNoneHere', 'Tidak ada tugas di fokus ini')));
    if (shown.length > MAX) head.append(button('agents', fill(label('taskMore', '+{n} lainnya'), { n: shown.length - MAX }), `chip${split ? '' : ' end'}`));
    const urgent = list.some((t) => t.status === 'needs' || t.status === 'failed');
    return { id: 'tasks', icon: 'spark', live: list.some(isLive), urgent, items };
  }
  // An older main (no tasks): the working Sprites, the first row with Stop (it stops them all).
  function agentsPage(st) {
    const items = st.agents.slice(0, 2).map((a, i) => rowItem(iconBox('spark', 'busy'), a.name, a.line || '…', button(`agent:${a.slug}`, '', '', 'open'), ...(i === 0 ? [button('stopAgents', '', 'danger', 'stop')] : [])));
    if (st.agents.length > 2) items[1].querySelector('.main').append(div('more', `+${st.agents.length - 2}`));
    return { id: 'tasks', icon: 'spark', live: true, items };
  }
  // Every frame on the tasks page: how long each task has run (or ran), amber once it runs long.
  function updateTasks() {
    const now = Date.now();
    for (const r of taskRefs) {
      const live = isLive(r.t);
      const ms = (live ? now : r.t.ended || now) - r.t.started;
      const text = fmtRun(ms);
      if (r.el.textContent !== text) r.el.textContent = text;
      r.el.classList.toggle('long', live && ms > LONG_TASK_MS);
    }
  }
  // One widget per page, so the peek grows sideways instead of down; the page tabs switch them. The
  // pages keep one order, as in the notch design: Home, Timer & pomodoro, Reminder Center, Prayer
  // times, Music, Focus, Notes & knowledge, Tasks, then the shortcuts. A page with nothing to show
  // (no prayer city, no notes, no tasks) is left out. `live` pages (still running) keep a badge on
  // their tab.
  function peekPages(st, locale) {
    const pages = [];
    countEl = null;
    peekRing = null;
    endsRef = null;
    taskRefs = [];
    focusRef = null;
    const fi = st.focusInfo;
    // Timer & pomodoro: the running one, or the pomodoro waiting to be started (v2 sends its settings).
    if (st.timer) pages.push({ id: 'timer', icon: 'timer', live: true, items: [timerItem(st.timer, st)] });
    else if (st.pomodoro) pages.push({ id: 'timer', icon: 'timer', items: [pomodoroIdle(st.pomodoro)] });
    // The Reminder Center (v2 sends calendar:data); an older main lists its reminders.
    if (I.cal) pages.push(remindersPage(I.cal, st));
    else if (st.reminders.length) {
      const items = st.reminders.slice(0, 2).map((r) => {
        const when = new Date(r.at);
        const sameDay = when.toDateString() === new Date().toDateString();
        return rowItem(iconBox('bell', 'warm'), r.text, when.toLocaleString(locale, sameDay ? { hour: '2-digit', minute: '2-digit' } : { weekday: 'short', hour: '2-digit', minute: '2-digit' }), button(`remindOff:${r.id}`, '', '', 'close'));
      });
      pages.push({ id: 'later', icon: 'bell', items });
    }
    // Prayer times (when they are on and have a city).
    if (I.cal?.prayer?.enabled && I.cal.prayer.city) pages.push(prayerPage(I.cal.prayer, Date.now()));
    // Music: always there to open from its tab, with a stand-in when nothing plays.
    pages.push({ id: 'media', icon: 'music', live: Boolean(st.media?.playing), items: [st.media ? mediaItem(st.media) : mediaIdle()] });
    // Focus: the running one, or how to start one. An older main (no focusInfo) shows the folder.
    if (fi?.session) pages.push({ id: 'focus', icon: 'folder', live: true, items: [focusItem(fi, st)] });
    else if (fi) pages.push({ id: 'focus', icon: 'folder', items: [focusIdle(fi)] });
    else if (st.focus) pages.push({ id: 'focus', icon: 'folder', items: [rowItem(iconBox('folder', 'green'), st.focus, LABELS.focus || 'Fokus', button('endFocus', LABELS.endFocus || 'Selesai', '', 'check'))] });
    // Notes & knowledge, their page opening their lists.
    if (st.notes || st.knowledge) pages.push(libraryPage(st));
    // Background tasks (a task that needs the person shows on Home and keeps a badge on this tab).
    const tasks = st.tasks ? tasksPage(st.tasks) : st.agents.length ? agentsPage(st) : null;
    if (tasks) pages.push(tasks);
    const tiles = st.shortcuts.map((s) => button(s.id, s.label, 'tile', 'bolt'));
    if (tiles.length) {
      const sc = div('shortcuts');
      sc.append(...tiles);
      pages.push({ id: 'go', icon: 'bolt', items: [sc] });
    }
    // Home leads (the page the peek opens on); it is built once the others are known.
    pageIds = ['home', ...pages.map((p) => p.id)];
    pages.unshift(homePage(st));
    return pages;
  }
  function goToPage(id) {
    const from = pageIds.indexOf(pageId);
    const to = pageIds.indexOf(id);
    if (to < 0 || to === from) return;
    pageDir = to > from ? 1 : -1;
    pageId = id;
    renderPeek();
    wake();
  }
  let lastWheel = 0;
  // The page's header (its title, and arrows to the pages either side) and, under the page, the page
  // indicator: dots, a pill on the open page. The indicator is built again only when the set of pages
  // changes; otherwise its classes, icons and the pill's place (--i) change in place, so the pill's
  // slide runs once and a hovered indicator stays hovered.
  const peekMain = div('pmain-col');
  const peekSep = div('vsep');
  const pageArrow = (dir) => {
    const b = document.createElement('button');
    b.className = 'parrow';
    b.dataset.pagestep = dir;
    b.setAttribute('aria-label', dir < 0 ? label('pagePrev', 'Halaman sebelumnya') : label('pageNext', 'Halaman berikutnya'));
    b.textContent = dir < 0 ? '‹' : '›';
    return b;
  };
  const phdEl = div('phd');
  const phdTitle = div('ptitle');
  const phdArrows = div('parrows');
  phdArrows.append(pageArrow(-1), pageArrow(1));
  phdEl.append(phdTitle, phdArrows);
  peekMain.append(phdEl);
  let pageEl = null;
  const PAGE_TITLE = {
    home: ['pageHome', 'Home'],
    timer: ['pageTimer', 'Timer & pomodoro'],
    later: ['pageReminders', 'Pusat Pengingat'],
    prayer: ['pagePrayer', 'Jadwal salat'],
    media: ['pageMusic', 'Musik'],
    focus: ['pageFocus', 'Mode Fokus'],
    library: ['pageLibrary', 'Catatan & Pengetahuan'],
    tasks: ['pageTasks', 'Tugas'],
    go: ['pageShortcuts', 'Pintasan'],
  };
  const pageTitle = (id) => (PAGE_TITLE[id] ? label(...PAGE_TITLE[id]) : '');
  function updateHeader(pages) {
    const t = pageTitle(pageId);
    if (phdTitle.textContent !== t) phdTitle.textContent = t;
    const at = pageIds.indexOf(pageId);
    const [prev, next] = phdArrows.children;
    prev.disabled = at <= 0;
    next.disabled = at >= pages.length - 1;
    phdArrows.style.visibility = pages.length < 2 ? 'hidden' : '';
  }
  const dotsEl = div('dots');
  dotsEl.setAttribute('role', 'tablist');
  let dotsKey = '';
  function updateDots(pages) {
    updateHeader(pages);
    if (pages.length < 2) {
      dotsEl.remove();
      dotsKey = '';
      return;
    }
    const key = pages.map((p) => p.id).join('|');
    if (key !== dotsKey) {
      dotsKey = key;
      const track = div('dtrack');
      track.append(div('pind'));
      for (const p of pages) {
        const d = div(`pdot k-${p.id}`);
        d.dataset.page = p.id;
        d.setAttribute('role', 'tab');
        d.title = pageTitle(p.id);
        track.append(d);
      }
      dotsEl.replaceChildren(track);
    }
    if (dotsEl.parentNode !== peekMain) peekMain.append(dotsEl);
    const at = pageIds.indexOf(pageId);
    dotsEl.querySelector('.pind').style.setProperty('--i', at);
    for (const [i, d] of [...dotsEl.querySelectorAll('.pdot')].entries()) {
      const p = pages[i];
      const on = p.id === pageId;
      d.classList.toggle('on', on);
      d.classList.toggle('live', Boolean(p.live));
      d.setAttribute('aria-selected', String(on));
      // Ionicons from react-icons: outline at rest, filled for the open page. The prayer page has
      // the mosque, drawn here.
      const name = PAGE_ICONS[p.id]?.[on ? 1 : 0] || p.icon;
      if (d.dataset.icon !== name) {
        d.dataset.icon = name;
        d.replaceChildren(p.id === 'prayer' ? mosque(11) : (PAGE_ICONS[p.id] && window.ainaReactIcon?.(name, 13)) || icon(p.icon, 11));
      }
    }
  }
  let swipe = 0; // sideways touchpad travel since the last page change
  let swipeIdle = 0;
  const pageStep = (dir) => {
    const i = pageIds.indexOf(pageId) + dir;
    if (i >= 0 && i < pageIds.length) goToPage(pageIds[i]);
  };
  // Whether something under the cursor can still scroll that way (then the wheel scrolls it).
  const canScroll = (target, dy) => {
    for (let n = target; n && n !== el; n = n.parentElement) {
      if (n.scrollHeight > n.clientHeight + 1 && /auto|scroll/.test(getComputedStyle(n).overflowY)) {
        if (dy < 0 ? n.scrollTop > 0 : n.scrollTop + n.clientHeight < n.scrollHeight - 1) return true;
      }
    }
    return false;
  };
  el.addEventListener(
    'wheel',
    (e) => {
      // Over the volume bar the wheel turns the volume, 5% a notch.
      const vbar = e.target.closest('.vbar');
      const m = I.status.media;
      if (vbar && m?.volume != null) {
        I.volume = Math.max(0, Math.min(1, (I.volume ?? m.volume) + (e.deltaY < 0 ? 0.05 : -0.05)));
        vbar.firstChild.style.width = `${I.volume * 100}%`;
        aina.send('island:act', `volume:${Math.round(I.volume * 100)}`);
        clearTimeout(I.volumeReset);
        I.volumeReset = setTimeout(() => (I.volume = null), 2500);
        return;
      }
      if (I.view !== 'peek' || pageIds.length < 2) return;
      const now = performance.now();
      // Two fingers sideways on a touchpad: the pages follow the swipe, one page per swipe.
      if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
        if (now - swipeIdle > 220) swipe = 0;
        swipeIdle = now;
        swipe += e.deltaX;
        if (Math.abs(swipe) > 60 && now - lastWheel > 420) {
          lastWheel = now;
          pageStep(swipe > 0 ? 1 : -1);
          swipe = 0;
        }
        return;
      }
      // The mouse wheel: a long page scrolls first, then the wheel changes pages.
      if (Math.abs(e.deltaY) < 4 || canScroll(e.target, e.deltaY)) return;
      if (now - lastWheel < 380) return;
      lastWheel = now;
      pageStep(e.deltaY > 0 ? 1 : -1);
    },
    { passive: true }
  );
  // Dragging a page sideways: it follows the cursor, and past 50 px it goes to the next or previous page
  // (else it springs back). Buttons and the volume bar keep their own presses.
  el.addEventListener('pointerdown', (e) => {
    const page = el.querySelector('.peek .page');
    if (I.view !== 'peek' || pageIds.length < 2 || e.button !== 0 || !page?.contains(e.target)) return;
    if (e.target.closest('button, input, .vbar, [data-seek]')) return;
    const x0 = e.clientX;
    let dx = 0;
    let dragging = false;
    // The page takes the pointer only once it is really dragged: a plain click must still reach what
    // was clicked (a Home tile opening its page, a reminder row).
    const move = (ev) => {
      dx = ev.clientX - x0;
      if (!dragging && Math.abs(dx) > 6) {
        dragging = true;
        page.classList.add('dragging');
        page.setPointerCapture(ev.pointerId);
      }
      if (!dragging) return;
      const i = pageIds.indexOf(pageId);
      const edge = (dx > 0 && i === 0) || (dx < 0 && i === pageIds.length - 1); // nothing that way: resists
      page.style.transition = 'none';
      page.style.transform = `translateX(${dx * (edge ? 0.15 : 0.5)}px)`;
      page.style.opacity = String(1 - Math.min(0.5, Math.abs(dx) / 320));
    };
    const up = () => {
      removeEventListener('pointermove', move);
      removeEventListener('pointerup', up);
      removeEventListener('pointercancel', up);
      page.classList.remove('dragging');
      const i = pageIds.indexOf(pageId);
      const to = dx < -50 ? i + 1 : dx > 50 ? i - 1 : i;
      if (dragging && to !== i && to >= 0 && to < pageIds.length) return goToPage(pageIds[to]);
      page.style.transition = 'transform .32s var(--ease-pop), opacity .2s ease';
      page.style.transform = '';
      page.style.opacity = '';
    };
    // On the window, so the press always ends here, wherever the pointer is let go.
    addEventListener('pointermove', move);
    addEventListener('pointerup', up);
    addEventListener('pointercancel', up);
  });
  // Each page's tab icon from react-icons (reacticons.js): [at rest, open].
  const PAGE_ICONS = {
    timer: ['IoTimerOutline', 'IoTimer'],
    media: ['IoMusicalNotesOutline', 'IoMusicalNotes'],
    tasks: ['IoSparklesOutline', 'IoSparkles'],
    focus: ['IoFolderOpenOutline', 'IoFolderOpen'],
    later: ['IoNotificationsOutline', 'IoNotifications'],
    go: ['IoFlashOutline', 'IoFlash'],
  };
  // The clock as hours, a separator that ticks, and minutes; rebuilt only when the minute changes.
  function setClock(c) {
    const parts = new Intl.DateTimeFormat(LABELS.locale || 'id-ID', { hour: '2-digit', minute: '2-digit' }).formatToParts(new Date());
    const key = parts.map((p) => p.value).join('');
    if (c.dataset.t === key) return;
    c.dataset.t = key;
    c.replaceChildren(
      ...parts.map((p) => {
        if (p.type !== 'literal' || !/^[:.]$/.test(p.value)) return document.createTextNode(p.value);
        const s = document.createElement('span');
        s.className = 'sep';
        s.textContent = p.value;
        return s;
      })
    );
  }
  // Laid out sideways: the clock, the weather and the credits on the left; on the right the page's
  // title, the page, and its dots. The page is the same size on every page and scrolls when longer.
  function renderPeek() {
    const st = I.status;
    // While the quick box is typed in, Home isn't rebuilt (that would pull the field out from under
    // the keyboard); only its tiles follow the news.
    if (I.typing && pageId === 'home' && peekEl.contains(smartBox)) {
      homeStatus = st;
      if (!smart.q.trim()) updateHomeBody();
      return;
    }
    const locale = LABELS.locale || 'id-ID';
    const side = div('top');
    clockEl = div('clock');
    setClock(clockEl);
    // The clock and date, then aina, then the weather.
    side.append(clockEl, div('date', new Date().toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long' })), peekMascot);
    const wx = weatherRow(st.weather);
    if (wx) side.append(wx);
    // aina Cloud: the credits left, at the bottom of the column; signed out, a line asking to sign in.
    const cr = div(`credits${st.cloud ? '' : ' signedout'}`);
    cr.append(icon('spark', 13), st.cloud ? fill(label('creditsLeft', '{n} kredit tersisa'), { n: st.cloud.credits ?? 0 }) : label('creditsSignIn', 'Silakan login dulu'));
    // Signed out: a Login button beside it, opening Settings at the account.
    if (!st.cloud) cr.append(button('cloud:settings', label('creditsLogin', 'Login'), 'clogin'));
    side.append(cr);
    const pages = peekPages(st, locale);
    pageIds = pages.map((p) => p.id);
    if (!pageIds.includes(pageId)) pageId = pageIds[0] || '';
    const page = div('page');
    const fit = div('pfit');
    fit.append(...(pages.find((p) => p.id === pageId)?.items || []));
    page.append(fit);
    // A rebuild of the same page (new data) keeps its scroll; a new page starts at the top.
    // (Not data-page: that attribute means "go to this page" to the click handler.)
    const keepScroll = !pageDir && pageEl?.dataset.pid === pageId ? pageEl.scrollTop : 0;
    page.dataset.pid = pageId;
    if (pageDir) page.classList.add(pageDir > 0 ? 'slide-next' : 'slide-prev');
    pageDir = 0;
    // The column with the page and its indicator stays in the notch: only the page inside it is
    // swapped, and the indicator is updated in place, so its pill slides once and its hover holds.
    if (pageEl?.parentNode === peekMain) pageEl.replaceWith(page);
    else phdEl.after(page);
    pageEl = page;
    if (keepScroll) page.scrollTop = keepScroll;
    updateDots(pages);
    peekEl.classList.toggle('enter', peekFresh && !reduceMotion);
    peekEl.classList.toggle('empty', !pages.length);
    peekFresh = false;
    if (peekEl.lastElementChild === peekMain && peekEl.firstElementChild !== peekMain) peekEl.firstElementChild.replaceWith(side);
    else peekEl.replaceChildren(side, peekSep, peekMain);
  }

  // ---- hover ----
  // The hit area is the notch plus a margin, and a wider strip while it is small. Main is told again
  // every 700 ms while inside, since it ignores the request while the Brain moves the mouse. Only the
  // cursor position counts: switching the window to take clicks fires a mouseout that means nothing.
  // Once clickable the window gets every move on this display, so leaving the notch is always seen.
  let lastSent = 0;
  function setHover(inside) {
    const now = performance.now();
    if (inside === I.hover && (!inside || now - lastSent < 700)) return;
    lastSent = now;
    if (inside !== I.hover) {
      I.hover = inside;
      I.peekSince = inside ? now : 0;
      if (I.fullscreen && I.revealed) inside ? clearTimeout(hideTimer) : unreveal(500);
      wake();
    }
    aina.send('island:interactive', inside);
  }
  // Over a full-screen app the notch is hidden and takes no clicks. Resting the cursor at the very top,
  // near the middle, for half a second calls it up, already open (the peek, not the small pill); once
  // the cursor has left it again, it hides.
  const REVEAL_MS = 500;
  let revealTimer = 0;
  let hideTimer = 0;
  function unreveal(after) {
    clearTimeout(hideTimer);
    hideTimer = setTimeout(() => {
      if (I.hover && I.fullscreen) return;
      I.revealed = false;
      wake();
    }, after);
  }
  function watchReveal(e) {
    const atTop = e.clientY <= 2 && Math.abs(e.clientX - innerWidth / 2) <= 180;
    if (!atTop) {
      clearTimeout(revealTimer);
      revealTimer = 0;
      return;
    }
    if (revealTimer) return;
    revealTimer = setTimeout(() => {
      revealTimer = 0;
      I.revealed = true;
      unreveal(2500); // called up but not used: it tucks away again
      // Straight to the peek: hovered as of now, without the usual 120 ms before it opens.
      setHover(true);
      I.peekSince = performance.now() - 1000;
      wake();
    }, REVEAL_MS);
  }
  // Where the notch wants the cursor (cursor:zone): its hover area (as the test below, with room to
  // come in) and the strip at the top that calls it up over a full-screen app. Nothing on a display
  // the notch isn't on. Main sends no other moves, so the overlay sleeps while the mouse works elsewhere.
  let zoneKey = '';
  function reportZone(here) {
    let rects = [];
    if (here) {
      const w = Math.max(Math.max(S.w.x, I.target.w) + 24, 380) + 40;
      const h = Math.max(Math.max(S.h.x, I.target.h, 70) + 10, 26) + 30;
      rects = [[Math.round((innerWidth - w) / 2), 0, Math.round(w), Math.round(h)]];
    }
    // Rounded, so a notch springing open sends a few updates, not one a frame.
    const key = rects.map((r) => r.map((v) => Math.round(v / 20))).join();
    if (key === zoneKey) return;
    zoneKey = key;
    aina.send('cursor:zone', { part: 'island', rects });
  }

  document.addEventListener('mousemove', (e) => {
    if (!isHere()) return setHover(false);
    if (I.fullscreen && !I.revealed) {
      watchReveal(e);
      return setHover(false);
    }
    // The size it is growing to counts too, so moving down into a peek that is still opening keeps it
    // open; while hovered (peek about to open) the zone reaches a little lower.
    const w = Math.max(Math.max(S.w.x, I.target.w) + 24, 240);
    const h = Math.max(Math.max(S.h.x, I.target.h, I.hover ? 70 : 0) + 10, 26);
    setHover(Math.abs(e.clientX - innerWidth / 2) <= w / 2 && e.clientY <= h);
    // The fallback eyes look toward the cursor (steps of 0.1, so the state machine isn't poked every pixel).
    if (eyes) eyes.set('lookX', Math.round(Math.max(-1, Math.min(1, (e.clientX - innerWidth / 2) / 400)) * 10) / 10);
    // aina perks up under the cursor (the overlay is click-through, so Rive can't see it itself).
    let over = false;
    if (M.draw !== 'off') {
      const r = mascotCanvas.getBoundingClientRect();
      over = e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom;
    }
    if (mascot.set('interaction', over ? (e.buttons ? 'pressed' : 'hover') : 'none')) nudge();
    // Shaken: the cursor turning back and forth over the notch six times in two seconds.
    const dx = e.clientX - M.lastX;
    M.lastX = e.clientX;
    if (I.hover && Math.abs(dx) > 4 && Math.sign(dx) !== M.dir) {
      M.dir = Math.sign(dx);
      const now = Date.now();
      M.turns = M.turns.filter((t) => now - t < 2000);
      M.turns.push(now);
      if (M.turns.length >= 6) {
        M.turns = [];
        flash('isDizzy', 2500);
      }
    }
  });

  // ---- wave ----
  const BARS = 22;
  const heights = new Array(BARS).fill(0.08);
  const waveRgb = [90, 169, 255];
  function drawWave(now, active, color) {
    const w = wave.clientWidth;
    const h = wave.clientHeight;
    if (wave.width !== w * devicePixelRatio) {
      wave.width = w * devicePixelRatio;
      wave.height = h * devicePixelRatio;
    }
    wg.setTransform(devicePixelRatio, 0, 0, devicePixelRatio, 0, 0);
    wg.clearRect(0, 0, w, h);
    const gap = w / BARS;
    for (let i = 0; i < BARS; i++) {
      // Bars near the center move most, like an audio visualizer.
      const center = 1 - Math.abs(i - (BARS - 1) / 2) / (BARS / 2);
      let target;
      if (active === 'mic') target = 0.1 + I.level * (0.5 + 0.9 * center) * (0.65 + 0.35 * Math.sin(now / 70 + i * 1.7));
      else if (active === 'voice') target = 0.15 + (0.35 + 0.5 * center) * Math.abs(Math.sin(now / 110 + i * 0.9) * Math.sin(now / 263 + i * 0.37));
      else if (active === 'think') target = 0.1 + 0.18 * (0.5 + 0.5 * Math.sin(now / 180 - i * 0.45));
      else target = 0.08;
      heights[i] += (Math.min(1, target) - heights[i]) * 0.35;
      const bh = Math.max(3, heights[i] * h);
      wg.fillStyle = color;
      wg.globalAlpha = 0.55 + 0.45 * center;
      wg.beginPath();
      wg.roundRect(i * gap + gap * 0.25, (h - bh) / 2, gap * 0.5, bh, gap * 0.25);
      wg.fill();
    }
  }

  let shownLabel = '';
  function frame(now) {
    raf = 0;
    const dt = Math.min(1 / 30, lastNow ? (now - lastNow) / 1000 : 1 / 60);
    lastNow = now;
    const listening = I.phase === 'listening' || I.phase === 'conversing';
    const busy = I.phase === 'transcribing' || I.phase === 'looking' || I.phase === 'thinking' || (I.phase === 'acting' && !I.voice);
    const waiting = I.phase === 'confirm' || I.phase === 'asking';
    const hasText = Boolean(I.text || I.error);
    const lingering = !I.hideAt || now < I.hideAt;
    const here = isHere();
    const turn = here && (I.phase !== 'idle' || (hasText && lingering));
    const st = I.status;
    // A drop zone or an upload owns the notch, except while aina asks the user something.
    const own = I.panel && (I.panel.kind === 'drop' || I.panel.kind === 'upload') && !waiting ? I.panel.kind : null;
    // A song playing is enough for the compact pill; a new one opens the track card for a moment.
    const music = Boolean(st.media?.playing);
    const track = Boolean(st.media) && now < I.trackUntil;
    const next = st.nextReminder && st.nextReminder.at - Date.now() < 60 * 60000 ? st.nextReminder : null;
    const view = !here ? 'rest' : own || (turn ? 'turn' : I.panel ? 'panel' : (I.hover && now - I.peekSince > 120) || I.typing ? 'peek' : track ? 'track' : st.agents.length || st.timer || st.focus || music || next ? 'mini' : 'rest');
    if (view !== I.view) {
      I.view = view;
      // The new mode's content fades and focuses in instead of popping in.
      content.classList.remove('view-in');
      void content.offsetWidth;
      if (!reduceMotion) content.classList.add('view-in');
      if (view === 'track') renderTrack();
      if (view === 'peek') {
        // Each time it opens, the peek starts at the most relevant page, unless a new song just
        // started (then its music page).
        if (!(now - (I.trackAt || -1e9) < 8000)) pageId = '';
        I.calView = ''; // each time it opens, the reminder page starts with the view from Settings
        peekFresh = true;
        renderPeek();
      }
      playOnly(view === 'drop' ? 'file-upload' : view === 'upload' ? 'cloud' : null);
    }
    if (I.hover && (view === 'rest' || view === 'mini')) wake(); // the peek opens after a short hover
    const open = view !== 'rest' && view !== 'mini';
    if (view === 'turn' && (hasText || waiting || I.panel || (busy && Boolean(I.transcript)))) I.wide = true;
    if (view !== 'turn') I.wide = false;
    const wide = view === 'turn' && I.wide;

    el.classList.toggle('here', here);
    // Over a full-screen app only what the person started or must answer shows: their own turn, a
    // question, a drop zone or an upload. Passive notices, the pill and the peek wait.
    const asked = view === 'turn' || view === 'drop' || view === 'upload' || (view === 'panel' && I.panel?.kind === 'confirm');
    el.classList.toggle('fs-hidden', I.fullscreen && !I.revealed && !asked);
    el.classList.toggle('open', open);
    el.classList.toggle('expanded', wide);
    el.classList.toggle('hover', I.hover);
    el.classList.toggle('has-panel', Boolean(I.panel) && (view === 'turn' || view === 'panel' || view === 'drop'));
    el.classList.toggle('drop-over', I.dropOver);
    for (const m of ['rest', 'mini', 'turn', 'panel', 'peek', 'drop', 'upload', 'track']) el.classList.toggle(`mode-${m}`, view === m);
    if (view !== 'mini') el.classList.remove('mini-music');
    el.classList.toggle('error', Boolean(I.error));
    el.classList.toggle('dictation', I.mode === 'dictation');
    el.classList.toggle('waiting', waiting);
    // A panel with one line shows it under its title, beside aina; a short one (no note or list, at
    // most two buttons) is a single row with its buttons on the right, like a notice.
    const pnl = view === 'panel' ? I.panel : null;
    const panelSub = Boolean(pnl) && (pnl.lines || []).length === 1;
    el.classList.toggle('panel-sub', panelSub);
    el.classList.toggle('panel-compact', Boolean(pnl) && (pnl.lines || []).length <= 1 && !pnl.note && !pnl.list && (pnl.actions || []).length <= 2);

    // Mini, the compact state: aina on the left, then what is going on: a pulsing dot and the Sprite at
    // work, the countdown's ring and the time, a dot and the reminder coming, an equalizer and the song
    // (the line being sung), or a folder and the focus with how long it has run.
    let miniKind = '';
    let miniMascot = false;
    if (view === 'mini') {
      const t = st.timer;
      // What is going on, most pressing first: background tasks, the timer, a reminder within the hour
      // (pulsing in its last 10 minutes), a song (the line being sung, or its title), the focus.
      const kind = st.agents.length ? 'agents' : t ? 'timer' : next ? 'remind' : music ? 'music' : st.focus ? 'focus' : 'none';
      miniKind = kind;
      for (const k of ['agents', 'timer', 'remind', 'music', 'focus', 'none']) miniEl.classList.toggle(`kind-${k}`, kind === k);
      const m = st.media;
      const cover = kind === 'music' && I.art.key === m?.key && I.art.src;
      if (cover && miniCover.getAttribute('src') !== I.art.src) miniCover.src = I.art.src;
      miniArt.classList.toggle('cover', Boolean(cover));
      // aina is on the pill's left, unless a song's cover is.
      miniMascot = !cover;
      const lines = kind === 'music' ? lyricLines(m) : null;
      const pos = lines ? mediaPos(m) : null;
      const lyric = lines && pos != null ? (lines[lyricAt(lines, pos)]?.[1] || '') : '';
      miniEl.classList.toggle('soon', Boolean(next) && next.at - Date.now() < 10 * 60000);
      miniEl.classList.toggle('music', music);
      for (const p of ['work', 'short', 'long']) miniEl.classList.toggle(`phase-${p}`, phaseOf(t) === p);
      miniEl.classList.toggle('paused', Boolean(t?.paused));
      if (t) miniRing.set(timerLeft(t));
      const fs = st.focusInfo?.session;
      const [text, sub] =
        kind === 'agents' ? [`${st.agents[0].name || 'Sprite'} ${label('miniWorking', 'bekerja')}`, [st.agents.length > 1 ? `+${st.agents.length - 1}` : '', t ? mmss(timerMs(t)) : ''].filter(Boolean).join(' · ')]
        : kind === 'timer' ? [mmss(timerMs(t)), t.paused ? (t.left === t.ms ? label('pomoReady', 'Siap') : label('pomoPaused', 'Dijeda')) : timerLabel(t)]
        : kind === 'remind' ? [next.title, rel(next.at, Date.now())]
        : kind === 'music' ? [lyric || m.title || '', lyric ? '' : m.artist || '']
        : kind === 'focus' ? [st.focus || '', fs?.startedAt ? fmtDur(Date.now() - fs.startedAt) : '']
        : ['', ''];
      if (miniText.textContent !== text) miniText.textContent = text;
      if (miniSub.textContent !== sub) miniSub.textContent = sub;
      miniEl.classList.toggle('lyric', Boolean(lyric));
      // A song playing (not paused): the drifting violet shadow under the pill.
      el.classList.toggle('mini-music', kind === 'music' && music);
    }

    // aina: where he shows and how he draws. Nowhere over a full-screen app or while the notch rests.
    const expr = faceState(view, listening, busy, waiting, now);
    const slot = el.classList.contains('fs-hidden') ? null : view === 'turn' || view === 'panel' ? 'row' : view === 'peek' ? 'peek' : miniMascot ? 'mini' : null;
    placeMascot(slot, slot === 'mini' || (slot === 'row' && expr === 'idle') ? 'burst' : slot ? 'live' : 'off', expr);

    // The mode's color: the shadow under the notch, the glow inside it and the turn's label.
    const glow = I.error ? '#ff5a64'
      : waiting || (I.mode === 'dictation' && listening) ? '#ff8a3d'
      : view === 'peek' ? PAGE_COLOR[pageId] || '#2b8aff'
      : view === 'mini' ? { timer: '#ff8a3d', music: '#a78bfa', agents: '#f5a524', focus: '#3ddc97', remind: next && next.at - Date.now() < 10 * 60000 ? '#ffb27a' : '#8b7cff' }[miniKind] || '#2b8aff'
      : view === 'panel' && I.panel?.kind === 'credits' ? '#ffb27a'
      // The next pomodoro phase waiting to start: focus orange, a break green.
      : view === 'panel' && I.panel?.kind === 'pomodoro' ? (I.panel.next === 'work' ? '#ff8a3d' : '#3ddc97')
      : view === 'panel' && (I.panel?.kind === 'uploaded' || I.panel?.kind === 'agentDone' || I.panel?.kind === 'dictation') ? '#3ddc97'
      : '#2b8aff';
    if (glow !== I.glow) {
      I.glow = glow;
      el.style.setProperty('--glow', glow);
      el.style.setProperty('--glow-text', glow === '#2b8aff' ? '#5aa9ff' : glow);
    }

    // The content is laid out at the notch's final width; the notch grows around it.
    const sized = (w, min = 48) => ({ w, h: 0, min, r: 26, ear: 14 });
    const PANEL_W = { dictation: 600, confirm: 600, credits: 440 };
    const target =
      view === 'rest' ? (I.hover ? SIZES.restHover : SIZES.rest)
      // The pill fits its line (measured by the text's full width, not the part shown now): at most
      // 440 px, except a song's lyric, which is shown whole (up to 90 % of the screen).
      : view === 'mini' ? { ...SIZES.mini, w: Math.min(miniEl.classList.contains('lyric') ? fitWidth(2000) : 440, Math.max(SIZES.mini.w, miniMain.scrollWidth - miniText.clientWidth + miniText.scrollWidth + 66) + (I.hover ? 12 : 0)), h: SIZES.mini.h + (I.hover ? 2 : 0) }
      : view === 'track' ? sized(fitWidth(440))
      : view === 'peek' ? { ...SIZES.peek, w: fitWidth(pageIds.length ? SIZES.peek.w : 260) }
      : view === 'drop' ? sized(360) // DROP_ZONE in notch.rs
      : view === 'upload' ? { w: 320, h: 48, r: 24, ear: 12 }
      : view === 'panel' ? sized(fitWidth(PANEL_W[I.panel?.kind] || 460))
      : wide ? sized(wideWidth(), SIZES.turn.h)
      : { ...SIZES.turn, w: wideWidth() };
    content.style.width = `${open ? target.w : SIZES.turn.w}px`;
    if (open && target.h === 0) target.h = Math.max(target.min || 48, content.offsetHeight);
    I.target = target;
    reportZone(here);
    let settled = true;
    for (const k of ['w', 'h', 'r', 'ear']) settled = step(S[k], target[k], dt, target[k] > S[k].x ? ZETA_GROW : ZETA_SHRINK) && settled;
    el.style.setProperty('--w', `${S.w.x.toFixed(2)}px`);
    el.style.setProperty('--h', `${Math.max(0, S.h.x).toFixed(2)}px`);
    el.style.setProperty('--r', `${Math.max(0, S.r.x).toFixed(2)}px`);
    el.style.setProperty('--ear', `${Math.max(0, S.ear.x).toFixed(2)}px`);

    if (!open) {
      // Keep running until the notch has settled; a mini pill with a countdown ticks once a second.
      if (!settled) wake();
      else {
        lastNow = 0;
        // A mini pill ticks once a second with a countdown, else once a minute for its clock.
        // A mini pill ticks once a second with a countdown, every 0.4 s while it follows a song's
        // lyrics, else once a minute for its clock (aina keeps his own time, burstStep).
        const following = miniEl.classList.contains('kind-music') && miniEl.classList.contains('lyric');
        if (view === 'mini') setTimeout(wake, st.timer && !st.timer.paused ? 1000 - (Date.now() % 1000) : following ? 400 : 60000 - (Date.now() % 60000));
      }
      return;
    }
    wake();

    if (view === 'track') {
      updateMedia();
      return;
    }
    if (view === 'peek') {
      updateMedia();
      if (clockEl) setClock(clockEl);
      if (countEl && st.timer) countEl.textContent = mmss(timerMs(st.timer));
      if (peekRing && st.timer) peekRing.set(timerLeft(st.timer));
      updateEnds();
      updateFocus();
      updateTasks();
      // The reminder page's "in 12 minutes" and the prayer bar move on twice a minute.
      if ((pageId === 'later' || pageId === 'prayer') && now - calRendered > 30000 && !I.dragging) renderPeek();
      if (pageId === 'home' && now - homeRendered > (st.timer && !st.timer.paused ? 1000 : 30000) && !I.dragging) renderPeek();
      return;
    }
    if (view === 'upload') {
      I.bar += (uploadProgress(I.panel.progress) - I.bar) * 0.12;
      barEl.style.width = `${(I.bar * 100).toFixed(2)}%`;
      const pct = `${Math.round(I.bar * 100)}%`;
      if (upPct.textContent !== pct) upPct.textContent = pct;
      return;
    }
    if (view === 'drop') return;

    const rowLabel = view === 'panel' ? I.panel.title || 'aina' : I.error ? LABELS.error : LABELS[listening && I.mode === 'dictation' ? 'dictation' : I.phase] || 'aina';
    if (rowLabel !== shownLabel) {
      shownLabel = rowLabel;
      labelEl.classList.remove('swap');
      void labelEl.offsetWidth; // restart the fade-in for the new label
      labelEl.classList.add('swap');
    }
    const timer = I.elapsed && !I.error ? ` ${I.elapsed.s} ${LABELS.seconds || 's'}` : '';
    labelEl.textContent = rowLabel + timer;
    const hint = I.elapsed && I.elapsed.hint ? I.elapsed.hint : waiting && I.hint ? I.hint : '';
    const sub = view === 'panel' ? (panelSub ? I.panel.lines[0] : '') : I.error ? '' : hint || (I.transcript ? '“' + I.transcript + '”' : '');
    if (youEl.textContent !== sub) youEl.textContent = sub;

    if (I.error) {
      sayEl.textContent = I.error;
    } else {
      const t = Math.min(1, (now - I.typeStart) / I.typeMs);
      const n = Math.min(I.text.length, Math.max(Math.round(I.text.length * t), I.spokenChars));
      sayEl.textContent = I.text.slice(0, n);
      sayEl.classList.toggle('typing', t < 1);
    }

    // The wave's color eases toward the new state instead of switching at once.
    const goal = I.error ? [255, 90, 100] : (I.mode === 'dictation' && listening) || waiting ? [255, 138, 61] : [90, 169, 255];
    for (let i = 0; i < 3; i++) waveRgb[i] += (goal[i] - waveRgb[i]) * 0.15;
    drawWave(now, listening ? 'mic' : busy ? 'think' : I.voice ? 'voice' : 'rest', `rgb(${waveRgb.map(Math.round).join(',')})`);
  }
  wake();
})();
