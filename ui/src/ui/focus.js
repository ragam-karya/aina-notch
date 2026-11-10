// The small Focus window (aina v2), opened from the notch's Focus page for what needs typing:
// ?mode=new (a new context), note (the running focus: about, goal, notes, saved as they're typed),
// end (the optional summary when Selesai) or history (time per project this week, last sessions).
const $ = (id) => document.getElementById(id);
const mode = new URLSearchParams(location.search).get('mode') || 'new';
let data = null;

const fmtDur = (ms) => {
  const m = Math.floor(ms / 60000);
  return m >= 60 ? `${Math.floor(m / 60)} j ${m % 60} mnt` : `${m} mnt`;
};
const when = (ms) => new Date(ms).toLocaleString('id-ID', { weekday: 'short', day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
const el = (tag, props = {}, ...kids) => {
  const n = Object.assign(document.createElement(tag), props);
  n.append(...kids.filter((k) => k != null));
  return n;
};

function show(id) {
  for (const s of document.querySelectorAll('section')) s.hidden = s.id !== id;
  // The tabs: the focus (running, or a new one) and the history; ending stands alone.
  $('nav').hidden = id === 'end';
  for (const b of document.querySelectorAll('nav button')) b.classList.toggle('on', b.dataset.tab === (id === 'new' ? 'work' : id));
}
for (const b of document.querySelectorAll('nav button')) {
  b.onclick = () => (b.dataset.tab === 'history' ? (renderHistory(), show('history')) : show(data.session ? 'work' : 'new'));
}

// ---- new ----
function lockRow() {
  $('newLockRow').hidden = !$('newFolder').value;
}
$('pick').onclick = async () => {
  const folder = await aina.invoke('focus:pick');
  if (folder) {
    $('newFolder').value = folder;
    if (!$('newName').value.trim()) $('newName').placeholder = folder.split(/[\\/]/).filter(Boolean).pop();
  }
  lockRow();
};
$('start').onclick = async () => {
  try {
    await aina.invoke('focus:start', { name: $('newName').value, folder: $('newFolder').value, about: $('newAbout').value, goal: $('newGoal').value, lock: $('newLock').checked });
    window.close();
  } catch (e) {
    $('newErr').textContent = String(e.message || e);
  }
};

// ---- work (the running focus) ----
let saveTimer = 0;
function saveSoon() {
  clearTimeout(saveTimer);
  $('saved').textContent = '';
  saveTimer = setTimeout(async () => {
    await aina.invoke('focus:update', { about: $('workAbout').value, goal: $('workGoal').value, note: $('noteText').value });
    $('saved').textContent = 'Tersimpan';
  }, 500);
}
for (const id of ['workAbout', 'workGoal', 'noteText']) $(id).addEventListener('input', saveSoon);
function renderWork() {
  const s = data.session;
  $('workName').textContent = s.name;
  $('workMeta').textContent = [s.folder, `${fmtDur(data.now - s.startedAt)} berjalan`, s.pomodoros ? `${s.pomodoros} pomodoro` : ''].filter(Boolean).join(' · ');
  $('workAbout').value = s.about || '';
  $('workGoal').value = s.goal || '';
  $('noteText').value = s.note || '';
}

// ---- end ----
async function finish(withSummary) {
  const summary = withSummary ? { done: $('done').value, pending: $('pending').value, next: $('next').value } : {};
  await aina.invoke('focus:end', summary);
  window.close();
}
$('endSave').onclick = () => finish(true);
$('endSkip').onclick = () => finish(false);

// ---- history ----
function renderHistory() {
  const max = Math.max(1, ...data.week.map((w) => w.ms));
  $('week').replaceChildren(
    ...(data.week.length
      ? data.week.map((w) => el('div', { className: 'bar' }, el('span', { textContent: w.name }), el('span', { textContent: `${fmtDur(w.ms)} · ${w.sessions} sesi` }), el('i', { style: `width:${Math.max(4, (w.ms / max) * 100)}%` })))
      : [el('div', { className: 'empty', textContent: 'Belum ada sesi fokus minggu ini.' })])
  );
  const line = (label, text) => (text ? el('p', {}, el('b', { textContent: `${label}: ` }), text) : null);
  $('recs').replaceChildren(
    ...(data.history.length
      ? data.history.map((r) =>
          el(
            'div',
            { className: 'rec' },
            el('div', { className: 't' }, el('span', { textContent: r.name }), el('span', { textContent: `${when(r.startedAt)} · ${fmtDur(r.ms)}${r.pomodoros ? ` · ${r.pomodoros} pomodoro` : ''}` })),
            line('Target', r.goal),
            line('Selesai', r.done),
            line('Tertunda', r.pending),
            line('Berikutnya', r.next),
            line('Catatan', r.note)
          )
        )
      : [el('div', { className: 'empty', textContent: 'Riwayat masih kosong.' })])
  );
}

(async () => {
  data = await aina.invoke('focus:get');
  lockRow();
  if (data.session) renderWork();
  if (mode === 'end' && data.session) {
    $('endName').textContent = data.session.name;
    $('endMeta').textContent = [`${fmtDur(data.now - data.session.startedAt)}`, data.session.pomodoros ? `${data.session.pomodoros} pomodoro (${fmtDur(data.session.pomodoroMs)} fokus)` : ''].filter(Boolean).join(' · ');
    $('next').value = '';
    return show('end');
  }
  if (mode === 'history') {
    renderHistory();
    return show('history');
  }
  show(data.session && mode !== 'new' ? 'work' : 'new');
})();
