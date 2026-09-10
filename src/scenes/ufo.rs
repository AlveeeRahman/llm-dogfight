//! UFO dogfight. Two saucer factions with steering-behaviour pilots:
//! pursue (with lead prediction) / orbit at engagement range / flee + jink when
//! damaged / wander / abduct a cow when nobody is shooting at them.
use super::{Scene, Starfield};
use crate::canvas::Canvas;
use crate::math::{gradient, vnoise, Rgb};
use crate::rng::Rng;
use std::f32::consts::TAU;

const TEAM_COL: [Rgb; 2] = [Rgb::hex(0x3cf0d8), Rgb::hex(0xff5aa8)];
const TEAM_NAME: [&str; 2] = ["ZORB", "KRELL"];
const STEEL_HI: Rgb = Rgb::hex(0xd6dee9);
const STEEL_LO: Rgb = Rgb::hex(0x4b5566);

#[derive(Clone)]
struct Ship {
    team: usize,
    id: usize,
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    hp: f32,
    alive: bool,
    respawn: f32,
    cooldown: f32,
    target: Option<usize>,
    phase: f32,
    warp: f32,
    flash: f32,
    abduct: Option<usize>,
    bored: f32,
}
struct Bolt {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    team: usize,
}
#[derive(Clone, Copy, PartialEq)]
enum PK {
    Fire,
    Smoke,
    Debris,
    Spark,
}
struct Part {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    max: f32,
    kind: PK,
    col: Rgb,
}
struct Ring {
    x: f32,
    y: f32,
    r: f32,
    speed: f32,
    life: f32,
    col: Rgb,
}
struct Building {
    x: i32,
    w: i32,
    h: i32,
    windows: Vec<(i32, i32, f32)>,
}
struct Cow {
    x: f32,
    y: f32,
    lift: f32,
    gone: f32,
    dir: f32,
}

pub struct Ufo {
    rng: Rng,
    w: f32,
    h: f32,
    t: f32,
    s: f32,
    ground: f32,
    ships: Vec<Ship>,
    bolts: Vec<Bolt>,
    parts: Vec<Part>,
    rings: Vec<Ring>,
    city: Vec<Building>,
    cows: Vec<Cow>,
    stars: Starfield,
    score: [u32; 2],
}

impl Ufo {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let stars = Starfield::new(&mut rng, 1, 1, 0.0, 1.0);
        Ufo { rng, w: 1.0, h: 1.0, t: 0.0, s: 4.0, ground: 1.0, ships: vec![], bolts: vec![], parts: vec![], rings: vec![], city: vec![], cows: vec![], stars, score: [0; 2] }
    }

    fn max_speed(&self) -> f32 {
        self.s * 6.5
    }

    fn spawn_ship(&mut self, team: usize, id: usize) -> Ship {
        let x = if team == 0 { self.rng.range(0.05, 0.4) } else { self.rng.range(0.6, 0.95) } * self.w;
        Ship {
            team,
            id,
            x,
            y: self.rng.range(0.15, 0.55) * self.ground,
            vx: 0.0,
            vy: 0.0,
            hp: 100.0,
            alive: true,
            respawn: 0.0,
            cooldown: self.rng.range(0.5, 2.0),
            target: None,
            phase: self.rng.range(0.0, TAU),
            warp: 1.0,
            flash: 0.0,
            abduct: None,
            bored: self.rng.range(3.0, 9.0),
        }
    }

    fn explode(&mut self, x: f32, y: f32, col: Rgb) {
        let s = self.s;
        self.rings.push(Ring { x, y, r: s * 0.5, speed: s * 9.0, life: 0.6, col: col.lerp(Rgb::WHITE, 0.5) });
        let n = (24.0 + s * 6.0) as usize;
        for _ in 0..n {
            let a = self.rng.range(0.0, TAU);
            let v = self.rng.range(0.2, 1.0).powf(0.6) * s * 9.0;
            let life = self.rng.range(0.4, 1.1);
            self.parts.push(Part { x, y, vx: a.cos() * v, vy: a.sin() * v, life, max: life, kind: PK::Fire, col });
        }
        for _ in 0..8 {
            let a = self.rng.range(0.0, TAU);
            let v = self.rng.range(0.3, 1.0) * s * 7.0;
            let life = self.rng.range(1.5, 3.0);
            self.parts.push(Part { x, y, vx: a.cos() * v, vy: a.sin() * v - s * 3.0, life, max: life, kind: PK::Debris, col: STEEL_LO });
        }
        for _ in 0..10 {
            let life = self.rng.range(1.0, 2.2);
            self.parts.push(Part { x: x + self.rng.range(-s, s), y, vx: self.rng.range(-4.0, 4.0), vy: -self.rng.range(2.0, 6.0), life, max: life, kind: PK::Smoke, col: Rgb::hex(0x3a3848) });
        }
    }

    fn sparks(&mut self, x: f32, y: f32, col: Rgb, n: usize) {
        for _ in 0..n {
            let a = self.rng.range(0.0, TAU);
            let v = self.rng.range(4.0, 14.0) * self.s * 0.4;
            let life = self.rng.range(0.15, 0.45);
            self.parts.push(Part { x, y, vx: a.cos() * v, vy: a.sin() * v, life, max: life, kind: PK::Spark, col });
        }
    }

    fn steer(&mut self, i: usize, dt: f32) {
        let s = self.s;
        let maxv = self.max_speed() * if self.ships[i].team == 1 { 1.05 } else { 1.0 };
        let range = s * 16.0;
        let me = self.ships[i].clone();
        // nearest enemy
        let mut best: Option<(usize, f32)> = None;
        for (j, o) in self.ships.iter().enumerate() {
            if o.alive && o.team != me.team && o.warp >= 1.0 {
                let d = ((o.x - me.x).powi(2) + (o.y - me.y).powi(2)).sqrt();
                if best.map_or(true, |(_, bd)| d < bd) {
                    best = Some((j, d));
                }
            }
        }
        let mut target = me.target.filter(|&t| self.ships[t].alive && self.ships[t].team != me.team);
        if let Some((bj, bd)) = best {
            let keep = target.map(|t| {
                let o = &self.ships[t];
                ((o.x - me.x).powi(2) + (o.y - me.y).powi(2)).sqrt()
            });
            if keep.map_or(true, |kd| bd < kd * 0.6) {
                target = Some(bj);
            }
        }
        let threat_d = best.map_or(f32::MAX, |b| b.1);
        let (mut dx, mut dy);
        let mut abduct = me.abduct;
        if abduct.is_some() && (threat_d < range * 0.9 || me.hp < 100.0 && me.flash > 0.5) {
            abduct = None;
        }
        if abduct.is_none() && threat_d > range * 1.6 && me.bored <= 0.0 {
            // pick a grazing cow
            let free: Vec<usize> = (0..self.cows.len()).filter(|&c| self.cows[c].gone <= 0.0 && self.cows[c].lift <= 0.0 && !self.ships.iter().any(|o| o.abduct == Some(c))).collect();
            if !free.is_empty() {
                abduct = Some(free[self.rng.below(free.len())]);
            }
        }
        if let Some(c) = abduct {
            let cow = &self.cows[c];
            let hover_y = self.ground - s * 7.0;
            dx = cow.x - me.x;
            dy = hover_y - me.y;
            let d = (dx * dx + dy * dy).sqrt();
            if d < s * 1.5 {
                dx *= 0.2;
                dy *= 0.2;
                self.cows[c].lift += dt * 0.35;
                if self.cows[c].lift >= 1.0 {
                    self.cows[c].lift = 0.0;
                    self.cows[c].gone = self.rng.range(6.0, 14.0);
                    self.rings.push(Ring { x: me.x, y: me.y, r: s, speed: s * 3.0, life: 0.5, col: TEAM_COL[me.team] });
                    abduct = None;
                    self.ships[i].bored = self.rng.range(8.0, 20.0);
                }
            }
        } else if me.hp < 35.0 && threat_d < range * 0.8 {
            let o = &self.ships[best.unwrap().0];
            dx = me.x - o.x;
            dy = me.y - o.y;
            let n = (dx * dx + dy * dy).sqrt().max(0.01);
            let jink = (self.t * 7.0 + me.phase).sin();
            let (ux, uy) = (dx / n, dy / n);
            dx = ux - uy * jink * 0.8;
            dy = uy + ux * jink * 0.8;
        } else if let Some(tg) = target {
            let o = self.ships[tg].clone();
            let px = o.x + o.vx * 0.5;
            let py = o.y + o.vy * 0.5;
            dx = px - me.x;
            dy = py - me.y;
            let d = (dx * dx + dy * dy).sqrt().max(0.01);
            let (ux, uy) = (dx / d, dy / d);
            if d > range * 0.75 {
                dx = ux;
                dy = uy;
            } else {
                let side = if me.id % 2 == 0 { 1.0 } else { -1.0 };
                let radial = (d - range * 0.5) / (range * 0.5);
                dx = -uy * side + ux * radial;
                dy = ux * side + uy * radial;
            }
            // fire
            self.ships[i].cooldown -= dt;
            if d < range && self.ships[i].cooldown <= 0.0 && me.warp >= 1.0 {
                let bs = s * 22.0;
                let tt = d / bs;
                let ax = o.x + o.vx * tt - me.x;
                let ay = o.y + o.vy * tt - me.y;
                let an = ay.atan2(ax) + self.rng.gauss() * 0.12;
                self.bolts.push(Bolt { x: me.x, y: me.y, vx: an.cos() * bs, vy: an.sin() * bs, life: 1.6, team: me.team });
                self.ships[i].cooldown = self.rng.range(0.45, 1.2);
            }
        } else {
            let a = vnoise(self.t * 0.15, me.id as f32 * 7.3, 11) * TAU * 2.0;
            dx = a.cos();
            dy = a.sin() * 0.6;
        }
        self.ships[i].abduct = abduct;
        self.ships[i].target = target;
        if abduct.is_none() {
            self.ships[i].bored -= dt;
        }
        // separation from allies
        for o in self.ships.iter() {
            if o.id != me.id && o.alive && o.team == me.team {
                let (ox, oy) = (me.x - o.x, me.y - o.y);
                let d2 = ox * ox + oy * oy;
                if d2 < (s * 4.0).powi(2) && d2 > 0.01 {
                    let d = d2.sqrt();
                    dx += ox / d * (s * 4.0 - d) / (s * 2.0);
                    dy += oy / d * (s * 4.0 - d) / (s * 2.0);
                }
            }
        }
        // soft world bounds (sky only)
        let m = s * 3.0;
        if me.x < m {
            dx += (m - me.x) / m * 2.0;
        }
        if me.x > self.w - m {
            dx -= (me.x - (self.w - m)) / m * 2.0;
        }
        if me.y < m * 0.8 {
            dy += (m * 0.8 - me.y) / m * 2.0;
        }
        let floor = self.ground - s * 4.5;
        if me.y > floor {
            dy -= (me.y - floor) / m * 3.0;
        }
        let n = (dx * dx + dy * dy).sqrt().max(0.001);
        let (tx, ty) = (dx / n * maxv, dy / n * maxv);
        let acc = maxv * 1.8 * dt;
        let (mut sx, mut sy) = (tx - me.vx, ty - me.vy);
        let sn = (sx * sx + sy * sy).sqrt();
        if sn > acc {
            sx *= acc / sn;
            sy *= acc / sn;
        }
        let sh = &mut self.ships[i];
        sh.vx += sx;
        sh.vy += sy;
        sh.x += sh.vx * dt;
        sh.y += sh.vy * dt;
    }
}

impl Scene for Ufo {
    fn resize(&mut self, w: usize, h: usize) {
        let (ow, oh) = (self.w, self.h);
        self.w = w as f32;
        self.h = h as f32;
        self.s = (self.w / 26.0).clamp(3.2, 8.0);
        self.ground = (self.h * 0.86).floor();
        self.stars = Starfield::new(&mut self.rng, w, h, 0.03, self.ground * 0.8);
        // skyline
        self.city.clear();
        let mut x = -2;
        while x < w as i32 + 2 {
            let bw = 3 + self.rng.below((self.s * 1.4) as usize + 2) as i32;
            let bh = (self.rng.range(0.03, 0.13) * self.h) as i32 + 2;
            let mut windows = vec![];
            for wy in (2..bh - 1).step_by(2) {
                for wx in (1..bw - 1).step_by(2) {
                    if self.rng.chance(0.35) {
                        windows.push((wx, wy, self.rng.range(0.0, 60.0)));
                    }
                }
            }
            self.city.push(Building { x, w: bw, h: bh, windows });
            x += bw + self.rng.below(2) as i32;
        }
        // cows in the fields
        self.cows = (0..((w / 40).clamp(2, 5))).map(|_| Cow { x: self.rng.range(0.1, 0.9) * self.w, y: self.ground + 2.0, lift: 0.0, gone: 0.0, dir: 1.0 }).collect();
        if self.ships.is_empty() {
            let per = (w / 45).clamp(2, 5);
            let mut id = 0;
            for team in 0..2 {
                for _ in 0..per {
                    let s = self.spawn_ship(team, id);
                    self.ships.push(s);
                    id += 1;
                }
            }
        } else if ow > 1.0 {
            for s in self.ships.iter_mut() {
                s.x *= self.w / ow;
                s.y *= self.h / oh;
            }
        }
    }

    fn update(&mut self, dt: f32) {
        self.t += dt;
        let s = self.s;
        for i in 0..self.ships.len() {
            if self.ships[i].alive {
                if self.ships[i].warp < 1.0 {
                    self.ships[i].warp = (self.ships[i].warp + dt * 0.9).min(1.0);
                }
                self.steer(i, dt);
                let sh = &mut self.ships[i];
                sh.flash = (sh.flash - dt * 4.0).max(0.0);
                if sh.hp < 45.0 && self.rng.chance(dt * 12.0) {
                    let (x, y) = (sh.x, sh.y);
                    let life = self.rng.range(0.8, 1.6);
                    self.parts.push(Part { x, y, vx: self.rng.range(-2.0, 2.0), vy: -self.rng.range(2.0, 5.0), life, max: life, kind: PK::Smoke, col: Rgb::hex(0x4a4658) });
                }
            } else {
                self.ships[i].respawn -= dt;
                if self.ships[i].respawn <= 0.0 {
                    let (team, id) = (self.ships[i].team, self.ships[i].id);
                    let mut ns = self.spawn_ship(team, id);
                    ns.y = self.rng.range(0.12, 0.35) * self.ground;
                    ns.warp = 0.0;
                    self.ships[i] = ns;
                }
            }
        }
        // bolts
        let mut i = 0;
        while i < self.bolts.len() {
            let b = &mut self.bolts[i];
            b.x += b.vx * dt;
            b.y += b.vy * dt;
            b.life -= dt;
            let (bx, by, team) = (b.x, b.y, b.team);
            let mut dead = b.life <= 0.0 || bx < -5.0 || bx > self.w + 5.0 || by < -5.0;
            if by >= self.ground {
                dead = true;
                self.sparks(bx, self.ground, Rgb::hex(0xffc070), 5);
            }
            if !dead {
                for j in 0..self.ships.len() {
                    let o = &self.ships[j];
                    if !o.alive || o.team == team || o.warp < 1.0 {
                        continue;
                    }
                    let ex = (bx - o.x) / s;
                    let ey = (by - o.y) / (s * 0.55);
                    if ex * ex + ey * ey < 1.0 {
                        dead = true;
                        let dmg = self.rng.range(12.0, 22.0);
                        let o = &mut self.ships[j];
                        o.hp -= dmg;
                        o.flash = 1.0;
                        let (ox, oy, ot) = (o.x, o.y, o.team);
                        let killed = o.hp <= 0.0;
                        self.sparks(bx, by, TEAM_COL[team].lerp(Rgb::WHITE, 0.5), 7);
                        if killed {
                            let o = &mut self.ships[j];
                            o.alive = false;
                            o.abduct = None;
                            o.respawn = self.rng.range(2.5, 4.5);
                            self.score[team] += 1;
                            self.explode(ox, oy, TEAM_COL[ot]);
                        }
                        break;
                    }
                }
            }
            if dead {
                self.bolts.swap_remove(i);
            } else {
                i += 1;
            }
        }
        // particles
        let g = s * 4.0;
        let ground = self.ground;
        self.parts.retain_mut(|p| {
            p.life -= dt;
            let drag = match p.kind {
                PK::Fire => 2.6,
                PK::Smoke => 0.8,
                PK::Debris => 0.3,
                PK::Spark => 3.0,
            };
            p.vx -= p.vx * drag * dt;
            p.vy -= p.vy * drag * dt;
            match p.kind {
                PK::Debris => p.vy += g * dt,
                PK::Smoke => p.vy -= 1.5 * dt,
                _ => {}
            }
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            if p.kind == PK::Debris && p.y > ground {
                p.y = ground;
                p.vy = -p.vy * 0.3;
                p.vx *= 0.5;
            }
            p.life > 0.0
        });
        self.rings.retain_mut(|r| {
            r.life -= dt;
            r.r += r.speed * dt;
            r.life > 0.0
        });
        for c in self.cows.iter_mut() {
            if c.gone > 0.0 {
                c.gone -= dt;
                if c.gone <= 0.0 {
                    c.x = self.rng.range(0.1, 0.9) * self.w;
                }
            } else if c.lift <= 0.0 && self.rng.chance(dt * 0.1) {
                c.dir = -c.dir;
            } else if c.lift <= 0.0 {
                c.x += c.dir * dt * 0.4;
            }
        }
        // cows being lifted fall back if the beam breaks
        for ci in 0..self.cows.len() {
            if self.cows[ci].lift > 0.0 && !self.ships.iter().any(|o| o.alive && o.abduct == Some(ci)) {
                self.cows[ci].lift = (self.cows[ci].lift - dt * 1.5).max(0.0);
            }
        }
        if self.parts.len() > 4000 {
            self.parts.drain(0..1000);
        }
    }

    fn render(&mut self, cv: &mut Canvas) {
        cv.clear_overlays();
        let (w, h) = (cv.w, cv.h);
        let t = self.t;
        let s = self.s;
        let sky = [(0.0, Rgb::hex(0x04050d)), (0.55, Rgb::hex(0x121538)), (0.85, Rgb::hex(0x2c1f4a)), (1.0, Rgb::hex(0x5a2d52))];
        for y in 0..h {
            let c = if (y as f32) < self.ground { gradient(&sky, y as f32 / self.ground) } else { Rgb::hex(0x0a0d0c) };
            cv.px[y * w..(y + 1) * w].iter_mut().for_each(|p| *p = c);
        }
        self.stars.draw(cv, t, 1.0);
        // moon
        let (mx, my, mr) = (self.w * 0.83, self.h * 0.16, s * 1.5);
        cv.glow(mx, my, mr * 4.0, Rgb::hex(0x3a3550).scale(0.6));
        cv.disc(mx, my, mr, Rgb::hex(0xf3ecd2), 1.0);
        cv.disc(mx - mr * 0.3, my - mr * 0.1, mr * 0.28, Rgb::hex(0xcfc6aa), 0.8);
        cv.disc(mx + mr * 0.35, my + mr * 0.35, mr * 0.2, Rgb::hex(0xd8cfb3), 0.7);
        // skyline
        let gi = self.ground as i32;
        for b in &self.city {
            cv.rect(b.x, gi - b.h, b.w, b.h, Rgb::hex(0x0b0c19), 1.0);
            for &(wx, wy, ph) in &b.windows {
                let on = ((t * 0.05 + ph).sin() > -0.6) as i32 as f32;
                if on > 0.0 {
                    let flick = 0.8 + 0.2 * (t * 0.7 + ph * 3.0).sin();
                    cv.set(b.x + wx, gi - b.h + wy, Rgb::hex(0xffcf73).scale(0.55 * flick));
                }
            }
        }
        // grass strip + cows
        for y in gi..h as i32 {
            for x in 0..w as i32 {
                let n = vnoise(x as f32 * 0.3, y as f32 * 0.7, 3);
                cv.set(x, y, Rgb::hex(0x0d1a12).lerp(Rgb::hex(0x16291b), n));
            }
        }
        for (ci, c) in self.cows.iter().enumerate() {
            if c.gone > 0.0 {
                continue;
            }
            let lifter = self.ships.iter().find(|o| o.alive && o.abduct == Some(ci));
            let cy = match lifter {
                Some(o) => c.y + (o.y - c.y) * c.lift,
                None => c.y - (c.lift * 4.0),
            };
            let (x, y) = (c.x.round() as i32, cy.round() as i32);
            let (white, spot, dark) = (Rgb::hex(0xeeeae0), Rgb::hex(0x26262a), Rgb::hex(0x8c867c));
            let f = if c.dir > 0.0 { 1 } else { -1 };
            // 4x2 body with a spot, head, legs
            for bx in 0..4 {
                cv.set(x + bx * f, y - 2, if bx == 1 { spot } else { white });
                cv.set(x + bx * f, y - 1, if bx == 2 { spot } else { white });
            }
            cv.set(x + 4 * f, y - 2, white);
            cv.set(x + 4 * f, y - 3, dark);
            cv.set(x, y, dark);
            cv.set(x + 3 * f, y, dark);
        }
        // abduction beams (under the ships)
        for o in self.ships.iter() {
            if let (true, Some(ci)) = (o.alive, o.abduct) {
                let c = &self.cows[ci];
                if ((o.x - c.x).abs()) < s * 2.0 {
                    let top = o.y + s * 0.4;
                    let bot = self.ground;
                    let steps = (bot - top).max(1.0) as i32;
                    for k in 0..steps {
                        let yy = top + k as f32;
                        let f = k as f32 / steps as f32;
                        let half = s * 0.4 + s * 1.2 * f;
                        let pulse = 0.75 + 0.25 * ((t * 6.0) - f * 8.0).sin();
                        for xx in (o.x - half) as i32..=(o.x + half) as i32 {
                            let e = 1.0 - ((xx as f32 - o.x).abs() / half).powi(2);
                            cv.add(xx, yy as i32, TEAM_COL[o.team].scale(0.07 * e * pulse));
                        }
                    }
                }
            }
        }
        // particles
        for p in &self.parts {
            let k = p.life / p.max;
            match p.kind {
                PK::Fire => {
                    let c = if k > 0.7 { Rgb::hex(0xfff3c4) } else if k > 0.45 { Rgb::hex(0xffb347) } else if k > 0.2 { Rgb::hex(0xff5a36) } else { Rgb::hex(0x6a2a3a) };
                    cv.splat_add(p.x, p.y, c.lerp(p.col, 0.2).scale(0.9 * k + 0.2));
                }
                PK::Smoke => cv.splat(p.x, p.y, p.col, 0.55 * k),
                PK::Debris => cv.splat(p.x, p.y, Rgb::hex(0x8a8f99), 0.9),
                PK::Spark => cv.dot_px(p.x, p.y, p.col.lerp(Rgb::WHITE, k)),
            }
        }
        for r in &self.rings {
            let n = (r.r * 5.0).max(12.0) as i32;
            let a = r.life * 1.5;
            for k in 0..n {
                let an = k as f32 / n as f32 * TAU;
                cv.splat_add(r.x + an.cos() * r.r, r.y + an.sin() * r.r * 0.8, r.col.scale(a * 0.5));
            }
        }
        // ships
        for o in &self.ships {
            if !o.alive {
                continue;
            }
            let tc = TEAM_COL[o.team];
            let a = o.warp;
            if a < 1.0 {
                // warp-in beam from the top
                for yy in 0..o.y as i32 {
                    let f = (1.0 - a) * (0.6 + 0.4 * (t * 20.0 + yy as f32 * 0.5).sin());
                    for dx in -1..=1 {
                        cv.add(o.x as i32 + dx, yy, tc.scale(0.18 * f * if dx == 0 { 1.5 } else { 0.6 }));
                    }
                }
            }
            let rot = (o.vx / self.max_speed()).clamp(-1.0, 1.0) * 0.3;
            let (sn, cs) = rot.sin_cos();
            let (rx, ry) = (s, s * 0.34);
            let pulse = 0.7 + 0.3 * (t * 5.0 + o.phase).sin();
            cv.glow(o.x, o.y + ry * 1.3, s * 1.0, tc.scale(0.28 * pulse * a));
            cv.ellipse(o.x, o.y, rx, ry, rot, STEEL_LO, a);
            let (hx, hy) = (o.x + sn * ry * 0.35, o.y - cs * ry * 0.35);
            cv.ellipse(hx, hy, rx * 0.88, ry * 0.55, rot, STEEL_HI.lerp(tc, 0.15), 0.85 * a);
            // dome
            let d = ry * 1.05;
            let (dx, dy) = (o.x + sn * d, o.y - cs * d);
            cv.ellipse(dx, dy, s * 0.45, s * 0.4, rot, tc.lerp(Rgb::WHITE, 0.35), 0.9 * a);
            cv.splat(dx - s * 0.15, dy - s * 0.15, Rgb::WHITE, 0.8 * a);
            // chasing rim lights
            let lights = 5;
            for k in 0..lights {
                let f = (k as f32 / (lights - 1) as f32) * 2.0 - 1.0;
                let lx = f * rx * 0.78;
                let ly = ry * 0.15;
                let (px, py) = (o.x + lx * cs - ly * sn, o.y + lx * sn + ly * cs);
                let on = ((t * 6.0 + k as f32 * 1.3 + o.phase).sin() * 0.5 + 0.5).powi(2);
                cv.splat_add(px, py, tc.lerp(Rgb::WHITE, 0.3).scale((0.3 + 0.9 * on) * a));
            }
            if o.flash > 0.0 {
                cv.glow(o.x, o.y, s * 1.6, Rgb::WHITE.scale(0.45 * o.flash));
            }
        }
        // bolts
        for b in &self.bolts {
            let tc = TEAM_COL[b.team];
            let (tx, ty) = (b.x - b.vx * 0.035, b.y - b.vy * 0.035);
            cv.line_add(tx, ty, b.x, b.y, tc.scale(0.9));
            cv.splat_add(b.x, b.y, Rgb::WHITE.scale(0.8));
        }
        // HUD
        let hud = format!("{} {}", TEAM_NAME[0], self.score[0]);
        cv.text(2, 0, &hud, TEAM_COL[0].scale(0.55));
        let hud2 = format!("{} {}", TEAM_NAME[1], self.score[1]);
        cv.text(4 + hud.len() as i32, 0, &hud2, TEAM_COL[1].scale(0.55));
    }
}
