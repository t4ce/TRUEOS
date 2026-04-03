#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub const fn opposite(self) -> Self {
        match self {
            Self::White => Self::Black,
            Self::Black => Self::White,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub color: Color,
    pub kind: PieceKind,
}

impl Piece {
    pub const fn new(color: Color, kind: PieceKind) -> Self {
        Self { color, kind }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Square(u8);

impl Square {
    pub const fn new(file: u8, rank: u8) -> Option<Self> {
        if file < 8 && rank < 8 {
            Some(Self(rank * 8 + file))
        } else {
            None
        }
    }

    pub const fn from_index(index: u8) -> Option<Self> {
        if index < 64 {
            Some(Self(index))
        } else {
            None
        }
    }

    pub const fn file(self) -> u8 {
        self.0 % 8
    }

    pub const fn rank(self) -> u8 {
        self.0 / 8
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastleSide {
    Kingside,
    Queenside,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CastlingRights(u8);

impl CastlingRights {
    const WHITE_KINGSIDE: u8 = 1 << 0;
    const WHITE_QUEENSIDE: u8 = 1 << 1;
    const BLACK_KINGSIDE: u8 = 1 << 2;
    const BLACK_QUEENSIDE: u8 = 1 << 3;

    pub const fn new() -> Self {
        Self(
            Self::WHITE_KINGSIDE
                | Self::WHITE_QUEENSIDE
                | Self::BLACK_KINGSIDE
                | Self::BLACK_QUEENSIDE,
        )
    }

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn can_castle(self, color: Color, side: CastleSide) -> bool {
        let mask = match (color, side) {
            (Color::White, CastleSide::Kingside) => Self::WHITE_KINGSIDE,
            (Color::White, CastleSide::Queenside) => Self::WHITE_QUEENSIDE,
            (Color::Black, CastleSide::Kingside) => Self::BLACK_KINGSIDE,
            (Color::Black, CastleSide::Queenside) => Self::BLACK_QUEENSIDE,
        };
        (self.0 & mask) != 0
    }

    pub fn set_castle(&mut self, color: Color, side: CastleSide, allowed: bool) {
        let mask = match (color, side) {
            (Color::White, CastleSide::Kingside) => Self::WHITE_KINGSIDE,
            (Color::White, CastleSide::Queenside) => Self::WHITE_QUEENSIDE,
            (Color::Black, CastleSide::Kingside) => Self::BLACK_KINGSIDE,
            (Color::Black, CastleSide::Queenside) => Self::BLACK_QUEENSIDE,
        };

        if allowed {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }
    }
}

impl Default for CastlingRights {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameState {
    Active,
    Checkmate { winner: Color },
    Stalemate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveError {
    GameOver,
    EmptySource,
    WrongSideToMove,
    IllegalMove,
    MissingPromotion,
    InvalidPromotion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: Square,
    pub to: Square,
    pub promotion: Option<PieceKind>,
}

impl Move {
    pub const fn new(from: Square, to: Square) -> Self {
        Self {
            from,
            to,
            promotion: None,
        }
    }

    pub const fn with_promotion(from: Square, to: Square, promotion: PieceKind) -> Self {
        Self {
            from,
            to,
            promotion: Some(promotion),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MoveOutcome {
    pub moved_piece: Piece,
    pub captured_piece: Option<Piece>,
    pub promotion: Option<PieceKind>,
    pub castle: Option<CastleSide>,
    pub was_en_passant: bool,
    pub check: bool,
    pub state: GameState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MoveMeta {
    captured_square: Option<Square>,
    castle: Option<CastleSide>,
    rook_from: Option<Square>,
    rook_to: Option<Square>,
    next_en_passant_target: Option<Square>,
    resets_halfmove_clock: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Game {
    board: [Option<Piece>; 64],
    side_to_move: Color,
    castling_rights: CastlingRights,
    en_passant_target: Option<Square>,
    halfmove_clock: u16,
    fullmove_number: u16,
    state: GameState,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    pub fn new() -> Self {
        let mut game = Self::empty();

        for file in 0..8 {
            game.board[Square::new(file, 1).unwrap().index()] =
                Some(Piece::new(Color::White, PieceKind::Pawn));
            game.board[Square::new(file, 6).unwrap().index()] =
                Some(Piece::new(Color::Black, PieceKind::Pawn));
        }

        game.set_back_rank(Color::White, 0);
        game.set_back_rank(Color::Black, 7);
        game.refresh_state();
        game
    }

    pub const fn empty() -> Self {
        Self {
            board: [None; 64],
            side_to_move: Color::White,
            castling_rights: CastlingRights::empty(),
            en_passant_target: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            state: GameState::Active,
        }
    }

    pub const fn state(&self) -> GameState {
        self.state
    }

    pub const fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    pub const fn castling_rights(&self) -> CastlingRights {
        self.castling_rights
    }

    pub const fn en_passant_target(&self) -> Option<Square> {
        self.en_passant_target
    }

    pub const fn halfmove_clock(&self) -> u16 {
        self.halfmove_clock
    }

    pub const fn fullmove_number(&self) -> u16 {
        self.fullmove_number
    }

    pub fn piece_at(&self, square: Square) -> Option<Piece> {
        self.board[square.index()]
    }

    pub fn set_piece_at(&mut self, square: Square, piece: Option<Piece>) {
        self.board[square.index()] = piece;
    }

    pub fn set_side_to_move(&mut self, color: Color) {
        self.side_to_move = color;
    }

    pub fn set_castling_rights(&mut self, rights: CastlingRights) {
        self.castling_rights = rights;
    }

    pub fn set_en_passant_target(&mut self, square: Option<Square>) {
        self.en_passant_target = square;
    }

    pub fn set_move_counters(&mut self, halfmove_clock: u16, fullmove_number: u16) {
        self.halfmove_clock = halfmove_clock;
        self.fullmove_number = fullmove_number.max(1);
    }

    pub fn refresh_state(&mut self) {
        self.state = self.compute_state_for_side(self.side_to_move);
    }

    pub fn is_in_check(&self, color: Color) -> bool {
        let Some(king_square) = self.find_king(color) else {
            return false;
        };
        self.is_square_attacked(king_square, color.opposite())
    }

    pub fn is_legal_move(&self, chess_move: Move) -> bool {
        self.validate_move_for_side(self.side_to_move, chess_move).is_ok()
    }

    pub fn apply_move(&mut self, chess_move: Move) -> Result<MoveOutcome, MoveError> {
        if self.state != GameState::Active {
            return Err(MoveError::GameOver);
        }

        let moving_piece = self
            .piece_at(chess_move.from)
            .ok_or(MoveError::EmptySource)?;
        if moving_piece.color != self.side_to_move {
            return Err(MoveError::WrongSideToMove);
        }

        let meta = self.validate_move_for_side(self.side_to_move, chess_move)?;
        let captured_piece = self.execute_move_unchecked(chess_move, moving_piece, meta);

        self.side_to_move = self.side_to_move.opposite();
        if moving_piece.color == Color::Black {
            self.fullmove_number = self.fullmove_number.saturating_add(1);
        }

        self.state = self.compute_state_for_side(self.side_to_move);

        Ok(MoveOutcome {
            moved_piece: moving_piece,
            captured_piece,
            promotion: chess_move.promotion,
            castle: meta.castle,
            was_en_passant: meta.captured_square.is_some()
                && chess_move.to == self.en_passant_target.unwrap_or(chess_move.to)
                && moving_piece.kind == PieceKind::Pawn
                && captured_piece.is_some()
                && self.piece_at(chess_move.to).map(|piece| piece.kind) != Some(PieceKind::Pawn),
            check: self.is_in_check(self.side_to_move),
            state: self.state,
        })
    }

    fn set_back_rank(&mut self, color: Color, rank: u8) {
        let pieces = [
            PieceKind::Rook,
            PieceKind::Knight,
            PieceKind::Bishop,
            PieceKind::Queen,
            PieceKind::King,
            PieceKind::Bishop,
            PieceKind::Knight,
            PieceKind::Rook,
        ];

        let mut file = 0;
        while file < 8 {
            let square = Square::new(file, rank).unwrap();
            self.board[square.index()] = Some(Piece::new(color, pieces[file as usize]));
            file += 1;
        }

        self.castling_rights.set_castle(color, CastleSide::Kingside, true);
        self.castling_rights
            .set_castle(color, CastleSide::Queenside, true);
    }

    fn compute_state_for_side(&self, side: Color) -> GameState {
        if self.has_any_legal_move(side) {
            GameState::Active
        } else if self.is_in_check(side) {
            GameState::Checkmate {
                winner: side.opposite(),
            }
        } else {
            GameState::Stalemate
        }
    }

    fn has_any_legal_move(&self, side: Color) -> bool {
        let mut from_index = 0_u8;
        while from_index < 64 {
            let from = Square::from_index(from_index).unwrap();
            let Some(piece) = self.piece_at(from) else {
                from_index += 1;
                continue;
            };

            if piece.color != side {
                from_index += 1;
                continue;
            }

            let mut to_index = 0_u8;
            while to_index < 64 {
                let to = Square::from_index(to_index).unwrap();
                if piece.kind == PieceKind::Pawn && self.is_promotion_rank(piece.color, to.rank()) {
                    for promotion in [
                        PieceKind::Queen,
                        PieceKind::Rook,
                        PieceKind::Bishop,
                        PieceKind::Knight,
                    ] {
                        if self
                            .validate_move_for_side(
                                side,
                                Move::with_promotion(from, to, promotion),
                            )
                            .is_ok()
                        {
                            return true;
                        }
                    }
                } else if self.validate_move_for_side(side, Move::new(from, to)).is_ok() {
                    return true;
                }

                to_index += 1;
            }

            from_index += 1;
        }

        false
    }

    fn validate_move_for_side(&self, side: Color, chess_move: Move) -> Result<MoveMeta, MoveError> {
        let piece = self.piece_at(chess_move.from).ok_or(MoveError::EmptySource)?;
        if piece.color != side {
            return Err(MoveError::WrongSideToMove);
        }

        if chess_move.from == chess_move.to {
            return Err(MoveError::IllegalMove);
        }

        if let Some(target_piece) = self.piece_at(chess_move.to) {
            if target_piece.color == piece.color {
                return Err(MoveError::IllegalMove);
            }
        }

        let meta = self.validate_piece_move(piece, chess_move)?;

        let mut next = *self;
        next.execute_move_unchecked(chess_move, piece, meta);
        if next.is_in_check(side) {
            return Err(MoveError::IllegalMove);
        }

        Ok(meta)
    }

    fn validate_piece_move(&self, piece: Piece, chess_move: Move) -> Result<MoveMeta, MoveError> {
        match piece.kind {
            PieceKind::Pawn => self.validate_pawn_move(piece, chess_move),
            PieceKind::Knight => self.validate_knight_move(chess_move),
            PieceKind::Bishop => self.validate_bishop_move(chess_move),
            PieceKind::Rook => self.validate_rook_move(chess_move),
            PieceKind::Queen => self.validate_queen_move(chess_move),
            PieceKind::King => self.validate_king_move(piece.color, chess_move),
        }
    }

    fn validate_pawn_move(&self, piece: Piece, chess_move: Move) -> Result<MoveMeta, MoveError> {
        let from_file = chess_move.from.file() as i8;
        let from_rank = chess_move.from.rank() as i8;
        let to_file = chess_move.to.file() as i8;
        let to_rank = chess_move.to.rank() as i8;
        let dx = to_file - from_file;
        let dy = to_rank - from_rank;
        let direction = match piece.color {
            Color::White => 1,
            Color::Black => -1,
        };
        let start_rank = match piece.color {
            Color::White => 1,
            Color::Black => 6,
        };

        let mut meta = MoveMeta {
            captured_square: None,
            castle: None,
            rook_from: None,
            rook_to: None,
            next_en_passant_target: None,
            resets_halfmove_clock: true,
        };

        let is_promotion = self.is_promotion_rank(piece.color, chess_move.to.rank());
        match chess_move.promotion {
            Some(PieceKind::Queen | PieceKind::Rook | PieceKind::Bishop | PieceKind::Knight) if is_promotion => {}
            Some(PieceKind::Pawn | PieceKind::King) => return Err(MoveError::InvalidPromotion),
            Some(_) => return Err(MoveError::IllegalMove),
            None if is_promotion => return Err(MoveError::MissingPromotion),
            None => {}
        }

        if dx == 0 && dy == direction {
            if self.piece_at(chess_move.to).is_some() {
                return Err(MoveError::IllegalMove);
            }
            return Ok(meta);
        }

        if dx == 0 && dy == direction * 2 && chess_move.from.rank() == start_rank as u8 {
            let middle_rank = (from_rank + direction) as u8;
            let middle = Square::new(chess_move.from.file(), middle_rank).unwrap();
            if self.piece_at(middle).is_some() || self.piece_at(chess_move.to).is_some() {
                return Err(MoveError::IllegalMove);
            }
            meta.next_en_passant_target = Some(middle);
            return Ok(meta);
        }

        if dy == direction && (dx == -1 || dx == 1) {
            if self.piece_at(chess_move.to).is_some() {
                meta.captured_square = Some(chess_move.to);
                return Ok(meta);
            }

            if self.en_passant_target == Some(chess_move.to) {
                let captured_rank = match piece.color {
                    Color::White => chess_move.to.rank().saturating_sub(1),
                    Color::Black => chess_move.to.rank().saturating_add(1),
                };
                let captured_square = Square::new(chess_move.to.file(), captured_rank).unwrap();
                let captured_piece = self.piece_at(captured_square);
                if captured_piece == Some(Piece::new(piece.color.opposite(), PieceKind::Pawn)) {
                    meta.captured_square = Some(captured_square);
                    return Ok(meta);
                }
            }
        }

        Err(MoveError::IllegalMove)
    }

    fn validate_knight_move(&self, chess_move: Move) -> Result<MoveMeta, MoveError> {
        let dx = (chess_move.to.file() as i8 - chess_move.from.file() as i8).abs();
        let dy = (chess_move.to.rank() as i8 - chess_move.from.rank() as i8).abs();
        if !matches!((dx, dy), (1, 2) | (2, 1)) {
            return Err(MoveError::IllegalMove);
        }

        Ok(self.standard_move_meta(chess_move))
    }

    fn validate_bishop_move(&self, chess_move: Move) -> Result<MoveMeta, MoveError> {
        let dx = (chess_move.to.file() as i8 - chess_move.from.file() as i8).abs();
        let dy = (chess_move.to.rank() as i8 - chess_move.from.rank() as i8).abs();
        if dx == 0 || dx != dy || !self.is_path_clear(chess_move.from, chess_move.to) {
            return Err(MoveError::IllegalMove);
        }

        Ok(self.standard_move_meta(chess_move))
    }

    fn validate_rook_move(&self, chess_move: Move) -> Result<MoveMeta, MoveError> {
        let same_file = chess_move.from.file() == chess_move.to.file();
        let same_rank = chess_move.from.rank() == chess_move.to.rank();
        if (!same_file && !same_rank) || !self.is_path_clear(chess_move.from, chess_move.to) {
            return Err(MoveError::IllegalMove);
        }

        Ok(self.standard_move_meta(chess_move))
    }

    fn validate_queen_move(&self, chess_move: Move) -> Result<MoveMeta, MoveError> {
        let dx = (chess_move.to.file() as i8 - chess_move.from.file() as i8).abs();
        let dy = (chess_move.to.rank() as i8 - chess_move.from.rank() as i8).abs();
        let valid = dx == dy || chess_move.from.file() == chess_move.to.file() || chess_move.from.rank() == chess_move.to.rank();
        if !valid || !self.is_path_clear(chess_move.from, chess_move.to) {
            return Err(MoveError::IllegalMove);
        }

        Ok(self.standard_move_meta(chess_move))
    }

    fn validate_king_move(&self, color: Color, chess_move: Move) -> Result<MoveMeta, MoveError> {
        let dx = chess_move.to.file() as i8 - chess_move.from.file() as i8;
        let dy = chess_move.to.rank() as i8 - chess_move.from.rank() as i8;
        if dx.abs() <= 1 && dy.abs() <= 1 {
            return Ok(self.standard_move_meta(chess_move));
        }

        let home_rank = match color {
            Color::White => 0,
            Color::Black => 7,
        };
        let from = Square::new(4, home_rank).unwrap();
        if chess_move.from != from || dy != 0 {
            return Err(MoveError::IllegalMove);
        }

        let side = match chess_move.to.file() {
            6 if chess_move.to.rank() == home_rank => CastleSide::Kingside,
            2 if chess_move.to.rank() == home_rank => CastleSide::Queenside,
            _ => return Err(MoveError::IllegalMove),
        };

        if !self.castling_rights.can_castle(color, side) || self.is_in_check(color) {
            return Err(MoveError::IllegalMove);
        }

        let (rook_from, rook_to, clear_files, transit_files) = match side {
            CastleSide::Kingside => (
                Square::new(7, home_rank).unwrap(),
                Square::new(5, home_rank).unwrap(),
                [5_u8, 6_u8, 0_u8],
                [5_u8, 6_u8, 0_u8],
            ),
            CastleSide::Queenside => (
                Square::new(0, home_rank).unwrap(),
                Square::new(3, home_rank).unwrap(),
                [1_u8, 2_u8, 3_u8],
                [3_u8, 2_u8, 0_u8],
            ),
        };

        if self.piece_at(rook_from) != Some(Piece::new(color, PieceKind::Rook)) {
            return Err(MoveError::IllegalMove);
        }

        for file in clear_files {
            if file == 0 {
                continue;
            }
            if self.piece_at(Square::new(file, home_rank).unwrap()).is_some() {
                return Err(MoveError::IllegalMove);
            }
        }

        for file in transit_files {
            if file == 0 {
                continue;
            }
            let square = Square::new(file, home_rank).unwrap();
            if self.is_square_attacked(square, color.opposite()) {
                return Err(MoveError::IllegalMove);
            }
        }

        Ok(MoveMeta {
            captured_square: None,
            castle: Some(side),
            rook_from: Some(rook_from),
            rook_to: Some(rook_to),
            next_en_passant_target: None,
            resets_halfmove_clock: false,
        })
    }

    fn standard_move_meta(&self, chess_move: Move) -> MoveMeta {
        MoveMeta {
            captured_square: self.piece_at(chess_move.to).map(|_| chess_move.to),
            castle: None,
            rook_from: None,
            rook_to: None,
            next_en_passant_target: None,
            resets_halfmove_clock: self.piece_at(chess_move.to).is_some(),
        }
    }

    fn execute_move_unchecked(
        &mut self,
        chess_move: Move,
        moving_piece: Piece,
        meta: MoveMeta,
    ) -> Option<Piece> {
        let captured_piece = meta
            .captured_square
            .and_then(|square| self.board[square.index()].take());

        self.board[chess_move.from.index()] = None;

        let placed_piece = if let Some(promotion) = chess_move.promotion {
            Piece::new(moving_piece.color, promotion)
        } else {
            moving_piece
        };
        self.board[chess_move.to.index()] = Some(placed_piece);

        if let (Some(rook_from), Some(rook_to)) = (meta.rook_from, meta.rook_to) {
            let rook = self.board[rook_from.index()].take();
            self.board[rook_to.index()] = rook;
        }

        self.update_castling_rights_after_move(chess_move, moving_piece, captured_piece, meta.captured_square);
        self.en_passant_target = meta.next_en_passant_target;

        if meta.resets_halfmove_clock || moving_piece.kind == PieceKind::Pawn {
            self.halfmove_clock = 0;
        } else {
            self.halfmove_clock = self.halfmove_clock.saturating_add(1);
        }

        captured_piece
    }

    fn update_castling_rights_after_move(
        &mut self,
        chess_move: Move,
        moving_piece: Piece,
        captured_piece: Option<Piece>,
        captured_square: Option<Square>,
    ) {
        match moving_piece.kind {
            PieceKind::King => {
                self.castling_rights
                    .set_castle(moving_piece.color, CastleSide::Kingside, false);
                self.castling_rights
                    .set_castle(moving_piece.color, CastleSide::Queenside, false);
            }
            PieceKind::Rook => {
                self.clear_rook_right_for_square(chess_move.from, moving_piece.color);
            }
            _ => {}
        }

        if let Some(Piece {
            color,
            kind: PieceKind::Rook,
        }) = captured_piece
        {
            self.clear_rook_right_for_square(captured_square.unwrap_or(chess_move.to), color);
        }
    }

    fn clear_rook_right_for_square(&mut self, square: Square, color: Color) {
        match (color, square.file(), square.rank()) {
            (Color::White, 0, 0) => self
                .castling_rights
                .set_castle(Color::White, CastleSide::Queenside, false),
            (Color::White, 7, 0) => self
                .castling_rights
                .set_castle(Color::White, CastleSide::Kingside, false),
            (Color::Black, 0, 7) => self
                .castling_rights
                .set_castle(Color::Black, CastleSide::Queenside, false),
            (Color::Black, 7, 7) => self
                .castling_rights
                .set_castle(Color::Black, CastleSide::Kingside, false),
            _ => {}
        }
    }

    fn find_king(&self, color: Color) -> Option<Square> {
        let mut index = 0_u8;
        while index < 64 {
            let square = Square::from_index(index).unwrap();
            if self.piece_at(square) == Some(Piece::new(color, PieceKind::King)) {
                return Some(square);
            }
            index += 1;
        }
        None
    }

    fn is_square_attacked(&self, target: Square, attacker: Color) -> bool {
        self.is_attacked_by_pawn(target, attacker)
            || self.is_attacked_by_knight(target, attacker)
            || self.is_attacked_by_slider(
                target,
                attacker,
                &[(1, 0), (-1, 0), (0, 1), (0, -1)],
                PieceKind::Rook,
            )
            || self.is_attacked_by_slider(
                target,
                attacker,
                &[(1, 1), (1, -1), (-1, 1), (-1, -1)],
                PieceKind::Bishop,
            )
            || self.is_attacked_by_queen(target, attacker)
            || self.is_attacked_by_king(target, attacker)
    }

    fn is_attacked_by_pawn(&self, target: Square, attacker: Color) -> bool {
        let source_rank = match attacker {
            Color::White => target.rank().checked_sub(1),
            Color::Black => target.rank().checked_add(1).filter(|rank| *rank < 8),
        };
        let Some(source_rank) = source_rank else {
            return false;
        };

        for file_delta in [-1_i8, 1_i8] {
            let source_file = target.file() as i8 + file_delta;
            if !(0..8).contains(&source_file) {
                continue;
            }
            let square = Square::new(source_file as u8, source_rank).unwrap();
            if self.piece_at(square) == Some(Piece::new(attacker, PieceKind::Pawn)) {
                return true;
            }
        }

        false
    }

    fn is_attacked_by_knight(&self, target: Square, attacker: Color) -> bool {
        for (df, dr) in [
            (-2_i8, -1_i8),
            (-2, 1),
            (-1, -2),
            (-1, 2),
            (1, -2),
            (1, 2),
            (2, -1),
            (2, 1),
        ] {
            let file = target.file() as i8 + df;
            let rank = target.rank() as i8 + dr;
            if !(0..8).contains(&file) || !(0..8).contains(&rank) {
                continue;
            }

            let square = Square::new(file as u8, rank as u8).unwrap();
            if self.piece_at(square) == Some(Piece::new(attacker, PieceKind::Knight)) {
                return true;
            }
        }

        false
    }

    fn is_attacked_by_slider(
        &self,
        target: Square,
        attacker: Color,
        directions: &[(i8, i8)],
        primary_kind: PieceKind,
    ) -> bool {
        for &(df, dr) in directions {
            let mut file = target.file() as i8 + df;
            let mut rank = target.rank() as i8 + dr;

            while (0..8).contains(&file) && (0..8).contains(&rank) {
                let square = Square::new(file as u8, rank as u8).unwrap();
                if let Some(piece) = self.piece_at(square) {
                    if piece.color == attacker
                        && (piece.kind == primary_kind || piece.kind == PieceKind::Queen)
                    {
                        return true;
                    }
                    break;
                }
                file += df;
                rank += dr;
            }
        }

        false
    }

    fn is_attacked_by_queen(&self, target: Square, attacker: Color) -> bool {
        self.is_attacked_by_slider(
            target,
            attacker,
            &[
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ],
            PieceKind::Queen,
        )
    }

    fn is_attacked_by_king(&self, target: Square, attacker: Color) -> bool {
        let mut rank_delta = -1_i8;
        while rank_delta <= 1 {
            let mut file_delta = -1_i8;
            while file_delta <= 1 {
                if file_delta == 0 && rank_delta == 0 {
                    file_delta += 1;
                    continue;
                }

                let file = target.file() as i8 + file_delta;
                let rank = target.rank() as i8 + rank_delta;
                if (0..8).contains(&file) && (0..8).contains(&rank) {
                    let square = Square::new(file as u8, rank as u8).unwrap();
                    if self.piece_at(square) == Some(Piece::new(attacker, PieceKind::King)) {
                        return true;
                    }
                }

                file_delta += 1;
            }
            rank_delta += 1;
        }

        false
    }

    fn is_path_clear(&self, from: Square, to: Square) -> bool {
        let file_step = (to.file() as i8 - from.file() as i8).signum();
        let rank_step = (to.rank() as i8 - from.rank() as i8).signum();
        let mut file = from.file() as i8 + file_step;
        let mut rank = from.rank() as i8 + rank_step;

        while file != to.file() as i8 || rank != to.rank() as i8 {
            let square = Square::new(file as u8, rank as u8).unwrap();
            if self.piece_at(square).is_some() {
                return false;
            }
            file += file_step;
            rank += rank_step;
        }

        true
    }

    fn is_promotion_rank(&self, color: Color, rank: u8) -> bool {
        match color {
            Color::White => rank == 7,
            Color::Black => rank == 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CastleSide, CastlingRights, Color, Game, GameState, Move, MoveError, Piece, PieceKind, Square};

    fn sq(file: u8, rank: u8) -> Square {
        Square::new(file, rank).unwrap()
    }

    #[test]
    fn initial_position_is_populated() {
        let game = Game::new();

        assert_eq!(game.side_to_move(), Color::White);
        assert_eq!(game.state(), GameState::Active);
        assert_eq!(game.piece_at(sq(4, 0)), Some(Piece::new(Color::White, PieceKind::King)));
        assert_eq!(game.piece_at(sq(4, 7)), Some(Piece::new(Color::Black, PieceKind::King)));
        assert!(game.castling_rights().can_castle(Color::White, CastleSide::Kingside));
        assert!(game.castling_rights().can_castle(Color::Black, CastleSide::Queenside));
    }

    #[test]
    fn double_pawn_push_sets_en_passant_target() {
        let mut game = Game::new();

        let outcome = game.apply_move(Move::new(sq(4, 1), sq(4, 3))).unwrap();

        assert_eq!(outcome.state, GameState::Active);
        assert_eq!(game.en_passant_target(), Some(sq(4, 2)));
        assert_eq!(game.side_to_move(), Color::Black);
    }

    #[test]
    fn rejects_illegal_opening_move() {
        let mut game = Game::new();

        let result = game.apply_move(Move::new(sq(2, 0), sq(5, 3)));

        assert_eq!(result, Err(MoveError::IllegalMove));
    }

    #[test]
    fn supports_en_passant_capture() {
        let mut game = Game::new();
        game.apply_move(Move::new(sq(4, 1), sq(4, 3))).unwrap();
        game.apply_move(Move::new(sq(0, 6), sq(0, 5))).unwrap();
        game.apply_move(Move::new(sq(4, 3), sq(4, 4))).unwrap();
        game.apply_move(Move::new(sq(3, 6), sq(3, 4))).unwrap();

        let outcome = game.apply_move(Move::new(sq(4, 4), sq(3, 5))).unwrap();

        assert!(outcome.was_en_passant);
        assert_eq!(outcome.captured_piece, Some(Piece::new(Color::Black, PieceKind::Pawn)));
        assert_eq!(game.piece_at(sq(3, 4)), None);
        assert_eq!(game.piece_at(sq(3, 5)), Some(Piece::new(Color::White, PieceKind::Pawn)));
    }

    #[test]
    fn supports_kingside_castling() {
        let mut game = Game::new();
        game.apply_move(Move::new(sq(4, 1), sq(4, 3))).unwrap();
        game.apply_move(Move::new(sq(4, 6), sq(4, 4))).unwrap();
        game.apply_move(Move::new(sq(6, 0), sq(5, 2))).unwrap();
        game.apply_move(Move::new(sq(1, 7), sq(2, 5))).unwrap();
        game.apply_move(Move::new(sq(5, 0), sq(4, 1))).unwrap();
        game.apply_move(Move::new(sq(6, 7), sq(5, 5))).unwrap();

        let outcome = game.apply_move(Move::new(sq(4, 0), sq(6, 0))).unwrap();

        assert_eq!(outcome.castle, Some(CastleSide::Kingside));
        assert_eq!(game.piece_at(sq(6, 0)), Some(Piece::new(Color::White, PieceKind::King)));
        assert_eq!(game.piece_at(sq(5, 0)), Some(Piece::new(Color::White, PieceKind::Rook)));
        assert!(!game.castling_rights().can_castle(Color::White, CastleSide::Kingside));
    }

    #[test]
    fn detects_fools_mate_checkmate() {
        let mut game = Game::new();
        game.apply_move(Move::new(sq(5, 1), sq(5, 2))).unwrap();
        game.apply_move(Move::new(sq(4, 6), sq(4, 4))).unwrap();
        game.apply_move(Move::new(sq(6, 1), sq(6, 3))).unwrap();

        let outcome = game.apply_move(Move::new(sq(3, 7), sq(7, 3))).unwrap();

        assert!(outcome.check);
        assert_eq!(outcome.state, GameState::Checkmate { winner: Color::Black });
        assert_eq!(game.state(), GameState::Checkmate { winner: Color::Black });
    }

    #[test]
    fn promotion_requires_choice_and_upgrades_piece() {
        let mut game = Game::empty();
        game.set_piece_at(sq(0, 0), Some(Piece::new(Color::White, PieceKind::King)));
        game.set_piece_at(sq(7, 7), Some(Piece::new(Color::Black, PieceKind::King)));
        game.set_piece_at(sq(6, 6), Some(Piece::new(Color::White, PieceKind::Pawn)));
        game.set_side_to_move(Color::White);
        game.refresh_state();

        let missing = game.apply_move(Move::new(sq(6, 6), sq(6, 7)));
        assert_eq!(missing, Err(MoveError::MissingPromotion));

        let outcome = game
            .apply_move(Move::with_promotion(sq(6, 6), sq(6, 7), PieceKind::Queen))
            .unwrap();

        assert_eq!(outcome.promotion, Some(PieceKind::Queen));
        assert_eq!(game.piece_at(sq(6, 7)), Some(Piece::new(Color::White, PieceKind::Queen)));
    }

    #[test]
    fn refresh_state_detects_stalemate() {
        let mut game = Game::empty();
        game.set_piece_at(sq(2, 5), Some(Piece::new(Color::White, PieceKind::King)));
        game.set_piece_at(sq(2, 6), Some(Piece::new(Color::White, PieceKind::Queen)));
        game.set_piece_at(sq(0, 7), Some(Piece::new(Color::Black, PieceKind::King)));
        game.set_side_to_move(Color::Black);
        game.set_castling_rights(CastlingRights::empty());
        game.refresh_state();

        assert_eq!(game.state(), GameState::Stalemate);
        assert!(!game.is_in_check(Color::Black));
    }
}