const $ = (id) => document.getElementById(id);

const LABELS = {
  connected: ['Tersambung ke aina.', 'ok'],
  connecting: ['Menyambungkan…', ''],
  disconnected: ['Terputus. Menyambungkan ulang…', ''],
  offline: [null, 'err'],
  unpaired: ['Masukkan kode 6 digit dari Settings aina.', ''],
  replaced: ['Browser lain sedang tersambung ke aina.', 'err'],
};

async function render() {
  const { status } = await chrome.storage.session.get('status');
  const s = status || { state: 'unpaired' };
  const [text, cls] = LABELS[s.state] || [s.state, ''];
  $('status').textContent = text || s.error || 'Tidak tersambung.';
  $('status').className = cls;
  $('dot').className = s.state === 'connected' ? 'connected' : '';
}

async function load() {
  const { code = '', port = '' } = await chrome.storage.local.get(['code', 'port']);
  $('code').value = code;
  $('port').value = port;
  render();
}

$('save').onclick = async () => {
  const code = $('code').value.replace(/\D/g, '');
  if (code.length !== 6) {
    $('status').textContent = 'Kodenya 6 angka.';
    $('status').className = 'err';
    return;
  }
  const port = Number($('port').value) || undefined;
  await chrome.storage.local.set({ code, ...(port ? { port } : {}) });
  if (!port) await chrome.storage.local.remove('port');
  render();
};
$('retry').onclick = () => chrome.runtime.sendMessage('reconnect').then(render);
$('code').addEventListener('keydown', (e) => e.key === 'Enter' && $('save').click());
chrome.storage.onChanged.addListener((_c, area) => area === 'session' && render());

load();
