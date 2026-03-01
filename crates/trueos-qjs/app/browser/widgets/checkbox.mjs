import { clearContainerEvents } from '/qjs/browser/pixi_reuse.mjs';

// Ported from Parse5/src/widgets/input.ts checkbox path.
export function renderCheckbox(opts) {
  const { container, graphics: g, w, h, theme, state, onChange } = opts;

  const draw = () => {
    g.clear();

    const sw = 1;
    const inset = sw / 2;
    g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
    g.fill(theme.control.background);
    g.stroke({ width: sw, color: theme.control.border });

    if (state.indeterminate) {
      const dInset = 4;
      const lw = 2;
      g.moveTo(dInset, dInset);
      g.lineTo(Math.max(dInset, w - dInset), Math.max(dInset, h - dInset));
      g.stroke({ width: lw, color: theme.control.accent });
      g.moveTo(Math.max(dInset, w - dInset), dInset);
      g.lineTo(dInset, Math.max(dInset, h - dInset));
      g.stroke({ width: lw, color: theme.control.accent });
    } else if (state.checked) {
      const cInset = 3;
      g.rect(cInset, cInset, Math.max(0, w - cInset * 2), Math.max(0, h - cInset * 2));
      g.fill(theme.control.accent);
    }
  };

  draw();

  clearContainerEvents(container);
  container.eventMode = 'static';
  container.cursor = 'pointer';
  container.on('pointerdown', (ev) => {
    if (ev?.button === 2) return;

    // Tri-state cycle: unchecked -> checked -> indeterminate -> unchecked.
    if (!state.checked && !state.indeterminate) {
      state.checked = true;
      state.indeterminate = false;
    } else if (state.checked && !state.indeterminate) {
      state.checked = false;
      state.indeterminate = true;
    } else {
      state.checked = false;
      state.indeterminate = false;
    }

    draw();
    onChange?.(state);
    ev?.stopPropagation?.();
  });
}

export function applyYogaDefaultsCheckbox(yogaNode, Yoga) {
  yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
  yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
  yogaNode.setPadding(Yoga.EDGE_TOP, 0);
  yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
  yogaNode.setWidth(16);
  yogaNode.setHeight(16);
  yogaNode.setMinWidth(16);
  yogaNode.setMargin(Yoga.EDGE_RIGHT, 6);
}
