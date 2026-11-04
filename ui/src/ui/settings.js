const $ = (id) => document.getElementById(id);
const pretty = (combo) => combo.replace('Meta', 'Win');
let cloudOnly = false; // aina v2: aina Cloud holds every key (see loadCloud)

// A key's status next to its field: the user's own, the demo build's (until a date), or none.
function keyState(c, k) {
  if (c[k + 'Demo']) return `(memakai key demo sampai ${new Date(c.demoUntil).toLocaleDateString('id-ID', { day: 'numeric', month: 'long', year: 'numeric' })}, isi key-mu sendiri untuk menggantinya)`;
  return c[k + 'Set'] ? '(tersimpan, kosongkan untuk tetap memakai)' : '(belum diisi)';
}

async function load() {
  const c = await badi.invoke('config:get');
  $('anthropicState').textContent = keyState(c, 'anthropicApiKey');
  $('openaiState').textContent = keyState(c, 'openaiApiKey');
  $('composioState').textContent = keyState(c, 'composioApiKey');
  loadApps();
  loadBrands();
  $('elevenlabsState').textContent = keyState(c, 'elevenlabsApiKey');
  $('elevenlabsVoice').value = c.elevenlabsVoice || '';
  $('userName').value = c.userName || '';
  $('notchShortcuts').value = (c.notchShortcuts || []).map((s) => `${s.label} = ${s.prompt}`).join('\n');
  loadReminders(c);
  loadLocation();
  $('language').value = c.language || 'id';
  $('model').value = c.model;
  $('brainModel').value = c.brainModel || 'claude-sonnet-5-5';
  $('effort').value = c.effort;
  $('voiceEngine').value = c.voiceEngine || 'classic';
  $('bargeIn').value = c.bargeIn || 'low';
  $('realtimeVoice').value = c.realtimeVoice || 'marin';
  $('realtimeIdleSec').value = String(c.realtimeIdleSec || 30);
  $('realtimeBox').hidden = $('voiceEngine').value !== 'realtime';
  $('tts').value = c.tts;
  $('openaiVoice').value = c.openaiVoice;
  $('ptt').value = c.pttCombo.join('+');
  $('dict').value = (c.dictationCombo || []).join('+');
  $('dictationCleanup').checked = c.dictationCleanup;
  $('wakeWord').checked = Boolean(c.wakeWord);
  $('launchAtLogin').checked = c.launchAtLogin;
  $('notchLyrics').checked = c.notchLyrics !== false;
  $('focusSummary').checked = Boolean(c.focusSummary);
  $('focusDnd').checked = Boolean(c.focusDnd);  $('computerControl').checked = c.computerControl;
  $('agentFileAccess').checked = Boolean(c.agentFileAccess);
  $('brainEffort').value = c.brainEffort || 'medium';
  loadPomodoro(c);
  loadExtension();
  $('pttHint').textContent = pretty(c.pttCombo.join('+'));
  $('dictHint').textContent = c.dictationCombo?.length ? pretty(c.dictationCombo.join('+')) : 'mati';
  fillVoices(c.systemVoice);
  toggleVoiceBoxes();
}

// ---------- pomodoro ----------
// The built-in patterns (the same as builtin_presets in badi-core/src/timers.rs) and the person's own,
// kept in config.json as pomodoroPresets. Choosing one fills the fields; changing a field makes the
// settings custom (no preset chosen).
const POMO_BUILTIN = [
  { id: 'classic', focusMin: 25, shortMin: 5, longMin: 15, sessions: 4, longEvery: 4 },
  { id: 'deep', focusMin: 50, shortMin: 10, longMin: 30, sessions: 3, longEvery: 3 },
  { id: 'sprint', focusMin: 15, shortMin: 3, longMin: 10, sessions: 4, longEvery: 4 },
];
const POMO_FIELDS = { focusMin: 'pomoFocus', shortMin: 'pomoShort', longMin: 'pomoLong', sessions: 'pomoSessions', longEvery: 'pomoLongEvery' };
let pomoCustom = [];
const pomoName = (p) => p.name || `${p.focusMin}/${p.shortMin}${p.sessions === 4 ? '' : ` ×${p.sessions}`}`;
const pomoAll = () => [...POMO_BUILTIN, ...pomoCustom.map((p, i) => ({ ...p, id: `custom:${i}` }))];

function fillPomoPresets(selected) {
  $('pomoPreset').replaceChildren(new Option('Atur sendiri', ''), ...pomoAll().map((p) => new Option(pomoName(p), p.id, false, p.id === selected)));
  $('pomoDelete').hidden = !selected.startsWith('custom:');
}
function loadPomodoro(c) {
  const p = { ...POMO_BUILTIN[0], autoStart: true, notify: true, ...(c.pomodoro || {}) };
  pomoCustom = Array.isArray(c.pomodoroPresets) ? c.pomodoroPresets : [];
  for (const [k, id] of Object.entries(POMO_FIELDS)) $(id).value = p[k];
  $('pomoAuto').checked = p.autoStart !== false;
  $('pomoNotify').checked = p.notify !== false;
  fillPomoPresets(c.pomodoroPreset ?? 'classic');
}
const pomoValues = () => Object.fromEntries(Object.entries(POMO_FIELDS).map(([k, id]) => [k, Math.max(1, Math.round(Number($(id).value) || 1))]));
function pomodoroPatch() {
  return {
    pomodoro: { ...pomoValues(), autoStart: $('pomoAuto').checked, notify: $('pomoNotify').checked },
    pomodoroPreset: $('pomoPreset').value,
    pomodoroPresets: pomoCustom,
  };
}
$('pomoPreset').onchange = () => {
  const p = pomoAll().find((x) => x.id === $('pomoPreset').value);
  if (p) for (const [k, id] of Object.entries(POMO_FIELDS)) $(id).value = p[k];
  $('pomoDelete').hidden = !$('pomoPreset').value.startsWith('custom:');
};
for (const id of Object.values(POMO_FIELDS)) $(id).addEventListener('input', () => fillPomoPresets(''));
$('pomoSave').onclick = () => {
  const name = $('pomoName').value.trim();
  if (!name) return $('pomoName').focus();
  pomoCustom = [...pomoCustom.filter((p) => p.name !== name), { name, ...pomoValues() }];
  $('pomoName').value = '';
  fillPomoPresets(`custom:${pomoCustom.length - 1}`);
};
$('pomoDelete').onclick = () => {
  const i = Number($('pomoPreset').value.split(':')[1]);
  pomoCustom = pomoCustom.filter((_, k) => k !== i);
  fillPomoPresets('');
};

function fillVoices(selected) {
  const voices = speechSynthesis.getVoices();
  if (!voices.length) return;
  $('systemVoice').replaceChildren(
    new Option('Otomatis', ''),
    ...voices.map((v) => new Option(`${v.name} (${v.lang})`, v.name, false, v.name === selected))
  );
}
speechSynthesis.onvoiceschanged = () => badi.invoke('config:get').then((c) => {
  fillVoices(c.systemVoice);
  updateVoiceHint();
});

function toggleVoiceBoxes() {
  $('systemVoiceBox').hidden = $('tts').value !== 'system';
  $('openaiVoiceBox').hidden = $('tts').value !== 'openai';
  $('elevenlabsBox').hidden = $('tts').value !== 'elevenlabs';
  updateVoiceHint();
}
$('tts').onchange = toggleVoiceBoxes;
$('voiceEngine').onchange = () => ($('realtimeBox').hidden = $('voiceEngine').value !== 'realtime');

// Windows only ships voices for some languages; say so when the chosen one has none installed.
function updateVoiceHint() {
  const lang = $('language').value;
  const voices = speechSynthesis.getVoices();
  const has = lang === 'auto' || voices.some((v) => v.lang.toLowerCase().startsWith(lang));
  $('voiceHint').textContent =
    voices.length && !has
      ? `Belum ada suara Windows untuk ${lang === 'id' ? 'Bahasa Indonesia' : 'English'}. Selama OpenAI key terisi, aina otomatis memakai Suara OpenAI. Bisa juga pasang suara lewat Windows Settings > Time & language > Speech.`
      : 'Otomatis = suara yang cocok dengan bahasa aina.';
}
$('language').onchange = updateVoiceHint;

$('systemVoice').onchange = () => {
  const v = speechSynthesis.getVoices().find((x) => x.name === $('systemVoice').value);
  const u = new SpeechSynthesisUtterance('Hi, I am aina.');
  if (v) u.voice = v;
  speechSynthesis.cancel();
  speechSynthesis.speak(u);
};

// "Label = prompt" lines -> [{ label, prompt }]; a line without "=" uses its text for both.
function parseShortcuts(text) {
  return text
    .split(/\r?\n/)
    .map((line) => {
      const i = line.indexOf('=');
      const label = (i >= 0 ? line.slice(0, i) : line).trim().slice(0, 28);
      const prompt = (i >= 0 ? line.slice(i + 1) : line).trim();
      return label && prompt ? { label, prompt } : null;
    })
    .filter(Boolean)
    .slice(0, 4);
}

async function save() {
  const ptt = $('ptt').value.split('+');
  const dict = $('dict').value ? $('dict').value.split('+') : [];
  if (dict.length && ptt.join() === dict.join()) {
    $('saveStatus').textContent = 'Hotkey bicara dan dikte tidak boleh sama.';
    $('saveStatus').className = 'status err';
    return false;
  }
  await badi.invoke('config:set', {
    anthropicApiKey: $('anthropicApiKey').value.trim(),
    openaiApiKey: $('openaiApiKey').value.trim(),
    elevenlabsApiKey: $('elevenlabsApiKey').value.trim(),
    composioApiKey: $('composioApiKey').value.trim(),
    elevenlabsVoice: $('elevenlabsVoice').value.trim() || undefined,
    userName: $('userName').value.trim().slice(0, 30),
    language: $('language').value,
    model: $('model').value,
    brainModel: $('brainModel').value,
    effort: $('effort').value,
    voiceEngine: $('voiceEngine').value,
    realtimeVoice: $('realtimeVoice').value,
    bargeIn: $('bargeIn').value,
    realtimeIdleSec: Number($('realtimeIdleSec').value) || 30,
    tts: $('tts').value,
    systemVoice: $('systemVoice').value,
    openaiVoice: $('openaiVoice').value,
    pttCombo: ptt,
    dictationCombo: dict,
    dictationCleanup: $('dictationCleanup').checked,
    wakeWord: $('wakeWord').checked,
    launchAtLogin: $('launchAtLogin').checked,
    computerControl: $('computerControl').checked,
    agentFileAccess: $('agentFileAccess').checked,
    brainEffort: $('brainEffort').value,
    notchShortcuts: parseShortcuts($('notchShortcuts').value),
    notchLyrics: $('notchLyrics').checked,
    focusSummary: $('focusSummary').checked,
    focusDnd: $('focusDnd').checked,    ...pomodoroPatch(),
    ...remindersPatch(),
    ...locationPatch(),
  });
  $('anthropicApiKey').value = '';
  $('openaiApiKey').value = '';
  $('elevenlabsApiKey').value = '';
  $('composioApiKey').value = '';
  $('saveStatus').textContent = 'Tersimpan.';
  $('saveStatus').className = 'status ok';
  await load();
  return true;
}
$('save').onclick = save;

$('testAnthropic').onclick = async () => {
  $('testStatus').textContent = 'Mengetes…';
  $('testStatus').className = 'status';
  try {
    if ($('anthropicApiKey').value.trim() && !(await save())) return;
    const reply = await badi.invoke('test:anthropic');
    $('testStatus').textContent = `Terhubung. Balasan model: "${reply}"`;
    $('testStatus').className = 'status ok';
  } catch (e) {
    $('testStatus').textContent = e.message.replace(/^Error invoking remote method '[^']+': /, '');
    $('testStatus').className = 'status err';
  }
};

// ---- browser extension pairing ----
function showExtStatus(s) {
  if (!s) return;
  $('extState').textContent = s.connected
    ? `(tersambung: ${s.browser || 'browser'} ${s.version || ''})`
    : s.listening === false
      ? '(aina tidak bisa membuka port untuk ekstensi; mungkin port-nya sedang dipakai aplikasi lain)'
      : '(belum tersambung)';
  $('extState').className = s.connected ? 'hint ok' : 'hint';
}

async function loadExtension() {
  const info = await badi.invoke('extension:info');
  $('extCode').value = info.code;
  showExtStatus(info.status);
}
badi.on('extension:status', showExtStatus);
$('extNewCode').onclick = async () => {
  const info = await badi.invoke('extension:newCode');
  $('extCode').value = info.code;
  showExtStatus(info.status);
};
$('extFolder').onclick = () => badi.invoke('extension:folder');
$('extCopyUrl').onclick = () => {
  navigator.clipboard.writeText('chrome://extensions');
  $('extCopyUrl').textContent = 'Tersalin';
  setTimeout(() => ($('extCopyUrl').textContent = 'Salin alamat'), 1500);
};
// ---- location & weather ----
// Where the weather (and, when shared, the prayer times) are for: the laptop's location by default,
// or a city picked here (looked up on Open-Meteo as the person types).
let locCity = null;
const LOC_ERRORS = {
  locationOff: 'Lokasi Windows mati atau belum diizinkan. Nyalakan di Pengaturan Windows → Privasi & keamanan → Lokasi (termasuk "Izinkan aplikasi desktop mengakses lokasi"), lalu tekan Deteksi sekarang.',
  timeout: 'Windows belum memberi lokasi. Coba lagi sebentar lagi.',
  noPlace: 'Lokasi belum diketahui.',
};
const placeText = (p) => [p.name, p.region].filter(Boolean).join(', ');
function showLocMode() {
  const city = $('locationMode').value === 'city';
  $('locAutoBox').hidden = city;
  $('locCityBox').hidden = !city;
}
async function loadLocation() {
  const l = await badi.invoke('location:get');
  $('locationMode').value = l.mode || 'auto';
  $('locationShare').checked = l.share !== false;
  locCity = l.city || null;
  $('locChosen').textContent = locCity ? `Dipilih: ${placeText(locCity)}` : 'Belum ada kota.';
  $('locAutoState').textContent = l.mode !== 'city' && l.place?.name ? `Terdeteksi: ${placeText(l.place)}` : l.error ? LOC_ERRORS[l.error] || l.error : 'Belum dideteksi.';
  showLocMode();
}
function locationPatch() {
  const patch = { locationMode: $('locationMode').value, locationShare: $('locationShare').checked };
  if (locCity) patch.locationCity = locCity;
  if (patch.locationMode === 'city' && !locCity) patch.locationMode = 'auto';
  return patch;
}
$('locationMode').onchange = showLocMode;
$('locDetect').onclick = async () => {
  $('locAutoState').textContent = 'Mendeteksi…';
  try {
    const p = await badi.invoke('location:detect');
    $('locAutoState').textContent = `Terdeteksi: ${placeText(p) || `${p.lat.toFixed(3)}, ${p.lon.toFixed(3)}`}`;
  } catch (e) {
    const code = String(e.message || e);
    $('locAutoState').textContent = LOC_ERRORS[code] || `Belum bisa mendeteksi: ${code}`;
  }
};
let locTimer = 0;
$('locSearch').oninput = () => {
  clearTimeout(locTimer);
  const text = $('locSearch').value.trim();
  if (text.length < 2) return $('locResults').replaceChildren();
  locTimer = setTimeout(async () => {
    try {
      const found = await badi.invoke('location:search', { q: text });
      $('locResults').replaceChildren(
        ...found.map((p) => {
          const b = document.createElement('button');
          b.type = 'button';
          b.textContent = placeText(p);
          b.onclick = () => {
            locCity = p;
            $('locChosen').textContent = `Dipilih: ${placeText(p)} (simpan untuk memakainya)`;
            $('locResults').replaceChildren();
            $('locSearch').value = '';
          };
          return b;
        })
      );
      if (!found.length) $('locChosen').textContent = 'Kota tidak ditemukan.';
    } catch (e) {
      $('locChosen').textContent = `Belum bisa mencari kota: ${e.message || e}`;
    }
  }, 350);
};

// ---- reminders (the notch's reminder page) ----
// The view and density, Google Calendar's place (work apps), and prayer times: on/off, the city
// (looked up on api.myquran.com as the person types) and how early to be told.
let prayerCity = null;
function loadReminders(c) {
  $('reminderView').value = c.reminderView || 'agenda';
  $('reminderDensity').value = c.reminderDensity || 'compact';
  $('prayerEnabled').checked = c.prayerEnabled === true;
  prayerCity = c.prayerCity && c.prayerCity.id ? c.prayerCity : null;
  $('prayerChosen').textContent = prayerCity ? `Dipilih: ${prayerCity.name}${prayerCity.auto ? ' (ikut lokasi)' : ''}` : 'Belum ada kota.';
  $('prayerNotify').value = String(c.prayerNotify ?? 10);
  $('prayerBox').hidden = !$('prayerEnabled').checked;
}
function remindersPatch() {
  const patch = { reminderView: $('reminderView').value, reminderDensity: $('reminderDensity').value, prayerNotify: Number($('prayerNotify').value) };
  // Prayer times stay off until a city is chosen.
  patch.prayerEnabled = $('prayerEnabled').checked && Boolean(prayerCity);
  if (prayerCity) patch.prayerCity = prayerCity;
  return patch;
}
$('prayerEnabled').onchange = () => ($('prayerBox').hidden = !$('prayerEnabled').checked);
$('gcalGo').onclick = () => $('appsCard')?.scrollIntoView({ block: 'start', behavior: 'smooth' });
let prayerTimer = 0;
$('prayerSearch').oninput = () => {
  clearTimeout(prayerTimer);
  const text = $('prayerSearch').value.trim();
  if (text.length < 3) return $('prayerResults').replaceChildren();
  prayerTimer = setTimeout(async () => {
    try {
      const found = await badi.invoke('reminder:cities', { q: text });
      if (!found.length) {
        $('prayerResults').replaceChildren();
        $('prayerChosen').textContent = 'Kota tidak ditemukan. Coba nama kota atau kabupatennya saja.';
        return;
      }
      $('prayerResults').replaceChildren(
        ...found.slice(0, 8).map((c) => {
          const b = document.createElement('button');
          b.type = 'button';
          b.textContent = c.name;
          b.onclick = () => {
            prayerCity = c;
            $('prayerChosen').textContent = `Dipilih: ${c.name} (simpan untuk memakainya)`;
            $('prayerResults').replaceChildren();
            $('prayerSearch').value = '';
          };
          return b;
        })
      );
    } catch (e) {
      $('prayerChosen').textContent = `Belum bisa mencari kota: ${e.message || e}`;
    }
  }, 350);
};

// From the installer (--setup-extension): show the steps and mark them for a moment.
badi.on('settings:focus', (id) => {
  const el = $(id);
  if (!el) return;
  // From the notch's prayer guide: the switch on, the city field ready to type in.
  if (id === 'prayerSetup') {
    $('prayerEnabled').checked = true;
    $('prayerBox').hidden = false;
    setTimeout(() => $('prayerSearch').focus(), 300);
  }
  el.scrollIntoView({ block: 'center' });
  el.classList.add('focus');
  setTimeout(() => el.classList.remove('focus'), 4000);
});

load();

// ---------- work apps ----------
// Apps switched on whose sign-in is still open in the browser: their switch stays on and says so
// until the sign-in ends (apps:changed with the slug), however the list reloads meanwhile.
const linking = new Map(); // slug -> 'opening' | 'waiting'

// One list request at a time: asking Composio is slow, so calls made meanwhile become one more reload.
let appsLoading = null;
let appsAgain = false;
function loadApps() {
  if (appsLoading) {
    appsAgain = true;
    return appsLoading;
  }
  appsLoading = (async () => {
    do {
      appsAgain = false;
      await showApps();
    } while (appsAgain);
  })().finally(() => (appsLoading = null));
  return appsLoading;
}

async function showApps() {
  const box = $('apps');
  if (!box.children.length) $('appsStatus').textContent = 'Memuat aplikasi…';
  const res = await badi.invoke('apps:list');
  $('appsStatus').textContent = res.error ? `Composio belum bisa dihubungi: ${res.error}` : res.ready ? '' : cloudOnly ? 'Masuk ke aina Cloud dulu untuk menghubungkan aplikasi.' : 'Isi Composio API key lalu Simpan untuk menghubungkan aplikasi.';
  $('appsStatus').className = res.error ? 'status err' : 'status';
  box.replaceChildren(
    ...res.apps.map((a) => {
      const row = document.createElement('div');
      row.className = 'app' + (a.connected ? ' linked' : '');
      const sw = document.createElement('label');
      sw.className = 'switch';
      sw.title = a.enabled ? 'aina boleh memakai aplikasi ini' : 'aina tidak memakai aplikasi ini';
      const input = document.createElement('input');
      input.type = 'checkbox';
      const pending = !a.connected && (linking.get(a.slug) || (a.pending && 'waiting'));
      if (a.connected) linking.delete(a.slug);
      input.checked = Boolean(a.enabled || pending);
      input.disabled = pending === 'opening';
      sw.append(input, document.createElement('span'));
      const name = document.createElement('span');
      name.className = 'name';
      name.textContent = a.name;
      const state = document.createElement('span');
      state.className = 'state';
      state.textContent = pending === 'opening' ? 'Membuka login…' : pending ? 'Menunggu login di browser…' : a.connected ? (a.enabled ? 'Aktif' : 'Tersambung, mati') : 'Belum tersambung';
      input.onchange = async () => {
        // Switched off while its sign-in is open: stop waiting for it here (the link may still finish).
        if (!input.checked) linking.delete(a.slug);
        input.disabled = true;
        try {
          // No key saved yet: a key typed in the field is saved first; without one, say what's missing.
          if (!res.ready && !res.error && cloudOnly) {
            input.checked = !input.checked;
            input.disabled = false;
            $('appsStatus').textContent = 'Masuk ke aina Cloud dulu (bagian Akun di atas), lalu nyalakan saklarnya lagi.';
            $('appsStatus').className = 'status err';
            return;
          }
          if (!res.ready && !res.error) {
            const key = $('composioApiKey').value.trim();
            if (!key) {
              input.checked = !input.checked;
              input.disabled = false;
              $('appsStatus').textContent = 'Isi Composio API key dulu, lalu nyalakan saklarnya lagi.';
              $('appsStatus').className = 'status err';
              $('composioApiKey').focus();
              return;
            }
            await badi.invoke('config:set', { composioApiKey: key });
            loadBrands();
            $('composioApiKey').value = '';
            $('composioState').textContent = '(tersimpan, kosongkan untuk tetap memakai)';
          }
          const signIn = input.checked && !a.connected;
          if (signIn) {
            linking.set(a.slug, 'opening');
            state.textContent = 'Membuka login…';
          }
          const said = await badi.invoke('apps:toggle', { slug: a.slug, on: input.checked, connected: a.connected });
          if (signIn && said) {
            linking.set(a.slug, 'waiting');
            $('appsStatus').textContent = `Selesaikan login ${a.name} di browser. Saklarnya tetap menyala sambil menunggu; daftar ini diperbarui otomatis.`;
            $('appsStatus').className = 'status';
          }
          await loadApps();
        } catch (e) {
          // Only a real failure turns it back off, with the reason.
          linking.delete(a.slug);
          $('appsStatus').textContent = `${a.name}: ${e.message}`;
          $('appsStatus').className = 'status err';
          input.checked = !input.checked;
          input.disabled = false;
          state.textContent = a.connected ? (a.enabled ? 'Aktif' : 'Tersambung, mati') : 'Belum tersambung';
        }
      };
      // Still being built: a "Segera" label, and the switch stays off.
      if (a.soon) {
        row.className = 'app soon';
        input.checked = false;
        input.disabled = true;
        sw.title = 'Segera hadir';
        state.textContent = '';
        const tag = document.createElement('span');
        tag.className = 'soon-tag';
        tag.textContent = 'Segera';
        state.append(tag);
        row.append(sw, name, state);
        return row;
      }
      row.append(sw, name, state);
      if (a.connected || pending === 'waiting') {
        const unlink = document.createElement('button');
        unlink.className = 'unlink';
        unlink.textContent = a.connected ? 'Putuskan' : 'Batal';
        unlink.onclick = async () => {
          unlink.disabled = true;
          linking.delete(a.slug);
          try {
            await badi.invoke('apps:disconnect', a.slug);
          } catch (e) {
            $('appsStatus').textContent = e.message;
            $('appsStatus').className = 'status err';
          }
          await loadApps();
        };
        row.append(unlink);
      }
      return row;
    })
  );
}
// Back in Settings while a sign-in is still open (its browser page closed without signing in, say): a
// finished sign-in is noticed within seconds, so one still open after the grace goes back off.
const SIGN_IN_GRACE = 15000;
let graceTimer = 0;
addEventListener('focus', () => {
  clearTimeout(graceTimer);
  graceTimer = setTimeout(async () => {
    if (!document.hasFocus()) return;
    const res = await badi.invoke('apps:list').catch(() => null);
    const open = (res?.apps || []).filter((a) => !a.connected && (a.pending || linking.has(a.slug)));
    if (!open.length) return;
    for (const a of open) {
      linking.delete(a.slug);
      await badi.invoke('apps:toggle', { slug: a.slug, on: false, connected: false }).catch(() => {});
    }
    $('appsStatus').textContent = `Login ${open.map((a) => a.name).join(', ')} belum selesai, jadi saklarnya dimatikan. Nyalakan lagi untuk mencoba.`;
    $('appsStatus').className = 'status err';
    loadApps();
  }, SIGN_IN_GRACE);
});
addEventListener('blur', () => clearTimeout(graceTimer));

badi.on('apps:changed', (e) => {
  // A sign-in ended: linked, or not finished in time (its switch went back off).
  if (e && e.slug) {
    linking.delete(e.slug);
    $('appsStatus').textContent = e.error || '';
    $('appsStatus').className = e.error ? 'status err' : 'status';
  }
  loadApps();
});

// ---------- white-label sign-in ----------
let brandInfo = null;
function showBrands(info) {
  brandInfo = info;
  $('brandRedirect').textContent = info.redirect;
  const pick = $('brandWhich').value;
  $('brandWhich').replaceChildren(
    ...info.brands.map((b) => {
      const o = document.createElement('option');
      o.value = b.id;
      o.textContent = b.name + (b.on ? ' (terpasang)' : '');
      return o;
    })
  );
  if (pick) $('brandWhich').value = pick;
  showConsole();
  const project = info.kind === 'project';
  for (const id of ['brandWhich', 'brandClientId', 'brandSecret', 'brandSave']) $(id).disabled = !project;
  $('brandStatus').textContent = project ? '' : info.kind === 'connect' ? 'Key ck_… tidak bisa: ganti ke Project API key (ak_…) dulu.' : 'Simpan Project API key (ak_…) dulu.';
  $('brandStatus').className = 'status';
  $('brands').replaceChildren(
    ...info.brands
      .filter((b) => b.on)
      .map((b) => {
        const row = document.createElement('div');
        row.className = 'app linked';
        const name = document.createElement('span');
        name.className = 'name';
        name.textContent = b.name;
        const state = document.createElement('span');
        state.className = 'state';
        state.textContent = 'Login atas nama aplikasimu';
        const off = document.createElement('button');
        off.className = 'unlink';
        off.textContent = 'Lepas';
        off.onclick = async () => {
          off.disabled = true;
          try {
            showBrands(await badi.invoke('apps:unbrand', b.id));
            loadApps();
          } catch (e) {
            $('brandStatus').textContent = e.message;
            $('brandStatus').className = 'status err';
            off.disabled = false;
          }
        };
        row.append(name, state, off);
        return row;
      })
  );
}
function showConsole() {
  const b = brandInfo?.brands.find((x) => x.id === $('brandWhich').value);
  $('brandConsole').textContent = b?.console ? `Buat OAuth app di: ${b.console}` : '';
}
async function loadBrands() {
  showBrands(await badi.invoke('apps:brands'));
}
$('brandWhich').onchange = showConsole;
$('brandCopy').onclick = () => navigator.clipboard.writeText($('brandRedirect').textContent);
$('brandSave').onclick = async () => {
  $('brandSave').disabled = true;
  $('brandStatus').textContent = 'Memasang…';
  $('brandStatus').className = 'status';
  try {
    const info = await badi.invoke('apps:brand', { id: $('brandWhich').value, clientId: $('brandClientId').value, clientSecret: $('brandSecret').value });
    $('brandClientId').value = '';
    $('brandSecret').value = '';
    showBrands(info);
    $('brandStatus').textContent = 'Terpasang. Sambungkan ulang aplikasinya supaya login lewat OAuth app-mu.';
    $('brandStatus').className = 'status ok';
    loadApps();
  } catch (e) {
    $('brandStatus').textContent = e.message;
    $('brandStatus').className = 'status err';
  }
  $('brandSave').disabled = brandInfo?.kind !== 'project';
};

// ---------- Badi Cloud ----------
// Shown only where the app has an account backend (B.A.D.I. v2); elsewhere the section stays hidden.
// The price list comes from Badi Cloud (its `plans` table), so prices, credits and badges change there.
const rupiah = (n) => `Rp${Number(n).toLocaleString('id-ID')}`;
const fmtDate = (ms) => new Date(ms).toLocaleDateString('id-ID', { day: 'numeric', month: 'long', year: 'numeric' });

function cloudStatus(text, error = false) {
  $('cloudStatus').textContent = text || '';
  $('cloudStatus').className = error ? 'status error' : 'status';
}

// One plan card: name and badge, price, credits and price per credit, and its button.
function planCard(p, { per, button, disabled, current, onClick }) {
  const card = document.createElement('div');
  card.className = 'plan' + (current ? ' current' : '');
  const head = document.createElement('div');
  head.className = 'head';
  const name = document.createElement('b');
  name.textContent = p.name;
  head.append(name);
  if (p.badge) {
    const badge = document.createElement('span');
    badge.className = 'badge';
    badge.textContent = p.badge;
    head.append(badge);
  }
  const price = document.createElement('div');
  price.className = 'price';
  price.textContent = p.price ? `${rupiah(p.price)}${per}` : 'Custom';
  const credits = document.createElement('div');
  credits.className = 'credits';
  credits.textContent = p.credits ? `${p.credits} kredit` : 'Seat & kuota custom';
  const limits = document.createElement('div');
  limits.className = 'limits';
  limits.textContent = p.perCredit ? `${rupiah(p.perCredit)}/kredit` : '';
  const buy = document.createElement('button');
  buy.type = 'button';
  buy.textContent = button;
  buy.disabled = Boolean(disabled);
  buy.onclick = onClick;
  card.append(head, price, credits, limits, buy);
  return card;
}

// B.A.D.I. v2 keeps no API keys: Badi Cloud holds them, so the key fields and the own-OAuth-app
// section go away and signing in is what makes Badi work (cloudOnly, declared at the top).
let cloudSignedIn = null;
async function loadCloud() {
  let s;
  try {
    s = await badi.invoke('cloud:status');
  } catch {
    return; // no cloud in this build
  }
  $('cloudSection').hidden = false;
  cloudOnly = Boolean(s.cloudOnly);
  for (const id of ['keysHead', 'keysCard', 'elevenKeyBox', 'composioKeyBox', 'brandBox', 'cloudKeysNote']) $(id).hidden = cloudOnly;
  if (cloudOnly) {
    const free = s.plans?.free?.credits;
    $('cloudOut').querySelector('.hint').textContent = `Notch gratis tanpa akun. Masuk dengan akun Google untuk memakai AI aina${free ? `: dapat ${free} kredit gratis sekali, lalu langganan atau Top-Up` : ', lalu langganan atau Top-Up'}. Tidak ada API key yang perlu diisi atau disimpan di PC ini.`;
    // Its hint depends on being signed in: reload it only when that changes, not on every account update.
    if (cloudSignedIn !== Boolean(s.account)) loadApps();
  }
  cloudSignedIn = Boolean(s.account);
  $('cloudOut').hidden = Boolean(s.account);
  $('cloudIn').hidden = !s.account;
  if (!s.account) return;
  const a = s.account;
  const c = a.credits || {};
  $('cloudWho').textContent = a.email;
  $('cloudPlan').textContent = `Paket ${a.planName}`;
  const r = a.renewal || {};
  $('cloudUntil').textContent = a.subscribed ? (r.auto ? `· diperpanjang otomatis ${fmtDate(a.until)}` : `· berakhir ${fmtDate(a.until)}`) : '';
  showRenewal(a, r);
  $('cloudCredits').textContent = c.left ?? 0;
  $('cloudCreditsDetail').textContent = a.subscribed ? `· langganan ${c.subscription}/${c.subscriptionTotal} · Top-Up & gratis ${c.topup}` : c.out ? '· habis' : '';
  const bar = $('cloudSubBar');
  bar.hidden = !a.subscribed;
  if (a.subscribed) {
    const pct = c.subscriptionTotal ? Math.round((c.subscription / c.subscriptionTotal) * 100) : 0;
    bar.querySelector('i').style.width = `${pct}%`;
    bar.classList.toggle('warn', c.low && !c.out);
    bar.classList.toggle('full', c.out);
  }
  // The closed beta runs on its one-off credit, and an unlimited account needs none: nothing to buy.
  const beta = a.plan === 'beta';
  $('cloudBetaNote').hidden = !beta;
  $('cloudBuy').hidden = beta || a.unlimited;
  if (beta || a.unlimited) return;

  const list = s.plans || {};
  // No early renewal: while a subscription runs, more credits come from top-ups.
  const subs = $('cloudSubs');
  subs.textContent = '';
  for (const p of list.subscriptions || []) {
    const current = a.subscribed && a.plan === p.id;
    subs.append(planCard(p, { per: '/bln', button: current ? 'Aktif' : 'Langganan', disabled: !a.canSubscribe, current, onClick: () => checkout(p.id) }));
  }
  $('cloudSubNote').textContent = a.canSubscribe ? '' : `Langganan ${a.planName} berjalan sampai ${fmtDate(a.until)} dan tidak bisa diganti sebelum itu. Kalau kreditnya habis, beli Top-Up.`;
  const tops = $('cloudTopups');
  tops.textContent = '';
  for (const p of list.topups || []) tops.append(planCard(p, { per: '/pack', button: 'Beli', onClick: () => checkout(p.id) }));
  const ent = $('cloudEnterprise');
  ent.textContent = '';
  if (list.enterprise) {
    const contact = list.enterprise.contact;
    ent.append(planCard(list.enterprise, { per: '', button: 'Hubungi Sales', disabled: !contact, onClick: () => badi.invoke('open', contact).catch((e) => cloudStatus(e.message, true)) }));
    if (!contact) ent.querySelector('.limits').textContent = 'Untuk tim dan perusahaan: hubungi tim aina.';
  }
}

// A card subscription renews by itself until cancelled (the plan then runs to the end of its cycle).
// Other payments renew nothing, so the row shows only with a saved card or a renewal that stopped.
function showRenewal(a, r) {
  const card = r.card ? `kartu ${r.card.replace(/^(\d{4})\d*-?/, '$1 •••• ')}` : '';
  const box = $('cloudRenew');
  box.hidden = !a.subscribed || !(r.card || r.stopped);
  if (box.hidden) return;
  const btn = $('cloudRenewBtn');
  $('cloudCardDrop').hidden = !r.card;
  if (r.auto && r.failures > 0) {
    $('cloudRenewText').textContent = `Perpanjangan dengan ${card} ditolak (${r.failures}/${r.tries}); dicoba lagi besok. Paketmu tetap aktif selama itu. Cek kartumu.`;
  } else if (r.auto) {
    $('cloudRenewText').textContent = `Diperpanjang otomatis ${fmtDate(a.until)} dengan ${card}. Batalkan kapan saja; paket tetap aktif sampai tanggal itu.`;
  } else if (r.stopped) {
    $('cloudRenewText').textContent = `Perpanjangan otomatis berhenti: kartunya ditolak atau kedaluwarsa. Paket berakhir ${fmtDate(a.until)}.`;
  } else {
    $('cloudRenewText').textContent = `Perpanjangan otomatis dibatalkan. Paket berakhir ${fmtDate(a.until)}.`;
  }
  btn.hidden = !r.card;
  btn.textContent = r.auto ? 'Batalkan perpanjangan' : 'Aktifkan lagi';
  btn.onclick = async () => {
    if (r.auto && !confirm(`Batalkan perpanjangan otomatis? Paket ${a.planName} tetap aktif sampai ${fmtDate(a.until)}.`)) return;
    btn.disabled = true;
    try {
      await badi.invoke(r.auto ? 'cloud:cancelRenewal' : 'cloud:resumeRenewal');
      cloudStatus(r.auto ? `Perpanjangan dibatalkan. Paket tetap aktif sampai ${fmtDate(a.until)}.` : 'Perpanjangan otomatis aktif lagi.');
    } catch (e) {
      cloudStatus(e.message, true);
    }
    btn.disabled = false;
    loadCloud();
  };
  $('cloudCardDrop').onclick = async () => {
    if (!confirm('Hapus kartu yang tersimpan? Perpanjangan otomatis ikut berhenti; paket tetap aktif sampai akhir siklus.')) return;
    try {
      await badi.invoke('cloud:dropCard');
      cloudStatus('Kartu dihapus dari aina.');
    } catch (e) {
      cloudStatus(e.message, true);
    }
    loadCloud();
  };
}

async function checkout(item) {
  cloudStatus('Membuka halaman pembayaran…');
  try {
    await badi.invoke('cloud:checkout', item);
    cloudStatus('Selesaikan pembayaran di browser. Kreditnya masuk otomatis setelah lunas.');
  } catch (e) {
    cloudStatus(e.message, true);
  }
}

$('cloudLogin').onclick = async () => {
  cloudStatus('Membuka login Google di browser…');
  try {
    await badi.invoke('cloud:login');
    cloudStatus('Selesaikan login di browser. aina terbuka lagi dan langsung masuk.');
  } catch (e) {
    cloudStatus(e.message, true);
  }
};
$('cloudLogout').onclick = async () => {
  await badi.invoke('cloud:logout');
  cloudStatus(cloudOnly ? 'Sudah keluar. Masuk lagi untuk memakai aina.' : 'Sudah keluar. aina kembali memakai API key milikmu.');
  await loadCloud();
};
$('cloudDelete').onclick = async () => {
  if (!confirm('Hapus akun aina Cloud beserta datanya? Langganan yang tersisa ikut hilang.')) return;
  try {
    await badi.invoke('cloud:deleteAccount');
    cloudStatus('Akunmu sudah dihapus.');
    await loadCloud();
  } catch (e) {
    cloudStatus(e.message, true);
  }
};
for (const a of document.querySelectorAll('[data-open]')) a.onclick = (e) => (e.preventDefault(), badi.invoke('cloud:open', a.dataset.open));
badi.on('cloud:changed', loadCloud);
loadCloud();

// B.A.D.I. v2 keeps dictations as notes (Markdown files) and shows them in the notch, instead of typing them.
badi
  .invoke('dictation:info')
  .then((d) => {
    if (d?.folder) $('dictWhat').textContent = `untuk mendikte catatan (atau tekan sekali untuk mulai, tekan lagi untuk selesai, jadi tanganmu bebas). aina merapikan dan meringkasnya, menampilkannya di notch, lalu menyimpannya di ${d.folder}`;
  })
  .catch(() => {});
