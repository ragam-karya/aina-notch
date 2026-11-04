// Badi's Rive character (assets/badi.riv) for any page: badiRive(canvas, { artboards }) plays the
// "BADI" state machine and returns a small controller. Inputs (the file's contract):
//   state          0 idle · 1 listening · 2 thinking · 3 speaking · 4 success · 5 error
//   lookX          -1…1, where it looks (full body and BADI_Eyes)
//   glowIntensity  0–100, how bright the eyes glow (full body and BADI_Head)
//   wave           trigger, waves a hand (full body only; clicking the head waves too)
// Inputs an artboard doesn't have are ignored, so callers set everything they like. `artboards` is a
// preference list: the first one the file has is used, so a newer file can bring BADI_Eyes and the
// notch picks it up without a code change. Everything is loaded from the app itself (no CDN).
(function () {
  const url = (p) => new URL(p, location.href).href;
  let wasmSet = false;

  window.badiRive = function badiRive(canvas, { artboards = ['BADI'], state = 0 } = {}) {
    const ctl = { ready: false, failed: false, onReady: null, onFail: null, want: { state }, inputs: {}, r: null, paused: false };
    if (!window.rive || !canvas) {
      ctl.failed = true;
      return attach(ctl);
    }
    if (!wasmSet) {
      rive.RuntimeLoader.setWasmUrl(url('../../node_modules/@rive-app/canvas/rive.wasm'));
      wasmSet = true;
    }
    const start = (artboard) => {
      const r = new rive.Rive({
        src: url('../../assets/badi.riv'),
        canvas,
        artboard,
        stateMachines: 'BADI',
        autoplay: !ctl.paused,
        layout: new rive.Layout({ fit: rive.Fit.Contain, alignment: rive.Alignment.Center }),
        onLoad: () => {
          const have = r.contents.artboards.map((a) => a.name);
          const best = artboards.find((a) => have.includes(a));
          if (best && best !== artboard) {
            r.cleanup();
            return start(best);
          }
          r.resizeDrawingSurfaceToCanvas();
          ctl.inputs = Object.fromEntries((r.stateMachineInputs('BADI') || []).map((i) => [i.name, i]));
          ctl.ready = true;
          apply(ctl);
          ctl.onReady?.();
        },
        onLoadError: () => {
          ctl.failed = true;
          ctl.onFail?.();
        },
      });
      ctl.r = r;
    };
    // The last name is the one every version of the file has.
    start(artboards[artboards.length - 1]);
    return attach(ctl);
  };

  // The mascot (aina-v1.0.0/badi-mascot, assets/badi-mascot.riv): badiMascot(canvas, { artboard }) plays the
  // BADIController state machine, driven through the bound view model BADI rather than inputs:
  //   state        idle listening thinking speaking success error alert guiding sleeping annoyed dizzy
  //   mode         compact | expanded
  //   interaction  none | hover | pressed (set by the page: the overlay is click-through, so Rive's own
  //                pointer listeners are off and its hit-testing with them)
  //   isListening isThinking isSpeaking isAnnoyed isDizzy (flags over state), audioLevel 0–1
  // One instance lives as long as its page: expressions and sizes only write view model values, and
  // the canvas may move between places (resize() after a move). destroy() releases it.
  window.badiMascot = function badiMascot(canvas, { artboard = 'Compact', src = '../../assets/badi-mascot.riv', autoplay = false } = {}) {
    const ctl = { ready: false, failed: false, onReady: null, onFail: null, want: {}, props: {}, r: null, paused: !autoplay, rendering: true };
    ctl.set = (name, value) => {
      if (ctl.want[name] === value) return false;
      ctl.want[name] = value;
      if (ctl.ready) write(ctl, name, value);
      return true;
    };
    // An expression is the base state with every flag cleared (one source of truth); the flags stay
    // for what lies over it (annoyed, dizzy) and isSpeaking with audioLevel.
    ctl.expression = (name) => {
      let changed = ctl.set('state', name);
      for (const f of ['isListening', 'isThinking', 'isSpeaking']) changed = ctl.set(f, false) || changed;
      return changed;
    };
    ctl.pause = () => {
      if (ctl.paused) return;
      ctl.paused = true;
      ctl.r?.pause();
    };
    ctl.play = () => {
      if (!ctl.paused) return;
      ctl.paused = false;
      ctl.r?.play();
    };
    // Hidden: no frames at all, not even a paused one (Rive keeps where its animations are).
    ctl.hide = () => {
      if (!ctl.rendering) return;
      ctl.rendering = false;
      ctl.r?.stopRendering();
    };
    ctl.show = () => {
      if (ctl.rendering) return;
      ctl.rendering = true;
      ctl.r?.startRendering();
    };
    ctl.resize = () => {
      if (!ctl.ready) return;
      ctl.r.resizeDrawingSurfaceToCanvas();
      if (ctl.paused) ctl.r.drawFrame(); // a still mascot keeps a sharp picture at its new size
    };
    ctl.destroy = () => {
      ctl.ready = false;
      ctl.props = {};
      ctl.r?.cleanup();
      ctl.r = null;
    };
    if (!window.rive || !canvas) {
      ctl.failed = true;
      return ctl;
    }
    if (!wasmSet) {
      rive.RuntimeLoader.setWasmUrl(url('../../node_modules/@rive-app/canvas/rive.wasm'));
      wasmSet = true;
    }
    const r = new rive.Rive({
      src: url(src),
      canvas,
      artboard,
      stateMachines: 'BADIController',
      autoplay,
      autoBind: true,
      enableRiveAssetCDN: false,
      shouldDisableRiveListeners: true,
      layout: new rive.Layout({ fit: rive.Fit.Contain, alignment: rive.Alignment.Center }),
      onLoad: () => {
        const vm = r.viewModelInstance;
        if (!vm) {
          ctl.failed = true;
          ctl.onFail?.();
          return;
        }
        // Each property is looked up once; enum values are matched without case ("thinking" or
        // "Thinking", whichever the file names them).
        for (const name of ['state', 'mode', 'interaction']) {
          const p = vm.enum(name);
          if (p) ctl.props[name] = { p, values: new Map(p.values.map((v) => [v.toLowerCase(), v])) };
        }
        for (const name of ['isListening', 'isThinking', 'isSpeaking', 'isAnnoyed', 'isDizzy']) {
          const p = vm.boolean(name);
          if (p) ctl.props[name] = { p };
        }
        const level = vm.number('audioLevel');
        if (level) ctl.props.audioLevel = { p: level };
        r.resizeDrawingSurfaceToCanvas();
        ctl.ready = true;
        for (const [name, value] of Object.entries(ctl.want)) write(ctl, name, value);
        if (ctl.paused) r.pause();
        if (!ctl.rendering) r.stopRendering();
        ctl.onReady?.();
      },
      onLoadError: () => {
        ctl.failed = true;
        ctl.onFail?.();
      },
    });
    ctl.r = r;
    return ctl;
  };

  function write(ctl, name, value) {
    const prop = ctl.props[name];
    if (!prop) return;
    if (prop.values) {
      const v = prop.values.get(String(value).toLowerCase());
      if (v !== undefined && prop.p.value !== v) prop.p.value = v;
    } else if (prop.p.value !== value) prop.p.value = value;
  }

  function apply(ctl) {
    for (const [name, value] of Object.entries(ctl.want)) {
      const input = ctl.inputs[name];
      if (input && input.value !== value) input.value = value;
    }
  }

  function attach(ctl) {
    ctl.set = (name, value) => {
      if (ctl.want[name] === value) return;
      ctl.want[name] = value;
      if (ctl.ready) apply(ctl);
    };
    ctl.fire = (name) => ctl.ready && ctl.inputs[name]?.fire?.();
    // For a canvas that was hidden when it loaded: size its drawing surface now that it shows.
    ctl.resize = () => ctl.ready && ctl.r.resizeDrawingSurfaceToCanvas();
    ctl.pause = () => {
      if (ctl.paused) return;
      ctl.paused = true;
      ctl.r?.pause();
    };
    ctl.play = () => {
      if (!ctl.paused) return;
      ctl.paused = false;
      ctl.r?.play();
    };
    return ctl;
  }
})();
