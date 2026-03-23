function resolveNodeByPath(root, path = 'root') {
  if (!root || typeof root !== 'object') return null;
  const value = typeof path === 'string' ? path.trim() : '';
  if (!value || value === 'root') return root;
  const parts = value.split('.');
  if (parts.length <= 0 || parts[0] !== 'root') return null;

  let node = root;
  for (let i = 1; i < parts.length; i++) {
    const index = Number(parts[i]);
    if (!Number.isInteger(index) || index < 0) return null;
    const kids = Array.isArray(node && node.childNodes) ? node.childNodes : [];
    node = kids[index] || null;
    if (!node || typeof node !== 'object') return null;
  }
  return node;
}

function cloneDomEventPayload(payload = null) {
  const source = payload && typeof payload === 'object' ? payload : {};
  return {
    type: typeof source.type === 'string' ? source.type : 'click',
    path: typeof source.path === 'string' ? source.path : '',
    slotId: Number(source.slotId || 0) | 0,
    x: Number(source.x || 0),
    y: Number(source.y || 0),
  };
}

function attachInteractiveRuntime(node, layout, hooks) {
  if (!node || typeof node !== 'object' || !layout || typeof layout !== 'object') return;
  const ui = node.__ui && typeof node.__ui === 'object' ? node.__ui : {};
  const path = String(layout.path || '');
  const caption = String(layout.caption || '');
  const kind = String(layout.kind || (String(layout.tag || '').toLowerCase() === 'a' ? 'link' : 'button'));
  ui.kind = kind;
  ui.path = path;
  ui.caption = caption;
  ui.href = String(layout.href || '');
  ui.rect = {
    x: Number(layout.x || 0),
    y: Number(layout.y || 0),
    width: Number(layout.width || 0),
    height: Number(layout.height || 0),
  };
  ui.hovered = !!ui.hovered;
  ui.pressed = !!ui.pressed;
  node.__ui = ui;

  if (kind === 'link') {
    // Links are navigational targets, not button-style DOM callbacks.
    if (typeof node.onClick === 'function' && typeof ui.onClick !== 'function') {
      ui.onClick = node.onClick;
    }
    return;
  }

  if (typeof node.onClick !== 'function' && typeof ui.onClick !== 'function') {
    const defaultHandler = (event = null) => hooks.dispatchBrowserAction('dom-click', {
      kind,
      path,
      caption,
      href: ui.href,
      event: cloneDomEventPayload(event),
    }, '__trueosBrowserDomClick');
    node.onClick = defaultHandler;
    ui.onClick = defaultHandler;
    return;
  }

  if (typeof node.onClick === 'function' && typeof ui.onClick !== 'function') {
    ui.onClick = node.onClick;
  } else if (typeof node.onClick !== 'function' && typeof ui.onClick === 'function') {
    node.onClick = ui.onClick;
  }
}

export function attachThemeLayoutRuntime(doc, themeLayout, hooks) {
  const root = doc && typeof doc === 'object' ? doc : null;
  const interactives = Array.isArray(themeLayout && themeLayout.interactives) ? themeLayout.interactives : [];
  if (!root || interactives.length <= 0) return;
  for (let i = 0; i < interactives.length; i++) {
    const layout = interactives[i];
    const node = resolveNodeByPath(root, layout && layout.path ? layout.path : '');
    const tag = String(node && node.tagName || '').toLowerCase();
    if (!node || !hooks.isElement(node) || (tag !== 'button' && tag !== 'a')) {
      continue;
    }
    attachInteractiveRuntime(node, layout, hooks);
  }
}

export function dispatchDomClick(path, payload = null, hooks) {
  const targetPath = typeof path === 'string' ? path.trim() : '';
  if (!targetPath) {
    return { ok: 0, handled: 0, reason: 'missing-path' };
  }

  const { vw } = hooks.computeViewport();
  const doc = hooks.ensureDoc(vw);
  const root = doc && doc.dom ? doc.dom : null;
  const node = resolveNodeByPath(root, targetPath);
  const tag = String(node && node.tagName || '').toLowerCase();
  if (!node || !hooks.isElement(node) || (tag !== 'button' && tag !== 'a')) {
    return { ok: 0, handled: 0, reason: 'interactive-not-found', path: targetPath };
  }

  const ui = node.__ui && typeof node.__ui === 'object' ? node.__ui : {};
  node.__ui = ui;
  ui.kind = tag === 'a' ? 'link' : 'button';
  ui.path = targetPath;
  ui.href = typeof ui.href === 'string' ? ui.href : '';
  ui.lastClick = cloneDomEventPayload({
    ...(payload && typeof payload === 'object' ? payload : null),
    path: targetPath,
    type: 'click',
  });
  ui.pressed = false;

  if (tag === 'a') {
    const href = typeof ui.href === 'string' && ui.href.trim()
      ? ui.href.trim()
      : String((payload && typeof payload === 'object' && payload.href) || '');
    if (!href) {
      return { ok: 0, handled: 0, reason: 'missing-href', path: targetPath };
    }
    return hooks.surfToUrl(href, {
      ...ui.lastClick,
      href,
      path: targetPath,
    });
  }

  const handler = typeof node.onClick === 'function'
    ? node.onClick
    : (typeof ui.onClick === 'function' ? ui.onClick : null);
  if (typeof handler !== 'function') {
    return { ok: 1, handled: 0, path: targetPath };
  }

  try {
    handler.call(node, ui.lastClick);
    return { ok: 1, handled: 1, path: targetPath };
  } catch (err) {
    hooks.raiseBrowserError(
      'TRUEOS_BROWSER_DOM_CLICK_FAILED',
      'DOM onClick callback failed',
      {
        path: targetPath,
        reason: hooks.describeError(err),
      },
      err,
    );
  }
}

export const dispatchButtonClick = dispatchDomClick;