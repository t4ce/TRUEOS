export function createBrowserCursorController(options = {}) {
  const host = options.host;
  const computeViewport = typeof options.computeViewport === 'function' ? options.computeViewport : (() => ({ vw: 1280, vh: 800 }));
  const buildReleaseRect = typeof options.buildReleaseRect === 'function' ? options.buildReleaseRect : (() => null);
  const paint = typeof options.paint === 'function' ? options.paint : () => {};
  const onWheelDelta = typeof options.onWheelDelta === 'function' ? options.onWheelDelta : (() => false);
  const onReleaseRect = typeof options.onReleaseRect === 'function' ? options.onReleaseRect : () => {};
  const logDebug = typeof options.logDebug === 'function' ? options.logDebug : () => {};
  const defaultAiCursorId = typeof options.defaultAiCursorId === 'string' && options.defaultAiCursorId
    ? options.defaultAiCursorId
    : 'ai-default';
  const aiCursorSlotBase = Math.max(1, Number(options.aiCursorSlotBase || 10000) | 0);
  const wheelStepPx = Math.max(1, Number(options.wheelStepPx || 16) | 0);
  const cursorFlagButtonsChanged = Number(options.cursorFlagButtonsChanged || (1 << 2)) >>> 0;
  const cursorEventFields = Math.max(1, Number(options.cursorEventFields || 6) | 0);

  let cursorReadSeq = 0;
  let cursorMoveLogCount = 0;
  let nextAiCursorSlot = aiCursorSlotBase;
  const kernelCursorState = new Map();
  const cursorButtonEvents = [];
  const cursorDragOrigins = new Map();
  const aiCursorSlots = new Map();
  const pendingSyntheticCursorEchoes = [];

  function queueCursorButtonEvent(event) {
    cursorButtonEvents.push(event);
    if (cursorButtonEvents.length > 128) {
      cursorButtonEvents.splice(0, cursorButtonEvents.length - 128);
    }
  }

  function resolveAiCursorId(value) {
    if (typeof value === 'string') {
      const trimmed = value.trim();
      return trimmed || defaultAiCursorId;
    }
    if (typeof value === 'number' && Number.isFinite(value)) {
      return String(value);
    }
    return defaultAiCursorId;
  }

  function getOrCreateAiCursorSlot(cursorId) {
    const key = String(cursorId || '').trim();
    if (!key) return 0;
    const existing = aiCursorSlots.get(key);
    if (existing) return existing;
    const slotId = nextAiCursorSlot;
    nextAiCursorSlot += 1;
    aiCursorSlots.set(key, slotId);
    return slotId;
  }

  function rememberSyntheticCursorEcho(slotId, x, y, buttonsDown, wheel, flags) {
    pendingSyntheticCursorEchoes.push(`${Number(slotId) | 0}:${Number(x || 0)}:${Number(y || 0)}:${Number(buttonsDown || 0) >>> 0}:${Number(wheel || 0) | 0}:${Number(flags || 0) >>> 0}`);
    if (pendingSyntheticCursorEchoes.length > 64) {
      pendingSyntheticCursorEchoes.splice(0, pendingSyntheticCursorEchoes.length - 64);
    }
  }

  function consumeSyntheticCursorEcho(slotId, x, y, buttonsDown, wheel, flags) {
    const signature = `${Number(slotId) | 0}:${Number(x || 0)}:${Number(y || 0)}:${Number(buttonsDown || 0) >>> 0}:${Number(wheel || 0) | 0}:${Number(flags || 0) >>> 0}`;
    const index = pendingSyntheticCursorEchoes.indexOf(signature);
    if (index < 0) return false;
    pendingSyntheticCursorEchoes.splice(index, 1);
    return true;
  }

  function rememberKernelCursor(slotId, x, y, buttonsDown, flags) {
    if (!Number.isFinite(slotId) || slotId <= 0) return 0;
    const id = Number(slotId) | 0;
    const nextButtons = Number(buttonsDown || 0) >>> 0;
    const nextX = Number(x || 0);
    const nextY = Number(y || 0);
    const nextFlags = Number(flags || 0) >>> 0;
    const prev = kernelCursorState.get(id);
    const prevButtons = prev ? (Number(prev.buttonsDown || 0) >>> 0) : 0;

    kernelCursorState.set(id, {
      slotId: id,
      x: nextX,
      y: nextY,
      buttonsDown: nextButtons,
      flags: nextFlags,
    });

    if (((nextFlags & cursorFlagButtonsChanged) !== 0) || nextButtons !== prevButtons) {
      if (prevButtons === 0 && nextButtons !== 0) {
        cursorDragOrigins.set(id, { x: nextX, y: nextY });
      } else if (prevButtons !== 0 && nextButtons === 0) {
        const origin = cursorDragOrigins.get(id);
        if (origin) {
          const rect = buildReleaseRect(origin, nextX, nextY);
          if (rect) onReleaseRect(rect);
        }
        cursorDragOrigins.delete(id);
      }
      logDebug(`[browser.mjs] cursor buttons slot=${id} prev=${prevButtons} next=${nextButtons}`);
      queueCursorButtonEvent({
        slotId: id,
        x: nextX,
        y: nextY,
        buttonsDown: nextButtons,
        previousButtonsDown: prevButtons,
        flags: nextFlags,
      });
      return 1;
    }
    return 0;
  }

  function processCursorEvent(slotId, x, y, buttonsDown, wheel, flags, source = 'kernel') {
    const prev = kernelCursorState.get(Number(slotId || 0) | 0) || null;
    let updated = rememberKernelCursor(slotId, x, y, buttonsDown, flags);

    if (!prev || Number(prev.x || 0) !== Number(x || 0) || Number(prev.y || 0) !== Number(y || 0)) {
      cursorMoveLogCount += 1;
      if ((cursorMoveLogCount % 25) === 0) {
        logDebug(`[browser.mjs] cursor move chunk=25 source=${source} slot=${slotId} x=${Number(x || 0)} y=${Number(y || 0)}`);
      }
      updated += 1;
    }

    if (wheel !== 0) {
      logDebug(`[browser.mjs] cursor wheel source=${source} slot=${slotId} wheel=${wheel}`);
      const dy = Number(wheel) * -wheelStepPx;
      if (onWheelDelta(dy)) updated += 1;
    }
    return updated;
  }

  function injectCursorEvent(event = null) {
    const source = event && typeof event === 'object' ? event : {};
    const slotId = Number(source.slotId || 0) | 0;
    const aiCursorId = resolveAiCursorId(source.aiCursorId);
    const resolvedSlotId = slotId > 0 ? slotId : getOrCreateAiCursorSlot(aiCursorId);
    if (resolvedSlotId <= 0) return false;

    const x = Number(source.x || 0);
    const y = Number(source.y || 0);
    const buttonsDown = Number(source.buttonsDown || 0) >>> 0;
    const wheel = Number(source.wheel || 0) | 0;
    const flags = Number(source.flags || 0) >>> 0;

    const writeCursorEvent = host.__trueosWriteCursorEvent;
    if (typeof writeCursorEvent === 'function') {
      try {
        if (writeCursorEvent(resolvedSlotId, x, y, buttonsDown, wheel, flags)) {
          rememberSyntheticCursorEcho(resolvedSlotId, x, y, buttonsDown, wheel, flags);
          const updated = processCursorEvent(resolvedSlotId, x, y, buttonsDown, wheel, flags, 'synthetic');
          if (updated > 0 && wheel === 0) {
            paint();
          }
          return true;
        }
      } catch (_) {}
    }

    processCursorEvent(resolvedSlotId, x, y, buttonsDown, wheel, flags, 'synthetic');
    return true;
  }

  function moveCursor(target = null) {
    const source = target && typeof target === 'object' ? target : {};
    const slotId = Number(source.slotId || 0) | 0;
    const aiCursorId = resolveAiCursorId(source.aiCursorId);
    const resolvedSlotId = slotId > 0 ? slotId : getOrCreateAiCursorSlot(aiCursorId);
    if (resolvedSlotId <= 0) return false;

    const state = kernelCursorState.get(resolvedSlotId);
    const x = Number(source.x);
    const y = Number(source.y);
    if (!Number.isFinite(x) || !Number.isFinite(y)) return false;

    const buttonsDown = Object.prototype.hasOwnProperty.call(source, 'buttonsDown')
      ? (Number(source.buttonsDown || 0) >>> 0)
      : (state ? (Number(state.buttonsDown || 0) >>> 0) : 0);
    const flags = Object.prototype.hasOwnProperty.call(source, 'flags')
      ? (Number(source.flags || 0) >>> 0)
      : 1;

    return injectCursorEvent({
      slotId: resolvedSlotId,
      x,
      y,
      buttonsDown,
      wheel: 0,
      flags,
    });
  }

  function synthesizeClickAt(target = null, resolveTarget = null) {
    const source = target && typeof target === 'object' && !Array.isArray(target) ? target : {};
    const slotId = Number(source.slotId || 0) | 0;
    const aiCursorId = resolveAiCursorId(source.aiCursorId);
    const resolvedSlotId = slotId > 0 ? slotId : getOrCreateAiCursorSlot(aiCursorId);
    if (resolvedSlotId <= 0) {
      return { ok: 0, handled: 0, reason: 'invalid-slot' };
    }

    const resolved = typeof resolveTarget === 'function' ? resolveTarget(target) : null;
    const state = kernelCursorState.get(resolvedSlotId) || null;
    const x = resolved && Number.isFinite(Number(resolved.x))
      ? Number(resolved.x)
      : Number(state && state.x);
    const y = resolved && Number.isFinite(Number(resolved.y))
      ? Number(resolved.y)
      : Number(state && state.y);
    if (!Number.isFinite(x) || !Number.isFinite(y)) {
      return { ok: 0, handled: 0, reason: 'target-not-found' };
    }

    const buttonMask = Math.max(1, Number(source.buttonMask || source.button || 1) | 0) >>> 0;
    moveCursor({ slotId: resolvedSlotId, aiCursorId, x, y });
    const downOk = injectCursorEvent({
      slotId: resolvedSlotId,
      x,
      y,
      buttonsDown: buttonMask,
      wheel: 0,
      flags: 1 | cursorFlagButtonsChanged,
    });
    const upOk = injectCursorEvent({
      slotId: resolvedSlotId,
      x,
      y,
      buttonsDown: 0,
      wheel: 0,
      flags: 1 | cursorFlagButtonsChanged,
    });
    if (!downOk || !upOk) {
      return { ok: 0, handled: 0, reason: 'cursor-injection-failed' };
    }

    const interactive = resolved && resolved.interactive && typeof resolved.interactive === 'object'
      ? resolved.interactive
      : null;
    return {
      ok: 1,
      handled: 1,
      simulated: 0,
      slotId: resolvedSlotId,
      x,
      y,
      path: resolved && typeof resolved.path === 'string' ? resolved.path : '',
      href: interactive ? String(interactive.href || '') : '',
      caption: interactive ? String(interactive.caption || '') : '',
      kind: interactive ? String(interactive.kind || '') : '',
    };
  }

  function pumpCursorEvents() {
    const fn = host.__trueosReadCursorEventsSince;
    if (typeof fn !== 'function') return 0;

    let updated = 0;
    for (let batch = 0; batch < 8; batch++) {
      let packed = null;
      try {
        packed = fn(Number(cursorReadSeq || 0));
      } catch (_) {
        break;
      }
      if (!Array.isArray(packed) || packed.length < 3) break;

      const nextSeq = Number(packed[0] || cursorReadSeq || 0);
      const wrote = Math.max(0, Number(packed[2] || 0) | 0);
      let p = 3;
      for (let i = 0; i < wrote; i++) {
        if (p + 5 >= packed.length) break;
        const slotId = Number(packed[p + 0] || 0) | 0;
        const x = Number(packed[p + 1] || 0);
        const y = Number(packed[p + 2] || 0);
        const buttonsDown = Number(packed[p + 3] || 0) >>> 0;
        const wheel = Number(packed[p + 4] || 0) | 0;
        const flags = Number(packed[p + 5] || 0) >>> 0;
        p += cursorEventFields;

        if (consumeSyntheticCursorEcho(slotId, x, y, buttonsDown, wheel, flags)) {
          continue;
        }

        const prevState = kernelCursorState.get(slotId) || null;
        const prevButtonsDown = prevState ? (Number(prevState.buttonsDown || 0) >>> 0) : 0;

        updated += processCursorEvent(slotId, x, y, buttonsDown, wheel, flags, 'kernel');
        if (prevButtonsDown !== 0 && buttonsDown === 0) {
          paint();
        }
      }

      cursorReadSeq = nextSeq;
      if (wrote < 32) break;
    }
    return updated;
  }

  function startPump() {
    if (typeof host.setInterval === 'function') {
      try {
        host.setInterval(pumpCursorEvents, 16);
        return;
      } catch (_) {}
    }
    if (typeof host.setTimeout === 'function') {
      const step = () => {
        pumpCursorEvents();
        try { host.setTimeout(step, 16); } catch (_) {}
      };
      try { host.setTimeout(step, 16); } catch (_) {}
    }
  }

  function placeDefaultAiCursor() {
    const { vw, vh } = computeViewport();
    const x = Math.max(0, Math.floor(vw * 0.25));
    const y = Math.max(0, Math.floor(vh * 0.25));
    moveCursor({
      aiCursorId: defaultAiCursorId,
      x,
      y,
    });
  }

  function getKernelCursors() {
    return Array.from(kernelCursorState.values()).sort((a, b) => a.slotId - b.slotId);
  }

  function getKernelCursor(slotId) {
    const id = Number(slotId || 0) | 0;
    if (id <= 0) return null;
    const state = kernelCursorState.get(id);
    return state ? { ...state } : null;
  }

  function popCursorButtonEvent() {
    if (cursorButtonEvents.length <= 0) return null;
    return cursorButtonEvents.shift() || null;
  }

  return {
    injectCursorEvent,
    moveCursor,
    synthesizeClickAt,
    startPump,
    placeDefaultAiCursor,
    getKernelCursors,
    getKernelCursor,
    popCursorButtonEvent,
  };
}
