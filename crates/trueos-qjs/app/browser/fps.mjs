export function createFpsOverlay() {
  const st = {
    frames: 0,
    sampleStartMs: 0,
    text: '000:00',
    runs: [],
    x: 0,
    y: 8,
    stepPx: 12,
    rightPad: 8,
    rgba: 0x101010ff,
  };

  function appendRuns(textRuns, vw) {
    const nowMs = (typeof Date !== 'undefined' && typeof Date.now === 'function')
      ? Date.now()
      : 0;

    if (st.sampleStartMs <= 0) {
      st.sampleStartMs = nowMs;
    }

    const glyphCount = st.text.length;
    if (st.runs.length < (glyphCount * 4)) {
      for (let i = 0; i < st.text.length; i++) {
        st.runs.push(st.x + (i * st.stepPx), st.y, st.text[i], st.rgba);
      }
    }

    const totalW = glyphCount * st.stepPx;
    const nextX = Math.max(0, Math.round(Number(vw || 0) - totalW - st.rightPad));
    if (nextX !== st.x) {
      st.x = nextX;
      for (let i = 0; i < glyphCount; i++) {
        st.runs[(i * 4) + 0] = st.x + (i * st.stepPx);
      }
    }

    st.frames += 1;
    const elapsed = nowMs - Number(st.sampleStartMs || nowMs);
    if (elapsed >= 1000) {
      const fps = (st.frames * 1000) / Math.max(1, elapsed);
      let scaled = Math.round(fps * 100);
      if (!Number.isFinite(scaled) || scaled < 0) scaled = 0;
      if (scaled > 99999) scaled = 99999;
      const whole = Math.floor(scaled / 100);
      const frac = scaled % 100;
      const nextText = `${String(whole).padStart(3, '0')}:${String(frac).padStart(2, '0')}`;

      if (nextText !== st.text) {
        // Update only glyph slots that changed.
        for (let i = 0; i < nextText.length; i++) {
          if (st.text[i] === nextText[i]) continue;
          st.runs[(i * 4) + 2] = nextText[i];
        }
        st.text = nextText;
      }

      st.frames = 0;
      st.sampleStartMs = nowMs;
    }

    for (let i = 0; i < st.runs.length; i++) {
      textRuns.push(st.runs[i]);
    }
  }

  return { appendRuns };
}
