// Badi browser extension: keeps a WebSocket to the Badi desktop app (127.0.0.1 only) and runs the
// browser actions Badi's brain asks for. Pair it with the 6-digit code from Badi's Settings.
import { Executor } from './executor.js';

const DEFAULT_PORT = 47821;
const PING_MS = 20000; // regular traffic also keeps this service worker alive
const executor = new Executor();

let ws = null;
let pingTimer = null;
let retryTimer = null;
let retryMs = 2000;
let queue = Promise.resolve(); // actions run one at a time, in order

async function settings() {
  const { code = '', port = DEFAULT_PORT } = await chrome.storage.local.get(['code', 'port']);
  return { code: String(code).trim(), port: Number(port) || DEFAULT_PORT };
}

async function setStatus(state, error = '') {
  await chrome.storage.session.set({ status: { state, error, at: Date.now() } });
  await chrome.action.setBadgeText({ text: state === 'connected' ? 'ON' : '' });
  await chrome.action.setBadgeBackgroundColor({ color: '#2B8AFF' });
}

function scheduleRetry() {
  clearTimeout(retryTimer);
  retryTimer = setTimeout(connect, retryMs);
  retryMs = Math.min(30000, retryMs * 2);
}

async function connect() {
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return;
  const { code, port } = await settings();
  if (!/^\d{6}$/.test(code)) return setStatus('unpaired');
  await setStatus('connecting');
  let opened = false;
  const socket = new WebSocket(`ws://127.0.0.1:${port}/?token=${encodeURIComponent(code)}`);
  ws = socket;
  socket.onopen = () => {
    opened = true;
    retryMs = 2000;
    const browser = /Edg\//.test(navigator.userAgent) ? 'Edge' : 'Chrome';
    socket.send(JSON.stringify({ type: 'hello', browser, version: chrome.runtime.getManifest().version }));
    clearInterval(pingTimer);
    pingTimer = setInterval(() => socket.readyState === WebSocket.OPEN && socket.send(JSON.stringify({ type: 'ping' })), PING_MS);
    setStatus('connected');
  };
  socket.onmessage = (ev) => {
    let msg;
    try {
      msg = JSON.parse(ev.data);
    } catch {
      return;
    }
    if (msg.type === 'call') queue = queue.then(() => handleCall(socket, msg));
    else if (msg.type === 'end') queue = queue.then(() => executor.endSession());
  };
  socket.onclose = (ev) => {
    clearInterval(pingTimer);
    if (ws === socket) ws = null;
    executor.endSession();
    // Closed before opening usually means Badi isn't running or the code is wrong.
    if (ev.code === 4000) return setStatus('replaced');
    setStatus(opened ? 'disconnected' : 'offline', opened ? '' : 'aina tidak menjawab. Pastikan app aina berjalan dan kodenya benar.');
    scheduleRetry();
  };
  socket.onerror = () => {};
}

async function handleCall(socket, { id, name, input }) {
  let reply;
  try {
    const content = await executor.run(name, input || {});
    reply = { type: 'result', id, content, isError: false };
  } catch (e) {
    reply = { type: 'result', id, content: [{ type: 'text', text: `Error: ${e.message}` }], isError: true };
  }
  if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify(reply));
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.alarms.create('badi-reconnect', { periodInMinutes: 1 });
  connect();
});
chrome.runtime.onStartup.addListener(connect);
chrome.alarms.onAlarm.addListener((a) => a.name === 'badi-reconnect' && connect());
chrome.storage.onChanged.addListener((changes, area) => {
  if (area === 'local' && (changes.code || changes.port)) {
    retryMs = 2000;
    ws?.close();
    ws = null;
    connect();
  }
});
chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg === 'reconnect') {
    retryMs = 2000;
    connect().then(() => reply(true));
    return true;
  }
});

connect();
