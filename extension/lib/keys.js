// Key names from the browser toolset ("Return", "ctrl+a", "shift+Tab") to DevTools key events. Pure.

const MOD_BITS = { alt: 1, option: 1, ctrl: 2, control: 2, meta: 4, cmd: 4, command: 4, super: 4, win: 4, shift: 8 };

const NAMED = {
  return: ['Enter', 'Enter', 13, '\r'], enter: ['Enter', 'Enter', 13, '\r'], kp_enter: ['Enter', 'NumpadEnter', 13, '\r'],
  tab: ['Tab', 'Tab', 9], escape: ['Escape', 'Escape', 27], esc: ['Escape', 'Escape', 27],
  backspace: ['Backspace', 'Backspace', 8], delete: ['Delete', 'Delete', 46], del: ['Delete', 'Delete', 46], insert: ['Insert', 'Insert', 45],
  space: [' ', 'Space', 32, ' '],
  home: ['Home', 'Home', 36], end: ['End', 'End', 35],
  page_up: ['PageUp', 'PageUp', 33], prior: ['PageUp', 'PageUp', 33], pageup: ['PageUp', 'PageUp', 33],
  page_down: ['PageDown', 'PageDown', 34], next: ['PageDown', 'PageDown', 34], pagedown: ['PageDown', 'PageDown', 34],
  left: ['ArrowLeft', 'ArrowLeft', 37], up: ['ArrowUp', 'ArrowUp', 38], right: ['ArrowRight', 'ArrowRight', 39], down: ['ArrowDown', 'ArrowDown', 40],
  arrowleft: ['ArrowLeft', 'ArrowLeft', 37], arrowup: ['ArrowUp', 'ArrowUp', 38], arrowright: ['ArrowRight', 'ArrowRight', 39], arrowdown: ['ArrowDown', 'ArrowDown', 40],
};

// Editing shortcuts need an explicit editor command, or the page just sees the key press.
const COMMANDS = { 'ctrl+a': 'selectAll', 'ctrl+c': 'copy', 'ctrl+x': 'cut', 'ctrl+v': 'paste', 'ctrl+z': 'undo', 'ctrl+y': 'redo', 'ctrl+shift+z': 'redo' };

function keyInfo(name) {
  const n = name.toLowerCase();
  if (NAMED[n]) {
    const [key, code, keyCode, text] = NAMED[n];
    return { key, code, keyCode, text };
  }
  const f = n.match(/^f([1-9]|1[0-2])$/);
  if (f) return { key: `F${f[1]}`, code: `F${f[1]}`, keyCode: 111 + +f[1] };
  const chars = [...name];
  if (chars.length === 1) {
    const ch = chars[0];
    const up = ch.toUpperCase();
    if (/^[a-z]$/i.test(ch)) return { key: ch, code: `Key${up}`, keyCode: up.charCodeAt(0), text: ch };
    if (/^[0-9]$/.test(ch)) return { key: ch, code: `Digit${ch}`, keyCode: ch.charCodeAt(0), text: ch };
    return { key: ch, code: '', keyCode: 0, text: ch };
  }
  return null;
}

export function modifierBits(mods) {
  if (!mods) return 0;
  const list = Array.isArray(mods) ? mods : String(mods).split('+');
  let bits = 0;
  for (const m of list) {
    const b = MOD_BITS[String(m).trim().toLowerCase()];
    if (b == null) throw new Error(`unknown modifier "${m}".`);
    bits |= b;
  }
  return bits;
}

// Returns the keyDown/keyUp event params for Input.dispatchKeyEvent.
export function keyEvents(text) {
  if (typeof text !== 'string' || !text.trim()) throw new Error('key: "text" must name a key, e.g. "Return" or "ctrl+a".');
  const parts = text.trim() === '+' ? ['+'] : text.split('+').map((p) => p.trim()).filter(Boolean);
  const keyName = parts[parts.length - 1];
  const modifiers = modifierBits(parts.slice(0, -1));
  const info = keyInfo(keyName);
  if (!info) throw new Error(`key: unknown key "${keyName}".`);
  // Text is only produced without Ctrl/Alt/Meta (those make shortcuts, not characters).
  const typing = info.text != null && !(modifiers & 7);
  let printed = info.text;
  if (typing && modifiers & 8 && /^[a-z]$/.test(printed)) printed = printed.toUpperCase();
  const command = COMMANDS[parts.map((p) => p.toLowerCase()).join('+')];
  const base = { key: info.key, code: info.code, windowsVirtualKeyCode: info.keyCode, nativeVirtualKeyCode: info.keyCode, modifiers };
  return [
    { ...base, type: typing ? 'keyDown' : 'rawKeyDown', ...(typing ? { text: printed, unmodifiedText: printed } : {}), ...(command ? { commands: [command] } : {}) },
    { ...base, type: 'keyUp' },
  ];
}
