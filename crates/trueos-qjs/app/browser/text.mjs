import { Text } from 'pixi.js';
export const WRAP_EPSILON_PX = 24;
// Pixi text metrics tend to sit slightly high; a tiny nudge improves visual centering.
export const TEXT_BASELINE_NUDGE_Y = 1;
export function makeThemedText(opts) {
    const wrapWidth = opts.wrapWidth;
    const useWrap = opts.wordWrap ?? wrapWidth != null;
    const wordWrapWidth = opts.wordWrapWidth ??
        (wrapWidth == null ? undefined : Math.max(0, Math.ceil(wrapWidth) + WRAP_EPSILON_PX));
    return new Text({
        text: opts.text,
        style: {
            fontFamily: opts.fontFamily,
            fontSize: opts.fontSize,
            fill: opts.fill,
            fontWeight: opts.bold ? '700' : '400',
            wordWrap: useWrap,
            wordWrapWidth,
        },
    });
}
