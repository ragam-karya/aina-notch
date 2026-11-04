// Hidden window: microphone capture, speech synthesis and chimes.
let stream = null;
let releaseTimer = null;
let session = null; // the current hold-to-talk recording
let audioCtx = null;

// Opening the mic takes 100 ms to over a second (Bluetooth), which used to eat the start of short
// questions. Keep it open briefly after a recording so follow-ups start instantly, then release it
// so Windows' "microphone in use" indicator goes away.
const MIC_WARM_MS = 20000;

function ctx() {
  if (!audioCtx) audioCtx = new AudioContext();
  return audioCtx;
}

function releaseMicLater() {
  clearTimeout(releaseTimer);
  releaseTimer = setTimeout(() => {
    stream?.getTracks().forEach((t) => t.stop());
    stream = null;
  }, MIC_WARM_MS);
}

function finish(s, buffer, durationMs) {
  clearInterval(s.levelTimer);
  s.source?.disconnect();
  releaseMicLater();
  badi.send('record:data', { buffer, mimeType: 'audio/webm', durationMs });
}

// A "Hey Badi" turn has no key to release, so it ends by itself: after the user speaks and then pauses,
// when nothing is said at all, or at a hard limit. The noise floor adapts so a fan or AC isn't speech.
const AUTO = { startLevel: 0.12, stopLevel: 0.07, startFrames: 3, pauseMs: 1100, noSpeechMs: 6000, maxMs: 20000 };
function autoStop(s, level, now) {
  const a = s.auto;
  if (!a || a.sent) return;
  const elapsed = now - s.startedAt;
  if (!a.speaking) {
    a.floor = a.floor * 0.95 + level * 0.05;
    a.loud = level > Math.max(AUTO.startLevel, a.floor * 2.5) ? a.loud + 1 : 0;
    if (a.loud >= AUTO.startFrames) {
      a.speaking = true;
      a.quietSince = 0;
    } else if (elapsed > AUTO.noSpeechMs) {
      a.sent = true;
      return badi.send('record:autostop', { speech: false });
    }
    return;
  }
  if (level < Math.max(AUTO.stopLevel, a.floor * 1.6)) a.quietSince ||= now;
  else a.quietSince = 0;
  if ((a.quietSince && now - a.quietSince > AUTO.pauseMs) || elapsed > AUTO.maxMs) {
    a.sent = true;
    badi.send('record:autostop', { speech: true });
  }
}

badi.on('record:start', async (opts = {}) => {
  const s = (session = { stopRequested: false, recorder: null, chunks: [], startedAt: 0 });
  if (opts && opts.auto) s.auto = { speaking: false, loud: 0, floor: 0.05, quietSince: 0, sent: false };
  clearTimeout(releaseTimer);
  try {
    if (!stream || !stream.active) stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true } });
  } catch (e) {
    session = null;
    badi.send('record:error', e.message);
    return;
  }
  // The key was released while the mic was still opening: nothing was recorded.
  if (s.stopRequested) return finish(s, new Uint8Array(0), 0);

  const mimeType = MediaRecorder.isTypeSupported('audio/webm;codecs=opus') ? 'audio/webm;codecs=opus' : 'audio/webm';
  s.recorder = new MediaRecorder(stream, { mimeType });
  s.recorder.ondataavailable = (e) => e.data.size && s.chunks.push(e.data);
  s.recorder.start(250);
  s.startedAt = performance.now();

  const analyser = ctx().createAnalyser();
  analyser.fftSize = 512;
  s.source = ctx().createMediaStreamSource(stream);
  s.source.connect(analyser);
  const data = new Uint8Array(analyser.fftSize);
  s.levelTimer = setInterval(() => {
    analyser.getByteTimeDomainData(data);
    let sum = 0;
    for (const v of data) sum += ((v - 128) / 128) ** 2;
    const level = Math.min(1, Math.sqrt(sum / data.length) * 4);
    badi.send('record:level', level);
    autoStop(s, level, performance.now());
  }, 50);
});

badi.on('record:stop', () => {
  const s = session;
  session = null;
  if (!s) return badi.send('record:data', { buffer: new Uint8Array(0), mimeType: 'audio/webm', durationMs: 0 });
  if (!s.recorder) {
    s.stopRequested = true; // record:start will answer once the mic finishes opening
    return;
  }
  s.recorder.onstop = async () => {
    const blob = new Blob(s.chunks, { type: s.recorder.mimeType });
    finish(s, new Uint8Array(await blob.arrayBuffer()), performance.now() - s.startedAt);
  };
  s.recorder.stop();
});

// ---- speech ----
// Picks the chosen voice, else the best voice for the language (natural voices first).
function pickVoice(name, lang) {
  const voices = speechSynthesis.getVoices();
  const named = voices.find((v) => v.name === name);
  if (named) return named;
  const forLang = voices.filter((v) => !lang || v.lang.toLowerCase().startsWith(lang));
  return forLang.find((v) => /natural|online/i.test(v.name)) || forLang[0] || null;
}

badi.on('tts:speak', ({ id, payload: { text, voice, lang } }) => {
  const u = new SpeechSynthesisUtterance(text);
  u.voice = pickVoice(voice, lang);
  if (lang) u.lang = lang === 'id' ? 'id-ID' : 'en-US';
  u.rate = 1.05;
  const done = () => badi.send('worker:done', id);
  u.onstart = () => badi.send('tts:started', { id });
  // Each word as it is spoken, so the typed text never lags behind the voice.
  u.onboundary = (e) => badi.send('tts:boundary', e.charIndex + (e.charLength || 0));
  u.onend = done;
  u.onerror = done;
  speechSynthesis.speak(u);
});

let current = null;
badi.on('tts:play', ({ id, payload }) => {
  const url = URL.createObjectURL(new Blob([payload], { type: 'audio/mpeg' }));
  current = new Audio(url);
  const done = () => {
    URL.revokeObjectURL(url);
    badi.send('worker:done', id);
  };
  current.onended = done;
  current.onerror = done;
  // Report the real clip length the moment it starts, so typing finishes with the voice.
  current.onplaying = () => {
    const d = current.duration;
    badi.send('tts:started', { id, durationMs: Number.isFinite(d) ? d * 1000 : 0 });
  };
  current.play().catch(done);
});

badi.on('tts:stop', () => {
  speechSynthesis.cancel();
  if (current) {
    current.pause();
    current.dispatchEvent(new Event('ended'));
    current = null;
  }
});

// ---- chimes (synthesized, no bundled audio) ----
const CHIMES = {
  start: [[660, 0.06], [880, 0.08]],
  step: [[784, 0.05], [1046, 0.07]],
  sent: [[988, 0.045], [740, 0.06]], // the question went out: aina is on it
  'agent-start': [[523, 0.07], [659, 0.07], [784, 0.09]],
  'agent-done': [[784, 0.07], [988, 0.07], [1318, 0.12]],
  // Pomodoro (sent by v2): a focus session ends falling and soft, a break ends rising.
  'focus-done': [[1046, 0.08], [784, 0.08], [659, 0.14]],
  'break-done': [[523, 0.06], [784, 0.06], [1046, 0.06], [1318, 0.1]],
};
badi.on('chime', (name) => {
  const notes = CHIMES[name];
  if (!notes) return;
  let t = ctx().currentTime;
  for (const [freq, dur] of notes) {
    const osc = ctx().createOscillator();
    const gain = ctx().createGain();
    osc.type = 'sine';
    osc.frequency.value = freq;
    gain.gain.setValueAtTime(0.0001, t);
    gain.gain.exponentialRampToValueAtTime(0.12, t + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    osc.connect(gain).connect(ctx().destination);
    osc.start(t);
    osc.stop(t + dur + 0.02);
    t += dur * 0.8;
  }
});

// Tell main which Windows voices exist so it can fall back to cloud TTS for languages without one.
function reportVoices() {
  const voices = speechSynthesis.getVoices();
  if (voices.length) badi.send('worker:voices', voices.map((v) => v.lang.toLowerCase()));
}
speechSynthesis.onvoiceschanged = reportVoices;
reportVoices();

// ---- realtime conversation (OpenAI Realtime over WebRTC) ----
// The main process mints a short-lived client secret; this window only ever sees that, never the API key.
// The mic is a clone of the shared mic track, so muting it here never silences a local recording. The
// model's voice plays through an <audio> element in this window, which lets Chromium's echo
// cancellation keep Badi from hearing itself.
const rt = { pc: null, dc: null, track: null, audio: null, levelTimer: null };

function rtState(state, detail) {
  badi.send('rt:state', { state, detail });
}

function rtClose() {
  clearInterval(rt.levelTimer);
  try {
    rt.dc?.close();
    rt.pc?.close();
  } catch {}
  rt.track?.stop();
  if (rt.audio) rt.audio.srcObject = null;
  Object.assign(rt, { pc: null, dc: null, track: null, levelTimer: null });
  releaseMicLater();
}

badi.on('rt:connect', async ({ secret }) => {
  rtClose();
  try {
    clearTimeout(releaseTimer);
    if (!stream || !stream.active) stream = await navigator.mediaDevices.getUserMedia({ audio: { echoCancellation: true, noiseSuppression: true } });
    const pc = new RTCPeerConnection();
    rt.pc = pc;
    rt.audio ||= Object.assign(document.createElement('audio'), { autoplay: true });
    pc.ontrack = (e) => (rt.audio.srcObject = e.streams[0]);
    rt.track = stream.getAudioTracks()[0].clone();
    rt.track.enabled = false; // the main process opens it per turn
    pc.addTrack(rt.track, new MediaStream([rt.track]));
    const dc = pc.createDataChannel('oai-events');
    rt.dc = dc;
    dc.onmessage = (e) => badi.send('rt:event', e.data);
    dc.onopen = () => rtState('open');
    dc.onclose = () => rt.dc === dc && rtState('closed');
    pc.onconnectionstatechange = () => {
      if (['failed', 'disconnected'].includes(pc.connectionState) && rt.pc === pc) rtState('closed', pc.connectionState);
    };
    const offer = await pc.createOffer();
    await pc.setLocalDescription(offer);
    const res = await fetch('https://api.openai.com/v1/realtime/calls', {
      method: 'POST',
      body: offer.sdp,
      headers: { Authorization: `Bearer ${secret}`, 'Content-Type': 'application/sdp' },
    });
    if (!res.ok) throw new Error(`Realtime ${res.status}: ${(await res.text()).slice(0, 200)}`);
    await pc.setRemoteDescription({ type: 'answer', sdp: await res.text() });
    // Mic level for the island wave while the mic is open.
    const analyser = ctx().createAnalyser();
    analyser.fftSize = 512;
    ctx().createMediaStreamSource(new MediaStream([rt.track])).connect(analyser);
    const data = new Uint8Array(analyser.fftSize);
    rt.levelTimer = setInterval(() => {
      if (!rt.track?.enabled) return;
      analyser.getByteTimeDomainData(data);
      let sum = 0;
      for (const v of data) sum += ((v - 128) / 128) ** 2;
      badi.send('record:level', Math.min(1, Math.sqrt(sum / data.length) * 4));
    }, 50);
  } catch (e) {
    rtClose();
    rtState('error', e.message);
  }
});

badi.on('rt:send', (event) => {
  if (rt.dc?.readyState === 'open') rt.dc.send(JSON.stringify(event));
});
badi.on('rt:mic', (on) => {
  if (rt.track) rt.track.enabled = Boolean(on);
});
badi.on('rt:close', () => {
  rtClose();
  rtState('closed');
});
