// Line icons for the notch and the Badi windows, in place of emoji and text glyphs (⏮ ▶ × 📝), which
// render differently per font and look heavier than the text around them. 24×24, stroked with
// currentColor, so they take the button's color. badiIcon(name) returns a fresh <svg>.
(function () {
  const NS = 'http://www.w3.org/2000/svg';
  // Each icon: a list of [tag, attributes]. "fill" marks shapes filled instead of stroked.
  const ICONS = {
    play: [['path', { d: 'M8 5.5v13a1 1 0 0 0 1.5.86l11-6.5a1 1 0 0 0 0-1.72l-11-6.5A1 1 0 0 0 8 5.5z', fill: 1 }]],
    pause: [['rect', { x: 6, y: 5, width: 4, height: 14, rx: 1.2, fill: 1 }], ['rect', { x: 14, y: 5, width: 4, height: 14, rx: 1.2, fill: 1 }]],
    prev: [['path', { d: 'M18 6.2v11.6a1 1 0 0 1-1.55.83L8 13a1.2 1.2 0 0 1 0-2l8.45-5.63A1 1 0 0 1 18 6.2z', fill: 1 }], ['rect', { x: 5, y: 5.5, width: 2.4, height: 13, rx: 1.2, fill: 1 }]],
    next: [['path', { d: 'M6 6.2v11.6a1 1 0 0 0 1.55.83L16 13a1.2 1.2 0 0 0 0-2L7.55 5.37A1 1 0 0 0 6 6.2z', fill: 1 }], ['rect', { x: 16.6, y: 5.5, width: 2.4, height: 13, rx: 1.2, fill: 1 }]],
    close: [['path', { d: 'M17 7 7 17M7 7l10 10' }]],
    open: [['path', { d: 'M8 16 16 8M9 8h7v7' }]],
    stop: [['rect', { x: 6.5, y: 6.5, width: 11, height: 11, rx: 2.5, fill: 1 }]],
    check: [['path', { d: 'm5 12.5 4.5 4.5L19 7.5' }]],
    note: [['path', { d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z' }], ['path', { d: 'M14 3v5h5M9 13h6M9 17h4' }]],
    book: [['path', { d: 'M4 19.5A2.5 2.5 0 0 1 6.5 17H20V3H6.5A2.5 2.5 0 0 0 4 5.5z' }], ['path', { d: 'M4 19.5A2.5 2.5 0 0 0 6.5 22H20v-5' }]],
    bell: [['path', { d: 'M18 8.5a6 6 0 0 0-12 0c0 6.5-2.5 8.5-2.5 8.5h17S18 15 18 8.5' }], ['path', { d: 'M13.7 20.5a2 2 0 0 1-3.4 0' }]],
    timer: [['circle', { cx: 12, cy: 13.5, r: 7.5 }], ['path', { d: 'M12 10v3.5l2.2 2.2M10 2.5h4' }]],
    folder: [['path', { d: 'M21 18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h4.5l2 2.5H19a2 2 0 0 1 2 2z' }]],
    music: [['path', { d: 'M9 18V5.5l11-2v12.5' }], ['circle', { cx: 6.5, cy: 18, r: 2.5 }], ['circle', { cx: 17.5, cy: 16, r: 2.5 }]],
    spark: [['path', { d: 'M12 3.5 13.9 9a1.5 1.5 0 0 0 1 1L20.5 12l-5.6 1.9a1.5 1.5 0 0 0-1 1L12 20.5 10.1 15a1.5 1.5 0 0 0-1-1L3.5 12l5.6-1.9a1.5 1.5 0 0 0 1-1z' }]],
    bolt: [['path', { d: 'M13 2.5 4.5 13.5H12l-1 8 8.5-11H12z' }]],
    home: [['path', { d: 'M4 10.5 12 4l8 6.5V19a1.5 1.5 0 0 1-1.5 1.5h-13A1.5 1.5 0 0 1 4 19z' }], ['path', { d: 'M9.5 20.5v-6h5v6' }]],
    plus: [['path', { d: 'M12 5v14M5 12h14' }]],
    send: [['path', { d: 'M5 12h13M13 6l6 6-6 6' }]],
    trash: [['path', { d: 'M4 7h16M10 11v6M14 11v6M6 7l1 12.5A1.5 1.5 0 0 0 8.5 21h7a1.5 1.5 0 0 0 1.5-1.5L18 7M9 7V4.5A1.5 1.5 0 0 1 10.5 3h3A1.5 1.5 0 0 1 15 4.5V7' }]],
    file: [['path', { d: 'M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z' }], ['path', { d: 'M14 3v5h5' }]],
    mute: [['path', { d: 'M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z' }], ['path', { d: 'm16 9.5 5 5M21 9.5l-5 5' }]],
    lyrics: [['path', { d: 'M4 6h11M4 10.5h11M4 15h6' }], ['circle', { cx: 16.5, cy: 17.5, r: 2.5 }], ['path', { d: 'M19 17.5V8.5l2 1' }]],
    speaker: [['rect', { x: 5.5, y: 2.5, width: 13, height: 19, rx: 2.5 }], ['circle', { cx: 12, cy: 14, r: 3.5 }], ['path', { d: 'M12 6.5h.01' }]],
    flag: [['path', { d: 'M5 21V4M5 4h11l-2 4 2 4H5' }]],
    cloud: [['path', { d: 'M7 18.5a4.5 4.5 0 0 1-.6-8.96A6 6 0 0 1 18 9.5h.5a4.5 4.5 0 0 1 0 9z' }]],
    globe: [['circle', { cx: 12, cy: 12, r: 9 }], ['path', { d: 'M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18' }]],
    key: [['circle', { cx: 8, cy: 15, r: 4 }], ['path', { d: 'm11 12 9-9M17 6l2.5 2.5M14.5 8.5 16.5 10.5' }]],
    cpu: [['rect', { x: 6, y: 6, width: 12, height: 12, rx: 2.5 }], ['path', { d: 'M10 10h4v4h-4zM9 2.5V6M15 2.5V6M9 18v3.5M15 18v3.5M2.5 9H6M2.5 15H6M18 9h3.5M18 15h3.5' }]],
    mic: [['rect', { x: 9, y: 2.5, width: 6, height: 11.5, rx: 3 }], ['path', { d: 'M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5v4' }]],
    volume: [['path', { d: 'M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z' }], ['path', { d: 'M15.5 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11' }]],
    pointer: [['path', { d: 'M5 3.5 19 10l-6 1.8L11 18z' }], ['path', { d: 'm13 11.8 5.5 6' }]],
    grid: [['rect', { x: 3.5, y: 3.5, width: 7, height: 7, rx: 2 }], ['rect', { x: 13.5, y: 3.5, width: 7, height: 7, rx: 2 }], ['rect', { x: 3.5, y: 13.5, width: 7, height: 7, rx: 2 }], ['rect', { x: 13.5, y: 13.5, width: 7, height: 7, rx: 2 }]],
    notch: [['rect', { x: 2.5, y: 4, width: 19, height: 16, rx: 3 }], ['path', { d: 'M8.5 4v1.5a1.5 1.5 0 0 0 1.5 1.5h4a1.5 1.5 0 0 0 1.5-1.5V4' }]],
    keyboard: [['rect', { x: 2.5, y: 6, width: 19, height: 12, rx: 2.5 }], ['path', { d: 'M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M8 14.5h8' }]],
    repeat: [['path', { d: 'M17 2.5 20.5 6 17 9.5' }], ['path', { d: 'M3.5 11.5V10A4 4 0 0 1 7.5 6h13M7 21.5 3.5 18 7 14.5' }], ['path', { d: 'M20.5 12.5V14a4 4 0 0 1-4 4h-13' }]],
  };

  // An icon from react-icons (data in reacticons.js, made by scripts/gen-icons.js): its SVG tree drawn
  // the way react-icons' GenIcon draws it (fill and stroke in currentColor, no stroke unless a shape
  // sets one). Null when the icon was not generated.
  const kebab = (k) => k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`);
  function build(node, isRoot) {
    const n = document.createElementNS(NS, node.tag);
    if (isRoot) for (const [k, v] of [['fill', 'currentColor'], ['stroke', 'currentColor'], ['stroke-width', '0']]) n.setAttribute(k, v);
    for (const [k, v] of Object.entries(node.attr || {})) n.setAttribute(k === 'viewBox' ? k : kebab(k), v);
    for (const c of node.child || []) n.append(build(c, false));
    return n;
  }
  window.badiReactIcon = function (name, size = 16) {
    const tree = window.BADI_REACT_ICONS?.[name];
    if (!tree) return null;
    const svg = build(tree, true);
    svg.setAttribute('width', size);
    svg.setAttribute('height', size);
    svg.setAttribute('aria-hidden', 'true');
    svg.setAttribute('class', `ico ri ri-${name}`);
    return svg;
  };

  window.badiIcon = function (name, size = 16) {
    const svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('width', size);
    svg.setAttribute('height', size);
    svg.setAttribute('aria-hidden', 'true');
    svg.setAttribute('class', `ico ico-${name}`);
    for (const [tag, attrs] of ICONS[name] || []) {
      const n = document.createElementNS(NS, tag);
      for (const [k, v] of Object.entries(attrs)) if (k !== 'fill') n.setAttribute(k, v);
      if (attrs.fill) n.setAttribute('fill', 'currentColor');
      else {
        n.setAttribute('fill', 'none');
        n.setAttribute('stroke', 'currentColor');
        n.setAttribute('stroke-width', '1.9');
        n.setAttribute('stroke-linecap', 'round');
        n.setAttribute('stroke-linejoin', 'round');
      }
      svg.append(n);
    }
    return svg;
  };
})();
