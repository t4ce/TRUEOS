extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone)]
pub struct WebGlTextureImage {
    pub width: i32,
    pub height: i32,
    pub format: u32,
    pub ty: u32,
    pub data: Vec<u8>,
}

#[derive(Clone)]
pub struct WebGlTextureState {
    pub active_unit: u32,
    pub unpack_alignment: i32,
    pub bound_tex2d_by_unit: BTreeMap<u32, u32>,
    pub params: BTreeMap<(u32, u32), i32>,
    pub images: BTreeMap<u32, WebGlTextureImage>,
}

impl Default for WebGlTextureState {
    fn default() -> Self {
        Self {
            active_unit: 0,
            unpack_alignment: 4,
            bound_tex2d_by_unit: BTreeMap::new(),
            params: BTreeMap::new(),
            images: BTreeMap::new(),
        }
    }
}

impl WebGlTextureState {
    pub fn active_texture(&mut self, tex_enum: u32) {
        const TEXTURE0: u32 = 0x84C0;
        self.active_unit = tex_enum.saturating_sub(TEXTURE0);
    }

    pub fn bind_texture_2d(&mut self, tex_id: u32) {
        self.bound_tex2d_by_unit.insert(self.active_unit, tex_id);
    }

    pub fn pixel_store_i(&mut self, pname: u32, param: i32) {
        // UNPACK_ALIGNMENT
        if pname == 0x0CF5 && (param == 1 || param == 2 || param == 4 || param == 8) {
            self.unpack_alignment = param;
        }
    }

    pub fn tex_parameter_i(&mut self, pname: u32, param: i32) {
        if let Some(tex_id) = self.current_tex2d() {
            self.params.insert((tex_id, pname), param);
        }
    }

    fn current_tex2d(&self) -> Option<u32> {
        self.bound_tex2d_by_unit.get(&self.active_unit).copied()
    }

    fn bytes_per_pixel(format: u32, ty: u32) -> Option<usize> {
        // UNSIGNED_BYTE
        if ty != 0x1401 {
            return None;
        }
        match format {
            0x1908 => Some(4), // RGBA
            0x1907 => Some(3), // RGB
            _ => None,
        }
    }

    fn unpack_row_stride(width: usize, bpp: usize, align: usize) -> usize {
        let tight = width.saturating_mul(bpp);
        let mask = align.saturating_sub(1);
        (tight + mask) & !mask
    }

    pub fn tex_image_2d(
        &mut self,
        level: i32,
        width: i32,
        height: i32,
        border: i32,
        format: u32,
        ty: u32,
        data_opt: Option<&[u8]>,
    ) -> bool {
        if level != 0 || border != 0 || width <= 0 || height <= 0 {
            return false;
        }
        let Some(tex_id) = self.current_tex2d() else {
            return false;
        };
        let Some(bpp) = Self::bytes_per_pixel(format, ty) else {
            return false;
        };

        let w = width as usize;
        let h = height as usize;
        let tight_len = w.saturating_mul(h).saturating_mul(bpp);
        let mut out = vec![0u8; tight_len];

        if let Some(src) = data_opt {
            let align = self.unpack_alignment.max(1) as usize;
            let row_stride = Self::unpack_row_stride(w, bpp, align);
            let needed = row_stride.saturating_mul(h);
            if src.len() < needed {
                return false;
            }
            for y in 0..h {
                let src_off = y.saturating_mul(row_stride);
                let dst_off = y.saturating_mul(w).saturating_mul(bpp);
                let row_len = w.saturating_mul(bpp);
                out[dst_off..dst_off + row_len].copy_from_slice(&src[src_off..src_off + row_len]);
            }
        }

        self.images.insert(
            tex_id,
            WebGlTextureImage {
                width,
                height,
                format,
                ty,
                data: out,
            },
        );
        true
    }

    pub fn tex_sub_image_2d(
        &mut self,
        level: i32,
        xoffset: i32,
        yoffset: i32,
        width: i32,
        height: i32,
        format: u32,
        ty: u32,
        src: &[u8],
    ) -> bool {
        if level != 0 || width <= 0 || height <= 0 || xoffset < 0 || yoffset < 0 {
            return false;
        }
        let Some(tex_id) = self.current_tex2d() else {
            return false;
        };
        let Some(img) = self.images.get_mut(&tex_id) else {
            return false;
        };
        if img.format != format || img.ty != ty {
            return false;
        }
        let Some(bpp) = Self::bytes_per_pixel(format, ty) else {
            return false;
        };

        let w = width as usize;
        let h = height as usize;
        let xo = xoffset as usize;
        let yo = yoffset as usize;
        let tw = img.width.max(0) as usize;
        let th = img.height.max(0) as usize;
        if xo.saturating_add(w) > tw || yo.saturating_add(h) > th {
            return false;
        }

        let align = self.unpack_alignment.max(1) as usize;
        let row_stride = Self::unpack_row_stride(w, bpp, align);
        let needed = row_stride.saturating_mul(h);
        if src.len() < needed {
            return false;
        }

        for y in 0..h {
            let src_off = y.saturating_mul(row_stride);
            let dst_off = ((yo + y).saturating_mul(tw).saturating_add(xo)).saturating_mul(bpp);
            let row_len = w.saturating_mul(bpp);
            img.data[dst_off..dst_off + row_len].copy_from_slice(&src[src_off..src_off + row_len]);
        }
        true
    }
}
