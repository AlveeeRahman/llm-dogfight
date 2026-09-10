//! Scene contract (LOCKED interface — the harness and bench depend on it).
use crate::canvas::Canvas;
use crate::config::Config;
use crate::math::{fbm, smoothstep, Rgb};
use crate::rng::Rng;

pub mod galaxy;
pub mod garden;
pub mod meadow;
pub mod ufo;

pub trait Scene {
    /// Canvas pixel size changed (w = cols, h = rows*2).
    fn resize(&mut self, w: usize, h: usize);
    /// Advance simulation by `dt` seconds (clamped by the caller to <= 0.1).
    fn update(&mut self, dt: f32);
    /// Paint the full frame. Must overwrite every pixel it cares about.
    fn render(&mut self, cv: &mut Canvas);
    /// Persist anything long-lived (garden). Called on exit and periodically.
    fn save(&mut self) {}
}

pub const NAMES: &[(&str, &str)] = &[
    ("ufo", "Saucer factions dogfight over a sleeping city, with AI pilots, lasers and explosions"),
    ("galaxy", "A slowly turning spiral galaxy: ~10k stars on density-wave orbits, dust lanes, supernovae"),
    ("garden", "A garden that keeps growing across sessions: plants sprout, bloom, seed and wither"),
    ("meadow", "A painterly hillside: towering clouds, wind in the grass, a girl in a straw hat, day/night"),
    ("portrait:<name>", "Your own image, imported with `reverie import`, living in the meadow"),
];

pub fn make(name: &str, seed: u64, w: usize, h: usize, cfg: &Config, persist: bool) -> Option<Box<dyn Scene>> {
    let mut s: Box<dyn Scene> = match name {
        "ufo" => Box::new(ufo::Ufo::new(seed)),
        "galaxy" => Box::new(galaxy::Galaxy::new(seed)),
        "garden" => Box::new(garden::Garden::new(seed, cfg.garden_speed, persist)),
        "meadow" => Box::new(meadow::Meadow::new(seed, cfg.day_cycle_seconds, &cfg.character)),
        n if n.starts_with("portrait:") => Box::new(meadow::Meadow::new(seed, cfg.day_cycle_seconds, &n["portrait:".len()..])),
        _ => return None,
    };
    s.resize(w, h);
    Some(s)
}

// ---------------------------------------------------------------- shared scenery

/// A pre-rendered cumulus cloud: coverage + "height in the cloud" shading.
pub struct Cloud {
    pub x: f32,
    pub y: f32,
    pub speed: f32,
    pub w: usize,
    pub h: usize,
    alpha: Vec<f32>,
    shade: Vec<f32>,
}

impl Cloud {
    /// Towering cumulus: puffy dome, flat bottom, soft noisy edges.
    pub fn cumulus(rng: &mut Rng, w: usize, h: usize) -> Cloud {
        let (w, h) = (w.max(6), h.max(4));
        let seed = rng.next_u64() as u32;
        let n = 5 + rng.below(5);
        let mut blobs = Vec::with_capacity(n);
        for i in 0..n {
            let t = (i as f32 + 0.5) / n as f32; // 0..1 across
            let bell = (1.0 - (2.0 * t - 1.0).powi(2)).max(0.0);
            let r = h as f32 * (0.28 + 0.42 * bell) * rng.range(0.8, 1.15);
            let cx = w as f32 * (0.12 + 0.76 * t) + rng.range(-2.0, 2.0);
            let cy = h as f32 * 0.98 - r * rng.range(0.55, 0.9);
            blobs.push((cx, cy, r));
        }
        let base = h as f32 * 0.95;
        let mut alpha = vec![0.0; w * h];
        let mut shade = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let mut cov: f32 = 0.0;
                let mut lit: f32 = 0.0;
                for &(cx, cy, r) in &blobs {
                    let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() / r;
                    let c = 1.0 - d;
                    if c > cov {
                        cov = c;
                    }
                    if d < 1.0 {
                        // lit from the upper-left of each puff
                        let l = 0.5 + 0.5 * (((cy - fy) * 0.9 + (cx - fx) * 0.35) / r);
                        lit = lit.max(l);
                    }
                }
                let edge_noise = (fbm(fx * 0.18, fy * 0.18, seed, 3) - 0.5) * 0.35;
                let bottom = smoothstep(base + 1.5, base - 1.5, fy);
                let a = smoothstep(0.0, 0.12, cov + edge_noise) * bottom;
                alpha[y * w + x] = a;
                let vertical = 1.0 - (fy / h as f32);
                shade[y * w + x] = (0.55 * lit + 0.45 * vertical).clamp(0.0, 1.0);
            }
        }
        Cloud { x: 0.0, y: 0.0, speed: 0.0, w, h, alpha, shade }
    }

    pub fn draw(&self, cv: &mut Canvas, light: Rgb, dark: Rgb, opacity: f32) {
        let ox = self.x.floor() as i32;
        let fx = self.x - self.x.floor();
        let oy = self.y.round() as i32;
        for y in 0..self.h {
            for x in 0..self.w {
                let i = y * self.w + x;
                // horizontal sub-pixel interpolation keeps slow drift smooth
                let a0 = self.alpha[i];
                let a1 = if x > 0 { self.alpha[i - 1] } else { 0.0 };
                let a = a0 * (1.0 - fx) + a1 * fx;
                if a < 0.01 {
                    continue;
                }
                let s = self.shade[i];
                let c = dark.lerp(light, smoothstep(0.25, 0.8, s));
                cv.blend(ox + x as i32, oy + y as i32, c, a * opacity);
            }
        }
    }
}

/// Static deep-sky stars rendered as twinkling braille dots.
pub struct Starfield {
    stars: Vec<(f32, f32, f32, f32)>, // x, y (pixel space), brightness, phase
}

impl Starfield {
    pub fn new(rng: &mut Rng, w: usize, h: usize, density: f32, max_y: f32) -> Self {
        let n = ((w * h) as f32 * density) as usize;
        let stars = (0..n).map(|_| (rng.range(0.0, w as f32), rng.range(0.0, max_y), rng.range(0.25, 1.0).powi(2), rng.range(0.0, 6.28))).collect();
        Starfield { stars }
    }
    pub fn draw(&self, cv: &mut Canvas, t: f32, visibility: f32) {
        if visibility <= 0.02 {
            return;
        }
        for &(x, y, b, ph) in &self.stars {
            let tw = 0.8 + 0.2 * (t * (0.5 + ph * 0.1) + ph).sin();
            let k = b * tw * visibility;
            // never cover bright detail with a braille dot (it would replace the half-block)
            if k > 0.12 && cv.get(x as i32, y as i32).luma() < 0.3 {
                let c = Rgb::new(0.75, 0.82, 1.0).scale(0.4 + 0.6 * k);
                cv.dot_px(x, y, c);
            }
        }
    }
}
