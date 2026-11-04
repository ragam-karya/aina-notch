// Runs browser toolset members (browser_toolset_20260801) in the user's browser through the
// DevTools protocol (chrome.debugger). Screenshots are scaled to fit the model's image limits and
// coordinates are scaled back to CSS pixels before dispatching.
import { renderTree, findNodes, RefMap, MAX_CHARS } from './lib/axtree.js';
import { keyEvents, modifierBits } from './lib/keys.js';

const SHOT_MAX_EDGE = 1280;
const PROTOCOL = '1.3';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const tabKey = (id) => `tab-${id}`;
function parseTabId(tabId) {
  const m = /^tab-(\d+)$/.exec(String(tabId));
  if (!m) throw new Error(`Unknown tab_id "${tabId}". Use list_tabs to see the open tabs.`);
  return +m[1];
}

function toBase64(buf) {
  const bytes = new Uint8Array(buf);
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

// Decodes a screenshot, optionally crops it, and re-encodes it at the given size.
async function reencode(base64, width, height, crop) {
  const blob = await (await fetch(`data:image/png;base64,${base64}`)).blob();
  const bmp = await createImageBitmap(blob);
  const canvas = new OffscreenCanvas(width, height);
  const g = canvas.getContext('2d');
  if (crop) g.drawImage(bmp, crop.x, crop.y, crop.width, crop.height, 0, 0, width, height);
  else g.drawImage(bmp, 0, 0, width, height);
  bmp.close?.();
  const out = await canvas.convertToBlob({ type: 'image/jpeg', quality: 0.82 });
  return toBase64(await out.arrayBuffer());
}

const fit = (w, h, max = SHOT_MAX_EDGE) => {
  const s = Math.min(1, max / Math.max(w, h));
  return { width: Math.max(1, Math.round(w * s)), height: Math.max(1, Math.round(h * s)) };
};

export class Executor {
  constructor() {
    this.activeTab = null;
    this.attached = new Set();
    this.refs = new Map(); // tabId -> RefMap
    this.cancelled = false;
    chrome.debugger.onDetach.addListener(({ tabId }, reason) => {
      this.attached.delete(tabId);
      // The user pressed "Cancel" on the "is debugging this browser" bar: stop controlling.
      if (reason === 'canceled_by_user') this.cancelled = true;
    });
    chrome.tabs.onUpdated.addListener((tabId, info) => {
      if (info.status === 'loading') this.refs.get(tabId)?.reset();
    });
    chrome.tabs.onRemoved.addListener((tabId) => {
      this.attached.delete(tabId);
      this.refs.delete(tabId);
      if (this.activeTab === tabId) this.activeTab = null;
    });
  }

  // ---------- tabs & debugger ----------
  async currentTab() {
    if (this.activeTab != null) {
      try {
        await chrome.tabs.get(this.activeTab);
        return this.activeTab;
      } catch {
        this.activeTab = null;
      }
    }
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    if (!tab) {
      const created = await chrome.tabs.create({ active: true });
      this.activeTab = created.id;
    } else this.activeTab = tab.id;
    return this.activeTab;
  }

  async tabFor(input) {
    return input && input.tab_id != null ? parseTabId(input.tab_id) : this.currentTab();
  }

  refMap(tabId) {
    if (!this.refs.has(tabId)) this.refs.set(tabId, new RefMap());
    return this.refs.get(tabId);
  }

  async attach(tabId) {
    if (this.cancelled) throw new Error('The user stopped browser control (Cancel on the debugging bar). Ask them before trying again.');
    if (this.attached.has(tabId)) return;
    try {
      await chrome.debugger.attach({ tabId }, PROTOCOL);
    } catch (e) {
      if (!/already attached/i.test(e.message)) {
        const tab = await chrome.tabs.get(tabId).catch(() => null);
        throw new Error(`Can't control this tab${tab?.url ? ` (${tab.url.slice(0, 80)})` : ''}: ${e.message}. Browser pages such as chrome:// and the web store are off limits; navigate to a website first.`);
      }
    }
    this.attached.add(tabId);
  }

  async cdp(tabId, method, params = {}) {
    await this.attach(tabId);
    return chrome.debugger.sendCommand({ tabId }, method, params);
  }

  async browserState() {
    const tabs = await chrome.tabs.query({ windowType: 'normal' });
    const active = await this.currentTab();
    return {
      type: 'browser_state',
      tabs: tabs.slice(0, 100).map((t) => ({
        tab_id: tabKey(t.id),
        title: (t.title || '').slice(0, 4096),
        url: (t.url || t.pendingUrl || '').slice(0, 4096),
        active: t.id === active,
      })),
    };
  }

  async endSession() {
    for (const tabId of [...this.attached]) await chrome.debugger.detach({ tabId }).catch(() => {});
    this.attached.clear();
    this.activeTab = null;
    this.cancelled = false;
  }

  // ---------- geometry ----------
  // The screenshot's size and how it maps to CSS pixels of the viewport.
  async viewport(tabId) {
    const m = await this.cdp(tabId, 'Page.getLayoutMetrics');
    const vv = m.cssVisualViewport || m.visualViewport;
    const css = { width: Math.round(vv.clientWidth), height: Math.round(vv.clientHeight) };
    const shot = fit(css.width, css.height);
    return { css, shot, scale: css.width / shot.width };
  }

  async point(tabId, target, what = 'target') {
    if (!target || typeof target !== 'object') throw new Error(`${what} is required.`);
    if (target.type === 'ref') return this.refPoint(tabId, target.ref);
    if (target.type !== 'coordinate' || !Number.isFinite(target.x) || !Number.isFinite(target.y)) {
      throw new Error(`${what} must be {type:"coordinate", x, y} or {type:"ref", ref}.`);
    }
    const v = await this.viewport(tabId);
    if (target.x < 0 || target.y < 0 || target.x >= v.shot.width || target.y >= v.shot.height) {
      throw new Error(`${what} (${target.x}, ${target.y}) is outside the ${v.shot.width}x${v.shot.height} viewport.`);
    }
    return { x: target.x * v.scale, y: target.y * v.scale };
  }

  backendId(tabId, ref) {
    const id = this.refMap(tabId).backendFor(ref);
    if (id == null) throw new Error(`${ref} is stale or not found on the current page. Re-read the page to get fresh references.`);
    return id;
  }

  async refPoint(tabId, ref) {
    const backendNodeId = this.backendId(tabId, ref);
    try {
      await this.cdp(tabId, 'DOM.scrollIntoViewIfNeeded', { backendNodeId });
      const { quads } = await this.cdp(tabId, 'DOM.getContentQuads', { backendNodeId });
      if (!quads || !quads.length) throw new Error('not visible');
      const q = quads[0];
      return { x: (q[0] + q[2] + q[4] + q[6]) / 4, y: (q[1] + q[3] + q[5] + q[7]) / 4 };
    } catch (e) {
      if (/not visible/.test(e.message)) throw new Error(`${ref} is not visible on the page (it may be hidden or collapsed).`);
      throw new Error(`${ref} is stale or not found on the current page. Re-read the page to get fresh references.`);
    }
  }

  // ---------- input ----------
  async mouse(tabId, type, p, extra = {}) {
    await this.cdp(tabId, 'Input.dispatchMouseEvent', { type, x: p.x, y: p.y, ...extra });
  }

  async click(tabId, input, button, count) {
    const p = await this.point(tabId, input.target);
    const modifiers = modifierBits(input.modifiers);
    await this.mouse(tabId, 'mouseMoved', p, { modifiers });
    for (let i = 1; i <= count; i++) {
      await this.mouse(tabId, 'mousePressed', p, { button, clickCount: i, modifiers });
      await this.mouse(tabId, 'mouseReleased', p, { button, clickCount: i, modifiers });
    }
    await sleep(150);
    return 'OK';
  }

  async screenshot(tabId, region) {
    const v = await this.viewport(tabId);
    const { data } = await this.cdp(tabId, 'Page.captureScreenshot', { format: 'png' });
    if (!region) return { type: 'image', source: { type: 'base64', media_type: 'image/jpeg', data: await reencode(data, v.shot.width, v.shot.height) } };
    if (!Array.isArray(region) || region.length !== 4 || !region.every(Number.isFinite)) throw new Error('region must be [x0, y0, x1, y1].');
    const blob = await (await fetch(`data:image/png;base64,${data}`)).blob();
    const bmp = await createImageBitmap(blob);
    const px = bmp.width / v.shot.width; // screenshot px -> captured px (includes devicePixelRatio)
    bmp.close?.();
    const [x0, y0, x1, y1] = region;
    const left = Math.max(0, Math.min(x0, x1));
    const top = Math.max(0, Math.min(y0, y1));
    const right = Math.min(v.shot.width, Math.max(x0, x1));
    const bottom = Math.min(v.shot.height, Math.max(y0, y1));
    if (right - left < 4 || bottom - top < 4) throw new Error('zoom region is empty or outside the viewport.');
    const crop = { x: left * px, y: top * px, width: (right - left) * px, height: (bottom - top) * px };
    // The region is enlarged to the usual screenshot size, keeping its aspect ratio.
    const s = Math.min(v.shot.width / (right - left), v.shot.height / (bottom - top));
    const size = { width: Math.round((right - left) * s), height: Math.round((bottom - top) * s) };
    return { type: 'image', source: { type: 'base64', media_type: 'image/jpeg', data: await reencode(data, size.width, size.height, crop) } };
  }

  async axNodes(tabId) {
    await this.cdp(tabId, 'Accessibility.enable');
    const { nodes } = await this.cdp(tabId, 'Accessibility.getFullAXTree');
    return nodes || [];
  }

  async waitForLoad(tabId, timeoutMs = 15000) {
    const start = Date.now();
    await sleep(300);
    while (Date.now() - start < timeoutMs) {
      const tab = await chrome.tabs.get(tabId);
      if (tab.status === 'complete') return tab;
      await sleep(200);
    }
    return chrome.tabs.get(tabId);
  }

  // ---------- members ----------
  async run(name, input = {}) {
    const text = (t) => [{ type: 'text', text: t }];
    switch (name) {
      case 'navigate': {
        const tabId = await this.tabFor(input);
        const u = String(input.url || '').trim();
        if (u === 'back' || u === 'forward') await chrome.tabs[u === 'back' ? 'goBack' : 'goForward'](tabId);
        else if (u === 'reload') await chrome.tabs.reload(tabId);
        else await chrome.tabs.update(tabId, { url: safeHttpUrl(u) });
        const tab = await this.waitForLoad(tabId);
        return [...text(`Navigated to ${tab.url}${tab.status !== 'complete' ? ' (still loading)' : ''}`), await this.browserState()];
      }
      case 'screenshot':
        return [await this.screenshot(await this.tabFor(input))];
      case 'zoom':
        return [await this.screenshot(await this.tabFor(input), input.region)];
      case 'left_click':
        return text(await this.click(await this.tabFor(input), input, 'left', 1));
      case 'right_click':
        return text(await this.click(await this.tabFor(input), input, 'right', 1));
      case 'middle_click':
        return text(await this.click(await this.tabFor(input), input, 'middle', 1));
      case 'double_click':
        return text(await this.click(await this.tabFor(input), input, 'left', 2));
      case 'triple_click':
        return text(await this.click(await this.tabFor(input), input, 'left', 3));
      case 'hover':
      case 'mouse_move': {
        const tabId = await this.tabFor(input);
        await this.mouse(tabId, 'mouseMoved', await this.point(tabId, input.target));
        return text('OK');
      }
      case 'left_click_drag': {
        const tabId = await this.tabFor(input);
        const a = await this.point(tabId, input.from, 'from');
        const b = await this.point(tabId, input.target);
        await this.mouse(tabId, 'mouseMoved', a);
        await this.mouse(tabId, 'mousePressed', a, { button: 'left', clickCount: 1 });
        for (let i = 1; i <= 10; i++) {
          await this.mouse(tabId, 'mouseMoved', { x: a.x + ((b.x - a.x) * i) / 10, y: a.y + ((b.y - a.y) * i) / 10 }, { button: 'left', buttons: 1 });
        }
        await this.mouse(tabId, 'mouseReleased', b, { button: 'left', clickCount: 1 });
        return text('OK');
      }
      case 'scroll': {
        const tabId = await this.tabFor(input);
        const p = await this.point(tabId, input.target);
        const amount = Math.max(1, Math.min(10, Number(input.scroll_amount) || 3)) * 100;
        const dir = input.scroll_direction;
        const delta = { up: [0, -amount], down: [0, amount], left: [-amount, 0], right: [amount, 0] }[dir];
        if (!delta) throw new Error('scroll_direction must be up, down, left or right.');
        await this.mouse(tabId, 'mouseWheel', p, { deltaX: delta[0], deltaY: delta[1] });
        await sleep(250);
        return text('OK');
      }
      case 'scroll_to': {
        const tabId = await this.tabFor(input);
        if (input.target?.type !== 'ref') throw new Error('scroll_to needs a {type:"ref"} target.');
        await this.refPoint(tabId, input.target.ref);
        return text('OK');
      }
      case 'type': {
        const tabId = await this.tabFor(input);
        if (typeof input.text !== 'string') throw new Error('type: "text" is required.');
        await this.cdp(tabId, 'Input.insertText', { text: input.text });
        return text('OK');
      }
      case 'key': {
        const tabId = await this.tabFor(input);
        const [down, up] = keyEvents(input.text);
        const repeat = Math.max(1, Math.min(100, Number(input.repeat) || 1));
        for (let i = 0; i < repeat; i++) {
          await this.cdp(tabId, 'Input.dispatchKeyEvent', down);
          await this.cdp(tabId, 'Input.dispatchKeyEvent', up);
        }
        await sleep(100);
        return text('OK');
      }
      case 'wait':
        await sleep(Math.max(0, Math.min(30, Number(input.duration) || 1)) * 1000);
        return text('OK');
      case 'read_page': {
        const tabId = await this.tabFor(input);
        const nodes = await this.axNodes(tabId);
        const refs = this.refMap(tabId);
        const rootBackendId = input.ref ? this.backendId(tabId, input.ref) : undefined;
        const { text: out } = renderTree(nodes, {
          filter: input.filter,
          depth: Math.max(1, Number(input.depth) || 15),
          rootBackendId,
          refFor: (id) => refs.refFor(id),
        });
        if (out == null) throw new Error(`${input.ref} is stale or not found on the current page. Re-read the page to get fresh references.`);
        const tab = await chrome.tabs.get(tabId);
        return text(`${tab.title}\n${tab.url}\n\n${out}`);
      }
      case 'find': {
        const tabId = await this.tabFor(input);
        const refs = this.refMap(tabId);
        return text(findNodes(await this.axNodes(tabId), input.query, { refFor: (id) => refs.refFor(id) }));
      }
      case 'get_page_text': {
        const tabId = await this.tabFor(input);
        const { result } = await this.cdp(tabId, 'Runtime.evaluate', {
          expression: '(document.body ? document.body.innerText : "")',
          returnByValue: true,
        });
        const tab = await chrome.tabs.get(tabId);
        let body = String(result?.value || '');
        if (body.length > MAX_CHARS) body = body.slice(0, MAX_CHARS) + `\n[Truncated at ${MAX_CHARS} characters.]`;
        return text(`${tab.title}\n${tab.url}\n\n${body}`);
      }
      case 'form_input': {
        const tabId = await this.tabFor(input);
        if (input.target?.type !== 'ref') throw new Error('form_input needs a {type:"ref"} target.');
        const backendNodeId = this.backendId(tabId, input.target.ref);
        let objectId;
        try {
          ({ object: { objectId } } = await this.cdp(tabId, 'DOM.resolveNode', { backendNodeId }));
        } catch {
          throw new Error(`${input.target.ref} is stale or not found on the current page. Re-read the page to get fresh references.`);
        }
        const { result, exceptionDetails } = await this.cdp(tabId, 'Runtime.callFunctionOn', {
          objectId,
          functionDeclaration: FORM_INPUT,
          arguments: [{ value: input.value }],
          returnByValue: true,
        });
        if (exceptionDetails) throw new Error(exceptionDetails.exception?.description || 'form_input failed.');
        return text(String(result?.value || 'OK'));
      }
      case 'new_tab': {
        const tab = await chrome.tabs.create({ active: true });
        this.activeTab = tab.id;
        return [await this.browserState()];
      }
      case 'list_tabs':
        return [await this.browserState()];
      case 'switch_tab': {
        const tabId = parseTabId(input.tab_id);
        const tab = await chrome.tabs.update(tabId, { active: true });
        await chrome.windows.update(tab.windowId, { focused: true });
        this.activeTab = tabId;
        return [await this.browserState()];
      }
      case 'close_tab': {
        const tabId = parseTabId(input.tab_id);
        await chrome.tabs.remove(tabId);
        if (this.activeTab === tabId) this.activeTab = null;
        return [await this.browserState()];
      }
      default:
        throw new Error(`"${name}" is not supported by the aina extension.`);
    }
  }
}

// Sets a form control's value the way a user would, so page scripts notice the change.
const FORM_INPUT = `function (value) {
  const el = this;
  const tag = el.tagName;
  const fire = (t) => el.dispatchEvent(new Event(t, { bubbles: true }));
  if (tag === 'SELECT') {
    const want = String(value);
    const opt = [...el.options].find((o) => o.value === want || o.textContent.trim() === want);
    if (!opt) throw new Error('No option "' + want + '". Options: ' + [...el.options].map((o) => o.textContent.trim()).slice(0, 30).join(', '));
    el.value = opt.value; fire('input'); fire('change');
    return 'Selected "' + opt.textContent.trim() + '"';
  }
  if (tag === 'INPUT' && (el.type === 'checkbox' || el.type === 'radio')) {
    const on = value === true || value === 'true' || value === 1 || value === 'on';
    if (el.checked !== on) el.click();
    return el.type + ' is now ' + (el.checked ? 'checked' : 'unchecked');
  }
  if (el.isContentEditable) {
    el.focus(); el.textContent = String(value); fire('input');
    return 'OK';
  }
  if (tag === 'INPUT' || tag === 'TEXTAREA') {
    el.focus();
    const proto = tag === 'INPUT' ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, String(value));
    fire('input'); fire('change');
    return 'OK';
  }
  throw new Error('This element is not a form field (' + tag.toLowerCase() + ').');
}`;

// Only http(s) pages; a bare "example.com" means https.
export function safeHttpUrl(raw) {
  let text = String(raw || '').trim();
  if (!text) throw new Error('navigate: "url" is required.');
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(text)) text = `https://${text}`;
  let url;
  try {
    url = new URL(text);
  } catch {
    throw new Error(`navigate: "${raw}" is not a valid URL.`);
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') throw new Error('navigate: only http and https URLs are allowed.');
  return url.href;
}
