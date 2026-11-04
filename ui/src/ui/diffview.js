// A Sprite's code diff, the way Coucou shows one: the file with +N −M, then each hunk with its line
// numbers, added lines green and removed lines red, 3 lines of context around them. Used by the Sprite
// card's detail (overlay) and the Sprites window. The diff comes from sprites:diff (badi-agent diff.rs):
// { name, path, added, removed, newFile, tooLarge, hunks: [{ oldStart, newStart, lines: [[kind, oldNo, newNo, text]] }] }.
window.badiDiff = (function () {
  const el = (tag, cls, text) => {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  };

  // "+3 −1" in green and red (a side with nothing is left out).
  function counts(added, removed) {
    const c = el('span', 'dv-counts');
    if (added) c.append(el('span', 'dv-plus', `+${added}`));
    if (removed) c.append(el('span', 'dv-minus', `−${removed}`));
    return c;
  }

  // The header: back, the file name and counts, and open in editor.
  function head(d, L, { onBack, onOpen }) {
    const h = el('div', 'dv-head');
    if (onBack) {
      const b = el('button', 'dv-back', '‹');
      b.title = L.spriteBack || 'Kembali';
      b.dataset.tip = b.title;
      b.addEventListener('click', (e) => (e.stopPropagation(), onBack()));
      h.append(b);
    }
    const name = el('span', 'dv-name', d.name);
    name.title = d.path;
    h.append(name);
    if (d.newFile) h.append(el('span', 'dv-tag', L.spriteNewFile || 'file baru'));
    h.append(counts(d.added, d.removed));
    if (onOpen) {
      const o = el('button', 'dv-open', '↗');
      o.title = L.spriteOpenEditor || 'Buka di editor';
      o.dataset.tip = o.title;
      o.addEventListener('click', (e) => (e.stopPropagation(), onOpen(d)));
      h.append(o);
    }
    return h;
  }

  // The lines.
  function body(d, L) {
    const box = el('div', 'dv-body');
    if (!d) {
      box.append(el('div', 'dv-note', L.spriteDiffGone || 'Diff-nya sudah tidak tersimpan.'));
      return box;
    }
    if (d.tooLarge || !d.hunks.length) {
      box.append(el('div', 'dv-note', d.tooLarge ? L.spriteDiffTooLarge || 'Diff terlalu besar untuk ditampilkan' : '—'));
      return box;
    }
    for (const h of d.hunks) {
      box.append(el('div', 'dv-hunk', `@@ −${h.oldStart} +${h.newStart} @@`));
      for (const [kind, oldNo, newNo, text] of h.lines) {
        const row = el('div', kind === '+' ? 'dv-row dv-add' : kind === '-' ? 'dv-row dv-del' : 'dv-row');
        row.append(el('span', 'dv-no', oldNo || ''), el('span', 'dv-no', newNo || ''), el('span', 'dv-sign', kind === '-' ? '−' : kind), el('span', 'dv-code', text));
        box.append(row);
      }
    }
    return box;
  }

  // The first added line, to open the editor there.
  function firstLine(d) {
    for (const h of (d && d.hunks) || []) for (const l of h.lines) if (l[0] === '+') return l[2];
    return 1;
  }

  return { counts, head, body, firstLine };
})();
