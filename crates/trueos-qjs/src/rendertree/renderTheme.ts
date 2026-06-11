export type Theme = {
  fontFamily: string;
  fontSize: number;
  background: number;
  text: number;
  mutedText: number;
  boxBorder: number;

  hr: number;

  control: {
    border: number;
    focusBorder: number;
    background: number;
    accent: number;
    radius: number;

    button: {
      fill: number;
      hoverFill: number;
      activeFill: number;
      border: number;
      text: number;
      radius: number;
    };

    progress: {
      border: number;
      fill: number;
      background: number;
    };

    table: {
      border: number;
      cellBorder: number;
      headerFill: number;
    };
  };
};

export const defaultTheme: Theme = {
  fontFamily: 'system-ui, -apple-system, Segoe UI, Arial',
  fontSize: 16,
  background: 0xffffff,
  text: 0x111111,
  mutedText: 0x666666,
  boxBorder: 0xdddddd,

  hr: 0xcccccc,

  control: {
    border: 0x000000,
    focusBorder: 0x3b82f6,
    background: 0xffffff,
    accent: 0x3b82f6,
    radius: 0,

    button: {
      fill: 0xf2f2f2,
      hoverFill: 0xeaeaea,
      activeFill: 0xe0e0e0,
      border: 0x666666,
      text: 0x111111,
      radius: 0,
    },

    progress: {
      border: 0x999999,
      background: 0xffffff,
      fill: 0x6aa9ff,
    },

    table: {
      border: 0x999999,
      cellBorder: 0xb0b0b0,
      headerFill: 0xf7f7f7,
    },
  },
};
