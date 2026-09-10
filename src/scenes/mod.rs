//! Scene contract (LOCKED interface — the harness and bench depend on it).
use crate::canvas::Canvas;
use crate::config::Config;
use crate::math::Rgb;
use crate::rng::Rng;

pub mod ufo;

pub trait Scene {
    /// Canvas pixel size changed (w = cols, h = rows*2).
    fn resize(&mut self, w: usize, h: usize);
    /// Advance simulation by `dt` seconds (clamped by the caller to <= 0.1).
    fn update(&mut self, dt: f32);
    /// Paint the full frame. Must overwrite every pixel it cares about.
    fn render(&mut self, cv: &mut Canvas);
    /// Persist anything long-lived. Called on exit and periodically.
    fn save(&mut self) {}
}

pub const NAMES: &[(&str, &str)] = &[
    ("ufo", "Saucer factions dogfight over a sleeping city (built-in pilots, no GPU)"),
    ("ufo-battle", "The same dogfight commanded by two small language models: `reverie run cuda|mlx ufo-battle`"),
];

/// `live` is true for an interactive run (screensaver / `reverie run`). Bench and snapshot
/// pass false and always get the deterministic built-in pilots.
pub fn make(name: &str, seed: u64, w: usize, h: usize, cfg: &Config, live: bool) -> Option<Box<dyn Scene>> {
    let mut s: Box<dyn Scene> = match name {
        "ufo-battle" | "ufo" if live && (name == "ufo-battle" || cfg.ufo_pilots == "lm") => Box::new(ufo::Ufo::with_arena(seed, cfg)),
        "ufo" | "ufo-battle" => Box::new(ufo::Ufo::new(seed)),
        _ => return None,
    };
    s.resize(w, h);
    Some(s)
}

// ---------------------------------------------------------------- shared scenery

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
