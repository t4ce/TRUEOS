//! Game Boy GPU (LCD Controller) — PPU with background, window, sprite rendering
//! Output: 160×144 pixels, 4-shade palette

use alloc::vec;
use alloc::vec::Vec;

/// Classic Game Boy green palette (darker = higher index)
pub const GB_PALETTE: [u32; 4] = [
    0xFFE0F8D0, // 0: Lightest (almost white-green)
    0xFF88C070, // 1: Light green
    0xFF346856, // 2: Dark green
    0xFF081820, // 3: Darkest (almost black)
];

pub const SCREEN_W: usize = 160;
pub const SCREEN_H: usize = 144;
pub const DOTS_PER_SCANLINE: u32 = 456;
pub const SCANLINES_PER_FRAME: u32 = 154;
pub const DOTS_PER_FRAME: u32 = DOTS_PER_SCANLINE * SCANLINES_PER_FRAME;

const OAM_DOTS: u32 = 80;
const TRANSFER_DOTS: u32 = 172;
const HBLANK_DOTS: u32 = DOTS_PER_SCANLINE - OAM_DOTS - TRANSFER_DOTS;

pub struct Gpu {
    pub vram: [u8; 8192],      // $8000-$9FFF VRAM bank 0
    pub vram1: [u8; 8192],     // $8000-$9FFF VRAM bank 1 (CGB only)
    pub oam: [u8; 160],        // $FE00-$FE9F (40 sprites × 4 bytes)
    pub framebuffer: Vec<u32>, // 160×144 ARGB

    // LCD registers
    pub lcdc: u8, // $FF40 LCD Control
    pub stat: u8, // $FF41 LCD Status
    pub scy: u8,  // $FF42 Scroll Y
    pub scx: u8,  // $FF43 Scroll X
    pub ly: u8,   // $FF44 LY (current scanline)
    pub lyc: u8,  // $FF45 LY Compare
    pub bgp: u8,  // $FF47 BG Palette (DMG)
    pub obp0: u8, // $FF48 Object Palette 0 (DMG)
    pub obp1: u8, // $FF49 Object Palette 1 (DMG)
    pub wy: u8,   // $FF4A Window Y
    pub wx: u8,   // $FF4B Window X

    pub mode: u8,    // Current LCD mode (0-3)
    pub cycles: u32, // Dots elapsed in the current mode
    pub frame_ready: bool,
    pub stat_irq: bool,   // STAT interrupt request
    pub vblank_irq: bool, // VBlank interrupt request

    // Internal
    pub window_line: u8, // Current window line counter
    stat_line: bool,     // Combined STAT sources, for rising-edge detection

    // CGB extensions
    pub cgb_mode: bool,        // Running in CGB mode
    pub vram_bank: u8,         // FF4F: VRAM bank select (0 or 1)
    pub bg_palette: [u8; 64],  // CGB BG color palette data (8 palettes × 4 colors × 2 bytes)
    pub obj_palette: [u8; 64], // CGB OBJ color palette data
    pub bcps: u8,              // FF68: BG Color Palette Spec (auto-inc + index)
    pub ocps: u8,              // FF6A: OBJ Color Palette Spec
    // Internal: scanline BG priority info for sprite drawing
    bg_priority: [u8; 160], // Per-pixel: bit0=color_id!=0, bit1=BG-to-OAM priority
}

impl Gpu {
    pub fn new() -> Self {
        Self {
            vram: [0; 8192],
            vram1: [0; 8192],
            oam: [0; 160],
            framebuffer: vec![GB_PALETTE[0]; SCREEN_W * SCREEN_H],
            lcdc: 0x91,
            stat: 0x00,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            wy: 0,
            wx: 0,
            mode: 2,
            cycles: 0,
            frame_ready: false,
            stat_irq: false,
            vblank_irq: false,
            window_line: 0,
            stat_line: false,
            cgb_mode: false,
            vram_bank: 0,
            bg_palette: [0xFF; 64],
            obj_palette: [0xFF; 64],
            bcps: 0,
            ocps: 0,
            bg_priority: [0; 160],
        }
    }

    /// Advance the PPU by normal-speed CPU M-cycles (four dots each).
    pub fn step(&mut self, m_cycles: u32) {
        self.step_dots(m_cycles.saturating_mul(4));
    }

    /// Advance the PPU by master-clock dots.  This is also the correct input
    /// unit while the CGB CPU is in double-speed mode.
    pub fn step_dots(&mut self, dots: u32) {
        if self.lcdc & 0x80 == 0 {
            return;
        }

        self.cycles = self.cycles.saturating_add(dots);
        loop {
            let mode_duration = match self.mode {
                0 => HBLANK_DOTS,
                1 => DOTS_PER_SCANLINE,
                2 => OAM_DOTS,
                3 => TRANSFER_DOTS,
                _ => {
                    self.mode = 2;
                    OAM_DOTS
                }
            };
            if self.cycles < mode_duration {
                break;
            }

            self.cycles -= mode_duration;
            self.finish_mode();
        }
    }

    fn finish_mode(&mut self) {
        match self.mode {
            2 => self.mode = 3,
            3 => {
                self.render_scanline();
                self.mode = 0;
            }
            0 => {
                self.ly = self.ly.wrapping_add(1);
                if self.ly == SCREEN_H as u8 {
                    self.mode = 1;
                    self.vblank_irq = true;
                    self.frame_ready = true;
                    self.window_line = 0;
                } else {
                    self.mode = 2;
                }
                self.update_lyc_flag();
            }
            1 => {
                self.ly = self.ly.wrapping_add(1);
                if self.ly >= SCANLINES_PER_FRAME as u8 {
                    self.ly = 0;
                    self.mode = 2;
                }
                self.update_lyc_flag();
            }
            _ => self.mode = 2,
        }
        self.update_stat_line();
    }

    fn update_lyc_flag(&mut self) {
        if self.ly == self.lyc {
            self.stat |= 0x04;
        } else {
            self.stat &= !0x04;
        }
    }

    fn update_stat_line(&mut self) {
        let coincidence = self.ly == self.lyc && self.stat & 0x40 != 0;
        let mode_source = match self.mode {
            0 => self.stat & 0x08 != 0,
            1 => self.stat & 0x10 != 0,
            2 => self.stat & 0x20 != 0,
            _ => false,
        };
        let line = self.lcdc & 0x80 != 0 && (coincidence || mode_source);
        if line && !self.stat_line {
            self.stat_irq = true;
        }
        self.stat_line = line;
    }

    pub fn write_lcdc(&mut self, value: u8) {
        let was_enabled = self.lcdc & 0x80 != 0;
        let enabled = value & 0x80 != 0;
        self.lcdc = value;

        if was_enabled != enabled {
            self.ly = 0;
            self.cycles = 0;
            self.mode = if enabled { 2 } else { 0 };
            self.window_line = 0;
            self.update_lyc_flag();
        }
        self.update_stat_line();
    }

    pub fn write_stat(&mut self, value: u8) {
        self.stat = (self.stat & 0x07) | (value & 0xF8);
        self.update_stat_line();
    }

    pub fn write_lyc(&mut self, value: u8) {
        self.lyc = value;
        self.update_lyc_flag();
        self.update_stat_line();
    }

    pub fn read_stat(&self) -> u8 {
        let mode = if self.lcdc & 0x80 != 0 { self.mode } else { 0 };
        0x80 | (self.stat & 0x78) | (if self.ly == self.lyc { 0x04 } else { 0 }) | mode
    }

    /// Render one scanline to the framebuffer
    fn render_scanline(&mut self) {
        let ly = self.ly as usize;
        if ly >= SCREEN_H {
            return;
        }

        let offset = ly * SCREEN_W;

        // Clear scanline and priority buffer
        for x in 0..SCREEN_W {
            self.framebuffer[offset + x] = if self.cgb_mode {
                Self::cgb_color(&self.bg_palette, 0, 0)
            } else {
                GB_PALETTE[0]
            };
            self.bg_priority[x] = 0;
        }

        // Render background
        if self.cgb_mode || self.lcdc & 0x01 != 0 {
            if self.cgb_mode {
                self.render_bg_scanline_cgb(ly, offset);
            } else {
                self.render_bg_scanline(ly, offset);
            }
        }

        // Render window
        if self.lcdc & 0x20 != 0 && (self.cgb_mode || self.lcdc & 0x01 != 0) {
            if self.cgb_mode {
                self.render_window_scanline_cgb(ly, offset);
            } else {
                self.render_window_scanline(ly, offset);
            }
        }

        // Render sprites
        if self.lcdc & 0x02 != 0 {
            if self.cgb_mode {
                self.render_sprite_scanline_cgb(ly, offset);
            } else {
                self.render_sprite_scanline(ly, offset);
            }
        }
    }

    fn render_bg_scanline(&mut self, ly: usize, offset: usize) {
        let tile_data_area = if self.lcdc & 0x10 != 0 {
            0x0000usize
        } else {
            0x0800
        };
        let tile_map_area = if self.lcdc & 0x08 != 0 {
            0x1C00usize
        } else {
            0x1800
        };
        let signed_addr = self.lcdc & 0x10 == 0;

        let y = (self.scy as usize + ly) & 0xFF;
        let tile_row = y / 8;
        let pixel_row = y % 8;

        for x in 0..SCREEN_W {
            let real_x = (self.scx as usize + x) & 0xFF;
            let tile_col = real_x / 8;
            let pixel_col = real_x % 8;

            let map_idx = tile_row * 32 + tile_col;
            let tile_id = self.vram[tile_map_area + map_idx];

            let tile_addr = if signed_addr {
                let signed_id = tile_id as i8 as i32;
                (tile_data_area as i32 + (signed_id + 128) * 16) as usize
            } else {
                tile_data_area + tile_id as usize * 16
            };

            let row_addr = tile_addr + pixel_row * 2;
            if row_addr + 1 >= self.vram.len() {
                continue;
            }

            let lo = self.vram[row_addr];
            let hi = self.vram[row_addr + 1];
            let bit = 7 - pixel_col;
            let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);

            let palette_color = (self.bgp >> (color_id * 2)) & 3;
            self.framebuffer[offset + x] = GB_PALETTE[palette_color as usize];
        }
    }

    fn render_window_scanline(&mut self, ly: usize, offset: usize) {
        if ly < self.wy as usize {
            return;
        }
        let wx = self.wx as i32 - 7;

        let tile_data_area = if self.lcdc & 0x10 != 0 {
            0x0000usize
        } else {
            0x0800
        };
        let tile_map_area = if self.lcdc & 0x40 != 0 {
            0x1C00usize
        } else {
            0x1800
        };
        let signed_addr = self.lcdc & 0x10 == 0;

        let win_y = self.window_line as usize;
        let tile_row = win_y / 8;
        let pixel_row = win_y % 8;

        let mut rendered = false;

        for x in 0..SCREEN_W {
            let win_x = x as i32 - wx;
            if win_x < 0 {
                continue;
            }
            rendered = true;

            let tile_col = win_x as usize / 8;
            let pixel_col = win_x as usize % 8;
            let map_idx = tile_row * 32 + tile_col;
            if map_idx >= 1024 {
                continue;
            }

            let tile_id = self.vram[tile_map_area + map_idx];

            let tile_addr = if signed_addr {
                let signed_id = tile_id as i8 as i32;
                (tile_data_area as i32 + (signed_id + 128) * 16) as usize
            } else {
                tile_data_area + tile_id as usize * 16
            };

            let row_addr = tile_addr + pixel_row * 2;
            if row_addr + 1 >= self.vram.len() {
                continue;
            }

            let lo = self.vram[row_addr];
            let hi = self.vram[row_addr + 1];
            let bit = 7 - pixel_col;
            let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);

            let palette_color = (self.bgp >> (color_id * 2)) & 3;
            self.framebuffer[offset + x] = GB_PALETTE[palette_color as usize];
        }

        if rendered {
            self.window_line += 1;
        }
    }

    fn render_sprite_scanline(&mut self, ly: usize, offset: usize) {
        let sprite_h = if self.lcdc & 0x04 != 0 { 16 } else { 8 };

        // Collect sprites on this scanline (max 10)
        let mut sprites: [(u8, u8, u8, u8, usize); 10] = [(0, 0, 0, 0, 0); 10];
        let mut count = 0usize;

        for i in 0..40 {
            let sy = self.oam[i * 4] as i32 - 16;
            let tile = self.oam[i * 4 + 2];
            let flags = self.oam[i * 4 + 3];

            if ly as i32 >= sy && (ly as i32) < sy + sprite_h as i32 {
                if count < 10 {
                    sprites[count] = (self.oam[i * 4], self.oam[i * 4 + 1], tile, flags, i);
                    count += 1;
                }
            }
        }

        // Render sprites in reverse order (lower OAM index = higher priority)
        for i in (0..count).rev() {
            let (sy_raw, sx_raw, mut tile, flags, _oam_idx) = sprites[i];
            let sy = sy_raw as i32 - 16;
            let sx = sx_raw as i32 - 8;
            let flip_x = flags & 0x20 != 0;
            let flip_y = flags & 0x40 != 0;
            let behind_bg = flags & 0x80 != 0;
            let palette = if flags & 0x10 != 0 {
                self.obp1
            } else {
                self.obp0
            };

            let mut row = ly as i32 - sy;
            if flip_y {
                row = sprite_h as i32 - 1 - row;
            }

            if sprite_h == 16 {
                tile &= 0xFE; // In 8×16 mode, bit 0 is ignored
                if row >= 8 {
                    tile += 1;
                    row -= 8;
                }
            }

            let tile_addr = tile as usize * 16 + row as usize * 2;
            if tile_addr + 1 >= self.vram.len() {
                continue;
            }

            let lo = self.vram[tile_addr];
            let hi = self.vram[tile_addr + 1];

            for px in 0..8i32 {
                let screen_x = sx + px;
                if screen_x < 0 || screen_x >= SCREEN_W as i32 {
                    continue;
                }

                let bit = if flip_x { px } else { 7 - px } as u8;
                let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                if color_id == 0 {
                    continue;
                } // Transparent

                let screen_idx = offset + screen_x as usize;

                // Behind BG: only draw if BG pixel is color 0
                if behind_bg {
                    let bg_color = self.framebuffer[screen_idx];
                    if bg_color != GB_PALETTE[0] {
                        continue;
                    }
                }

                let palette_color = (palette >> (color_id * 2)) & 3;
                self.framebuffer[screen_idx] = GB_PALETTE[palette_color as usize];
            }
        }
    }

    // ======================== CGB RENDERING ========================

    fn render_bg_scanline_cgb(&mut self, ly: usize, offset: usize) {
        let tile_data_area = if self.lcdc & 0x10 != 0 {
            0x0000usize
        } else {
            0x0800
        };
        let tile_map_area = if self.lcdc & 0x08 != 0 {
            0x1C00usize
        } else {
            0x1800
        };
        let signed_addr = self.lcdc & 0x10 == 0;

        let y = (self.scy as usize + ly) & 0xFF;
        let tile_row = y / 8;
        let pixel_row = y % 8;

        for x in 0..SCREEN_W {
            let real_x = (self.scx as usize + x) & 0xFF;
            let tile_col = real_x / 8;
            let pixel_col = real_x % 8;

            let map_idx = tile_row * 32 + tile_col;
            let tile_id = self.vram[tile_map_area + map_idx];
            // CGB: tile attributes from VRAM bank 1
            let attrs = self.vram1[tile_map_area + map_idx];
            let cgb_palette = (attrs & 0x07) as usize;
            let tile_vram_bank = (attrs >> 3) & 1;
            let flip_x = attrs & 0x20 != 0;
            let flip_y = attrs & 0x40 != 0;
            let bg_priority = attrs & 0x80 != 0;

            let tile_addr = if signed_addr {
                let signed_id = tile_id as i8 as i32;
                (tile_data_area as i32 + (signed_id + 128) * 16) as usize
            } else {
                tile_data_area + tile_id as usize * 16
            };

            let actual_row = if flip_y { 7 - pixel_row } else { pixel_row };
            let row_addr = tile_addr + actual_row * 2;
            let vram_data = if tile_vram_bank == 1 {
                &self.vram1
            } else {
                &self.vram
            };
            if row_addr + 1 >= vram_data.len() {
                continue;
            }

            let lo = vram_data[row_addr];
            let hi = vram_data[row_addr + 1];
            let bit = if flip_x { pixel_col } else { 7 - pixel_col };
            let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);

            let color = Self::cgb_color(&self.bg_palette, cgb_palette, color_id as usize);
            self.framebuffer[offset + x] = color;
            // Store priority info for sprite rendering
            self.bg_priority[x] =
                (if color_id != 0 { 1 } else { 0 }) | (if bg_priority { 2 } else { 0 });
        }
    }

    fn render_window_scanline_cgb(&mut self, ly: usize, offset: usize) {
        if ly < self.wy as usize {
            return;
        }
        let wx = self.wx as i32 - 7;

        let tile_data_area = if self.lcdc & 0x10 != 0 {
            0x0000usize
        } else {
            0x0800
        };
        let tile_map_area = if self.lcdc & 0x40 != 0 {
            0x1C00usize
        } else {
            0x1800
        };
        let signed_addr = self.lcdc & 0x10 == 0;

        let win_y = self.window_line as usize;
        let tile_row = win_y / 8;
        let pixel_row = win_y % 8;

        let mut rendered = false;

        for x in 0..SCREEN_W {
            let win_x = x as i32 - wx;
            if win_x < 0 {
                continue;
            }
            rendered = true;

            let tile_col = win_x as usize / 8;
            let pixel_col = win_x as usize % 8;
            let map_idx = tile_row * 32 + tile_col;
            if map_idx >= 1024 {
                continue;
            }

            let tile_id = self.vram[tile_map_area + map_idx];
            let attrs = self.vram1[tile_map_area + map_idx];
            let cgb_palette = (attrs & 0x07) as usize;
            let tile_vram_bank = (attrs >> 3) & 1;
            let flip_x = attrs & 0x20 != 0;
            let flip_y = attrs & 0x40 != 0;
            let bg_priority = attrs & 0x80 != 0;

            let tile_addr = if signed_addr {
                let signed_id = tile_id as i8 as i32;
                (tile_data_area as i32 + (signed_id + 128) * 16) as usize
            } else {
                tile_data_area + tile_id as usize * 16
            };

            let actual_row = if flip_y { 7 - pixel_row } else { pixel_row };
            let row_addr = tile_addr + actual_row * 2;
            let vram_data = if tile_vram_bank == 1 {
                &self.vram1
            } else {
                &self.vram
            };
            if row_addr + 1 >= vram_data.len() {
                continue;
            }

            let lo = vram_data[row_addr];
            let hi = vram_data[row_addr + 1];
            let bit = if flip_x { pixel_col } else { 7 - pixel_col };
            let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);

            let color = Self::cgb_color(&self.bg_palette, cgb_palette, color_id as usize);
            self.framebuffer[offset + x] = color;
            self.bg_priority[x] =
                (if color_id != 0 { 1 } else { 0 }) | (if bg_priority { 2 } else { 0 });
        }

        if rendered {
            self.window_line += 1;
        }
    }

    fn render_sprite_scanline_cgb(&mut self, ly: usize, offset: usize) {
        let sprite_h = if self.lcdc & 0x04 != 0 { 16 } else { 8 };

        let mut sprites: [(u8, u8, u8, u8, usize); 10] = [(0, 0, 0, 0, 0); 10];
        let mut count = 0usize;

        for i in 0..40 {
            let sy = self.oam[i * 4] as i32 - 16;
            if ly as i32 >= sy && (ly as i32) < sy + sprite_h as i32 {
                if count < 10 {
                    sprites[count] = (
                        self.oam[i * 4],
                        self.oam[i * 4 + 1],
                        self.oam[i * 4 + 2],
                        self.oam[i * 4 + 3],
                        i,
                    );
                    count += 1;
                }
            }
        }

        // CGB: priority by OAM order (not X position)
        for i in (0..count).rev() {
            let (sy_raw, sx_raw, mut tile, flags, _) = sprites[i];
            let sy = sy_raw as i32 - 16;
            let sx = sx_raw as i32 - 8;
            let flip_x = flags & 0x20 != 0;
            let flip_y = flags & 0x40 != 0;
            let behind_bg = flags & 0x80 != 0;
            let cgb_palette = (flags & 0x07) as usize;
            let tile_vram_bank = (flags >> 3) & 1;

            let mut row = ly as i32 - sy;
            if flip_y {
                row = sprite_h as i32 - 1 - row;
            }

            if sprite_h == 16 {
                tile &= 0xFE;
                if row >= 8 {
                    tile += 1;
                    row -= 8;
                }
            }

            let tile_addr = tile as usize * 16 + row as usize * 2;
            let vram_data = if tile_vram_bank == 1 {
                &self.vram1
            } else {
                &self.vram
            };
            if tile_addr + 1 >= vram_data.len() {
                continue;
            }

            let lo = vram_data[tile_addr];
            let hi = vram_data[tile_addr + 1];

            for px in 0..8i32 {
                let screen_x = sx + px;
                if screen_x < 0 || screen_x >= SCREEN_W as i32 {
                    continue;
                }

                let bit = if flip_x { px } else { 7 - px } as u8;
                let color_id = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
                if color_id == 0 {
                    continue;
                } // Transparent

                let sx_idx = screen_x as usize;
                let screen_idx = offset + sx_idx;

                // CGB BG priority: if LCDC bit 0 set AND (BG-to-OAM priority OR sprite behind_bg)
                // AND BG color != 0, then BG wins
                if self.lcdc & 0x01 != 0 {
                    let bg_prio = self.bg_priority[sx_idx];
                    if (behind_bg || bg_prio & 2 != 0) && bg_prio & 1 != 0 {
                        continue;
                    }
                }

                let color = Self::cgb_color(&self.obj_palette, cgb_palette, color_id as usize);
                self.framebuffer[screen_idx] = color;
            }
        }
    }

    // VRAM read/write (bank-aware for CGB)
    pub fn read_vram(&self, addr: u16) -> u8 {
        let idx = (addr & 0x1FFF) as usize;
        if self.vram_bank == 1 {
            self.vram1[idx]
        } else {
            self.vram[idx]
        }
    }
    pub fn write_vram(&mut self, addr: u16, val: u8) {
        let idx = (addr & 0x1FFF) as usize;
        if self.vram_bank == 1 {
            self.vram1[idx] = val;
        } else {
            self.vram[idx] = val;
        }
    }
    /// Read VRAM from a specific bank (0 or 1)
    pub fn read_vram_bank(&self, addr: u16, bank: u8) -> u8 {
        let idx = (addr & 0x1FFF) as usize;
        if bank == 1 {
            self.vram1[idx]
        } else {
            self.vram[idx]
        }
    }

    // CGB color palette access
    pub fn read_bcpd(&self) -> u8 {
        let idx = (self.bcps & 0x3F) as usize;
        self.bg_palette[idx]
    }
    pub fn write_bcpd(&mut self, val: u8) {
        let idx = (self.bcps & 0x3F) as usize;
        self.bg_palette[idx] = val;
        if self.bcps & 0x80 != 0 {
            self.bcps = 0x80 | ((self.bcps + 1) & 0x3F);
        }
    }
    pub fn read_ocpd(&self) -> u8 {
        let idx = (self.ocps & 0x3F) as usize;
        self.obj_palette[idx]
    }
    pub fn write_ocpd(&mut self, val: u8) {
        let idx = (self.ocps & 0x3F) as usize;
        self.obj_palette[idx] = val;
        if self.ocps & 0x80 != 0 {
            self.ocps = 0x80 | ((self.ocps + 1) & 0x3F);
        }
    }

    /// Convert CGB RGB555 palette entry to ARGB8888
    fn cgb_color(palette_data: &[u8], palette_num: usize, color_idx: usize) -> u32 {
        let offset = palette_num * 8 + color_idx * 2;
        if offset + 1 >= palette_data.len() {
            return 0xFF000000;
        }
        let lo = palette_data[offset] as u16;
        let hi = palette_data[offset + 1] as u16;
        let rgb555 = lo | (hi << 8);
        let r5 = (rgb555 & 0x1F) as u8;
        let g5 = ((rgb555 >> 5) & 0x1F) as u8;
        let b5 = ((rgb555 >> 10) & 0x1F) as u8;
        // Convert 5-bit to 8-bit with proper CGB color correction
        let r = (r5 << 3) | (r5 >> 2);
        let g = (g5 << 3) | (g5 >> 2);
        let b = (b5 << 3) | (b5 >> 2);
        0xFF000000 | (r as u32) << 16 | (g as u32) << 8 | b as u32
    }

    // OAM read/write
    pub fn read_oam(&self, addr: u16) -> u8 {
        let idx = (addr - 0xFE00) as usize;
        if idx < 160 { self.oam[idx] } else { 0xFF }
    }
    pub fn write_oam(&mut self, addr: u16, val: u8) {
        let idx = (addr - 0xFE00) as usize;
        if idx < 160 {
            self.oam[idx] = val;
        }
    }
}

impl Default for Gpu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{DOTS_PER_FRAME, Gpu};

    #[test]
    fn a_full_frame_returns_to_line_zero_mode_two() {
        let mut gpu = Gpu::new();

        gpu.step_dots(DOTS_PER_FRAME);

        assert_eq!(gpu.ly, 0);
        assert_eq!(gpu.mode, 2);
        assert_eq!(gpu.cycles, 0);
        assert!(gpu.frame_ready);
        assert!(gpu.vblank_irq);
    }

    #[test]
    fn mode_boundaries_use_classic_scanline_lengths() {
        let mut gpu = Gpu::new();

        gpu.step_dots(79);
        assert_eq!(gpu.mode, 2);
        gpu.step_dots(1);
        assert_eq!(gpu.mode, 3);

        gpu.step_dots(172);
        assert_eq!(gpu.mode, 0);
        gpu.step_dots(204);
        assert_eq!(gpu.mode, 2);
        assert_eq!(gpu.ly, 1);
    }

    #[test]
    fn one_call_can_cross_multiple_mode_boundaries() {
        let mut gpu = Gpu::new();

        gpu.step_dots(456 + 80);

        assert_eq!(gpu.ly, 1);
        assert_eq!(gpu.mode, 3);
        assert_eq!(gpu.cycles, 0);
    }

    #[test]
    fn stat_sources_share_one_rising_edge_line() {
        let mut gpu = Gpu::new();
        gpu.write_lyc(99);
        gpu.write_stat(0x28); // Mode 2 and Mode 0 sources.
        assert!(gpu.stat_irq);

        gpu.stat_irq = false;
        gpu.step_dots(80 + 172);
        assert_eq!(gpu.mode, 0);
        assert!(gpu.stat_irq);

        gpu.stat_irq = false;
        gpu.step_dots(204);
        assert_eq!(gpu.mode, 2);
        assert!(!gpu.stat_irq, "mode 0 -> 2 keeps the shared line high");
    }

    #[test]
    fn lyc_write_updates_coincidence_and_interrupt_immediately() {
        let mut gpu = Gpu::new();
        gpu.write_lyc(1);
        gpu.write_stat(0x40);
        gpu.stat_irq = false;

        gpu.write_lyc(0);

        assert_ne!(gpu.read_stat() & 0x04, 0);
        assert!(gpu.stat_irq);
    }

    #[test]
    fn disabling_lcd_resets_visible_timing_state() {
        let mut gpu = Gpu::new();
        gpu.step_dots(500);

        gpu.write_lcdc(gpu.lcdc & !0x80);

        assert_eq!(gpu.ly, 0);
        assert_eq!(gpu.read_stat() & 0x03, 0);
        assert_eq!(gpu.cycles, 0);
    }
}
