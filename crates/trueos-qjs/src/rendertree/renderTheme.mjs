export const defaultTheme = Object.freeze({
  fontFamily: 'system-ui, -apple-system, Segoe UI, Arial',
  fontSize: 16,
  background: 0xffffff,
  text: 0x111111,
  mutedText: 0x666666,
  boxBorder: 0xdddddd,

  hr: 0xcccccc,

  control: Object.freeze({
    border: 0x000000,
    focusBorder: 0x3b82f6,
    background: 0xffffff,
    accent: 0x3b82f6,
    radius: 0,

    button: Object.freeze({
      fill: 0xf2f2f2,
      fillEnd: 0xffffff,
      hoverFill: 0xeaeaea,
      activeFill: 0xe0e0e0,
      border: 0x666666,
      borderWidth: 1,
      text: 0x111111,
      radius: 0,
    }),

    link: Object.freeze({
      fill: 0xf2f2f2,
      fillEnd: 0xffffff,
      text: 0x0000ee,
      borderWidth: 0,
      radius: 0,
    }),

    dialog: Object.freeze({
      fill: 0xadd8e6,
      fillEnd: 0xffffff,
      border: 0x666666,
      borderWidth: 0,
      radius: 0,
    }),

    iframe: Object.freeze({
      border: 0x666666,
      borderWidth: 1,
      radius: 0,
    }),

    rule: Object.freeze({
      color0Rgba: 0xff000000,
      color1Rgba: 0x00000000,
      height: 2,
    }),

    image: Object.freeze({
      fill: 0x90ee90,
      fillEnd: 0xffffff,
      borderWidth: 0,
      radius: 0,
    }),

    progress: Object.freeze({
      border: 0x999999,
      background: 0xffffff,
      fill: 0x6aa9ff,
    }),

    table: Object.freeze({
      border: 0x999999,
      borderWidth: 1,
      cellBorder: 0xb0b0b0,
      cellBorderWidth: 1,
      headerFill: 0xf7f7f7,
    }),
  }),
});

export const defaultLayoutMetrics = Object.freeze({
  tagDefaults: Object.freeze({
    a: Object.freeze({ paddingX: 4, paddingY: 2 }),
    hr: Object.freeze({ height: 2 }),
    summary: Object.freeze({ minHeight: 64 }),
    input: Object.freeze({ height: 36, minWidth: 220 }),
    button: Object.freeze({ height: 36, minWidth: 100 }),
    textarea: Object.freeze({ height: 108, minWidth: 220 }),
    select: Object.freeze({ height: 36, minWidth: 220 }),
    searchbutton: Object.freeze({ width: 36, height: 36, minWidth: 36, minHeight: 36 }),
    progress: Object.freeze({ height: 14, minWidth: 240 }),
    meter: Object.freeze({ height: 14, minWidth: 240 }),
    slider: Object.freeze({ height: 14, minWidth: 240 }),
    number: Object.freeze({ height: 36, minWidth: 140 }),
    color: Object.freeze({ width: 240, height: 200, minWidth: 240, minHeight: 200 }),
    timeinput: Object.freeze({ height: 36, minWidth: 220 }),
    dateinput: Object.freeze({ height: 36, minWidth: 220 }),
    monthinput: Object.freeze({ height: 36, minWidth: 220 }),
    weekinput: Object.freeze({ height: 36, minWidth: 220 }),
    datetimelocalinput: Object.freeze({ height: 36, minWidth: 340 }),
  }),
});
