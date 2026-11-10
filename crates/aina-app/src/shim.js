// window.aina for aina v2: the same API v1's preload.js exposed (invoke / send / on / pathsOf, same
// channel allowlists), carried over Tauri instead of Electron IPC, so every v1 page runs unchanged.
// Injected into each webview before its own scripts (initialization script).
(() => {
  if (window.aina) return;

  const INVOKE = new Set([
    'config:get', 'config:set', 'config:clearKey', 'test:anthropic', 'ask',
    'agents:list', 'agents:get', 'agents:create', 'agents:remove', 'agents:send', 'agents:stop',
    'agents:folder', 'agents:routine', 'agents:engine', 'sprites:diff', 'sprites:edit', 'open',
    'cloud:status', 'cloud:login', 'cloud:logout', 'cloud:checkout', 'cloud:deleteAccount', 'cloud:open',
    'apps:list', 'apps:connect', 'apps:disconnect', 'apps:toggle', 'apps:brands', 'apps:brand', 'apps:unbrand',
    'extension:info', 'extension:newCode', 'extension:folder',
    'dictation:info',
    'focus:get', 'focus:start', 'focus:update', 'focus:end', 'focus:pick',
    'reminder:get', 'reminder:save', 'reminder:delete', 'reminder:cities',
    'location:get', 'location:detect', 'location:search',
  ]);
  const SEND = new Set(['record:data', 'record:level', 'record:error', 'record:autostop', 'rt:event', 'rt:state', 'worker:done', 'overlay:stepDone', 'worker:voices', 'tts:started', 'tts:boundary', 'island:interactive', 'island:typing', 'island:act', 'drop:files', 'drop:hover', 'cursor:zone', 'perf:sample']);
  const LISTEN = new Set([
    'phase', 'mouse', 'subtitleDisplay', 'voice', 'islandLabels', 'textProgress', 'level', 'visual', 'text', 'transcript', 'clear', 'linger', 'error', 'mode',
    'record:start', 'record:stop', 'tts:speak', 'tts:stop', 'tts:play', 'chime',
    'rt:connect', 'rt:send', 'rt:mic', 'rt:close',
    'agents:changed', 'agents:log', 'agents:error', 'agents:select', 'sprites:status',
    'extension:status', 'settings:focus', 'island:panel', 'notch:status', 'island:lottie', 'island:dropHover', 'actionAt', 'hint', 'ink', 'gesture', 'scan', 'elapsed', 'apps:changed', 'cloud:changed',
    'media:art', 'media:lyrics', 'media:track', 'calendar:data', 'fullscreen',
  ]);

  // Tauri's own globals are injected alongside this script; wait for them rather than assume an order.
  const tauri = new Promise((resolve) => {
    const tick = () => (window.__TAURI__ && window.__TAURI__.event ? resolve(window.__TAURI__) : setTimeout(tick, 4));
    tick();
  });

  // Every listener registered while the page loads; main is told the page is ready only after they are
  // all in place, so nothing it sends at load (labels, notch status, Lottie data) is missed.
  const listening = [];
  const listen = (ch, fn) => {
    const p = tauri.then((t) => t.event.listen(ch, (e) => fn(e.payload)));
    listening.push(p);
    return p;
  };

  window.aina = {
    // Rust errors arrive as strings; pages expect Error objects with a message, as from Electron.
    invoke: (ch, ...args) =>
      INVOKE.has(ch)
        ? tauri.then((t) => t.core.invoke('aina_invoke', { ch, args })).catch((e) => Promise.reject(e instanceof Error ? e : new Error(typeof e === 'string' ? e : e?.message || String(e))))
        : Promise.reject(new Error('blocked: ' + ch)),
    send: (ch, payload) => {
      if (SEND.has(ch)) tauri.then((t) => t.core.invoke('aina_send', { ch, payload: payload === undefined ? null : payload }));
    },
    on: (ch, fn) => {
      if (LISTEN.has(ch)) listen(ch, fn);
    },
    // Dropped files carry their path here (see the drag-and-drop bridge below).
    pathsOf: (files) => Array.from(files || [], (f) => f && f.path).filter(Boolean),
  };

  // The overlay is click-through, so the page sees no mouse moves of its own; main sends the cursor
  // position and it becomes the mousemove events the notch's hover code already listens to.
  // A demo (AINA_DEMO=sprites) may also click there, without touching the real mouse.
  listen('aina:cursor', (p) => {
    document.dispatchEvent(new MouseEvent('mousemove', { clientX: p.x, clientY: p.y, bubbles: true }));
    if (p.click) (typeof p.click === 'string' ? document.querySelector(p.click) : document.elementFromPoint(p.x, p.y))?.click();
  });

  // Tauri takes OS drops itself (with real paths); replay them as the DOM drag events v1 pages handle.
  tauri.then((t) => {
    const view = t.webview && t.webview.getCurrentWebview && t.webview.getCurrentWebview();
    if (!view || !view.onDragDropEvent) return;
    let files = [];
    const fire = (type) => {
      const e = new Event(type, { bubbles: true, cancelable: true });
      Object.defineProperty(e, 'dataTransfer', { value: { files, dropEffect: 'copy', types: ['Files'] } });
      document.dispatchEvent(e);
    };
    view.onDragDropEvent(({ payload }) => {
      if (payload.paths) files = payload.paths.map((path) => ({ path, name: path.split(/[\\/]/).pop() }));
      if (payload.type === 'enter') fire('dragenter');
      else if (payload.type === 'over') fire('dragover');
      else if (payload.type === 'leave') fire('dragleave');
      else if (payload.type === 'drop') fire('drop');
    });
  });

  const ready = () => Promise.all(listening).then(() => tauri.then((t) => t.core.invoke('aina_ready')));
  if (document.readyState === 'complete') ready();
  else addEventListener('load', ready, { once: true });
})();
