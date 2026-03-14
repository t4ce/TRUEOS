export function createBrowserUiHookStream(host, options = {}) {
  const stateKey = typeof options.stateKey === 'string' && options.stateKey
    ? options.stateKey
    : '__trueosBrowserUiHookState';
  const lastEventKey = typeof options.lastEventKey === 'string' && options.lastEventKey
    ? options.lastEventKey
    : '__trueosBrowserUiHookLastEvent';
  const queueKey = typeof options.queueKey === 'string' && options.queueKey
    ? options.queueKey
    : '__trueosBrowserUiHookEventQueue';
  const hookName = typeof options.hookName === 'string' && options.hookName
    ? options.hookName
    : '__trueosBrowserUiHook';
  const maxQueue = Math.max(1, Number(options.maxQueue || 64) | 0);
  let eventSeq = 0;

  function publishState(state) {
    host[stateKey] = state;
    return state;
  }

  function pushEvent(kind, state) {
    eventSeq += 1;
    const event = {
      eventSeq,
      kind: String(kind || ''),
      ...(state && typeof state === 'object' ? state : {}),
    };
    host[lastEventKey] = event;
    let queue = host[queueKey];
    if (!Array.isArray(queue)) {
      queue = [];
      host[queueKey] = queue;
    }
    queue.push(event);
    if (queue.length > maxQueue) {
      queue.splice(0, queue.length - maxQueue);
    }
    const hook = host[hookName];
    if (typeof hook === 'function') {
      try { hook(event); } catch (_) {}
    }
    return event;
  }

  function popEvent() {
    const queue = host[queueKey];
    if (!Array.isArray(queue) || queue.length <= 0) return null;
    return queue.shift() || null;
  }

  return {
    publishState,
    pushEvent,
    popEvent,
  };
}
