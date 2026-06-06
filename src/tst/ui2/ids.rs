#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ui2DemoTexId {
    Mandelbrot = 4_702,
    Bgrt = 4_704,
    Shell = 4_705,
    SmileyFountain = 4_713,
    Coreticks = 4_716,
    Swarm = 4_720,
    Raple = 4_722,
    Player = 4_724,
    CursorPicker = 4_725,
    TextInput = 4_726,
    AnalogClock = 4_727,
    Gboi = 4_728,
    RenderAlbum = 4_729,
    IntelCanvas3d = 4_730,
    IntelCanvas3dPlanePatch = 4_731,
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
    Swarm = 46,
    Raple = 49,
    Player = 51,
    CursorPicker = 52,
    TextInput = 53,
    AnalogClock = 54,
    Gboi = 55,
}

impl Ui2DemoContentId {
    #[inline]
    pub const fn get(self) -> u32 {
        self as u32
    }
}
