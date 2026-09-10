//! A painterly hillside in the spirit of hand-painted animation backgrounds:
//! towering cumulus, layered mountains with aerial perspective, wind rolling
//! through the grass, a slow day -> sunset -> night -> dawn cycle.
//! The character is original pixel art (a girl in a straw hat with a small
//! moss sprite) — or any image you imported, rigged procedurally so it breathes,
//! sways in the wind and blinks.
use super::{Cloud, Scene, Starfield};
use crate::canvas::Canvas;
use crate::math::{fbm, gradient, smoothstep, vnoise, Rgb};
use crate::rng::Rng;
use crate::sprite::Sprite;
use std::f32::consts::TAU;

// ---- original built-in character (19 x 28). Groups: hair sways, hem sways, ribbon flutters, eyes blink.
pub const GIRL: [&str; 28] = [
    "......hHHHHHh......",
    ".....hHHHHHHHh.....",
    ".....HHHHHHHHH.....",
    "....RRRRRRRRRRRr...",
    "..hHHHHHHHHHHHHHh..",
    ".hHHHHHHHHHHHHHHHh.",
    "...hhKKKKKKKKKhhRr.",
    "....KKSSSSSSSKK.rR.",
    "...KKSSSSSSSSSKK.r.",
    "...KKSEESSSEESKK...",
    "...KKSSSSSSSSSKKk..",
    "...KKCSSSsSSSCKKKk.",
    "..kKKKSSSSSSSKKKKk.",
    "..kKKKKSSSSSKKKKKk.",
    "...kKKKKsSSKKKKKk..",
    "....kKKWWSWWKKKk...",
    ".....SWWWRWWWS.....",
    "....SSWWWWWWWSS....",
    "....SWWWWWWWWWS....",
    "....SwWWWWWWWwS....",
    ".....wWWWWWWWw.....",
    "....wWWWWWWWWWw....",
    "...wWWWWWWWWWWWw...",
    "..wwwWWWWWWWWWwww..",
    "..wwwwwwwwwwwwwww..",
    ".......LL.LL.......",
    ".......LL.LL.......",
    "......BBB.BBB......",
];

pub const SPRITE_PAL: &[(char, u32)] = &[
    ('H', 0xf2d58b),
    ('h', 0xc9a45c),
    ('R', 0xd8453d),
    ('r', 0xa8302c),
    ('K', 0x3b2a26),
    ('k', 0x5a4038),
    ('S', 0xf7d7bf),
    ('s', 0xe0b49a),
    ('E', 0x2a1f1f),
    ('C', 0xf3a39a),
    ('W', 0xfbfbf6),
    ('w', 0xc9d6e6),
    ('L', 0xf0cdb5),
    ('B', 0x7a4b32),
    ('M', 0x7fbf6a),
    ('G', 0x4f9e45),
    ('g', 0x8fd16f),
    ('e', 0x1d2a1d),
];

const MOSS: [&str; 5] = ["..g..", ".gG..", ".MMM.", "MeMeM", ".MMM."];

fn pal(c: char) -> Option<Rgb> {
    SPRITE_PAL.iter().find(|p| p.0 == c).map(|p| Rgb::hex(p.1))
}

struct Seed {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    col: Rgb,
}

pub struct Meadow {
    rng: Rng,
    w: f32,
    h: f32,
    t: f32,
    cycle: f32,
    clouds: Vec<Cloud>,
    stars: Starfield,
    seeds: Vec<Seed>,
    flowers: Vec<(i32, i32, Rgb)>,
    far: Vec<f32>,
    near: Vec<f32>,
    mid: Vec<f32>,
    sprite: Option<Sprite>,
    blink: f32,
    next_blink: f32,
    hop: f32,
    birds: Option<(f32, f32, f32)>,
    fireflies: Vec<(f32, f32, f32)>,
    phase_offset: f32,
}

impl Meadow {
    pub fn new(seed: u64, cycle: f32, character: &str) -> Self {
        let mut rng = Rng::new(seed);
        let stars = Starfield::new(&mut rng, 1, 1, 0.0, 1.0);
        let sprite = if character == "builtin" || character.is_empty() { None } else { Sprite::load(character) };
        let phase_offset = std::env::var("REVERIE_PHASE").ok().and_then(|v| v.parse().ok()).unwrap_or(0.08);
        Meadow {
            rng,
            w: 1.0,
            h: 1.0,
            t: 0.0,
            cycle: cycle.max(20.0),
            clouds: vec![],
            stars,
            seeds: vec![],
            flowers: vec![],
            far: vec![],
            near: vec![],
            mid: vec![],
            sprite,
            blink: 0.0,
            next_blink: 3.0,
            hop: 0.0,
            birds: None,
            fireflies: vec![],
            phase_offset,
        }
    }

    fn phase(&self) -> f32 {
        (self.t / self.cycle + self.phase_offset).fract()
    }

    fn mound(&self, x: f32) -> f32 {
        let hx = self.w * 0.66;
        self.h * 0.86 - self.h * 0.13 * (-((x - hx) / (self.w * 0.3)).powi(2)).exp()
    }

    fn wind(&self, x: f32) -> f32 {
        0.6 + 0.4 * (self.t * 0.5 + x * 0.01).sin() * vnoise(self.t * 0.2, 3.0, 17)
    }

    /// Built-in pixel art with per-group wind offsets.
    fn draw_girl(&self, cv: &mut Canvas, fx: f32, fy: f32, sc: i32, light: Rgb) {
        let rows = GIRL.len() as i32;
        let cols = GIRL[0].len() as i32;
        let wind = self.wind(fx);
        let gust = (self.t * 2.3).sin() * 0.35 + wind;
        let ox = fx as i32 - cols * sc / 2;
        let oy = fy as i32 - rows * sc;
        let blinking = self.blink > 0.0;
        for (r, line) in GIRL.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                let Some(mut col) = pal(ch) else { continue };
                let (r, c) = (r as i32, c as i32);
                let mut dx = 0.0f32;
                let mut dy = 0.0f32;
                match ch {
                    'K' | 'k' if r >= 9 => dx = gust * ((r - 8) as f32 / 7.0) * 1.6 * if c > cols / 2 { 1.0 } else { 0.6 },
                    'w' | 'W' if r >= 21 => dx = gust * ((r - 20) as f32 / 4.0) * 1.3,
                    'R' | 'r' if c >= 15 => {
                        dx = gust * 1.2;
                        dy = (self.t * 9.0 + r as f32).sin() * 0.6;
                    }
                    'E' if blinking => col = pal('s').unwrap(),
                    _ => {}
                }
                let col = col.mul(light);
                let px = ox + c * sc + (dx * sc as f32).round() as i32;
                let py = oy + r * sc + (dy * sc as f32).round() as i32;
                for yy in 0..sc {
                    for xx in 0..sc {
                        cv.set(px + xx, py + yy, col);
                    }
                }
            }
        }
    }

    fn draw_moss(&self, cv: &mut Canvas, fx: f32, fy: f32, sc: i32, light: Rgb) {
        let hopy = (self.hop.max(0.0) * 3.14).sin() * 4.0 * sc as f32;
        let bob = ((self.t * 3.0).sin() * 0.5) as i32;
        let ox = fx as i32 - 2 * sc;
        let oy = fy as i32 - 5 * sc - hopy as i32 + bob;
        for (r, line) in MOSS.iter().enumerate() {
            for (c, ch) in line.chars().enumerate() {
                let Some(col) = pal(ch) else { continue };
                let col = if ch == 'e' && self.blink > 0.0 { pal('M').unwrap() } else { col };
                for yy in 0..sc {
                    for xx in 0..sc {
                        cv.set(ox + c as i32 * sc + xx, oy + r as i32 * sc + yy, col.mul(light));
                    }
                }
            }
        }
    }

    /// Imported image: procedural rig (breathe + wind sway + edge flutter + blink).
    fn draw_sprite(&self, cv: &mut Canvas, sp: &Sprite, fx: f32, fy: f32, light: Rgb) {
        let target_h = self.h * 0.62;
        let sc = (target_h / sp.h as f32).max(0.25);
        let (dw, dh) = (sp.w as f32 * sc, sp.h as f32 * sc);
        let frame = if sp.frames.len() > 1 {
            let total: u32 = sp.delays.iter().map(|&d| d.max(20) as u32).sum();
            let mut ms = ((self.t * 1000.0) as u32) % total.max(1);
            let mut idx = 0;
            for (i, &d) in sp.delays.iter().enumerate() {
                let d = d.max(20) as u32;
                if ms < d {
                    idx = i;
                    break;
                }
                ms -= d;
            }
            &sp.frames[idx]
        } else {
            &sp.frames[0]
        };
        let wind = self.wind(fx);
        let breathe = 1.0 + 0.012 * (self.t * 1.7).sin();
        cv.ellipse(fx, fy, dw * 0.35, 1.5, 0.0, Rgb::BLACK, 0.18);
        let x0 = (fx - dw / 2.0 - 3.0) as i32;
        let x1 = (fx + dw / 2.0 + 3.0) as i32;
        let y0 = (fy - dh * breathe - 2.0) as i32;
        for py in y0..=fy as i32 {
            let from_bottom = (fy - py as f32) / breathe;
            let v = (from_bottom / dh).clamp(0.0, 1.0);
            let sway = wind * v * v * dh * 0.025 + (self.t * 1.1).sin() * v * v * dh * 0.01;
            for px in x0..=x1 {
                let flutter = if v > 0.45 { (vnoise(px as f32 * 0.3, self.t * 2.0, 31) - 0.5) * v * 1.2 } else { 0.0 };
                let sxf = (px as f32 - (fx - dw / 2.0) - sway - flutter) / sc;
                let syf = (dh - from_bottom) / sc;
                if sxf < 0.0 || syf < 0.0 {
                    continue;
                }
                let (sx, sy) = (sxf as usize, syf as usize);
                if sx >= sp.w || sy >= sp.h {
                    continue;
                }
                let mut p = frame[sy * sp.w + sx];
                if self.blink > 0.0 {
                    if let Some(e) = sp.eyes {
                        for (ex, ey) in [(e[0], e[1]), (e[2], e[3])] {
                            let (dx, dy) = (sx as i32 - ex as i32, sy as i32 - ey as i32);
                            if dx.abs() <= (sp.w as i32 / 40).max(1) && dy.abs() <= (sp.h as i32 / 60).max(1) {
                                let above = ((ey as i32 - (sp.h as i32 / 25).max(2)).max(0) as usize) * sp.w + ex as usize;
                                let lid = frame[above.min(frame.len() - 1)];
                                p = [lid[0], lid[1], lid[2], p[3]];
                            }
                        }
                    }
                }
                if p[3] > 8 {
                    let c = Rgb::from_u8([p[0], p[1], p[2]]).mul(light);
                    cv.blend(px, py, c, p[3] as f32 / 255.0);
                }
            }
        }
    }
}

impl Scene for Meadow {
    fn resize(&mut self, w: usize, h: usize) {
        self.w = w as f32;
        self.h = h as f32;
        self.stars = Starfield::new(&mut self.rng, w, h, 0.035, self.h * 0.6);
        self.clouds.clear();
        let n = 2 + w / 70;
        for i in 0..n {
            let cw = (self.w * self.rng.range(0.22, 0.38)) as usize;
            let ch = (cw as f32 * self.rng.range(0.42, 0.6)) as usize;
            let mut c = Cloud::cumulus(&mut self.rng, cw, ch);
            c.x = self.w * (i as f32 / n as f32) - cw as f32 * 0.3 + self.rng.range(-4.0, 4.0);
            c.y = self.h * self.rng.range(0.1, 0.3);
            c.speed = self.rng.range(0.25, 0.6);
            self.clouds.push(c);
        }
        let s1 = self.rng.next_u64() as u32;
        self.far = (0..w).map(|x| self.h * 0.5 + (fbm(x as f32 * 0.012, 0.5, s1, 4) - 0.5) * self.h * 0.26).collect();
        self.near = (0..w).map(|x| self.h * 0.6 + (fbm(x as f32 * 0.02, 3.5, s1 ^ 9, 4) - 0.5) * self.h * 0.16).collect();
        self.mid = (0..w).map(|x| self.h * 0.7 + (x as f32 * 0.03).sin() * self.h * 0.02 + (fbm(x as f32 * 0.03, 8.0, s1 ^ 5, 3) - 0.5) * self.h * 0.06).collect();
        self.flowers.clear();
        for _ in 0..(w / 3) {
            let x = self.rng.range(0.0, self.w);
            let top = self.mound(x);
            let y = self.rng.range(top + 2.0, self.h);
            let col = *self.rng.pick(&[Rgb::hex(0xffffff), Rgb::hex(0xffe066), Rgb::hex(0xff9cc2), Rgb::hex(0xc9b6ff)]);
            self.flowers.push((x as i32, y as i32, col));
        }
        self.seeds = (0..(w / 3).max(12))
            .map(|_| Seed { x: self.rng.range(0.0, self.w), y: self.rng.range(0.0, self.h * 0.9), vx: 0.0, vy: 0.0, col: *self.rng.pick(&[Rgb::hex(0xffffff), Rgb::hex(0xffd9ec), Rgb::hex(0xfff3c4)]) })
            .collect();
        self.fireflies = (0..(w / 12).max(5)).map(|_| (self.rng.range(0.0, self.w), self.rng.range(self.h * 0.65, self.h * 0.95), self.rng.range(0.0, TAU))).collect();
    }

    fn update(&mut self, dt: f32) {
        self.t += dt;
        for c in self.clouds.iter_mut() {
            c.x += c.speed * dt;
            if c.x > self.w + 2.0 {
                c.x = -(c.w as f32) - 2.0;
                c.y = self.h * self.rng.range(0.08, 0.3);
            }
        }
        self.blink -= dt;
        self.next_blink -= dt;
        if self.next_blink <= 0.0 {
            self.blink = 0.13;
            self.next_blink = self.rng.range(2.5, 6.0);
        }
        self.hop -= dt * 1.2;
        if self.hop < -3.0 && self.rng.chance(dt * 0.3) {
            self.hop = 1.0;
        }
        let t = self.t;
        let (w, h) = (self.w, self.h);
        for s in self.seeds.iter_mut() {
            let wx = 6.0 + 4.0 * (t * 0.5).sin();
            let n = vnoise(s.x * 0.05, t * 0.4 + s.y * 0.05, 41) - 0.5;
            s.vx += (wx - s.vx) * dt * 0.8;
            s.vy += (n * 8.0 - s.vy) * dt;
            s.x += s.vx * dt;
            s.y += s.vy * dt;
            if s.x > w + 2.0 {
                s.x = -2.0;
                s.y = h * (0.2 + 0.7 * vnoise(t, s.y, 3));
            }
            s.y = s.y.clamp(0.0, h);
        }
        match &mut self.birds {
            Some(b) => {
                b.0 += b.2 * dt;
                if b.0 > self.w + 20.0 {
                    self.birds = None;
                }
            }
            None => {
                if self.rng.chance(dt / 30.0) {
                    self.birds = Some((-10.0, self.h * self.rng.range(0.1, 0.35), self.w * 0.06));
                }
            }
        }
    }

    fn render(&mut self, cv: &mut Canvas) {
        cv.clear_overlays();
        let (w, h) = (cv.w, cv.h);
        let p = self.phase();
        let t = self.t;
        let key = |a: u32, b: u32, c: u32, d: u32| -> Vec<(f32, Rgb)> {
            vec![(0.0, Rgb::hex(a)), (0.45, Rgb::hex(a)), (0.58, Rgb::hex(b)), (0.66, Rgb::hex(c)), (0.75, Rgb::hex(d)), (0.9, Rgb::hex(d)), (0.96, Rgb::hex(b)), (1.0, Rgb::hex(a))]
        };
        let top = gradient(&key(0x2f7fd6, 0x3d5aa6, 0x2b2c63, 0x070b24), p);
        let hor = gradient(&key(0xd2eef7, 0xffc38a, 0xf07e6e, 0x1a2550), p);
        let light = gradient(&key(0xffffff, 0xffe0c0, 0xd99a9a, 0x4a5a8c), p);
        let cl_hi = gradient(&key(0xffffff, 0xffe2c4, 0xffab8e, 0x5b6594), p);
        let cl_lo = gradient(&key(0x9fb6de, 0xc79aa8, 0x7a4f79, 0x252c52), p);
        let night = smoothstep(0.68, 0.76, p) * (1.0 - smoothstep(0.9, 0.97, p));
        for y in 0..h {
            let c = top.lerp(hor, (y as f32 / (self.h * 0.72)).min(1.0).powf(1.6));
            cv.px[y * w..(y + 1) * w].iter_mut().for_each(|q| *q = c);
        }
        self.stars.draw(cv, t, night);
        // sun sets toward the right horizon, moon rises at night
        if p < 0.68 {
            let k = p / 0.66;
            let (sx, sy) = (self.w * (0.2 + 0.55 * k), self.h * (0.12 + 0.48 * k.powi(2)));
            let sc = Rgb::hex(0xfff8e0).lerp(Rgb::hex(0xff9a4a), smoothstep(0.45, 0.66, p));
            cv.glow(sx, sy, self.h * 0.28, sc.scale(0.22));
            cv.disc(sx, sy, (self.h * 0.045).max(2.0), sc, 1.0);
        } else {
            let k = ((p - 0.7) / 0.28).clamp(0.0, 1.0);
            let (mx, my) = (self.w * (0.15 + 0.3 * k), self.h * (0.35 - 0.2 * (k * 3.14).sin()));
            cv.glow(mx, my, self.h * 0.15, Rgb::hex(0xaab8ff).scale(0.1 * night));
            cv.disc(mx, my, (self.h * 0.035).max(1.8), Rgb::hex(0xf2f3ff).scale(night.max(0.2)), night);
        }
        for c in &self.clouds {
            c.draw(cv, cl_hi, cl_lo, 1.0 - night * 0.35);
        }
        // mountains with aerial perspective
        let far_c = hor.lerp(Rgb::hex(0x7d9cc4), 0.45).mul(light.lerp(Rgb::WHITE, 0.3));
        let near_c = Rgb::hex(0x5b8a86).lerp(hor, 0.25).mul(light);
        let mid_c = Rgb::hex(0x5fa24e).mul(light);
        for x in 0..w {
            for y in self.far[x].max(0.0) as usize..h {
                cv.px[y * w + x] = far_c;
            }
            for y in self.near[x].max(0.0) as usize..h {
                let d = (y as f32 - self.near[x]) / (self.h * 0.1);
                cv.px[y * w + x] = near_c.lerp(near_c.scale(0.8), d.min(1.0));
            }
            for y in self.mid[x].max(0.0) as usize..h {
                let n = vnoise(x as f32 * 0.25, y as f32 * 0.4, 51);
                cv.px[y * w + x] = mid_c.lerp(mid_c.scale(0.82), n * 0.6);
            }
        }
        // foreground mound: grass with wind sheen rolling across
        let g_top = Rgb::hex(0x8fd462);
        let g_bot = Rgb::hex(0x3f8a3f);
        for x in 0..w {
            let top_y = self.mound(x as f32);
            for y in top_y.max(0.0) as usize..h {
                let depth = ((y as f32 - top_y) / (self.h * 0.25)).clamp(0.0, 1.0);
                let wave = smoothstep(0.58, 0.9, vnoise(x as f32 * 0.045 - t * 0.9, y as f32 * 0.14 - t * 0.2, 61));
                let c = g_top.lerp(g_bot, depth).lerp(Rgb::hex(0xd9f5a0), wave * 0.28);
                cv.px[y * w + x] = c.mul(light);
            }
            let blade = 1.0 + vnoise(x as f32 * 0.8, 1.0, 62) * 2.5;
            let lean = self.wind(x as f32) * 1.4 + (t * 3.0 + x as f32 * 0.3).sin() * 0.3;
            cv.line(x as f32 + 0.5, top_y + 0.5, x as f32 + 0.5 + lean, top_y - blade, g_top.lerp(Rgb::hex(0xc8ee8a), 0.3).mul(light), 0.9);
        }
        for &(x, y, col) in &self.flowers {
            let sway = (t * 2.0 + x as f32 * 0.2).sin() * 0.6;
            cv.splat(x as f32 + 0.5 + sway, y as f32 + 0.5, col.mul(light), 0.95);
        }
        // characters
        let fx = self.w * 0.66;
        let fy = self.mound(fx) + 1.0;
        let sc = ((self.h / 60.0).round() as i32).max(1);
        if let Some(sp) = &self.sprite {
            self.draw_sprite(cv, sp, fx, fy, light);
        } else {
            cv.ellipse(fx, fy, 6.0 * sc as f32, 1.2, 0.0, Rgb::BLACK, 0.15);
            self.draw_girl(cv, fx, fy, sc, light);
            let mx = fx - 11.0 * sc as f32;
            self.draw_moss(cv, mx, self.mound(mx) + 1.0, sc, light);
        }
        // drifting seeds / petals
        for s in &self.seeds {
            cv.splat(s.x, s.y, s.col.mul(light.lerp(Rgb::WHITE, 0.4)), 0.8);
        }
        if let Some((bx, by, _)) = self.birds {
            for k in 0..4 {
                let (x, y) = (bx - k as f32 * 4.0, by + (k % 2) as f32 * 2.0 + (t * 3.0 + k as f32).sin());
                let flap = ((t * 8.0 + k as f32).sin() > 0.0) as i32;
                let c = Rgb::hex(0x2a3040).lerp(top, 0.3);
                cv.set(x as i32 - 1, y as i32 - flap, c);
                cv.set(x as i32, y as i32, c);
                cv.set(x as i32 + 1, y as i32 - flap, c);
            }
        }
        if night > 0.05 {
            for &(x, y, ph) in &self.fireflies {
                let fx = x + (t * 0.3 + ph).sin() * 6.0;
                let fy = y + (t * 0.4 + ph * 2.0).cos() * 3.0;
                let k = (0.5 + 0.5 * (t * 1.8 + ph * 3.0).sin()).powi(3) * night;
                cv.glow(fx, fy, 2.3, Rgb::hex(0xe4ff7a).scale(0.55 * k));
            }
        }
    }
}
