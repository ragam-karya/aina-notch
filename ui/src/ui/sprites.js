// Sprite panel: a mini card at the top right for every Sprite aina spawned (aina v2 sends
// sprites:status to the primary display's overlay; v1 never does, so this stays empty there).
// Cards stack down from the top-right corner, newest first; when a column is full the next one starts
// to its left. Hovering a card (after a short delay) opens its detail beside it: the task, its latest
// steps, the status, how long it has run, the result, and the OK buttons when it waits for one.
// Everything shown comes from the task's own events; nothing here pretends progress.
// The overlay is click-through: main forwards cursor moves (no :hover), so cards are hit-tested here and
// main is asked for clicks only while the cursor is on a card or its detail (island:interactive).
(function () {
  const CARD_W = 76; // square tiles
  const CARD_H = 76;
  const GAP = 8;
  const EDGE = 12;
  const HOVER_DELAY = 320;
  const LEAVE_DELAY = 160;

  const root = document.createElement('div');
  root.id = 'sprites';
  const detail = document.createElement('div');
  detail.className = 'sp-detail';
  root.append(detail);
  document.body.append(root);

  let LABELS = {};
  aina.on('islandLabels', (l) => {
    LABELS = l || {};
    render();
  });
  const L = (k, d) => LABELS[k] || d;

  let list = [];
  let skew = 0; // main's clock minus ours, for the elapsed time
  const cards = new Map(); // id -> element
  aina.on('sprites:status', (st) => {
    list = (st && st.sprites) || [];
    skew = st && st.now ? st.now - Date.now() : 0;
    render();
  });

  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };
  const button = (cls, text, tip, act) => {
    const b = el('button', cls, text);
    b.dataset.tip = tip;
    b.setAttribute('aria-label', tip);
    b.addEventListener('click', (e) => {
      e.stopPropagation();
      aina.send('island:act', act);
    });
    return b;
  };

  const ICON = { success: '✓', error: '✕', cancelled: '■', needs_approval: '!' };

  // How many cards fit in a column: down to the work area's bottom (above the taskbar).
  function perColumn() {
    const bottom = Math.min(innerHeight, screen.availHeight || innerHeight) - EDGE;
    return Math.max(1, Math.floor((bottom - EDGE + GAP) / (CARD_H + GAP)));
  }
  const slot = (i, per) => ({ right: EDGE + Math.floor(i / per) * (CARD_W + GAP), top: EDGE + (i % per) * (CARD_H + GAP) });

  // Where the cards (and an open detail) are, so main sends the cursor only there (cursor:zone).
  let zoneKey = '';
  function reportZone() {
    const rects = [];
    const n = cards.size;
    if (n) {
      const per = perColumn();
      const w = Math.ceil(n / per) * (CARD_W + GAP) + EDGE + 16;
      const h = Math.min(n, per) * (CARD_H + GAP) + EDGE + 16;
      rects.push([innerWidth - w, 0, w, h]);
    }
    if (open != null && detail.classList.contains('show')) {
      const r = detail.getBoundingClientRect();
      rects.push([r.left - 16, r.top - 16, r.width + 32, r.height + 32]);
    }
    const key = JSON.stringify(rects.map((r) => r.map(Math.round)));
    if (key === zoneKey) return;
    zoneKey = key;
    aina.send('cursor:zone', { part: 'sprites', rects });
  }

  function render() {
    const per = perColumn();
    const seen = new Set();
    list.forEach((s, i) => {
      seen.add(s.id);
      let c = cards.get(s.id);
      if (!c) {
        c = makeCard(s);
        cards.set(s.id, c);
        root.append(c);
      }
      fillCard(c, s);
      const p = slot(i, per);
      c.style.right = p.right + 'px';
      c.style.top = p.top + 'px';
    });
    for (const [id, c] of cards) {
      if (seen.has(id)) continue;
      cards.delete(id);
      c.classList.add('leaving');
      later(() => c.remove(), 260);
      if (open === id) closeDetail();
    }
    if (open != null) fillDetail();
    reportZone();
  }

  function makeCard(s) {
    const c = el('div', 'sp-card entering');
    c.dataset.id = s.id;
    c.dataset.kind = s.kind;
    const orb = el('div', 'sp-orb');
    orb.append(el('span', 'sp-icon', s.icon), el('span', 'sp-badge'));
    const text = el('div', 'sp-text');
    text.append(el('div', 'sp-name'), el('div', 'sp-status'));
    const tools = el('div', 'sp-tools');
    c.append(orb, text, tools, el('div', 'sp-bar'));
    c.addEventListener('click', () => aina.send('island:act', `sprite:open:${c.dataset.id}`));
    later(() => c.classList.remove('entering'), 600);
    return c;
  }

  function fillCard(c, s) {
    c.className = `sp-card state-${s.state}${c.classList.contains('entering') ? ' entering' : ''}${hovered === s.id ? ' hover' : ''}`;
    c.querySelector('.sp-name').textContent = s.name;
    c.querySelector('.sp-status').textContent = s.status;
    c.querySelector('.sp-badge').textContent = ICON[s.state] || '';
    const tools = c.querySelector('.sp-tools');
    const want = s.live ? 'stop' : 'close';
    if (tools.dataset.mode !== want) {
      tools.dataset.mode = want;
      tools.replaceChildren(
        s.live
          ? button('sp-tool', '■', `${L('spriteStop', 'Hentikan')} ${s.name}`, `sprite:stop:${s.id}`)
          : button('sp-tool', '×', L('spriteClose', 'Tutup'), `sprite:dismiss:${s.id}`),
      );
    }
    c.setAttribute('aria-label', `${s.name}: ${s.status}`);
  }

  // ---- delays ----
  // WebView2 throttles the click-through overlay: setTimeout can lag by about a second and animation
  // frames can stop altogether while nothing else draws. Each delay runs on whichever comes first, a
  // frame or a timer; the frame loop runs only while a delay is pending.
  const delays = new Map();
  let seq = 0;
  let raf = 0;
  function frame(now) {
    raf = 0;
    for (const [id, d] of delays) {
      if (now < d.at) continue;
      delays.delete(id);
      d.fn();
    }
    if (delays.size) raf = requestAnimationFrame(frame);
  }
  function later(fn, ms) {
    const id = ++seq;
    delays.set(id, { at: performance.now() + ms, fn });
    if (!raf) raf = requestAnimationFrame(frame);
    setTimeout(() => {
      if (!delays.has(id)) return;
      cancelAnimationFrame(raf);
      frame(performance.now());
    }, ms + 4);
    return id;
  }
  const cancel = (id) => delays.delete(id);

  // ---- detail ----
  let hovered = null; // the card under the cursor
  let open = null; // the card whose detail is shown
  let openTimer = 0;
  let openFor = null; // the card the open delay is running for
  let leaveTimer = 0;
  let tick = 0;

  const fmt = (ms) => {
    const t = Math.max(0, Math.round(ms / 1000));
    const m = Math.floor(t / 60);
    return m ? `${m}m ${String(t % 60).padStart(2, '0')}s` : `${t}s`;
  };

  // ---- a changed file's diff, inside the detail ----
  let diffView = null; // { sprite, i, data } while a diff is open
  const MAX_FILES = 6;

  async function openDiff(sprite, i) {
    let data = null;
    try {
      data = await aina.invoke('sprites:diff', { sprite, i });
    } catch {}
    if (open !== sprite) return;
    diffView = { sprite, i, data };
    fillDetail();
  }

  function showDiff() {
    // Already on screen: status updates don't rebuild it (that would lose the scroll position).
    if (diffView.shown) return place();
    diffView.shown = true;
    const d = diffView.data;
    const parts = [];
    if (d) {
      parts.push(
        ainaDiff.head(d, LABELS, {
          onBack: () => ((diffView = null), fillDetail()),
          onOpen: (x) => aina.invoke('sprites:edit', { path: x.path, line: ainaDiff.firstLine(x) }).catch(() => {}),
        }),
      );
    } else {
      const back = el('div', 'dv-head');
      const b = el('button', 'dv-back', '‹');
      b.addEventListener('click', () => ((diffView = null), fillDetail()));
      back.append(b);
      parts.push(back);
    }
    const body = ainaDiff.body(d, LABELS);
    body.style.maxHeight = Math.round((Math.min(innerHeight, screen.availHeight || innerHeight) - 2 * EDGE) * 0.6) + 'px';
    parts.push(body);
    detail.replaceChildren(...parts);
    detail.classList.add('wide');
    place();
  }

  function changesList(s) {
    const box = el('div');
    const label = el('div', 'sp-label sp-changes-label');
    const add = s.changes.reduce((n, c) => n + c.added, 0);
    const del = s.changes.reduce((n, c) => n + c.removed, 0);
    label.append(el('span', '', L('spriteChanges', 'Perubahan')), ainaDiff.counts(add, del));
    const files = el('div', 'dv-files');
    const shown = s.changes.slice(-MAX_FILES).reverse(); // the latest edit first
    for (const c of shown) {
      const row = el('button', 'dv-file');
      row.append(el('span', 'dv-fname', c.name), ainaDiff.counts(c.added, c.removed));
      row.title = c.path;
      row.addEventListener('click', (e) => (e.stopPropagation(), openDiff(s.id, c.i)));
      files.append(row);
    }
    if (s.changes.length > MAX_FILES) files.append(el('div', 'dv-more', `+${s.changes.length - MAX_FILES} ${L('spriteMore', 'lagi')}`));
    box.append(label, files);
    return box;
  }

  function fillDetail() {
    const s = list.find((x) => x.id === open);
    if (!s) return closeDetail();
    detail.dataset.kind = s.kind;
    if (diffView && diffView.sprite === s.id) return showDiff();
    detail.classList.remove('wide');
    const head = el('div', 'sp-dhead');
    head.append(el('span', 'sp-dname', s.name), el('span', 'sp-drole', ` · ${s.role}`));
    const parts = [head];
    if (s.task) {
      parts.push(el('div', 'sp-label', L('spriteTask', 'Tugas')), el('div', 'sp-task', s.task));
    }
    if (s.steps.length) {
      parts.push(el('div', 'sp-label', L('spriteSteps', 'Aktivitas')));
      const ul = el('div', 'sp-steps');
      s.steps.forEach((line, i) => ul.append(el('div', i === s.steps.length - 1 && s.live ? 'sp-step now' : 'sp-step', line)));
      parts.push(ul);
    }
    if (s.changes && s.changes.length) parts.push(changesList(s));
    const foot = el('div', 'sp-foot');
    foot.append(el('span', `sp-dstatus state-${s.state}`, `${L('spriteStatus', 'Status')}: ${s.status}`), el('span', 'sp-elapsed'));
    parts.push(foot);
    if (!s.live && s.result) parts.push(el('div', `sp-result state-${s.state}`, s.result));
    if (s.state === 'needs_approval') {
      const row = el('div', 'sp-actions');
      row.append(
        button('primary', L('spriteAllow', 'Boleh'), L('spriteAllow', 'Boleh'), 'allow'),
        button('', L('spriteAllowAll', 'Boleh semua'), L('spriteAllowAll', 'Boleh semua'), 'allowAll'),
        button('danger', L('spriteDeny', 'Jangan'), L('spriteDeny', 'Jangan'), 'deny'),
      );
      parts.push(row);
    }
    const links = el('div', 'sp-actions');
    links.append(button('', L('spriteOpen', 'Buka detail'), L('spriteOpen', 'Buka detail'), `sprite:open:${s.id}`));
    if (s.live) links.append(button('danger', L('spriteStop', 'Hentikan'), `${L('spriteStop', 'Hentikan')} ${s.name}`, `sprite:stop:${s.id}`));
    parts.push(links);
    detail.replaceChildren(...parts);
    detail.dataset.kind = s.kind;
    updateElapsed();
    place();
  }

  function updateElapsed() {
    const s = list.find((x) => x.id === open);
    const e = detail.querySelector('.sp-elapsed');
    if (!s || !e) return;
    const end = s.ended || Date.now() + skew;
    e.textContent = `${L('spriteElapsed', 'Berjalan')} ${fmt(end - s.started)}`;
  }

  // Beside the card, on the side with room (left, since cards hug the right edge), kept on screen.
  function place() {
    const c = cards.get(open);
    if (!c) return;
    const r = c.getBoundingClientRect();
    const w = detail.offsetWidth;
    const h = detail.offsetHeight;
    const bottom = Math.min(innerHeight, screen.availHeight || innerHeight) - EDGE;
    let left = r.left - GAP - w;
    if (left < EDGE) left = Math.min(r.right + GAP, innerWidth - EDGE - w);
    const top = Math.max(EDGE, Math.min(r.top, bottom - h));
    detail.style.left = left + 'px';
    detail.style.top = top + 'px';
  }

  function openDetail(id) {
    if (open !== id) diffView = null;
    open = id;
    detail.classList.add('show');
    fillDetail();
    clearInterval(tick);
    tick = setInterval(updateElapsed, 1000); // only while a detail is open
    requestAnimationFrame(reportZone); // once it is placed
  }

  function closeDetail() {
    open = null;
    diffView = null;
    detail.classList.remove('show');
    clearInterval(tick);
    tick = 0;
    reportZone();
  }

  reportZone();

  // ---- hover and clicks ----
  let interactive = false;
  let lastSent = 0;
  function setInteractive(on) {
    const now = performance.now();
    if (on === interactive && (!on || now - lastSent < 700)) return;
    interactive = on;
    lastSent = now;
    aina.send('island:interactive', on);
  }

  const inside = (e, x, y) => {
    const r = e.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  };

  document.addEventListener('mousemove', (e) => {
    if (!cards.size) {
      if (interactive) setInteractive(false);
      return;
    }
    const x = e.clientX;
    const y = e.clientY;
    let over = null;
    for (const [id, c] of cards) if (!c.classList.contains('leaving') && inside(c, x, y)) over = id;
    const onDetail = open != null && inside(detail, x, y);
    setInteractive(over != null || onDetail);

    if (over !== hovered) {
      if (hovered != null && cards.get(hovered)) cards.get(hovered).classList.remove('hover');
      hovered = over;
      if (over != null) cards.get(over).classList.add('hover');
    }
    // The open delay starts once per card: small moves over the same card don't push it back.
    if (over !== openFor) {
      cancel(openTimer);
      openFor = over;
      if (over != null && over !== open) openTimer = later(() => openDetail(over), open != null ? 60 : HOVER_DELAY);
    }
    if (over != null) {
      cancel(leaveTimer);
    } else if (open != null && !onDetail) {
      if (!delays.has(leaveTimer)) leaveTimer = later(closeDetail, LEAVE_DELAY);
    } else if (onDetail) {
      cancel(leaveTimer);
    }
  });

  addEventListener('resize', render);
})();
