import { Rectangle } from 'pixi.js';
import { TEXT_BASELINE_NUDGE_Y } from '../text.mjs';
import { clampWrappedLines, getCaretIndexFromPoint, wrapFieldTextWithIndices } from './textField.mjs';
import { clearContainerEvents, clearGraphics, getOrCreateGraphics, getOrCreateText } from '../pixiReuse.mjs';
export function applyYogaDefaultsInput(yogaNode, node, Yoga) {
    const inputType = (node.attrs?.type ?? 'text').toLowerCase();
    if (inputType === 'checkbox' || inputType === 'radio') {
        yogaNode.setPadding(Yoga.EDGE_LEFT, 0);
        yogaNode.setPadding(Yoga.EDGE_RIGHT, 0);
        yogaNode.setPadding(Yoga.EDGE_TOP, 0);
        yogaNode.setPadding(Yoga.EDGE_BOTTOM, 0);
        yogaNode.setWidth(16);
        yogaNode.setHeight(16);
        yogaNode.setMinWidth(16);
        // Breathing room between the control and label text.
        yogaNode.setMargin(Yoga.EDGE_RIGHT, 6);
    }
    else {
        yogaNode.setPadding(Yoga.EDGE_TOP, 6);
        yogaNode.setPadding(Yoga.EDGE_BOTTOM, 6);
        yogaNode.setHeight(36);
        yogaNode.setMinHeight(36);
        yogaNode.setMinWidth(220);
    }
}
export function renderInput(opts) {
    const { node, container, graphics: g, w, h, absX, absY, theme, textMeasure, uiState, getOrInitInputState, clamp, radioGroups, textDrags, requestPaint, } = opts;
    const type = (node.attrs?.type ?? 'text').toLowerCase();
    const key = node.key;
    const state = key ? getOrInitInputState(key, node.attrs) : undefined;
    const showCaret = opts.showCaret ?? false;
    const caretPointerId = opts.caretPointerId ?? null;
    const focusColor = opts.focusColor;
    const getCursorColor = opts.getCursorColor;
    // Retained rendering: keep a stable child list and use zIndex for overlays.
    container.sortableChildren = true;
    // Hide any previously-created selection/caret overlays; we'll re-enable as needed.
    for (const ch of container.children) {
        const lbl = ch.label;
        if (lbl && (lbl.startsWith('__sel:') || lbl === '__caret'))
            ch.visible = false;
    }
    const leftPad = 8;
    const topPad = 6 + TEXT_BASELINE_NUDGE_Y;
    const maxLines = 5;
    const lineHeight = theme.fontSize * 1.25;
    if (type === 'checkbox') {
        {
            const sw = 1;
            const inset = sw / 2;
            g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
            g.fill(theme.control.background);
            g.stroke({ width: sw, color: theme.control.border });
        }
        if (state?.indeterminate) {
            const inset = 4;
            const lw = 2;
            g.moveTo(inset, inset);
            g.lineTo(Math.max(inset, w - inset), Math.max(inset, h - inset));
            g.stroke({ width: lw, color: theme.control.accent });
            g.moveTo(Math.max(inset, w - inset), inset);
            g.lineTo(inset, Math.max(inset, h - inset));
            g.stroke({ width: lw, color: theme.control.accent });
        }
        else if (state?.checked) {
            const inset = 3;
            g.rect(inset, inset, Math.max(0, w - inset * 2), Math.max(0, h - inset * 2));
            g.fill(theme.control.accent);
        }
    }
    else if (type === 'radio') {
        {
            const sw = 1;
            const inset = sw / 2;
            const r = Math.max(0, Math.min(w, h) / 2 - inset);
            g.circle(w / 2, h / 2, r);
            g.fill(theme.control.background);
            g.stroke({ width: sw, color: theme.control.border });
        }
        if (state?.checked) {
            const r = Math.max(0, Math.min(w, h) / 2 - 4.5);
            g.circle(w / 2, h / 2, r);
            g.fill(theme.control.accent);
        }
    }
    else {
        const sw = focusColor != null ? 2 : 1;
        const inset = sw / 2;
        if (theme.control.radius > 0)
            g.roundRect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw), theme.control.radius);
        else
            g.rect(inset, inset, Math.max(0, w - sw), Math.max(0, h - sw));
        g.fill(theme.control.background);
        g.stroke({ width: sw, color: focusColor != null ? focusColor : theme.control.border });
        const shown = type === 'password' ? '•'.repeat((state?.value ?? '').length) : state?.value ?? '';
        const innerWidth = Math.max(0, w - leftPad * 2);
        if (key) {
            uiState.fieldBounds.set(key, {
                x: absX,
                y: absY,
                w,
                h,
                innerLeft: leftPad,
                innerTop: topPad,
                innerWidth,
                maxLines,
                isPassword: type === 'password',
            });
        }
        const allLines = wrapFieldTextWithIndices(shown, innerWidth, textMeasure);
        const visibleLines = clampWrappedLines(allLines, maxLines);
        const visibleEnd = visibleLines.length > 0 ? visibleLines[visibleLines.length - 1].end : 0;
        if (key && state && typeof state.value === 'string') {
            // Draw selection highlights for all pointers that have a selection in this field.
            const sels = state.selections;
            if (sels && sels.size > 0) {
                for (const [pid, sel] of sels.entries()) {
                    const a = clamp(sel.start ?? 0, 0, shown.length);
                    const b = clamp(sel.end ?? a, 0, shown.length);
                    const start = clamp(Math.min(a, b), 0, visibleEnd);
                    const end = clamp(Math.max(a, b), 0, visibleEnd);
                    if (start === end)
                        continue;
                    const sg = getOrCreateGraphics(container, `__sel:${pid}`);
                    clearGraphics(sg);
                    sg.zIndex = 0;
                    sg.visible = true;
                    for (let li = 0; li < visibleLines.length; li++) {
                        const ln = visibleLines[li];
                        const s0 = Math.max(start, ln.start);
                        const e0 = Math.min(end, ln.end);
                        if (s0 >= e0)
                            continue;
                        const px = leftPad + textMeasure(shown.slice(ln.start, s0));
                        const pw = textMeasure(shown.slice(s0, e0));
                        sg.rect(px, topPad + li * lineHeight, pw, lineHeight);
                    }
                    sg.fill({ color: getCursorColor(pid), alpha: 0.22 });
                }
            }
            // Draw a single caret for the active pointer (keyboard focus, or active drag).
            if (showCaret && caretPointerId != null) {
                const sel = state.selections?.get(caretPointerId);
                const caretAtRaw = sel ? sel.end : 0;
                const caretAt = clamp(caretAtRaw, 0, visibleEnd);
                let caretLineIdx = Math.max(0, visibleLines.length - 1);
                for (let li = 0; li < visibleLines.length; li++) {
                    const ln = visibleLines[li];
                    if (caretAt >= ln.start && caretAt <= ln.end) {
                        caretLineIdx = li;
                        break;
                    }
                }
                const ln = visibleLines[caretLineIdx] ?? { start: 0, end: 0, text: '' };
                const cx = leftPad + textMeasure(shown.slice(ln.start, caretAt));
                const caret = getOrCreateGraphics(container, '__caret');
                clearGraphics(caret);
                caret.zIndex = 2;
                caret.visible = true;
                caret.moveTo(cx, topPad + caretLineIdx * lineHeight);
                caret.lineTo(cx, topPad + caretLineIdx * lineHeight + lineHeight);
                caret.stroke({ width: 1, color: focusColor != null ? focusColor : theme.control.focusBorder });
            }
        }
        const displayText = visibleLines.map((l) => l.text).join('\n');
        const valueText = getOrCreateText(container, '__valueText', (t) => {
            // Style is constant for this widget; set it once.
            t.style = {
                fontFamily: theme.fontFamily,
                fontSize: theme.fontSize,
                fill: theme.text,
                fontWeight: '400',
                wordWrap: false,
            };
            t.zIndex = 1;
        });
        valueText.text = displayText;
        valueText.position.set(leftPad, topPad);
    }
    // Interactivity: focus and toggle
    if (!key)
        return;
    clearContainerEvents(container);
    container.eventMode = 'static';
    container.cursor = 'text';
    container.on('pointerdown', (ev) => {
        if (ev?.button === 2)
            return;
        const pid = opts.getPointerId ? opts.getPointerId(ev) : Number(ev?.pointerId ?? ev?.data?.pointerId ?? 0);
        if (pid <= 0)
            return;
        // Per-pointer focus: only this pointer's focus changes.
        uiState.focusedKeyByPointer.set(pid, key);
        uiState.keyboardOwnerPointerId = pid;
        if (type === 'checkbox') {
            const s = getOrInitInputState(key, node.attrs);
            const ind = s.indeterminate === true;
            const chk = s.checked === true;
            // Tri-state cycle: unchecked -> checked -> indeterminate -> unchecked
            if (!chk && !ind) {
                s.checked = true;
                s.indeterminate = false;
            }
            else if (chk && !ind) {
                s.checked = false;
                s.indeterminate = true;
            }
            else {
                s.checked = false;
                s.indeterminate = false;
            }
        }
        else if (type === 'radio') {
            const name = node.attrs?.name ?? '__default__';
            const groupKey = `radio:${name}`;
            const keys = radioGroups.get(groupKey) ?? [];
            for (const k of keys) {
                const s = getOrInitInputState(k, undefined);
                s.checked = k === key;
            }
        }
        else {
            const s = getOrInitInputState(key, node.attrs);
            if (typeof s.value === 'string') {
                // Clear this pointer's previous selection elsewhere (but keep other pointers' selections).
                for (const [k, st] of uiState.inputs.entries()) {
                    if (k === key)
                        continue;
                    st.selections?.delete(pid);
                }
                const shown = type === 'password' ? '•'.repeat(s.value.length) : s.value;
                const bounds = uiState.fieldBounds.get(key);
                const innerW = bounds?.innerWidth ?? Math.max(0, w - leftPad * 2);
                const lines = clampWrappedLines(wrapFieldTextWithIndices(shown, innerW, textMeasure), maxLines);
                const localX = (ev.global?.x ?? 0) - absX - leftPad;
                const localY = (ev.global?.y ?? 0) - absY - topPad;
                const idx = getCaretIndexFromPoint({
                    fullText: shown,
                    lines,
                    localX,
                    localY,
                    lineHeight,
                    measure: textMeasure,
                });
                if (!s.selections)
                    s.selections = new Map();
                s.selections.set(pid, { start: idx, end: idx });
                // "Last cursor wins" for selection drags on the same element.
                for (const [otherPid, d] of textDrags.entries()) {
                    if (d.key === key && otherPid !== pid)
                        textDrags.delete(otherPid);
                }
                textDrags.set(pid, { key, anchor: idx });
            }
        }
        // Prevent wrappers like <summary> (used by treeview via <details>) from also
        // treating checkbox/radio clicks as a toggle click.
        if (type === 'checkbox' || type === 'radio') {
            ev.stopPropagation?.();
        }
        requestPaint?.();
    });
    // Cursor hint
    if (type === 'checkbox' || type === 'radio')
        container.cursor = 'pointer';
    // Ensure hit testing works even for tiny checkbox/radio.
    container.hitArea = new Rectangle(0, 0, Math.max(0, w), Math.max(0, h));
}
