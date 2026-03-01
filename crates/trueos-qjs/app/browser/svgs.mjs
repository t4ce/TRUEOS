// Centralized SVG strings/generators used by the default renderer.
// Keep SVGs simple: Pixi's SVG parser supports only a subset.
export const ARROW_UP_SVG = `<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
  <polygon points="50,8 92,58 68,58 68,92 32,92 32,58 8,58" fill="none" stroke="black" stroke-width="6" stroke-linejoin="round" />
</svg>`;
export const ARROW_DOWN_SVG = `<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
  <polygon points="32,8 68,8 68,42 92,42 50,92 8,42 32,42" fill="none" stroke="black" stroke-width="6" stroke-linejoin="round" />
</svg>`;
export const ARROW_LEFT_SVG = `<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
  <polygon points="8,50 58,8 58,32 92,32 92,68 58,68 58,92" fill="none" stroke="black" stroke-width="6" stroke-linejoin="round" />
</svg>`;
export const ARROW_RIGHT_SVG = `<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg">
  <polygon points="92,50 42,8 42,32 8,32 8,68 42,68 42,92" fill="none" stroke="black" stroke-width="6" stroke-linejoin="round" />
</svg>`;
export function makeImgPlaceholderSvg(width, height) {
    const w = Math.max(1, Math.floor(width));
    const h = Math.max(1, Math.floor(height));
    return `<?xml version="1.0" encoding="UTF-8"?>
<svg viewBox="0 0 ${w} ${h}" xmlns="http://www.w3.org/2000/svg">
  <rect x="0" y="0" width="${w}" height="${h}" fill="#f6f6f6"/>
  <rect x="0.5" y="0.5" width="${Math.max(0, w - 1)}" height="${Math.max(0, h - 1)}" fill="none" stroke="#999"/>
  <path d="M2 2 L${Math.max(2, w - 2)} ${Math.max(2, h - 2)}" stroke="#c8c8c8"/>
  <path d="M${Math.max(2, w - 2)} 2 L2 ${Math.max(2, h - 2)}" stroke="#c8c8c8"/>
</svg>`;
}
export function makeImgNoSrcPlaceholderSvg(width, height) {
    const w = Math.max(1, Math.floor(width));
    const h = Math.max(1, Math.floor(height));
    return `<?xml version="1.0" encoding="UTF-8"?>
<svg viewBox="0 0 ${w} ${h}" xmlns="http://www.w3.org/2000/svg">
  <rect x="0" y="0" width="${w}" height="${h}" fill="#ffffff"/>
  <rect x="0.5" y="0.5" width="${Math.max(0, w - 1)}" height="${Math.max(0, h - 1)}" fill="none" stroke="#000"/>
</svg>`;
}
export function makeNeonOrbSvg({ ring = 34, core = 14, hueA = '#00e5ff', hueB = '#ff2bd6', } = {}) {
    const ring2 = Math.max(0, ring - 10);
    const coreH = Math.max(0, core * 0.35);
    return `
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <rect width="100" height="100" fill="#ffffff"/>
  <rect width="100" height="100" fill="${hueA}" opacity="0.08"/>

  <circle cx="50" cy="50" r="${ring}" fill="none" stroke="${hueB}" stroke-width="4" opacity="0.95"/>
  <circle cx="50" cy="50" r="${ring2}" fill="none" stroke="${hueA}" stroke-width="1" opacity="0.35"/>

  <circle cx="50" cy="50" r="${core}" fill="${hueA}" opacity="0.9"/>
  <circle cx="43" cy="43" r="${coreH}" fill="#ffffff" opacity="0.55"/>

  <path d="M50 16 L52 22 L58 24 L52 26 L50 32 L48 26 L42 24 L48 22 Z" fill="#ffffff" opacity="0.85"/>
  <path d="M82 52 L85 56 L90 57 L85 58 L82 62 L79 58 L74 57 L79 56 Z" fill="#ffffff" opacity="0.70"/>
  <path d="M20 70 L22 74 L27 75 L22 76 L20 80 L18 76 L13 75 L18 74 Z" fill="#ffffff" opacity="0.65"/>
</svg>
`;
}
