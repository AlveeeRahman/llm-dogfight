//! A garden that grows over time — across sessions. Plants are generated from a
//! seed as a growth schedule of segments (stem, leaf, flower, blossom), so each
//! idle session continues where the last left off. Growth happens while the
//! garden is on screen, plus a slow trickle while you're away. Mature plants
//! bloom, set seed, wither, and their seeds sprout nearby.
use super::{Cloud, Scene, Starfield};
use crate::canvas::Canvas;
use crate::config::state_dir;
use crate::math::{smoothstep, vnoise, Rgb};
use crate::rng::Rng;
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Species {
    Sunflower,
    Daisy,
    Tree,
    Tulip,
    Fern,
}
const ALL: [Species; 5] = [Species::Sunflower, Species::Daisy, Species::Tree, Species::Tulip, Species::Fern];

#[derive(Clone, Copy)]
enum Kind {
    Stem { width: f32 },
    Leaf { size: f32, ang: f32 },
    Flower { r: f32, petals: u8, petal: Rgb, center: Rgb },
    Blossom { r: f32 },
}

#[derive(Clone, Copy)]
struct Seg {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    birth: f32,
    dur: f32,
    kind: Kind,
    col: Rgb,
}

struct Plant {
    species: Species,
    x: f32, // 0..1 of width
    seed: u64,
    age: f32,
    height: f32, // in units of canvas height
    segs: Vec<Seg>,
}

const MATURE: f32 = 1.0;
const WITHER: f32 = 2.4;
const DEATH: f32 = 3.0;

fn build(species: Species, seed: u64) -> (Vec<Seg>, f32) {
    let mut r = Rng::new(seed);
    let mut segs = Vec::new();
    let greens = [Rgb::hex(0x3f8f3a), Rgb::hex(0x4fa545), Rgb::hex(0x2f7a3c), Rgb::hex(0x5bb04b)];
    let stem_c = Rgb::hex(0x3b7a33);
    let mut height = 0.5;
    match species {
        Species::Sunflower => {
            height = r.range(0.5, 0.66);
            let (mut x, mut y, mut a) = (0.0f32, 0.0f32, 0.0f32);
            let n = 7;
            for i in 0..n {
                a += r.range(-0.07, 0.07);
                let (nx, ny) = (x + a.sin() * 0.135, y + a.cos() * 0.135);
                let birth = i as f32 * 0.09;
                segs.push(Seg { x0: x, y0: y, x1: nx, y1: ny, birth, dur: 0.1, kind: Kind::Stem { width: 1.6 }, col: stem_c });
                if (1..6).contains(&i) {
                    let side = if i % 2 == 0 { 1.0 } else { -1.0 };
                    segs.push(Seg { x0: nx, y0: ny, x1: nx, y1: ny, birth: birth + 0.08, dur: 0.18, kind: Kind::Leaf { size: r.range(0.1, 0.15), ang: side * r.range(0.9, 1.25) }, col: *r.pick(&greens) });
                }
                x = nx;
                y = ny;
            }
            segs.push(Seg { x0: x, y0: y, x1: x, y1: y, birth: 0.72, dur: 0.35, kind: Kind::Flower { r: 0.085, petals: 12, petal: Rgb::hex(0xffcf2e), center: Rgb::hex(0x6b3e1c) }, col: stem_c });
        }
        Species::Daisy => {
            height = r.range(0.22, 0.32);
            let petal = *r.pick(&[Rgb::hex(0xffffff), Rgb::hex(0xff9ccf), Rgb::hex(0xd66bff), Rgb::hex(0xffd5e8), Rgb::hex(0xb9a4ff)]);
            let stems = 3 + r.below(3);
            for s in 0..stems {
                let (mut x, mut y) = (0.0f32, 0.0f32);
                let mut a = (s as f32 / (stems - 1).max(1) as f32 - 0.5) * 0.9 + r.range(-0.1, 0.1);
                let n = 3 + r.below(2);
                let start = s as f32 * 0.09;
                for i in 0..n {
                    a *= 0.8;
                    let l = r.range(0.2, 0.28);
                    let (nx, ny) = (x + a.sin() * l, y + a.cos() * l);
                    segs.push(Seg { x0: x, y0: y, x1: nx, y1: ny, birth: start + i as f32 * 0.1, dur: 0.12, kind: Kind::Stem { width: 1.0 }, col: stem_c });
                    if i == 0 {
                        segs.push(Seg { x0: nx, y0: ny, x1: nx, y1: ny, birth: start + 0.1, dur: 0.15, kind: Kind::Leaf { size: 0.18, ang: if s % 2 == 0 { 0.8 } else { -0.8 } }, col: *r.pick(&greens) });
                    }
                    x = nx;
                    y = ny;
                }
                segs.push(Seg { x0: x, y0: y, x1: x, y1: y, birth: start + n as f32 * 0.1 + 0.2, dur: 0.3, kind: Kind::Flower { r: 0.11, petals: 8, petal, center: Rgb::hex(0xffd23a) }, col: stem_c });
            }
            // leaves/units are relative to plant height; daisy segments are in their own scale
            for s in segs.iter_mut() {
                s.x0 *= 1.0;
            }
        }
        Species::Tree => {
            height = r.range(0.55, 0.72);
            let blossom = r.chance(0.6);
            let bark = Rgb::hex(0x5a3d2b);
            fn branch(r: &mut Rng, segs: &mut Vec<Seg>, x: f32, y: f32, a: f32, l: f32, depth: u32, birth: f32, bark: Rgb, blossom: bool) {
                let (nx, ny) = (x + a.sin() * l, y + a.cos() * l);
                let width = 0.8 + depth as f32 * 0.55;
                segs.push(Seg { x0: x, y0: y, x1: nx, y1: ny, birth, dur: 0.12, kind: Kind::Stem { width }, col: bark });
                if depth == 0 {
                    let col = if blossom { *r.pick(&[Rgb::hex(0xffb7d0), Rgb::hex(0xffc9dc), Rgb::hex(0xf79ac0)]) } else { *r.pick(&[Rgb::hex(0x4f9e45), Rgb::hex(0x3e8a3f), Rgb::hex(0x66b653)]) };
                    segs.push(Seg { x0: nx, y0: ny, x1: nx, y1: ny, birth: birth + 0.15, dur: 0.35, kind: Kind::Blossom { r: r.range(0.07, 0.1) }, col });
                    return;
                }
                let kids = 2 + (r.chance(0.35) as u32);
                for k in 0..kids {
                    let spread = if kids == 2 { [-0.42, 0.42][k as usize] } else { [-0.55, 0.0, 0.55][k as usize] };
                    let (ja, jl) = (r.range(-0.12, 0.12), r.range(0.66, 0.8));
                    branch(r, segs, nx, ny, a * 0.6 + spread + ja, l * jl, depth - 1, birth + 0.12, bark, blossom);
                }
            }
            segs.push(Seg { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.28, birth: 0.0, dur: 0.18, kind: Kind::Stem { width: 3.2 }, col: bark });
            let lean = r.range(-0.1, 0.1);
            branch(&mut r, &mut segs, 0.0, 0.28, lean, 0.24, 3, 0.18, bark, blossom);
            // normalise so the crown fits height 1
            let top = segs.iter().map(|s| s.y1).fold(0.0f32, f32::max).max(0.01);
            for s in segs.iter_mut() {
                s.x0 /= top;
                s.y0 /= top;
                s.x1 /= top;
                s.y1 /= top;
            }
        }
        Species::Tulip => {
            height = r.range(0.2, 0.26);
            let col = *r.pick(&[Rgb::hex(0xff4d5e), Rgb::hex(0xffcc33), Rgb::hex(0xff8fb1), Rgb::hex(0x9b5cff), Rgb::hex(0xff7a2f)]);
            segs.push(Seg { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0, birth: 0.0, dur: 0.2, kind: Kind::Leaf { size: 0.5, ang: 0.35 }, col: greens[1] });
            segs.push(Seg { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0, birth: 0.05, dur: 0.2, kind: Kind::Leaf { size: 0.45, ang: -0.4 }, col: greens[0] });
            segs.push(Seg { x0: 0.0, y0: 0.0, x1: 0.02, y1: 0.85, birth: 0.1, dur: 0.35, kind: Kind::Stem { width: 1.0 }, col: stem_c });
            segs.push(Seg { x0: 0.02, y0: 0.85, x1: 0.02, y1: 0.85, birth: 0.5, dur: 0.3, kind: Kind::Flower { r: 0.13, petals: 3, petal: col, center: col.scale(0.7) }, col: stem_c });
        }
        Species::Fern => {
            height = r.range(0.25, 0.34);
            let fronds = 4 + r.below(3);
            for f in 0..fronds {
                let side = (f as f32 / (fronds - 1).max(1) as f32 - 0.5) * 2.0;
                let (mut x, mut y, mut a) = (0.0f32, 0.0f32, side * 0.35);
                let n = 6;
                for i in 0..n {
                    a += side * 0.16 + 0.02 * side.signum();
                    let l = 0.2 * (1.0 - i as f32 * 0.09);
                    let (nx, ny) = (x + a.sin() * l, y + a.cos() * l);
                    let birth = f as f32 * 0.06 + i as f32 * 0.07;
                    segs.push(Seg { x0: x, y0: y, x1: nx, y1: ny, birth, dur: 0.08, kind: Kind::Stem { width: 1.0 }, col: greens[2] });
                    let ls = 0.13 * (1.0 - i as f32 / n as f32) + 0.03;
                    segs.push(Seg { x0: nx, y0: ny, x1: nx, y1: ny, birth: birth + 0.05, dur: 0.1, kind: Kind::Leaf { size: ls, ang: a + 1.2 }, col: greens[3] });
                    segs.push(Seg { x0: nx, y0: ny, x1: nx, y1: ny, birth: birth + 0.05, dur: 0.1, kind: Kind::Leaf { size: ls, ang: a - 1.2 }, col: greens[1] });
                    x = nx;
                    y = ny;
                }
            }
        }
    }
    (segs, height)
}

struct Critter {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    phase: f32,
    col: Rgb,
    kind: u8, // 0 butterfly, 1 bee, 2 firefly
}

pub struct Garden {
    rng: Rng,
    w: f32,
    h: f32,
    t: f32,
    speed: f32,
    persist: bool,
    plants: Vec<Plant>,
    critters: Vec<Critter>,
    petals: Vec<(f32, f32, f32, f32, Rgb)>,
    clouds: Vec<Cloud>,
    stars: Starfield,
    ground: f32,
    created: u64,
    save_timer: f32,
    caption: f32,
    hour_override: Option<f32>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn local_hour() -> f32 {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm.tm_hour as f32 + tm.tm_min as f32 / 60.0
    }
}

impl Garden {
    pub fn new(seed: u64, speed: f32, persist: bool) -> Self {
        let mut rng = Rng::new(seed);
        let stars = Starfield::new(&mut rng, 1, 1, 0.0, 1.0);
        let hour_override = std::env::var("REVERIE_HOUR").ok().and_then(|v| v.parse().ok());
        let mut g = Garden {
            rng,
            w: 1.0,
            h: 1.0,
            t: 0.0,
            speed: speed.max(0.01),
            persist,
            plants: vec![],
            critters: vec![],
            petals: vec![],
            clouds: vec![],
            stars,
            ground: 1.0,
            created: now_secs(),
            save_timer: 0.0,
            caption: 6.0,
            hour_override,
        };
        if !(persist && g.load()) {
            g.seed_new();
        }
        g
    }

    fn path() -> std::path::PathBuf {
        state_dir().join("garden.txt")
    }

    fn add_plant(&mut self, species: Species, x: f32, age: f32) {
        let seed = self.rng.next_u64();
        let (segs, height) = build(species, seed);
        self.plants.push(Plant { species, x, seed, age, height, segs });
    }

    fn seed_new(&mut self) {
        let n = 5;
        for i in 0..n {
            let sp = ALL[self.rng.below(ALL.len())];
            let x = (i as f32 + 0.5) / n as f32 + self.rng.range(-0.05, 0.05);
            let age = self.rng.range(0.05, 0.75);
            self.add_plant(sp, x, age);
        }
    }

    fn load(&mut self) -> bool {
        let Ok(text) = std::fs::read_to_string(Self::path()) else { return false };
        let mut last_seen = 0u64;
        for line in text.lines() {
            let p: Vec<&str> = line.split_whitespace().collect();
            match p.as_slice() {
                ["created", v] => self.created = v.parse().unwrap_or(self.created),
                ["last_seen", v] => last_seen = v.parse().unwrap_or(0),
                ["plant", sp, x, seed, age] => {
                    let (Ok(sp), Ok(x), Ok(seed), Ok(age)) = (sp.parse::<usize>(), x.parse::<f32>(), seed.parse::<u64>(), age.parse::<f32>()) else { continue };
                    let species = ALL[sp.min(ALL.len() - 1)];
                    let (segs, height) = build(species, seed);
                    self.plants.push(Plant { species, x, seed, age, height, segs });
                }
                _ => {}
            }
        }
        if self.plants.is_empty() {
            return false;
        }
        // slow growth while you were away: one growth unit per day, capped
        if last_seen > 0 {
            let away_days = now_secs().saturating_sub(last_seen) as f32 / 86400.0;
            let g = away_days.min(1.0);
            for p in self.plants.iter_mut() {
                p.age = (p.age + g).min(DEATH - 0.05);
            }
        }
        true
    }

    fn store(&self) {
        if !self.persist {
            return;
        }
        let mut s = String::from("reverie-garden 1\n");
        s += &format!("created {}\nlast_seen {}\n", self.created, now_secs());
        for p in &self.plants {
            let sp = ALL.iter().position(|&a| a == p.species).unwrap_or(0);
            s += &format!("plant {} {:.4} {} {:.4}\n", sp, p.x, p.seed, p.age);
        }
        let dir = state_dir();
        let _ = std::fs::create_dir_all(&dir);
        let tmp = dir.join("garden.txt.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(tmp, Self::path());
        }
    }

    fn hour(&self) -> f32 {
        self.hour_override.unwrap_or_else(local_hour)
    }

    fn wind(&self, x: f32) -> f32 {
        0.55 * (self.t * 0.8 + x * 0.015).sin() + 0.45 * (vnoise(self.t * 0.25, x * 0.01, 5) - 0.5) * 2.0
    }

    /// Plant-local (x, y up) -> screen, with wind sway growing with height.
    fn to_screen(&self, p: &Plant, lx: f32, ly: f32) -> (f32, f32) {
        let hpx = p.height * self.h;
        let root = p.x * self.w;
        let sway = self.wind(root) * (ly.max(0.0)).powi(2) * hpx * 0.07;
        (root + lx * hpx + sway, self.ground - ly * hpx)
    }
}

impl Scene for Garden {
    fn resize(&mut self, w: usize, h: usize) {
        self.w = w as f32;
        self.h = h as f32;
        self.ground = (self.h * 0.86).floor();
        self.stars = Starfield::new(&mut self.rng, w, h, 0.03, self.ground * 0.9);
        self.clouds.clear();
        for i in 0..3 {
            let cw = (self.w * self.rng.range(0.14, 0.24)) as usize;
            let mut c = Cloud::cumulus(&mut self.rng, cw, (cw as f32 * 0.35) as usize);
            c.x = self.w * (i as f32 / 3.0) + self.rng.range(0.0, self.w * 0.2);
            c.y = self.h * self.rng.range(0.04, 0.22);
            c.speed = self.rng.range(0.6, 1.4);
            self.clouds.push(c);
        }
        if self.critters.is_empty() {
            for k in 0..3 {
                self.critters.push(Critter {
                    x: self.rng.range(0.0, self.w),
                    y: self.ground * self.rng.range(0.4, 0.8),
                    vx: 0.0,
                    vy: 0.0,
                    phase: self.rng.range(0.0, TAU),
                    col: *self.rng.pick(&[Rgb::hex(0xffffff), Rgb::hex(0xffb23e), Rgb::hex(0x7ec8ff), Rgb::hex(0xff7ab8)]),
                    kind: if k == 2 { 1 } else { 0 },
                });
            }
            for _ in 0..6 {
                self.critters.push(Critter { x: self.rng.range(0.0, self.w), y: self.ground * self.rng.range(0.5, 0.95), vx: 0.0, vy: 0.0, phase: self.rng.range(0.0, TAU), col: Rgb::hex(0xd9ff6b), kind: 2 });
            }
        }
    }

    fn update(&mut self, dt: f32) {
        self.t += dt;
        self.caption -= dt;
        let grow = dt / 360.0 * self.speed;
        let mut births = vec![];
        for p in self.plants.iter_mut() {
            p.age += grow;
            if p.age >= DEATH {
                births.push((p.species, p.x));
            }
        }
        self.plants.retain(|p| p.age < DEATH);
        let max_plants = ((self.w / 9.0) as usize).clamp(4, 18);
        for (sp, x) in births {
            let kids = 1 + self.rng.below(2);
            for _ in 0..kids {
                if self.plants.len() >= max_plants {
                    break;
                }
                let nx = (x + self.rng.range(-0.12, 0.12)).clamp(0.03, 0.97);
                let sp = if self.rng.chance(0.25) { ALL[self.rng.below(ALL.len())] } else { sp };
                self.add_plant(sp, nx, 0.0);
            }
        }
        while self.plants.len() < 3 {
            let sp = ALL[self.rng.below(ALL.len())];
            let x = self.rng.range(0.05, 0.95);
            self.add_plant(sp, x, 0.0);
        }
        for c in self.clouds.iter_mut() {
            c.x += c.speed * dt;
            if c.x > self.w + 2.0 {
                c.x = -(c.w as f32) - 2.0;
            }
        }
        // critters
        let flowers: Vec<(f32, f32)> = self
            .plants
            .iter()
            .filter(|p| p.age > MATURE && p.age < WITHER)
            .filter_map(|p| p.segs.iter().rev().find(|s| matches!(s.kind, Kind::Flower { .. } | Kind::Blossom { .. })).map(|s| self.to_screen(p, s.x1, s.y1)))
            .collect();
        let (w, ground, t) = (self.w, self.ground, self.t);
        for c in self.critters.iter_mut() {
            let (tx, ty) = if !flowers.is_empty() && c.kind != 2 {
                let f = flowers[((c.phase * 3.0 + t * 0.02) as usize) % flowers.len()];
                (f.0, f.1 - 2.0)
            } else {
                (w * (0.5 + 0.4 * (t * 0.05 + c.phase).sin()), ground * (0.7 + 0.2 * (t * 0.13 + c.phase).cos()))
            };
            let jitter = if c.kind == 1 { 18.0 } else { 6.0 };
            let (nx, ny) = (vnoise(t * 0.9, c.phase * 10.0, 9) - 0.5, vnoise(t * 0.9, c.phase * 10.0 + 50.0, 9) - 0.5);
            c.vx += ((tx - c.x) * 0.25 + nx * jitter * 4.0) * dt;
            c.vy += ((ty - c.y) * 0.25 + ny * jitter * 4.0) * dt;
            c.vx *= 1.0 - 1.5 * dt;
            c.vy *= 1.0 - 1.5 * dt;
            c.x += c.vx * dt;
            c.y = (c.y + c.vy * dt).min(ground - 1.0);
        }
        // falling petals from blooming trees
        let blooming: Vec<(f32, f32, Rgb)> = self
            .plants
            .iter()
            .filter(|p| p.species == Species::Tree && p.age > MATURE * 1.1 && p.age < WITHER + 0.3)
            .filter_map(|p| p.segs.iter().filter(|s| matches!(s.kind, Kind::Blossom { .. })).nth(((self.t * 3.0) as usize) % 5).map(|s| {
                let (x, y) = self.to_screen(p, s.x1, s.y1);
                (x, y, s.col)
            }))
            .collect();
        for (x, y, col) in blooming {
            if self.rng.chance(dt * 1.2) {
                self.petals.push((x, y, self.rng.range(-1.0, 1.0), 0.0, col));
            }
        }
        let wind = self.wind(self.w * 0.5);
        self.petals.retain_mut(|p| {
            p.2 += (wind * 3.0 - p.2) * dt;
            p.0 += p.2 * dt * 2.0 + (t * 3.0 + p.1).sin() * 0.3 * dt * 10.0;
            p.1 += 2.5 * dt;
            p.3 += dt;
            p.1 < ground && p.3 < 20.0
        });
        self.save_timer += dt;
        if self.save_timer > 30.0 {
            self.save_timer = 0.0;
            self.store();
        }
    }

    fn render(&mut self, cv: &mut Canvas) {
        cv.clear_overlays();
        let (w, h) = (cv.w, cv.h);
        let hour = self.hour();
        // sky by real time of day
        let day = smoothstep(5.0, 7.5, hour) * (1.0 - smoothstep(18.0, 20.5, hour));
        let golden = (smoothstep(16.5, 18.5, hour) * (1.0 - smoothstep(19.0, 20.5, hour))).max(smoothstep(5.0, 6.5, hour) * (1.0 - smoothstep(6.5, 8.0, hour)));
        let top = Rgb::hex(0x0b1230).lerp(Rgb::hex(0x4a90d9), day).lerp(Rgb::hex(0x5a4f8f), golden * 0.6);
        let hor = Rgb::hex(0x1c2748).lerp(Rgb::hex(0xcfeaf7), day).lerp(Rgb::hex(0xffb27a), golden * 0.8);
        for y in 0..self.ground as usize {
            let c = top.lerp(hor, (y as f32 / self.ground).powf(1.4));
            cv.px[y * w..(y + 1) * w].iter_mut().for_each(|p| *p = c);
        }
        self.stars.draw(cv, self.t, 1.0 - day);
        // sun / moon arc
        let (orb, is_sun) = if (6.0..19.0).contains(&hour) { ((hour - 6.0) / 13.0, true) } else { (((hour + 24.0 - 19.0) % 24.0) / 11.0, false) };
        let ox = self.w * (0.1 + 0.8 * orb);
        let oy = self.ground * (0.75 - 0.6 * (orb * PI).sin());
        let r = (self.h * 0.045).max(2.0);
        if is_sun {
            cv.glow(ox, oy, r * 5.0, Rgb::hex(0xffe9a8).scale(0.35));
            cv.disc(ox, oy, r, Rgb::hex(0xfff6d8).lerp(Rgb::hex(0xffb35c), golden), 1.0);
        } else {
            cv.glow(ox, oy, r * 4.0, Rgb::hex(0x9fb2ff).scale(0.12));
            cv.disc(ox, oy, r * 0.9, Rgb::hex(0xeef0ff), 1.0);
            cv.disc(ox + r * 0.4, oy - r * 0.2, r * 0.8, top.lerp(hor, oy / self.ground), 1.0);
        }
        let light = Rgb::new(1.0, 1.0, 1.0).lerp(Rgb::hex(0xffd2b0), golden).lerp(Rgb::hex(0x5a6690), 1.0 - day.max(golden * 0.7));
        for c in &self.clouds {
            c.draw(cv, Rgb::WHITE.mul(light), Rgb::hex(0xa9b4d6).mul(light), 0.9);
        }
        // distant hedge + soil
        let gi = self.ground as i32;
        for x in 0..w as i32 {
            let hedge = (self.h * 0.06 * (0.6 + 0.4 * vnoise(x as f32 * 0.08, 1.0, 21))) as i32;
            for y in gi - hedge..gi {
                let n = vnoise(x as f32 * 0.35, y as f32 * 0.5, 22);
                cv.set(x, y, Rgb::hex(0x2e5c34).lerp(Rgb::hex(0x3d7340), n).mul(light));
            }
            for y in gi..h as i32 {
                let d = (y - gi) as f32 / (h as f32 - self.ground).max(1.0);
                let n = vnoise(x as f32 * 0.6, y as f32 * 0.9, 23);
                let pebble = if vnoise(x as f32 * 1.7, y as f32 * 3.1, 26) > 0.86 { 0.25 } else { 0.0 };
                let c = Rgb::hex(0x6b4a2f).lerp(Rgb::hex(0x3e2a1b), d).lerp(Rgb::hex(0x7d5a3a), n * 0.35).lerp(Rgb::hex(0xa08a70), pebble);
                cv.set(x, y, c.mul(light));
            }
            // grass fringe
            let blade = 1 + (vnoise(x as f32 * 0.9, 3.0, 24) * 3.0) as i32;
            let lean = self.wind(x as f32) * 1.2;
            let g = Rgb::hex(0x5fae4a).lerp(Rgb::hex(0x86c95a), vnoise(x as f32 * 0.4, 7.0, 25)).mul(light);
            cv.line(x as f32 + 0.5, gi as f32 + 0.5, x as f32 + 0.5 + lean, (gi - blade) as f32 + 0.5, g, 0.9);
        }
        // plants, back to front by height (tall first so small ones stay visible)
        let mut order: Vec<usize> = (0..self.plants.len()).collect();
        order.sort_by(|&a, &b| self.plants[b].height.partial_cmp(&self.plants[a].height).unwrap_or(std::cmp::Ordering::Equal));
        for &pi in &order {
            let p = &self.plants[pi];
            let hpx = p.height * self.h;
            let wither = smoothstep(WITHER, DEATH, p.age);
            let brown = Rgb::hex(0x8a6a3a);
            for s in &p.segs {
                if p.age < s.birth {
                    continue;
                }
                let g = ((p.age - s.birth) / s.dur).clamp(0.0, 1.0);
                let col = s.col.lerp(brown, wither).mul(light);
                match s.kind {
                    Kind::Stem { width } => {
                        let (x0, y0) = self.to_screen(p, s.x0, s.y0);
                        let (x1, y1) = self.to_screen(p, s.x0 + (s.x1 - s.x0) * g, s.y0 + (s.y1 - s.y0) * g);
                        let wpx = (width * (hpx / 30.0).clamp(0.6, 1.8)).max(1.0);
                        if wpx >= 1.6 {
                            cv.thick_line(x0, y0, x1, y1, wpx, col.lerp(Rgb::WHITE, 0.12), col.scale(0.7));
                        } else {
                            cv.line(x0, y0, x1, y1, col, 1.0);
                        }
                    }
                    Kind::Leaf { size, ang } => {
                        let droop = wither * 0.8 * ang.signum();
                        let a = ang + droop;
                        let (ax, ay) = self.to_screen(p, s.x0, s.y0);
                        let len = size * hpx * g;
                        let (cx, cy) = (ax + a.sin() * len * 0.5, ay - a.cos() * len * 0.5);
                        cv.ellipse(cx, cy, (len * 0.22).max(0.6), (len * 0.5).max(0.6), a, col, 1.0);
                        cv.ellipse(cx - 0.3, cy - 0.3, (len * 0.1).max(0.3), (len * 0.35).max(0.4), a, col.lerp(Rgb::WHITE, 0.2), 0.5);
                    }
                    Kind::Flower { r, petals, petal, center } => {
                        let (fx, fy) = self.to_screen(p, s.x0, s.y0);
                        let bloom = g * (1.0 - wither * 0.7);
                        let rp = (r * hpx).max(1.2) * bloom;
                        let pc = petal.lerp(brown, wither).mul(light);
                        if petals == 3 {
                            // tulip cup
                            cv.ellipse(fx, fy - rp * 0.4, rp * 0.75, rp, 0.0, pc, 1.0);
                            cv.ellipse(fx - rp * 0.45, fy - rp * 0.5, rp * 0.4, rp * 0.9, -0.2, pc.scale(0.85), 1.0);
                            cv.ellipse(fx + rp * 0.45, fy - rp * 0.5, rp * 0.4, rp * 0.9, 0.2, pc.scale(0.9), 1.0);
                        } else {
                            let spin = self.t * 0.05;
                            for k in 0..petals {
                                let an = k as f32 / petals as f32 * TAU + spin;
                                cv.ellipse(fx + an.cos() * rp * 0.7, fy + an.sin() * rp * 0.7, rp * 0.45, rp * 0.22, an, pc, 0.95);
                            }
                            cv.disc(fx, fy, rp * 0.42, center.mul(light), 1.0);
                        }
                    }
                    Kind::Blossom { r } => {
                        let (bx, by) = self.to_screen(p, s.x0, s.y0);
                        let rr = (r * hpx).max(1.0) * g * (1.0 - wither * 0.6);
                        cv.disc(bx, by, rr, col, 0.95);
                        cv.disc(bx - rr * 0.5, by + rr * 0.3, rr * 0.7, col.scale(0.85), 0.9);
                        cv.disc(bx + rr * 0.4, by - rr * 0.4, rr * 0.6, col.lerp(Rgb::WHITE, 0.25), 0.9);
                    }
                }
            }
        }
        for &(x, y, _, age, col) in &self.petals {
            cv.splat(x, y, col.mul(light), (1.0 - age / 20.0).max(0.0));
        }
        let night = 1.0 - day;
        for c in &self.critters {
            match c.kind {
                0 if day > 0.3 => {
                    let flap = ((self.t * 14.0 + c.phase).sin() > 0.0) as i32;
                    let (x, y) = (c.x.round() as i32, c.y.round() as i32);
                    cv.set(x, y, Rgb::hex(0x2b2b2b));
                    cv.set(x - 1, y - flap, c.col);
                    cv.set(x + 1, y - flap, c.col);
                }
                1 if day > 0.3 => cv.dot_px(c.x, c.y, Rgb::hex(0xffd23a)),
                2 if night > 0.4 => {
                    let k = (0.5 + 0.5 * (self.t * 2.0 + c.phase * 3.0).sin()).powi(3) * night;
                    cv.glow(c.x, c.y, 2.2, c.col.scale(0.5 * k));
                }
                _ => {}
            }
        }
        if self.caption > 0.0 {
            let days = (now_secs().saturating_sub(self.created) / 86400) + 1;
            let blooming = self.plants.iter().filter(|p| p.age > MATURE && p.age < WITHER).count();
            let text = format!(" garden · day {} · {} plants · {} in bloom ", days, self.plants.len(), blooming);
            let a = (self.caption / 1.5).min(1.0);
            let col = (cv.cols as i32 - text.chars().count() as i32) / 2;
            cv.text(col.max(0), 1, &text, Rgb::hex(0xfff3d6).scale(a));
        }
    }

    fn save(&mut self) {
        self.store();
    }
}
