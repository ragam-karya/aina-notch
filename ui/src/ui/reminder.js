// The small Reminder window (B.A.D.I. v2), opened from the notch's reminder page: ?mode=new (with
// ?day= the day chosen there) or edit (&id= a local reminder or a Google event). Saving a Google event
// waits for Google's answer. The page's options (view, Google Calendar, prayer times) are in Settings.
const $ = (id) => document.getElementById(id);
const q = new URLSearchParams(location.search);
let data = null;

const pad = (n) => String(n).padStart(2, '0');
const dateStr = (ms) => {
  const d = new Date(ms);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
};
const timeStr = (ms) => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
};
const el = (tag, props = {}, ...kids) => {
  const n = Object.assign(document.createElement(tag), props);
  n.append(...kids.filter((k) => k != null));
  return n;
};


// ---- edit ----
function setWhen(ms) {
  $('date').value = dateStr(ms);
  $('time').value = timeStr(ms);
}
function whenMs() {
  if (!$('date').value) return null;
  const [y, m, d] = $('date').value.split('-').map(Number);
  const [h, mi] = $('allDay').checked ? [0, 0] : ($('time').value || '09:00').split(':').map(Number);
  return new Date(y, m - 1, d, h, mi).getTime();
}
for (const b of document.querySelectorAll('[data-short]')) {
  b.onclick = () => {
    setWhen(data.shortcuts[b.dataset.short]);
    $('allDay').checked = false;
    sync();
    for (const o of document.querySelectorAll('[data-short]')) o.classList.toggle('on', o === b);
  };
}
// What a destination allows: Google takes no all-day events from Badi, and keeps its own reminders.
function sync() {
  const google = $('dest').value.startsWith('google:');
  $('timeBox').hidden = $('allDay').checked;
  $('notifyRow').hidden = google;
  $('destNote').textContent = google ? 'Acara dibuat di Google Calendar setelah Google mengonfirmasi.' : 'Pengingat lokal tidak dikirim ke mana pun.';
  if (google && $('allDay').checked) $('allDay').checked = false;
  $('allDay').disabled = google;
}
$('allDay').onchange = sync;
$('dest').onchange = sync;

function fillEdit() {
  const item = data.item;
  const google = data.google;
  // Where a new one goes: this PC by default; Google only when picked here.
  for (const c of google.calendars || []) $('dest').append(el('option', { value: `google:${c.id}`, textContent: `Google · ${c.name}` }));
  $('destBox').hidden = Boolean(item) || !(google.calendars || []).length;
  if (!item) {
    const day = Number(q.get('day')) || 0;
    const base = new Date(Math.max(data.now, day));
    // A day other than today starts at 09:00; today, at the next quarter hour.
    if (day && dateStr(day) !== dateStr(data.now)) base.setHours(9, 0, 0, 0);
    else base.setMinutes(Math.ceil((base.getMinutes() + 1) / 15) * 15, 0, 0);
    setWhen(base.getTime());
    sync();
    return;
  }
  $('editTitle').textContent = item.source === 'google' ? `Acara · ${item.calendar || 'Google Calendar'}` : 'Ubah pengingat';
  $('title').value = item.title;
  setWhen(item.start);
  $('allDay').checked = item.allDay;
  const len = item.end ? Math.round((item.end - item.start) / 60000) : 0;
  if (![...$('length').options].some((o) => Number(o.value) === len)) $('length').append(el('option', { value: len, textContent: `${len} menit` }));
  $('length').value = String(len);
  $('repeat').value = item.recurrence || '';
  $('note').value = item.description || '';
  $('notify').checked = item.notify;
  $('del').hidden = !(item.actions || []).includes('delete');
  if (item.source === 'google') {
    // Google's own recurrence and reminders stay as Google has them.
    $('repeat').disabled = true;
    $('notifyRow').hidden = true;
    $('allDay').disabled = true;
    if (!item.editable) {
      for (const n of document.querySelectorAll('#edit input, #edit textarea, #edit select')) n.disabled = true;
      $('save').hidden = true;
      $('readOnly').hidden = false;
      $('readOnly').textContent = item.readOnlyReason === 'readOnlyEvent' ? 'Acara ini tidak bisa diubah dari aina.' : 'Kalender ini hanya bisa dibaca, jadi acaranya tidak bisa diubah dari sini.';
    }
  }
  sync();
}

$('save').onclick = async () => {
  const at = whenMs();
  const len = Number($('length').value) || 0;
  const google = $('dest').value.startsWith('google:') || data.item?.source === 'google';
  $('err').textContent = '';
  $('save').disabled = true;
  if (google) $('save').textContent = 'Menyimpan ke Google…';
  try {
    await badi.invoke('reminder:save', {
      id: data.item?.id || '',
      dest: $('dest').value,
      title: $('title').value,
      at,
      end: $('allDay').checked ? at + 86400000 : len ? at + len * 60000 : null,
      allDay: $('allDay').checked,
      note: $('note').value,
      notify: $('notify').checked,
      repeat: $('repeat').value,
    });
    window.close();
  } catch (e) {
    $('err').textContent = String(e.message || e);
    $('save').disabled = false;
    $('save').textContent = 'Simpan';
  }
};
$('del').onclick = async () => {
  if ($('del').dataset.sure !== '1') {
    $('del').dataset.sure = '1';
    $('del').textContent = 'Yakin hapus?';
    return;
  }
  await badi.invoke('reminder:delete', { id: data.item.id });
  window.close();
};


(async () => {
  data = await badi.invoke('reminder:get', { id: q.get('id') || '' });
  fillEdit();
  $('title').focus();
})();
