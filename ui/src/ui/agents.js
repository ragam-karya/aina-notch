const $ = (id) => document.getElementById(id);
let selected = new URLSearchParams(location.search).get('slug');
let logText = '';

function el(tag, props = {}, ...children) {
  const n = Object.assign(document.createElement(tag), props);
  n.append(...children.filter((c) => c != null));
  return n;
}

const time = (ms) => new Date(ms).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' });
const icon = (name, size) => (window.badiIcon ? window.badiIcon(name, size) : null);
// Line icons in front of the fixed buttons' labels (icons.js).
for (const [id, name] of [['newBtn', 'plus'], ['folderBtn', 'folder'], ['stopBtn', 'stop'], ['deleteBtn', 'trash'], ['sendBtn', 'send']]) {
  const i = icon(name, 15);
  if (i) $(id).prepend(i);
}
// How many messages the open chat showed last time, so only new ones animate in.
let shown = { slug: null, n: 0 };

// Badi in this window (Rive, rive.js): the head in the sidebar thinks while a task runs and smiles
// when the last one finishes; the full body on the empty page waves hello (and when its head is
// clicked). Both look toward the cursor.
const head = window.badiRive($('badiHead'), { artboards: ['BADI_Head'] });
const body = window.badiRive($('badiBody'), { artboards: ['BADI'] });
body.onReady = () => setTimeout(() => body.fire('wave'), 500);
$('badiBody').addEventListener('click', () => body.fire('wave'));
let wasRunning = false;
let moodTimer = null;
function showMood(running) {
  clearTimeout(moodTimer);
  const set = (state) => {
    for (const b of [head, body]) {
      b.set('state', state);
      b.set('glowIntensity', running ? 95 : 60);
    }
  };
  if (wasRunning && !running) {
    set(4); // success
    moodTimer = setTimeout(() => set(0), 2500);
  } else set(running ? 2 : 0);
  wasRunning = running;
}
window.addEventListener('mousemove', (e) => {
  for (const [b, c] of [[head, $('badiHead')], [body, $('badiBody')]]) {
    const r = c.getBoundingClientRect();
    if (!r.width) continue;
    b.set('lookX', Math.round(Math.max(-1, Math.min(1, (e.clientX - (r.left + r.width / 2)) / 300)) * 10) / 10);
  }
});

// B.A.D.I. v2 lists Sprites (a fixed set, each task a new instance that may run beside others) instead
// of agents the person makes; the page switches its copy and buttons when the list says so.
let spriteMode = false;
function showSpriteMode(on) {
  if (on === spriteMode) return;
  spriteMode = on;
  document.title = on ? 'aina Sprites' : 'aina';
  $('newBtn').hidden = on;
  $('deleteBtn').hidden = on;
  if (!on) return;
  $('asideTitle').textContent = 'Sprites';
  $('emptyTitle').textContent = 'Sprite milik aina';
  $('emptyHint').textContent = 'aina memunculkan Sprite untuk pekerjaan panjang: Ciro (coding), Mino (riset), Routine (jadwal), Filo (file), Nilo (email) dan Daro (data). Tiap Sprite punya memori (AGENTS.md) dan folder kerjanya sendiri, dan bisa jalan bersamaan. Pilih satu untuk melihat riwayatnya atau memberi tugas.';
}

async function refreshList() {
  const items = await badi.invoke('agents:list');
  showSpriteMode(items.some((c) => c.sprite));
  showMood(items.some((c) => c.running));
  $('list').replaceChildren(
    ...items.map((c) =>
      el(
        'li',
        { className: c.slug === selected ? 'sel' : '', onclick: () => select(c.slug) },
        el('div', { className: 'name' }, c.running ? el('span', { className: 'dot' }) : null, c.icon ? `${c.icon} ${c.name}` : c.name),
        el('div', { className: 'hint' }, c.purpose || '')
      )
    )
  );
  if (!items.find((c) => c.slug === selected)) selected = null;
  renderDetail();
}

async function select(slug) {
  selected = slug;
  logText = '';
  await refreshList();
}

async function renderDetail() {
  $('empty').hidden = Boolean(selected);
  if (selected) body.pause(); // the full body only plays while the empty page shows
  else body.play();
  $('chatView').hidden = !selected;
  if (!selected) return;
  const c = await badi.invoke('agents:get', selected);
  if (!c) return;
  $('title').textContent = c.name;
  $('purpose').textContent = c.purpose + (c.workdir ? ` · Folder: ${c.workdir}` : '');
  $('stopBtn').hidden = !c.running;
  $('sendBtn').disabled = c.running && !spriteMode; // another Sprite of this kind can start beside it
  $('log').hidden = !c.running || !logText;
  $('log').textContent = logText;

  $('routines').replaceChildren(
    ...(c.routines || []).map((r) =>
      el(
        'div',
        { className: 'routine' },
        icon('repeat', 15),
        el('span', {}, `${r.title || 'Routine'}: setiap ${r.every_minutes} menit${r.paused ? ' (dijeda)' : ''}`),
        el('button', { textContent: r.paused ? 'Lanjutkan' : 'Jeda', onclick: () => badi.invoke('agents:routine', { slug: c.slug, id: r.id, patch: { paused: !r.paused } }) }),
        el('button', { className: 'danger', textContent: 'Hapus', onclick: () => badi.invoke('agents:routine', { slug: c.slug, id: r.id, patch: null }) })
      )
    )
  );

  const box = $('messages');
  const atBottom = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
  const msgs = c.messages || [];
  const switched = shown.slug !== c.slug;
  const firstNew = switched ? msgs.length : shown.n;
  shown = { slug: c.slug, n: msgs.length };
  box.classList.remove('switch');
  if (switched) {
    void box.offsetWidth; // restart the fade for the chat just opened
    box.classList.add('switch');
  }
  box.replaceChildren(
    ...msgs.map((m, i) => {
      const node = message(m);
      if (i >= firstNew) node.classList.add('enter');
      return node;
    }),
    c.running ? el('div', { className: 'msg assistant working' }, el('span', { className: 'typing' }, el('i'), el('i'), el('i')), el('span', { className: 'hint', textContent: 'Sedang bekerja…' })) : null
  );
  if (atBottom || switched) box.scrollTop = box.scrollHeight;
}

function message(m) {
  if (m.role === 'user') return el('div', { className: 'msg user' }, el('div', { className: 'body', textContent: m.text }), el('div', { className: 'meta', textContent: (m.sprite ? `${m.sprite} · ` : '') + (m.fromRoutine ? `Routine: ${m.fromRoutine} · ` : '') + time(m.at) }));
  return el(
    'div',
    { className: 'msg assistant' },
    el('div', { className: 'body', textContent: m.text }),
    ...(m.artifacts || []).map((a) => el('a', { className: 'artifact', title: a, onclick: () => badi.invoke('open', a) }, icon('file', 14), el('span', { textContent: a }))),
    m.changes?.length ? changesBlock(m) : null,
    m.nextActions?.length ? el('div', { className: 'chips' }, ...m.nextActions.map((a) => el('button', { textContent: a, onclick: () => send(a) }))) : null,
    el('div', { className: 'meta', textContent: time(m.at) + (m.stats ? ` · ${costLine(m.stats)}` : '') })
  );
}

// The files a Sprite changed in this reply; each opens its diff (kept in memory for an hour).
function changesBlock(m) {
  const add = m.changes.reduce((n, c) => n + c.added, 0);
  const del = m.changes.reduce((n, c) => n + c.removed, 0);
  const files = m.changes.map((c) => {
    const row = el('button', { className: 'dv-file', title: c.path, onclick: () => showDiff(m.spriteId, c.i) }, el('span', { className: 'dv-fname', textContent: c.name }));
    row.append(badiDiff.counts(c.added, c.removed));
    return row;
  });
  const head = el('div', { className: 'changes-head' }, el('span', { textContent: 'Perubahan' }));
  head.append(badiDiff.counts(add, del));
  return el('div', { className: 'changes' }, head, el('div', { className: 'dv-files' }, ...files));
}

async function showDiff(sprite, i) {
  let d = null;
  try {
    d = await badi.invoke('sprites:diff', { sprite, i });
  } catch {}
  const open = (x) => badi.invoke('sprites:edit', { path: x.path, line: badiDiff.firstLine(x) }).catch((e) => alert(e.message));
  $('diffHead').replaceChildren(d ? badiDiff.head(d, {}, { onOpen: open }) : '');
  $('diffBody').replaceChildren(badiDiff.body(d, {}));
  $('diffDialog').showModal();
}
$('closeDiff').onclick = () => $('diffDialog').close();

async function send(text) {
  if (!selected || !text.trim()) return;
  $('input').value = '';
  logText = '';
  await badi.invoke('agents:send', { slug: selected, text: text.trim() });
}

$('composer').onsubmit = (e) => {
  e.preventDefault();
  send($('input').value);
};
$('input').onkeydown = (e) => {
  if (e.key === 'Enter' && e.ctrlKey) send($('input').value);
};
$('folderBtn').onclick = () => badi.invoke('agents:folder', selected);
$('stopBtn').onclick = () => badi.invoke('agents:stop', selected);
$('deleteBtn').onclick = async () => {
  if (!confirm('Hapus aina ini beserta folder kerjanya?')) return;
  await badi.invoke('agents:remove', selected);
  selected = null;
  refreshList();
};

$('newBtn').onclick = () => {
  $('newForm').reset();
  $('newDialog').showModal();
};
$('cancelNew').onclick = () => $('newDialog').close();
$('newForm').onsubmit = async (e) => {
  e.preventDefault();
  const c = await badi.invoke('agents:create', { name: $('newName').value.trim(), purpose: $('newPurpose').value.trim() });
  $('newDialog').close();
  select(c.slug);
};

badi.on('agents:changed', refreshList);
// What a task took: "4 langkah · 7 tool · 0 screenshot · 12 dtk · ~$0.031".
function costLine(s) {
  return `${s.turns} langkah · ${s.tools} tool · ${s.screenshots} screenshot · ${Math.round(s.ms / 1000)} dtk · ~$${(s.usd || 0).toFixed(3)}`;
}

badi.on('agents:select', select);
badi.on('agents:error', (msg) => alert(msg));
badi.on('agents:log', ({ slug, chunk }) => {
  if (slug !== selected) return;
  logText = (logText + chunk).slice(-6000);
  $('log').hidden = false;
  $('log').textContent = logText;
  $('log').scrollTop = $('log').scrollHeight;
});

const ENGINES = {
  local: 'Mesin: bawaan aina (file, PowerShell, pencarian, web & kontrol aplikasi).',
  sdk: 'Mesin: Claude Code yang terpasang di PC ini (file, PowerShell, pencarian, web & kontrol aplikasi).',
  codex: 'Mesin: Codex CLI (shell, file & web).',
};
badi.invoke('agents:engine').then((e) => ($('engine').textContent = ENGINES[e] || ''));
refreshList();
