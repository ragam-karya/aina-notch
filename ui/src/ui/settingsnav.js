// The Settings side navigation, built from the page's own section headings (h2 with data-icon), so
// settings.js and the sections stay as they are. A click scrolls to its section; while scrolling,
// the section in view lights up in the nav. Sections settings.js hides (Badi Cloud when it is off)
// drop out of the nav, and a heading whose text changes is followed.
(function () {
  const nav = document.getElementById('sideNav');
  const main = document.querySelector('main');
  let items = [];

  const visible = (h) => h.offsetParent !== null;
  function build() {
    const heads = [...main.querySelectorAll('h2')].filter(visible);
    heads.forEach((h, i) => {
      h.id ||= `sec-${i}`;
      // The heading carries its icon too (again after settings.js rewrites a heading's text).
      if (window.badiIcon && !h.querySelector('.ico')) h.prepend(window.badiIcon(h.dataset.icon || 'spark', 16));
    });
    const key = heads.map((h) => h.id + h.textContent).join('|');
    if (key === build.key) return;
    build.key = key;
    items = heads.map((h) => {
      const a = document.createElement('a');
      a.href = `#${h.id}`;
      a.className = 'nav-item';
      if (window.badiIcon) a.append(window.badiIcon(h.dataset.icon || 'spark', 16));
      const t = document.createElement('span');
      t.textContent = h.textContent;
      a.append(t);
      a.onclick = (e) => {
        e.preventDefault();
        h.scrollIntoView({ behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth', block: 'start' });
        mark(h.id);
      };
      return { a, h };
    });
    nav.replaceChildren(...items.map((x) => x.a));
    spy();
  }

  function mark(id) {
    for (const { a, h } of items) {
      const on = h.id === id;
      if (on && !a.classList.contains('on')) a.scrollIntoView({ block: 'nearest', inline: 'nearest' });
      a.classList.toggle('on', on);
    }
  }
  // The section in view: the last heading above a line a little under the top of the window.
  function spy() {
    const line = 120;
    let cur = items[0];
    for (const it of items) if (it.h.getBoundingClientRect().top <= line) cur = it;
    if (innerHeight + scrollY >= document.documentElement.scrollHeight - 4) cur = items[items.length - 1];
    if (cur) mark(cur.h.id);
  }

  let queued = false;
  const later = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => {
      queued = false;
      build();
    });
  };
  new MutationObserver(later).observe(main, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ['hidden'] });
  addEventListener('scroll', spy, { passive: true });
  addEventListener('resize', later);
  build();

  // The cards' entrance plays the first time the window is actually on screen, then never again.
  const intro = () => {
    if (document.visibilityState !== 'visible') return;
    document.removeEventListener('visibilitychange', intro);
    document.body.classList.add('intro');
    setTimeout(() => document.body.classList.remove('intro'), 1200);
  };
  document.addEventListener('visibilitychange', intro);
  intro();
})();
