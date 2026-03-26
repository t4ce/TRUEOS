use super::world::TileId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarbleWidgetKind {
    WhiteHole,
    BlackHole,
    Route,
    Variable,
    Clause,
    WaverNode,
    EtcherNode,
    SatSink,
    UnsatSink,
}

#[derive(Clone, Copy, Debug)]
pub struct MarbleWidget {
    pub kind: MarbleWidgetKind,
    pub links: [Option<TileId>; 4],
    pub cost: u16,
    pub flags: u16,
}

impl MarbleWidget {
    pub fn new(kind: MarbleWidgetKind) -> Self {
        Self {
            kind,
            links: [None; 4],
            cost: 1,
            flags: 0,
        }
    }
}
