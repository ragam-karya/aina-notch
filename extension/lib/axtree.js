// Turns a Chrome DevTools accessibility tree (Accessibility.getFullAXTree) into the text the browser
// toolset's read_page and find return, tagging elements with refs like [ref_3]. Pure (no chrome.*),
// so it is unit-tested with node:test.

export const MAX_CHARS = 50000;

const INTERACTIVE = new Set([
  'button', 'link', 'textbox', 'searchbox', 'combobox', 'checkbox', 'radio', 'switch', 'slider', 'spinbutton',
  'menuitem', 'menuitemcheckbox', 'menuitemradio', 'tab', 'option', 'listbox', 'treeitem', 'textfield', 'popupbutton',
  'togglebutton', 'colorwell', 'datepicker', 'date', 'time', 'disclosuretriangle',
]);
// Wrappers that add nothing for the reader; their children are lifted up a level.
const TRANSPARENT = new Set(['generic', 'none', 'presentation', 'rootwebarea', 'webarea', 'group', 'section', 'div', 'span', 'inlinetextbox', 'linebreak']);
const STATES = ['checked', 'selected', 'expanded', 'disabled', 'required', 'focused', 'pressed', 'readonly'];

const val = (x) => (x && x.value != null ? String(x.value) : '');
const clean = (s, n = 200) => {
  const t = s.replace(/\s+/g, ' ').trim();
  return t.length > n ? t.slice(0, n - 1) + '…' : t;
};

export function indexTree(nodes) {
  const byId = new Map(nodes.map((n) => [n.nodeId, n]));
  const root = nodes.find((n) => !n.parentId || !byId.has(n.parentId)) || nodes[0];
  return { byId, root };
}

export function role(n) {
  return val(n.role).toLowerCase();
}

function states(n) {
  const out = [];
  for (const p of n.properties || []) {
    if (!STATES.includes(p.name)) continue;
    const v = val(p.value);
    if (v === 'true' || (p.name === 'checked' && v === 'mixed')) out.push(p.name === 'checked' && v === 'mixed' ? 'mixed' : p.name);
  }
  return out;
}

export function isInteractive(n) {
  return INTERACTIVE.has(role(n)) || (n.properties || []).some((p) => p.name === 'focusable' && val(p.value) === 'true' && role(n) !== 'rootwebarea');
}

// One line for a node: `button "Kirim" [ref_3]`, `textbox "Email" value="a@b.c" required [ref_4]`.
export function describe(n, ref) {
  const r = role(n) || 'node';
  const name = clean(val(n.name));
  const value = clean(val(n.value), 120);
  let line = r === 'statictext' ? `text "${name}"` : `${r}${name ? ` "${name}"` : ''}`;
  if (value && value !== name) line += ` value="${value}"`;
  const s = states(n);
  if (s.length) line += ` ${s.join(' ')}`;
  if (ref) line += ` [${ref}]`;
  return line;
}

// refFor(backendDOMNodeId) returns a stable ref string for an element.
// filter: 'interactive' (flat list of controls), 'all', or undefined (readable outline without wrappers).
export function renderTree(nodes, { filter, depth = 15, rootBackendId, refFor, maxChars = MAX_CHARS } = {}) {
  if (!nodes.length) return { text: '(empty page)', truncated: false };
  const { byId, root: treeRoot } = indexTree(nodes);
  let root = treeRoot;
  if (rootBackendId != null) {
    root = nodes.find((n) => n.backendDOMNodeId === rootBackendId);
    if (!root) return { text: null, truncated: false };
  }
  const lines = [];
  let chars = 0;
  let truncated = false;
  const push = (line) => {
    if (truncated) return;
    if (chars + line.length + 1 > maxChars) {
      truncated = true;
      return;
    }
    lines.push(line);
    chars += line.length + 1;
  };
  const walk = (n, level) => {
    if (truncated || !n) return;
    const r = role(n);
    const kids = (n.childIds || []).map((id) => byId.get(id)).filter(Boolean);
    let shown = false;
    if (!n.ignored) {
      const interactive = isInteractive(n);
      const named = Boolean(clean(val(n.name)));
      if (filter === 'interactive') shown = interactive;
      else if (filter === 'all') shown = r !== 'inlinetextbox';
      else shown = interactive || (!TRANSPARENT.has(r) && (named || r === 'heading' || r === 'list' || r === 'table' || r === 'form' || r === 'dialog'));
      if (shown) {
        const ref = n.backendDOMNodeId != null && (interactive || r !== 'statictext') ? refFor(n.backendDOMNodeId) : null;
        push(`${filter === 'interactive' ? '' : '  '.repeat(level)}- ${describe(n, ref)}`);
      }
    }
    const next = shown && filter !== 'interactive' ? level + 1 : level;
    if (next >= depth && shown) return;
    // Text inside a link or button is already its name.
    if (shown && (r === 'link' || r === 'button') && filter !== 'all') return;
    for (const k of kids) walk(k, next);
  };
  walk(root, 0);
  let text = lines.join('\n') || '(no matching elements)';
  if (truncated) text += `\n[Output truncated at ${maxChars} characters. Use find, or read_page with a ref or filter "interactive", to see the rest.]`;
  return { text, truncated };
}

// Keyword search over element names, roles and values. Returns up to `limit` lines with refs.
export function findNodes(nodes, query, { refFor, limit = 20 } = {}) {
  const words = String(query || '').toLowerCase().split(/[^\p{L}\p{N}]+/u).filter((w) => w.length > 1);
  if (!words.length) return '(empty query)';
  const scored = [];
  for (const n of nodes) {
    if (n.ignored || n.backendDOMNodeId == null) continue;
    const r = role(n);
    if (r === 'inlinetextbox' || TRANSPARENT.has(r)) continue;
    const hay = `${val(n.name)} ${val(n.value)} ${r} ${(n.properties || []).map((p) => (p.name === 'placeholder' ? val(p.value) : '')).join(' ')}`.toLowerCase();
    let score = 0;
    for (const w of words) if (hay.includes(w)) score += w.length > 3 ? 2 : 1;
    if (!score) continue;
    if (isInteractive(n)) score += 1.5;
    scored.push({ n, score });
  }
  scored.sort((a, b) => b.score - a.score);
  const top = scored.slice(0, limit);
  if (!top.length) return `No elements match "${query}". Try read_page or different words.`;
  return top.map(({ n }) => `- ${describe(n, refFor(n.backendDOMNodeId))}`).join('\n');
}

// Stable refs per tab: the same element keeps its ref until the page navigates.
export class RefMap {
  constructor() {
    this.reset();
  }
  reset() {
    this.byBackend = new Map();
    this.byRef = new Map();
    this.next = 1;
  }
  refFor(backendId) {
    let ref = this.byBackend.get(backendId);
    if (!ref) {
      ref = `ref_${this.next++}`;
      this.byBackend.set(backendId, ref);
      this.byRef.set(ref, backendId);
    }
    return ref;
  }
  backendFor(ref) {
    return this.byRef.get(ref);
  }
}
