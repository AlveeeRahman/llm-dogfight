//! Spiral galaxy via the density-wave trick: every star follows an ellipse whose
//! orientation rotates with radius. Where neighbouring ellipses crowd together you
//! get spiral arms — no N-body needed, O(n) per frame, and the arms persist as the
//! stars move through them (as in real galaxies).
use super::{Scene, Starfield};
use crate::canvas::Canvas;
use crate::math::{fbm, Rgb};
use crate::rng::Rng;
use std::f32::consts::TAU;

struct Star {
    a: f32,     // semi-major axis (0..1 of galaxy radius)
    e: f32,     // eccentricity
    tilt: f32,  // ellipse orientation
    theta: f32, // orbital phase
    omega: f32, // angular speed
    z: f32,     // thickness offset
    col: Rgb,
    dust: bool,
}

struct Nova {
    star: usize,
    age: f32,
}

pub struct Galaxy {
    rng: Rng,
    w: usize,
    h: usize,
    t: f32,
    stars: Vec<Star>,
    hdr: Vec<Rgb>,
    dust: Vec<f32>,
    nebula: Vec<Rgb>,
    bg: Starfield,
    novas: Vec<Nova>,
    comet: Option<(f32, f32, f32, f32, f32)>,
}

impl Galaxy {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let bg = Starfield::new(&mut rng, 1, 1, 0.0, 1.0);
        Galaxy { rng, w: 1, h: 1, t: 0.0, stars: vec![], hdr: vec![], dust: vec![], nebula: vec![], bg, novas: vec![], comet: None }
    }

    fn build_stars(&mut self) {
        let n = ((self.w * self.h) as f32 * 1.6).clamp(7000.0, 26000.0) as usize;
        let twist = 6.2;
        let rng = &mut self.rng;
        self.stars = (0..n)
            .map(|i| {
                let bulge = rng.chance(0.12);
                let a = if bulge { rng.f32().powf(1.5) * 0.16 } else { (0.06 + rng.gauss().abs() * 0.4).min(1.1) };
                // axis ratio b/a ~0.72 in the disk: strong enough ellipses for arms to form
                let ratio = if bulge { rng.range(0.85, 1.0) } else { 0.72 + rng.range(-0.06, 0.06) };
                let e = (1.0 - ratio * ratio).sqrt();
                let omega = 0.035 / (a + 0.12);
                let dust = !bulge && i % 5 == 0 && a > 0.1 && a < 0.8;
                let col = if bulge {
                    Rgb::hex(0xffdcae).lerp(Rgb::hex(0xffc27f), rng.f32())
                } else {
                    let r = rng.f32();
                    if r < 0.34 { Rgb::hex(0x8fb8ff) } else if r < 0.4 { Rgb::hex(0xff86b4) } else if r < 0.8 { Rgb::hex(0xe8ecff) } else { Rgb::hex(0xffe3bd) }
                };
                // dust sits slightly ahead of the arm (the inner edge), like real dust lanes
                let tilt = a * twist + if dust { 0.28 } else { 0.0 } + rng.gauss() * 0.07;
                Star { a, e, tilt, theta: rng.range(0.0, TAU), omega, z: rng.gauss() * 0.015 * (1.0 - a).max(0.25), col, dust }
            })
            .collect();
    }
}

impl Scene for Galaxy {
    fn resize(&mut self, w: usize, h: usize) {
        self.w = w;
        self.h = h;
        self.hdr = vec![Rgb::BLACK; w * h];
        self.dust = vec![0.0; w * h];
        self.bg = Starfield::new(&mut self.rng, w, h, 0.035, h as f32);
        // faint nebular background
        let seed = self.rng.next_u64() as u32;
        self.nebula = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f32, (i / w) as f32);
                let n = fbm(x * 0.03, y * 0.05, seed, 4);
                let m = fbm(x * 0.02 + 40.0, y * 0.04, seed ^ 7, 3);
                let k = ((n - 0.52) * 3.0).clamp(0.0, 1.0);
                Rgb::hex(0x1b0f33).lerp(Rgb::hex(0x0b2138), m).scale(k * 0.9)
            })
            .collect();
        if self.stars.is_empty() {
            self.build_stars();
        }
    }

    fn update(&mut self, dt: f32) {
        self.t += dt;
        for s in self.stars.iter_mut() {
            s.theta += s.omega * dt;
        }
        for n in self.novas.iter_mut() {
            n.age += dt;
        }
        self.novas.retain(|n| n.age < 6.0);
        if self.rng.chance(dt / 9.0) && !self.stars.is_empty() {
            let star = self.rng.below(self.stars.len());
            if self.stars[star].a > 0.2 {
                self.novas.push(Nova { star, age: 0.0 });
            }
        }
        match &mut self.comet {
            Some(c) => {
                c.0 += c.2 * dt;
                c.1 += c.3 * dt;
                c.4 -= dt;
                if c.4 <= 0.0 {
                    self.comet = None;
                }
            }
            None => {
                if self.rng.chance(dt / 25.0) {
                    let y = self.rng.range(0.05, 0.4) * self.h as f32;
                    let sp = self.w as f32 * 0.25;
                    self.comet = Some((-5.0, y, sp, sp * 0.18, 6.0));
                }
            }
        }
    }

    fn render(&mut self, cv: &mut Canvas) {
        cv.clear_overlays();
        let (w, h) = (self.w, self.h);
        let t = self.t;
        self.hdr.iter_mut().for_each(|p| *p = Rgb::BLACK);
        self.dust.iter_mut().for_each(|d| *d = 0.0);
        let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
        let radius = (w as f32 * 0.42).min(h as f32 * 0.62);
        let incl = 0.62 + 0.14 * (t * 0.02).sin(); // camera inclination (radians)
        let (ci, si) = (incl.cos(), incl.sin());
        let spin = t * 0.004;
        let area = (w * h) as f32;
        let gain = (area / self.stars.len() as f32).clamp(0.3, 3.0) * 1.1;
        let hdr = &mut self.hdr;
        let dust = &mut self.dust;
        let put = |x: f32, y: f32, c: Rgb, hdr: &mut Vec<Rgb>| {
            let (x, y) = (x - 0.5, y - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let (x0, y0) = (x0 as i32, y0 as i32);
            for (dx, dy, k) in [(0, 0, (1.0 - fx) * (1.0 - fy)), (1, 0, fx * (1.0 - fy)), (0, 1, (1.0 - fx) * fy), (1, 1, fx * fy)] {
                let (px, py) = (x0 + dx, y0 + dy);
                if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                    let i = py as usize * w + px as usize;
                    hdr[i] = hdr[i] + c * k;
                }
            }
        };
        for s in self.stars.iter() {
            let b = s.a * (1.0 - s.e * s.e).sqrt();
            let (ex, ey) = (s.a * s.theta.cos(), b * s.theta.sin());
            let rot = s.tilt + spin;
            let (sr, cr) = rot.sin_cos();
            let (gx, gy) = (ex * cr - ey * sr, ex * sr + ey * cr);
            let sx = cx + gx * radius;
            let sy = cy + (gy * ci + s.z * si) * radius;
            if s.dust {
                let (px, py) = (sx as i32, sy as i32);
                if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                    dust[py as usize * w + px as usize] += 0.3 * gain;
                }
            } else {
                let core = (1.0 - s.a * 4.0).max(0.0);
                put(sx, sy, s.col * (0.16 * gain * (1.0 + core * 0.6)), hdr);
            }
        }
        // soft core glow
        let core_r = radius * 0.11;
        for y in (cy - core_r) as i32..=(cy + core_r) as i32 {
            for x in (cx - core_r * 1.2) as i32..=(cx + core_r * 1.2) as i32 {
                if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                    continue;
                }
                let dx = (x as f32 - cx) / (core_r * 1.2);
                let dy = (y as f32 - cy) / (core_r * ci.max(0.3));
                let d2 = dx * dx + dy * dy;
                if d2 < 1.0 {
                    let i = y as usize * w + x as usize;
                    hdr[i] = hdr[i] + Rgb::hex(0xffd9a0) * (0.28 * (1.0 - d2).powi(3));
                }
            }
        }
        // supernovae
        for n in &self.novas {
            let s = &self.stars[n.star];
            let b = s.a * (1.0 - s.e * s.e).sqrt();
            let rot = s.tilt + spin;
            let (sr, cr) = rot.sin_cos();
            let (ex, ey) = (s.a * s.theta.cos(), b * s.theta.sin());
            let sx = cx + (ex * cr - ey * sr) * radius;
            let sy = cy + ((ex * sr + ey * cr) * ci) * radius;
            let k = if n.age < 0.4 { n.age / 0.4 } else { (1.0 - (n.age - 0.4) / 5.6).max(0.0).powi(2) };
            let r = 1.5 + n.age * 0.8;
            for y in (sy - r) as i32..=(sy + r) as i32 {
                for x in (sx - r) as i32..=(sx + r) as i32 {
                    if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                        continue;
                    }
                    let d = (((x as f32 - sx).powi(2) + (y as f32 - sy).powi(2)).sqrt() / r).min(1.0);
                    let i = y as usize * w + x as usize;
                    hdr[i] = hdr[i] + Rgb::hex(0xcfe3ff) * (1.4 * k * (1.0 - d).powi(2));
                }
            }
        }
        // soften: blend each pixel with its 3x3 neighbourhood so arms read as glow, not speckle
        let src = hdr.clone();
        let dsrc = dust.clone();
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                let mut acc = Rgb::BLACK;
                let mut dacc = 0.0;
                for dy in 0..3 {
                    for dx in 0..3 {
                        let k = if dx == 1 && dy == 1 { 4.0 } else if dx == 1 || dy == 1 { 2.0 } else { 1.0 };
                        acc = acc + src[(y + dy - 1) * w + x + dx - 1] * k;
                        dacc += dsrc[(y + dy - 1) * w + x + dx - 1] * k;
                    }
                }
                let i = y * w + x;
                hdr[i] = src[i] * 0.35 + acc * (0.65 / 16.0);
                dust[i] = dacc / 16.0;
            }
        }
        // compose: nebula + (stars attenuated by dust) -> tonemap
        for i in 0..w * h {
            let absorb = (-dust[i] * 2.2).exp();
            let c = hdr[i] * absorb + self.nebula[i];
            cv.px[i] = Rgb::new(1.0 - (-c.r * 1.25).exp(), 1.0 - (-c.g * 1.25).exp(), 1.0 - (-c.b * 1.25).exp());
        }
        // cheap bloom: 3x3 box of bright pixels, added back
        let mut bloom = vec![Rgb::BLACK; w * h];
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                let mut acc = Rgb::BLACK;
                for dy in 0..3 {
                    for dx in 0..3 {
                        let p = cv.px[(y + dy - 1) * w + x + dx - 1];
                        let l = p.luma();
                        if l > 0.55 {
                            acc = acc + p * (l - 0.55);
                        }
                    }
                }
                bloom[y * w + x] = acc * 0.22;
            }
        }
        for i in 0..w * h {
            cv.px[i] = cv.px[i] + bloom[i];
        }
        self.bg.draw(cv, t, 0.9);
        if let Some((x, y, vx, vy, life)) = self.comet {
            let k = (life / 6.0).min(1.0);
            let n = (w as f32 * 0.08) as i32;
            for j in 0..n {
                let f = j as f32 / n as f32;
                let px = x - vx * f * 0.5;
                let py = y - vy * f * 0.5;
                cv.splat_add(px, py, Rgb::hex(0xbfe8ff) * (0.9 * (1.0 - f).powi(2) * k));
            }
        }
    }
}
