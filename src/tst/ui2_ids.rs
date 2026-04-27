#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ui2DemoTexId {
    Triangle = 4_700,
    Tetris = 4_701,
    Mandelbrot = 4_702,
    Bgrt = 4_704,
    Shell = 4_705,
    Particle = 4_709,
    Svg = 4_710,
    ParticleSprite = 4_711,
    SmileyFountain = 4_713,
    Weather = 4_715,
    Coreticks = 4_716,
    Swarm = 4_720,
    Raple = 4_722,
    Currency = 4_723,
    Player = 4_724,
    CursorPicker = 4_725,
    TextInput = 4_726,
    TrueosfsExplorer = 4_901,
}

impl Ui2DemoTexId {
    #[inline]
    pub const fn get(self) -> u32 {
        self as u32
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ui2DemoContentId {
    Shell = 43,
    TrueosfsExplorer = 44,
    Coreticks = 45,
    Swarm = 46,
    Weather = 47,
    Raple = 49,
    Currency = 50,
    Player = 51,
    CursorPicker = 52,
    TextInput = 53,
}

impl Ui2DemoContentId {
    #[inline]
    pub const fn get(self) -> u32 {
        self as u32
    }
}
