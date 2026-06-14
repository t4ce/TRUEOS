import { BLOCK_TAGS } from './htmlDefaults.mjs';
import { defaultTheme } from '../rendertree/renderTheme.mjs';

const FONT_PX = 15;
const FONT_RGB = 0x000000;

const TAG_THEME_DEFAULTS = {
  a: {
    color: rgbIntToCss(defaultTheme.control.link.text),
    display: 'inline-block',
    paint: {
      role: 'link',
      fill: 'linear-gradient',
      color0: defaultTheme.control.link.fill,
      color1: defaultTheme.control.link.fillEnd,
      borderWidth: defaultTheme.control.link.borderWidth,
      radius: defaultTheme.control.link.radius,
      textColor: defaultTheme.control.link.text,
    },
  },
  b: {
    fontWeight: 'bold',
  },
  h1: {
    fontSizePx: 30,
    lineHeightPx: 38,
    fontWeight: 'bold',
  },
  h2: {
    fontSizePx: 22,
    lineHeightPx: 30,
    fontWeight: 'bold',
  },
  h3: {
    fontSizePx: 18,
    lineHeightPx: 26,
    fontWeight: 'bold',
  },
  h4: {
    fontSizePx: 15,
    lineHeightPx: 22,
    fontWeight: 'bold',
  },
  h5: {
    fontSizePx: 12,
    lineHeightPx: 20,
    fontWeight: 'bold',
  },
  h6: {
    fontSizePx: 10,
    lineHeightPx: 20,
    fontWeight: 'bold',
  },
  button: {
    display: 'inline-block',
    backgroundColor: '#e9ecef',
    textAlign: 'center',
    marginTopPx: 4,
    marginRightPx: 0,
    marginBottomPx: 4,
    marginLeftPx: 0,
    paddingTopPx: 6,
    paddingRightPx: 12,
    paddingBottomPx: 6,
    paddingLeftPx: 12,
    paint: {
      role: 'button',
      fill: 'linear-gradient',
      color0: defaultTheme.control.button.fill,
      color1: defaultTheme.control.button.fillEnd,
      borderColor: defaultTheme.control.button.border,
      borderWidth: defaultTheme.control.button.borderWidth,
      radius: defaultTheme.control.button.radius,
    },
  },
  dialog: {
    paint: {
      role: 'dialog',
      fill: 'linear-gradient',
      color0: defaultTheme.control.dialog.fill,
      color1: defaultTheme.control.dialog.fillEnd,
      borderColor: defaultTheme.control.dialog.border,
      borderWidth: defaultTheme.control.dialog.borderWidth,
      radius: defaultTheme.control.dialog.radius,
    },
  },
  em: {
    fontStyle: 'italic',
  },
  i: {
    fontStyle: 'italic',
  },
  iframe: {
    paint: {
      role: 'iframe',
      borderColor: defaultTheme.control.iframe.border,
      borderWidth: defaultTheme.control.iframe.borderWidth,
      radius: defaultTheme.control.iframe.radius,
    },
  },
  hr: {
    paint: {
      role: 'rule',
      fill: 'linear-gradient',
      color0Rgba: defaultTheme.control.rule.color0Rgba,
      color1Rgba: defaultTheme.control.rule.color1Rgba,
      borderWidth: 0,
      radius: 0,
    },
  },
  img: {
    display: 'inline-block',
    paint: {
      role: 'image',
      fill: 'linear-gradient',
      color0: defaultTheme.control.image.fill,
      color1: defaultTheme.control.image.fillEnd,
      borderWidth: defaultTheme.control.image.borderWidth,
      radius: defaultTheme.control.image.radius,
    },
  },
  strong: {
    fontWeight: 'bold',
  },
};

export const INHERITED_STYLE_FIELDS = [
  'color',
  'fontSizePx',
  'lineHeightPx',
  'fontWeight',
  'fontStyle',
  'textAlign',
  'whiteSpace',
];

function rgbByteToHex(v) {
  const n = Math.max(0, Math.min(255, Number(v || 0) | 0));
  return n.toString(16).padStart(2, '0');
}

export function rgbIntToCss(rgb) {
  const value = Math.max(0, Number(rgb || 0) >>> 0) & 0xFFFFFF;
  return `#${rgbByteToHex((value >> 16) & 0xFF)}${rgbByteToHex((value >> 8) & 0xFF)}${rgbByteToHex(value & 0xFF)}`;
}

export function defaultDisplayForTag(tagName) {
  const tag = String(tagName || '').toLowerCase();
  if (!tag) return 'inline';
  if (tag === 'li') return 'list-item';
  if (tag === 'img' || tag === 'svg' || tag === 'canvas' || tag === 'iframe') return 'inline-block';
  if (BLOCK_TAGS.has(tag)) return 'block';
  return 'inline';
}

export function createComputedStyle(tagName = '', path = '', parentStyle = null) {
  const tag = String(tagName || '').toLowerCase();
  const fontSizePx = Number(FONT_PX || 16);
  const style = {
    path: String(path || ''),
    tag,
    display: defaultDisplayForTag(tag),
    color: rgbIntToCss(FONT_RGB),
    backgroundColor: 'transparent',
    fontSizePx,
    lineHeightPx: Math.max(20, fontSizePx + 4),
    fontWeight: 'normal',
    fontStyle: 'normal',
    textAlign: 'left',
    whiteSpace: tag === 'pre' ? 'pre' : 'normal',
    marginLeftPx: 0,
    marginTopPx: 0,
    marginRightPx: 0,
    marginBottomPx: 0,
    paddingLeftPx: 0,
    paddingTopPx: 0,
    paddingRightPx: 0,
    paddingBottomPx: 0,
    paint: null,
    source: {
      matchedRules: [],
      inline: false,
    },
  };

  if (parentStyle && typeof parentStyle === 'object') {
    for (let i = 0; i < INHERITED_STYLE_FIELDS.length; i++) {
      const key = INHERITED_STYLE_FIELDS[i];
      if (parentStyle[key] != null) style[key] = parentStyle[key];
    }
  }

  const tagDefaults = TAG_THEME_DEFAULTS[tag] || null;
  if (tagDefaults && typeof tagDefaults === 'object') {
    const keys = Object.keys(tagDefaults);
    for (let i = 0; i < keys.length; i++) {
      const key = keys[i];
      style[key] = tagDefaults[key];
    }
  }

  return style;
}
