import { Rectangle } from 'pixi.js';
import { TEXT_BASELINE_NUDGE_Y } from '../text.mjs';
import { clearContainerEvents, getOrCreateText } from '../pixiReuse.mjs';
export function getOrInitSliderState(map, key, attrs) {
    const existing = map.get(key);
    if (existing)
        return existing;
    const raw = Number(attrs?.value ?? '0');
    const v = Number.isFinite(raw) ? raw : 0;
    const state = { value: Math.max(0, Math.min(1, v)) };
    map.set(key, state);
    return state;
}
export function applyYogaDefaultsSlider(yogaNode, Yoga) {
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
    yogaNode.setHeight(14);
    yogaNode.setMinWidth(240);
}
export function createYogaNodeForSliderLabel(opts) {
    const { node, Yoga, measurer } = opts;
    const yogaNode = Yoga.Node.create();
    yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
    yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
    yogaNode.setPadding(Yoga.EDGE_TOP, 0);
    yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
    yogaNode.setMargin(Yoga.EDGE_RIGHT, 6);
    yogaNode.setMeasureFunc(() => {
        const m = measurer.measure('100');
        return { width: m.width, height: m.height };
    });
    return {
        yogaNode,
        buildBox: () => ({
            kind: 'block',
            key: node.key,
            tagName: node.tagName,
            attrs: node.attrs,
            x: yogaNode.getComputedLeft(),
            y: yogaNode.getComputedTop(),
            width: yogaNode.getComputedWidth(),
            height: yogaNode.getComputedHeight(),
            children: [],
        }),
    };
}
export function renderSliderLabel(opts) {
    const { node, container, theme, sliderStates } = opts;
    const sliderKey = node.attrs?.['data-slider-key'];
    let st = null;
    if (sliderKey) {
        const existing = sliderStates.get(sliderKey);
        if (existing)
            st = existing;
        else {
            const init = node.attrs?.['data-slider-init'];
            st = getOrInitSliderState(sliderStates, sliderKey, init != null ? { value: String(init) } : undefined);
        }
    }
    const pct = st ? Math.round(st.value * 100) : 0;
    const t = getOrCreateText(container, '__pct', (tt) => {
        tt.style = {
            fontFamily: theme.fontFamily,
            fontSize: theme.fontSize,
            fill: theme.text,
            fontWeight: '400',
            wordWrap: false,
        };
    });
    t.text = String(pct);
    t.position.set(0, TEXT_BASELINE_NUDGE_Y);
}
export function renderSlider(opts) {
    const { node, container, graphics: g, w, h, absX, absY, theme, sliderStates, sliderBounds, sliderDrags, requestPaint } = opts;
    const key = node.key;
    const state = key ? getOrInitSliderState(sliderStates, key, node.attrs) : null;
    const bw = Math.max(0, Math.round(w));
    const bh = Math.max(0, Math.round(h));
    const innerPad = 3;
    if (key)
        sliderBounds.set(key, { x: absX, y: absY, w: bw, h: bh, innerPad });
    {
        const sw = 1;
        const inset = sw / 2;
        g.rect(inset, inset, Math.max(0, bw - sw), Math.max(0, bh - sw));
        g.fill(theme.control.progress.background);
        g.stroke({ width: sw, color: theme.control.progress.border });
    }
    const ratio = state ? Math.max(0, Math.min(1, state.value)) : 0;
    const innerW = Math.max(0, bw - innerPad * 2);
    const innerH = Math.max(0, bh - innerPad * 2);
    g.rect(innerPad, innerPad, Math.max(0, innerW * ratio), innerH);
    g.fill(theme.control.progress.fill);
    // Indicator: vertical line at current value, 2x the bar height.
    const ix = innerPad + innerW * ratio;
    const overhang = innerH / 2;
    g.moveTo(ix, innerPad - overhang);
    g.lineTo(ix, innerPad + innerH + overhang);
    g.stroke({ width: 2, color: theme.text });
    if (!key)
        return;
    clearContainerEvents(container);
    container.eventMode = 'static';
    container.cursor = 'pointer';
    container.hitArea = new Rectangle(0, 0, Math.max(0, bw), Math.max(0, bh));
    container.on('pointerdown', (ev) => {
        if (ev?.button === 2)
            return;
        const pid = opts.getPointerId ? opts.getPointerId(ev) : Number(ev?.pointerId ?? ev?.data?.pointerId ?? 0);
        if (pid <= 0)
            return; // Ignore pointerdown when effective pointerId is 0 (noop mode)
        for (const [otherPid, d] of sliderDrags.entries()) {
            if (d.key === key && otherPid !== pid)
                sliderDrags.delete(otherPid);
        }
        sliderDrags.set(pid, { key });
        const b = sliderBounds.get(key);
        const gx = ev.global?.x ?? 0;
        const localX = b ? gx - b.x : 0;
        const innerW2 = b ? Math.max(1, b.w - b.innerPad * 2) : 1;
        const r = (localX - (b?.innerPad ?? 0)) / innerW2;
        const s = getOrInitSliderState(sliderStates, key, node.attrs);
        s.value = Math.max(0, Math.min(1, r));
        requestPaint?.();
    });
}
