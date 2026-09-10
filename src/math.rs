//! Colour + noise helpers. Colours are sRGB floats in 0..1 (values >1 are allowed
//! for additive light and clamped at encode time).
use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb { r: 0.0, g: 0.0, b: 0.0 };
    pub const WHITE: Rgb = Rgb { r: 1.0, g: 1.0, b: 1.0 };
    #[inline]
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Rgb { r, g, b }
    }
    pub const fn hex(h: u32) -> Self {
        Rgb {
            r: ((h >> 16) & 255) as f32 / 255.0,
            g: ((h >> 8) & 255) as f32 / 255.0,
            b: (h & 255) as f32 / 255.0,
        }
    }
    #[inline]
    pub fn lerp(self, o: Rgb, t: f32) -> Rgb {
        Rgb::new(self.r + (o.r - self.r) * t, self.g + (o.g - self.g) * t, self.b + (o.b - self.b) * t)
    }
    #[inline]
    pub fn scale(self, k: f32) -> Rgb {
        Rgb::new(self.r * k, self.g * k, self.b * k)
    }
    #[inline]
    pub fn luma(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }
    #[inline]
    pub fn to_u8(self) -> [u8; 3] {
        [q(self.r), q(self.g), q(self.b)]
    }
}
#[inline]
fn q(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}
impl Add for Rgb {
    type Output = Rgb;
    #[inline]
    fn add(self, o: Rgb) -> Rgb {
        Rgb::new(self.r + o.r, self.g + o.g, self.b + o.b)
    }
}
impl Sub for Rgb {
    type Output = Rgb;
    #[inline]
    fn sub(self, o: Rgb) -> Rgb {
        Rgb::new(self.r - o.r, self.g - o.g, self.b - o.b)
    }
}
impl Mul<f32> for Rgb {
    type Output = Rgb;
    #[inline]
    fn mul(self, k: f32) -> Rgb {
        self.scale(k)
    }
}

#[inline]
pub fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
pub fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_216.0
}

/// Smooth 2-D value noise in 0..1.
pub fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (xf, yf) = (x - xi, y - yi);
    let (xi, yi) = (xi as i32, yi as i32);
    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let a = hash2(xi, yi, seed);
    let b = hash2(xi + 1, yi, seed);
    let c = hash2(xi, yi + 1, seed);
    let d = hash2(xi + 1, yi + 1, seed);
    lerp(lerp(a, b, u), lerp(c, d, u), v)
}

/// Piecewise-linear gradient over sorted keys in 0..1.
pub fn gradient(keys: &[(f32, Rgb)], t: f32) -> Rgb {
    if t <= keys[0].0 {
        return keys[0].1;
    }
    for w in keys.windows(2) {
        if t <= w[1].0 {
            let k = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
            return w[0].1.lerp(w[1].1, k);
        }
    }
    keys[keys.len() - 1].1
}
