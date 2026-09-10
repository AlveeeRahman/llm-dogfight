//! The frame being drawn. Three layers, composited per terminal cell by the encoder:
//!
//! 1. `px`     — RGB pixels at (cols x rows*2). Each cell shows two pixels via '▀'.
//! 2. `dots`   — braille sub-pixels at (cols*2 x rows*4), one colour per cell. Used for
//!    fine detail (stars, sparks) that would be mush at half-block resolution.
//! 3. `glyphs` — whole-cell characters (text, HUD). Highest priority.
//!
//! All primitives take float coordinates and anti-alias, so motion is smooth even on
//! an 80x24 terminal (80x48 pixels).
use crate::math::{clamp01, Rgb};

pub struct Canvas {
    pub cols: usize,
    pub rows: usize,
    pub w: usize,
    pub h: usize,
    pub px: Vec<Rgb>,
    pub glyphs: Vec<(char, Rgb)>,
    pub dots: Vec<u8>,
    pub dot_col: Vec<Rgb>,
}

const BRAILLE_BIT: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

impl Canvas {
    pub fn new(cols: usize, rows: usize) -> Self {
        let mut c = Canvas { cols: 0, rows: 0, w: 0, h: 0, px: vec![], glyphs: vec![], dots: vec![], dot_col: vec![] };
        c.resize(cols, rows);
        c
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
        self.w = self.cols;
        self.h = self.rows * 2;
        self.px = vec![Rgb::BLACK; self.w * self.h];
        self.glyphs = vec![('\0', Rgb::BLACK); self.cols * self.rows];
        self.dots = vec![0; self.cols * self.rows];
        self.dot_col = vec![Rgb::BLACK; self.cols * self.rows];
    }

    /// Clears glyph + dot layers (pixels are normally fully repainted by the scene).
    pub fn clear_overlays(&mut self) {
        for g in self.glyphs.iter_mut() {
            g.0 = '\0';
        }
        self.dots.iter_mut().for_each(|d| *d = 0);
    }

    #[inline]
    pub fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h
    }
    #[inline]
    pub fn get(&self, x: i32, y: i32) -> Rgb {
        if self.inside(x, y) {
            self.px[y as usize * self.w + x as usize]
        } else {
            Rgb::BLACK
        }
    }
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if self.inside(x, y) {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        if a <= 0.0 || !self.inside(x, y) {
            return;
        }
        let i = y as usize * self.w + x as usize;
        self.px[i] = self.px[i].lerp(c, a.min(1.0));
    }
    #[inline]
    pub fn add(&mut self, x: i32, y: i32, c: Rgb) {
        if self.inside(x, y) {
            let i = y as usize * self.w + x as usize;
            self.px[i] = self.px[i] + c;
        }
    }

    /// Bilinear blend of a point — sub-pixel positioned.
    pub fn splat(&mut self, x: f32, y: f32, c: Rgb, a: f32) {
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        self.blend(x0, y0, c, a * (1.0 - fx) * (1.0 - fy));
        self.blend(x0 + 1, y0, c, a * fx * (1.0 - fy));
        self.blend(x0, y0 + 1, c, a * (1.0 - fx) * fy);
        self.blend(x0 + 1, y0 + 1, c, a * fx * fy);
    }

    pub fn splat_add(&mut self, x: f32, y: f32, c: Rgb) {
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        self.add(x0, y0, c * ((1.0 - fx) * (1.0 - fy)));
        self.add(x0 + 1, y0, c * (fx * (1.0 - fy)));
        self.add(x0, y0 + 1, c * ((1.0 - fx) * fy));
        self.add(x0 + 1, y0 + 1, c * (fx * fy));
    }

    /// Additive radial light with quadratic falloff.
    pub fn glow(&mut self, x: f32, y: f32, radius: f32, c: Rgb) {
        let r = radius.max(0.5);
        let (x0, x1) = ((x - r).floor() as i32, (x + r).ceil() as i32);
        let (y0, y1) = ((y - r).floor() as i32, (y + r).ceil() as i32);
        for py in y0..=y1 {
            for px in x0..=x1 {
                let dx = px as f32 + 0.5 - x;
                let dy = py as f32 + 0.5 - y;
                let d = (dx * dx + dy * dy).sqrt() / r;
                if d < 1.0 {
                    let k = (1.0 - d) * (1.0 - d);
                    self.add(px, py, c * k);
                }
            }
        }
    }

    /// Anti-aliased filled disc.
    pub fn disc(&mut self, x: f32, y: f32, r: f32, c: Rgb, a: f32) {
        self.ellipse(x, y, r, r, 0.0, c, a);
    }

    /// Anti-aliased filled ellipse, rotated by `rot` radians.
    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, rot: f32, c: Rgb, a: f32) {
        let (rx, ry) = (rx.max(0.3), ry.max(0.3));
        let ext = rx.max(ry) + 1.0;
        let (s, co) = rot.sin_cos();
        for py in (cy - ext).floor() as i32..=(cy + ext).ceil() as i32 {
            for px in (cx - ext).floor() as i32..=(cx + ext).ceil() as i32 {
                let dx = px as f32 + 0.5 - cx;
                let dy = py as f32 + 0.5 - cy;
                let lx = dx * co + dy * s;
                let ly = -dx * s + dy * co;
                let q = ((lx / rx).powi(2) + (ly / ry).powi(2)).sqrt();
                // approx distance to edge in pixels
                let edge = (1.0 - q) * rx.min(ry);
                let cov = clamp01(edge + 0.5);
                if cov > 0.0 {
                    self.blend(px, py, c, a * cov);
                }
            }
        }
    }

    /// Additive anti-aliased line (bolts, trails).
    pub fn line_add(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb) {
        let len = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        let n = (len * 1.2).ceil().max(1.0) as i32;
        let k = 1.0 / 1.2f32.max(1.0);
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.splat_add(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t, c * k);
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb, a: f32) {
        for py in y.max(0)..(y + h).min(self.h as i32) {
            for px in x.max(0)..(x + w).min(self.w as i32) {
                self.blend(px, py, c, a);
            }
        }
    }

    /// Whole-cell text overlay (col/row in cells).
    pub fn text(&mut self, col: i32, row: i32, s: &str, c: Rgb) {
        for (i, ch) in s.chars().enumerate() {
            self.glyph(col + i as i32, row, ch, c);
        }
    }

    #[inline]
    pub fn glyph(&mut self, col: i32, row: i32, ch: char, c: Rgb) {
        if col >= 0 && row >= 0 && (col as usize) < self.cols && (row as usize) < self.rows {
            self.glyphs[row as usize * self.cols + col as usize] = (ch, c);
        }
    }

    /// Braille sub-pixel. `sx` in 0..cols*2, `sy` in 0..rows*4. Colours in one cell mix.
    pub fn dot(&mut self, sx: i32, sy: i32, c: Rgb) {
        if sx < 0 || sy < 0 {
            return;
        }
        let (col, row) = (sx as usize / 2, sy as usize / 4);
        if col >= self.cols || row >= self.rows {
            return;
        }
        let i = row * self.cols + col;
        let bit = BRAILLE_BIT[sx as usize % 2][sy as usize % 4];
        if self.dots[i] == 0 {
            self.dot_col[i] = c;
        } else {
            // keep the brighter colour so a faint dot never dims a bright one
            if c.luma() > self.dot_col[i].luma() {
                self.dot_col[i] = c;
            }
        }
        self.dots[i] |= bit;
    }

    /// Dot at pixel-space coordinates (x in 0..w, y in 0..h).
    pub fn dot_px(&mut self, x: f32, y: f32, c: Rgb) {
        self.dot((x * 2.0) as i32, (y * 2.0) as i32, c);
    }
}
