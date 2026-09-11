//! UFO dogfight. Two saucer factions over a sleeping city with a farm. Every ship flies
//! under an *order* (attack / hunt / flee / abduct; patrol only as the built-in fallback)
//! executed by steering behaviours: lead pursuit, orbiting at engagement range, jinking,
//! ally separation, world bounds. Who issues the orders is the "pilots" choice:
//!
//!   builtin — the heuristic commander in `builtin_order` (deterministic; bench/snapshot)
//!   lm      — two small language models in the arena sidecar (src/arena.rs), one per
//!             team. A *game*: `lm_max_alive` (4) saucers per team on screen, a destroyed one
//!             is replaced REINFORCE seconds later while the team has reinforcements left
//!             (`lm_regens`, 20 per game). A team with no saucer in the air loses the game;
//!             GAME_PAUSE later the next game starts, the loser with one extra saucer. Games
//!             won are persisted per model pair. With `evolve`, each team's bounded doctrine
//!             (src/evolve.rs) is enforced on the model's orders and bred by a GA.
//!
//! Graphics: dithered sky with a slow aurora and moonlit clouds, shaded saucers with
//! banking, motion trails and shield shimmer, dynamic lighting from bolts, explosions and
//! beams on the city and the field, a farm (barn, fence, road with cars) with 10 grazing
//! cows that are replaced from the barn after each abduction, a title card and game
//! banners in a 3x5 pixel font.
use super::{Scene, Starfield};
use crate::arena::{json_str, Arena, Msg, Order};
use crate::canvas::Canvas;
use crate::config::{state_dir, Config};
use crate::evolve::{Doctrine, Population, SEGMENT};
use crate::math::{gradient, vnoise, Rgb};
use crate::rng::Rng;
use std::f32::consts::TAU;

const TEAM_COL: [Rgb; 2] = [Rgb::hex(0x3cf0d8), Rgb::hex(0xff5aa8)];
const TEAM_NAME: [&str; 2] = ["ZORB", "KRELL"];
const STEEL_HI: Rgb = Rgb::hex(0xd6dee9);
const STEEL_LO: Rgb = Rgb::hex(0x4b5566);
/// slots per team: `lm_max_alive` on screen plus one for the previous loser's extra saucer
const LM_SLOTS: usize = 5;
const NCOWS: usize = 10;
/// seconds before a destroyed saucer's replacement warps in (lm mode)
const REINFORCE: f32 = 4.0;
/// seconds between a game's end and the next game
const GAME_PAUSE: f32 = 10.0;
const TRAIL: usize = 7;

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
    order: Order,
    /// who this ship is shooting at (derived from the order each frame)
    target: Option<usize>,
    phase: f32,
    warp: f32,
    flash: f32,
    abduct: Option<usize>,
    bored: f32,
    /// for post-mortems (evolve mode)
    born: f32,
    low_for: f32,
    /// recent positions for the motion trail (newest last)
    trail: Vec<(f32, f32)>,
    trail_t: f32,
    /// seconds under the current order (abduct orders are kept until done, see `lm_poll`)
    order_t: f32,
}
struct Bolt {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    life: f32,
    team: usize,
    from: usize,
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
    dir: f32,
    /// where it is walking to (grazes when it gets there)
    goal: f32,
    walk: f32,
    graze: f32,
}
struct Car {
    x: f32,
    v: f32,
}
struct Cloud {
    x: f32,
    y: f32,
    rx: f32,
    ry: f32,
    v: f32,
    seed: u32,
}
/// A transient light source (bolt, explosion, beam) that lights the city and the field.
struct Light {
    x: f32,
    y: f32,
    r: f32,
    col: Rgb,
}

/// State of the language-model commanders (only in `lm` mode).
struct Lm {
    link: Option<Arena>,
    ready: [Option<String>; 2],
    pending: [bool; 2],
    since: [f32; 2],
    wait: [f32; 2],
    tick: [u32; 2],
    say: [String; 2],
    say_age: [f32; 2],
    status: String,
    events: [Vec<String>; 2],
    think: f32,
    evolve: bool,
    lesson: [String; 2],
    lesson_age: [f32; 2],
    /// a saucer of this team died since its last orders: the next prompt asks for a battle cry
    cry_due: [bool; 2],
    /// kills this game, per team (for the game post-mortem)
    game_kills: [u32; 2],
    /// genetic doctrine evolution (the `evolve` word)
    pop: Option<[Population; 2]>,
    seg_t: f32,
    /// kills / losses / cows at the start of the evaluation window
    seg_base: [[u32; 3]; 2],
    corrected: [u32; 2],
}

/// One game of the match and the persistent scoreboard between the two models.
struct Game {
    n: u32,
    max_alive: usize,
    regens_total: u32,
    regens: [u32; 2],
    /// seconds until the next game starts (0 = playing)
    pause: f32,
    /// the previous game's loser starts the next one with an extra saucer
    handicap: Option<usize>,
    wins: [u32; 2],
    total_kills: [u32; 2],
    total_cows: [u32; 2],
    losses: [u32; 2],
    score_path: Option<std::path::PathBuf>,
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
    cars: Vec<Car>,
    clouds: Vec<Cloud>,
    lights: Vec<Light>,
    stars: Starfield,
    /// two barns, one per side of the field; replacement cows walk out of the emptier side
    barns: [f32; 2],
    score: [u32; 2],
    cows_taken: [u32; 2],
    game: Game,
    title: f32,
    banner: String,
    banner_t: f32,
    banner_col: Rgb,
    lm: Option<Lm>,
}

impl Ufo {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let stars = Starfield::new(&mut rng, 1, 1, 0.0, 1.0);
        Ufo {
            rng,
            w: 1.0,
            h: 1.0,
            t: 0.0,
            s: 4.0,
            ground: 1.0,
            ships: vec![],
            bolts: vec![],
            parts: vec![],
            rings: vec![],
            city: vec![],
            cows: vec![],
            cars: vec![],
            clouds: vec![],
            lights: vec![],
            stars,
            barns: [0.0; 2],
            score: [0; 2],
            cows_taken: [0; 2],
            game: Game {
                n: 1,
                max_alive: 4,
                regens_total: 20,
                regens: [20; 2],
                pause: 0.0,
                handicap: None,
                wins: [0; 2],
                total_kills: [0; 2],
                total_cows: [0; 2],
                losses: [0; 2],
                score_path: None,
            },
            title: 0.0,
            banner: String::new(),
            banner_t: 0.0,
            banner_col: Rgb::WHITE,
            lm: None,
        }
    }

    /// Two language models command the teams (see agents/arena.py). If the sidecar cannot
    /// start, the built-in pilots fly and the HUD says why.
    pub fn with_arena(seed: u64, cfg: &Config) -> Self {
        let mut u = Self::new(seed);
        let mut lm = Lm {
            link: None,
            ready: [None, None],
            pending: [false; 2],
            since: [0.0; 2],
            wait: [0.0; 2],
            tick: [0; 2],
            say: [String::new(), String::new()],
            say_age: [0.0; 2],
            status: String::new(),
            events: [vec![], vec![]],
            think: cfg.lm_think_seconds.max(0.2),
            evolve: cfg.lm_evolve,
            lesson: [String::new(), String::new()],
            lesson_age: [0.0; 2],
            cry_due: [false; 2],
            game_kills: [0; 2],
            pop: None,
            seg_t: 0.0,
            seg_base: [[0; 3]; 2],
            corrected: [0; 2],
        };
        if cfg.lm_genetic {
            let a = Population::load_or_new(0, TEAM_NAME[0], &mut u.rng);
            let b = Population::load_or_new(1, TEAM_NAME[1], &mut u.rng);
            lm.pop = Some([a, b]);
        }
        u.game.max_alive = cfg.lm_max_alive.clamp(1, 4) as usize;
        u.game.regens_total = cfg.lm_regens;
        u.game.regens = [cfg.lm_regens; 2];
        match Arena::spawn(cfg) {
            Ok(a) => {
                lm.link = Some(a);
                lm.status = "arena: starting commanders".into();
            }
            Err(e) => lm.status = format!("arena: {e} (built-in pilots)"),
        }
        u.lm = Some(lm);
        u.title = 4.5;
        u
    }

    fn lm_mode(&self) -> bool {
        self.lm.as_ref().is_some_and(|l| l.link.is_some())
    }
    /// True when this team's orders come from its language model.
    fn commanded(&self, team: usize) -> bool {
        self.lm.as_ref().is_some_and(|l| l.link.is_some() && l.ready[team].is_some())
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
            order: Order::Hunt,
            target: None,
            phase: self.rng.range(0.0, TAU),
            warp: 1.0,
            flash: 0.0,
            abduct: None,
            bored: self.rng.range(3.0, 9.0),
            born: self.t,
            low_for: 0.0,
            trail: Vec::with_capacity(TRAIL),
            trail_t: 0.0,
            order_t: 0.0,
        }
    }

    /// Warp a fresh saucer in for slot `i` (its previous occupant was destroyed).
    fn deploy(&mut self, i: usize) {
        let (team, id) = (self.ships[i].team, self.ships[i].id);
        let mut ns = self.spawn_ship(team, id);
        ns.y = self.rng.range(0.12, 0.35) * self.ground;
        ns.warp = 0.0;
        self.ships[i] = ns;
    }

    fn new_cow(&mut self, x: f32) -> Cow {
        let goal = self.rng.range(0.06, 0.94) * self.w;
        Cow { x, y: self.ground + 3.0, lift: 0.0, dir: if goal > x { 1.0 } else { -1.0 }, goal, walk: self.rng.range(0.0, TAU), graze: 0.0 }
    }

    /// A replacement cow walks out of the barn on the side that has fewer cows and grazes on
    /// that side, so the herd stays balanced between the two teams' halves.
    fn replacement_cow(&mut self) -> Cow {
        let left = self.cows.iter().filter(|c| c.x < self.w * 0.5).count();
        let side = if left * 2 <= self.cows.len() { 0 } else { 1 };
        let x = self.barns[side];
        let goal = if side == 0 { self.rng.range(0.08, 0.46) } else { self.rng.range(0.54, 0.92) } * self.w;
        Cow { x, y: self.ground + 3.0, lift: 0.0, dir: if goal > x { 1.0 } else { -1.0 }, goal, walk: self.rng.range(0.0, TAU), graze: 0.0 }
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
            self.parts.push(Part {
                x: x + self.rng.range(-s, s),
                y,
                vx: self.rng.range(-4.0, 4.0),
                vy: -self.rng.range(2.0, 6.0),
                life,
                max: life,
                kind: PK::Smoke,
                col: Rgb::hex(0x3a3848),
            });
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

    fn dist(&self, a: usize, b: usize) -> f32 {
        let (p, q) = (&self.ships[a], &self.ships[b]);
        ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt()
    }

    /// Nearest enemy that is fully warped in.
    fn nearest_enemy(&self, i: usize) -> Option<(usize, f32)> {
        let me = &self.ships[i];
        let mut best: Option<(usize, f32)> = None;
        for (j, o) in self.ships.iter().enumerate() {
            if o.alive && o.team != me.team && o.warp >= 1.0 {
                let d = ((o.x - me.x).powi(2) + (o.y - me.y).powi(2)).sqrt();
                if best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((j, d));
                }
            }
        }
        best
    }

    /// Nearest cow nobody else is lifting (index, distance).
    fn nearest_cow(&self, i: usize) -> Option<(usize, f32)> {
        let me = &self.ships[i];
        let mut best: Option<(usize, f32)> = None;
        for (c, cow) in self.cows.iter().enumerate() {
            if !self.cow_free_for(c, i) {
                continue;
            }
            let d = ((cow.x - me.x).powi(2) + (cow.y - me.y).powi(2)).sqrt();
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((c, d));
            }
        }
        best
    }

    fn cow_free_for(&self, c: usize, i: usize) -> bool {
        c < self.cows.len() && !self.ships.iter().enumerate().any(|(j, o)| j != i && o.alive && o.abduct == Some(c))
    }

    /// The heuristic commander: what the built-in pilots did in v0.1, expressed as an order.
    fn builtin_order(&mut self, i: usize) -> Order {
        let range = self.s * 16.0;
        let me = self.ships[i].clone();
        let best = self.nearest_enemy(i);
        let threat_d = best.map_or(f32::MAX, |b| b.1);
        // sticky target: keep it unless a much closer enemy shows up
        let mut target = match me.order {
            Order::Attack(t) if t < self.ships.len() && self.ships[t].alive && self.ships[t].team != me.team => Some(t),
            _ => None,
        };
        if let Some((bj, bd)) = best {
            let keep = target.map(|t| self.dist(i, t));
            if keep.is_none_or(|kd| bd < kd * 0.6) {
                target = Some(bj);
            }
        }
        if let Order::Abduct(c) = me.order {
            let disturbed = threat_d < range * 0.9 || (me.hp < 100.0 && me.flash > 0.5);
            if self.cow_free_for(c, i) && !disturbed {
                return Order::Abduct(c);
            }
        } else if threat_d > range * 1.6 && me.bored <= 0.0 {
            let free: Vec<usize> = (0..self.cows.len()).filter(|&c| self.cows[c].lift <= 0.0 && self.cow_free_for(c, i)).collect();
            if !free.is_empty() {
                return Order::Abduct(free[self.rng.below(free.len())]);
            }
        }
        if me.hp < 35.0 && threat_d < range * 0.8 {
            return Order::Flee;
        }
        match target {
            Some(t) => Order::Attack(t),
            None => Order::Patrol,
        }
    }

    /// Fly one frame under the ship's order: pick a desired direction, maybe fire, then
    /// apply ally separation, world bounds and acceleration limits.
    fn execute(&mut self, i: usize, dt: f32) {
        let s = self.s;
        let n = self.ships.len();
        let me = self.ships[i].clone();
        let maxv = self.max_speed() * if me.team == 1 { 1.05 } else { 1.0 };
        let range = s * 16.0;
        let best = self.nearest_enemy(i);
        // validate the order against the world (targets die, cows get taken); degrade to hunt
        let mut order = match me.order {
            Order::Attack(t) if !(t < n && self.ships[t].alive && self.ships[t].team != me.team && self.ships[t].warp >= 1.0) => Order::Hunt,
            Order::Guard(a) if !(a < n && a != i && self.ships[a].alive && self.ships[a].team == me.team) => Order::Hunt,
            Order::Abduct(c) if !self.cow_free_for(c, i) => Order::Hunt,
            o => o,
        };
        let wander = |t: f32, id: usize| {
            let a = vnoise(t * 0.15, id as f32 * 7.3, 11) * TAU * 2.0;
            (a.cos(), a.sin() * 0.6)
        };
        let engage = |o: &Ship| {
            // lead pursuit until 75% of range, then orbit at half range
            let px = o.x + o.vx * 0.5;
            let py = o.y + o.vy * 0.5;
            let (dx, dy) = (px - me.x, py - me.y);
            let d = (dx * dx + dy * dy).sqrt().max(0.01);
            let (ux, uy) = (dx / d, dy / d);
            if d > range * 0.75 {
                (ux, uy)
            } else {
                let side = if me.id.is_multiple_of(2) { 1.0 } else { -1.0 };
                let radial = (d - range * 0.5) / (range * 0.5);
                (-uy * side + ux * radial, ux * side + uy * radial)
            }
        };
        let (mut dx, mut dy);
        let mut fire_at: Option<usize> = None;
        let mut abduct: Option<usize> = None;
        match order {
            Order::Abduct(c) => {
                abduct = Some(c);
                let cow = &self.cows[c];
                let hover_y = self.ground - s * 7.0;
                dx = cow.x - me.x;
                dy = hover_y - me.y;
                let d = (dx * dx + dy * dy).sqrt();
                if d < s * 1.5 {
                    dx *= 0.2;
                    dy *= 0.2;
                    self.cows[c].lift += dt * 0.5;
                    if self.cows[c].lift >= 1.0 {
                        self.rings.push(Ring { x: me.x, y: me.y, r: s, speed: s * 3.0, life: 0.5, col: TEAM_COL[me.team] });
                        // the herd is replenished from the barn on the emptier side
                        self.cows[c] = self.replacement_cow();
                        self.cows_taken[me.team] += 1;
                        self.mlog(&format!("cow: {} S{} abducted C{c}", TEAM_NAME[me.team], me.id));
                        abduct = None;
                        order = Order::Hunt;
                        self.ships[i].bored = self.rng.range(8.0, 20.0);
                        self.event(me.team, format!("your S{} abducted cow C{c}", me.id));
                        self.event(1 - me.team, format!("enemy E{} abducted cow C{c}", me.id));
                    }
                }
                // fire back if attacked while beaming
                fire_at = best.filter(|&(_, d)| d < range * 0.5).map(|b| b.0);
            }
            Order::Flee => match best {
                Some((e, _)) => {
                    let o = &self.ships[e];
                    let (fx, fy) = (me.x - o.x, me.y - o.y);
                    let nn = (fx * fx + fy * fy).sqrt().max(0.01);
                    let jink = (self.t * 7.0 + me.phase).sin();
                    let (ux, uy) = (fx / nn, fy / nn);
                    dx = ux - uy * jink * 0.8;
                    dy = uy + ux * jink * 0.8;
                }
                None => (dx, dy) = wander(self.t, me.id),
            },
            Order::Attack(t) => {
                (dx, dy) = engage(&self.ships[t]);
                fire_at = Some(t);
            }
            Order::Hunt => match best {
                Some((e, _)) => {
                    (dx, dy) = engage(&self.ships[e]);
                    fire_at = Some(e);
                }
                None => (dx, dy) = wander(self.t, me.id),
            },
            Order::Guard(a) => {
                let o = &self.ships[a];
                let ang = me.id as f32 * 2.1 + self.t * 0.3;
                let (gx, gy) = (o.x + ang.cos() * s * 3.5, o.y + ang.sin() * s * 2.0);
                dx = gx - me.x;
                dy = gy - me.y;
                let d = (dx * dx + dy * dy).sqrt().max(0.01);
                let k = (d / (s * 3.0)).min(1.0);
                dx = dx / d * k;
                dy = dy / d * k;
                fire_at = best.filter(|&(_, d)| d < range).map(|b| b.0);
            }
            Order::Patrol => {
                (dx, dy) = wander(self.t, me.id);
                fire_at = best.filter(|&(_, d)| d < range).map(|b| b.0);
            }
        }
        // fire
        self.ships[i].cooldown -= dt;
        if let Some(t) = fire_at {
            let o = self.ships[t].clone();
            let d = self.dist(i, t);
            if d < range && self.ships[i].cooldown <= 0.0 && me.warp >= 1.0 {
                let bs = s * 22.0;
                let tt = d / bs;
                let ax = o.x + o.vx * tt - me.x;
                let ay = o.y + o.vy * tt - me.y;
                let an = ay.atan2(ax) + self.rng.gauss() * 0.12;
                self.bolts.push(Bolt { x: me.x, y: me.y, vx: an.cos() * bs, vy: an.sin() * bs, life: 1.6, team: me.team, from: i });
                self.ships[i].cooldown = self.rng.range(0.45, 1.2);
            }
        }
        self.ships[i].order = order;
        self.ships[i].abduct = abduct;
        self.ships[i].target = fire_at;
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
        let nn = (dx * dx + dy * dy).sqrt().max(0.001);
        let (tx, ty) = (dx / nn * maxv, dy / nn * maxv);
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
        // motion trail sample every 60 ms
        sh.trail_t += dt;
        if sh.trail_t > 0.06 {
            sh.trail_t = 0.0;
            if sh.trail.len() >= TRAIL {
                sh.trail.remove(0);
            }
            sh.trail.push((sh.x, sh.y));
        }
    }

    fn event(&mut self, team: usize, what: String) {
        if let Some(lm) = self.lm.as_mut() {
            let ev = &mut lm.events[team];
            if ev.len() >= 6 {
                ev.remove(0);
            }
            ev.push(what);
        }
    }

    /// One line per match event in ~/.local/state/reverie/match.log (lm mode only; a few
    /// lines a minute, so it costs nothing and shows exactly how a game went).
    fn mlog(&self, what: &str) {
        if self.lm.is_none() {
            return;
        }
        use std::io::Write;
        let p = state_dir().join("match.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{:7.1}s game {} | {what}", self.t, self.game.n);
        }
    }

    fn show_banner(&mut self, text: &str, col: Rgb, secs: f32) {
        self.banner = text.to_string();
        self.banner_t = secs;
        self.banner_col = col;
    }

    /// Evolve mode: tell the losing commander exactly how its saucer died.
    fn post_mortem(&mut self, j: usize, killer: usize) {
        // only for saucers the model itself was commanding, and only in evolve mode
        if !self.commanded(self.ships[j].team) || !self.lm.as_ref().is_some_and(|l| l.evolve) {
            return;
        }
        let range = self.s * 16.0;
        let k = 100.0 / self.w;
        let v = self.ships[j].clone();
        let d = self.dist(j, killer);
        let enemies_near = self.ships.iter().filter(|o| o.alive && o.team != v.team && ((o.x - v.x).powi(2) + (o.y - v.y).powi(2)).sqrt() < range).count();
        let allies_near =
            self.ships.iter().filter(|o| o.alive && o.team == v.team && o.id != v.id && ((o.x - v.x).powi(2) + (o.y - v.y).powi(2)).sqrt() < range).count();
        let killer_hp = self.ships[killer].hp.round().max(1.0) as i32;
        let line = format!(
            "{{\"t\":\"loss\",\"kind\":\"ship\",\"team\":{},\"ship\":{},\"killer\":{},\"killer_hp\":{killer_hp},\"dist\":{},\"range\":{},\"order\":{},\"enemies_near\":{enemies_near},\"allies_near\":{allies_near},\"low_for\":{:.0},\"alive_for\":{:.0},\"x\":{},\"y\":{},\"ground\":{}}}",
            v.team,
            v.id,
            self.ships[killer].id,
            (d * k).round() as i32,
            (range * k).round() as i32,
            json_str(&v.order.describe()),
            v.low_for,
            self.t - v.born,
            (v.x * k).round() as i32,
            (v.y * k).round() as i32,
            (self.ground * k).round() as i32
        );
        if let Some(link) = self.lm.as_mut().and_then(|l| l.link.as_mut()) {
            link.send(&line);
        }
    }

    /// Evolve mode: the game is lost; a bigger-picture lesson.
    fn post_mortem_game(&mut self, team: usize) {
        if !self.commanded(team) || !self.lm.as_ref().is_some_and(|l| l.evolve) {
            return;
        }
        let foes: Vec<String> =
            self.ships.iter().filter(|o| o.alive && o.team != team).map(|o| format!("E{} ({} hp)", o.id, o.hp.round().max(1.0) as i32)).collect();
        let lm = self.lm.as_ref().unwrap();
        let line = format!(
            "{{\"t\":\"loss\",\"kind\":\"round\",\"team\":{team},\"round\":{},\"kills\":{},\"losses\":{},\"foes_left\":{},\"cows\":{},\"regens_left\":{}}}",
            self.game.n,
            lm.game_kills[team],
            lm.game_kills[1 - team],
            json_str(&foes.join(", ")),
            self.cows_taken[team],
            self.game.regens[team]
        );
        if let Some(link) = self.lm.as_mut().and_then(|l| l.link.as_mut()) {
            link.send(&line);
        }
    }

    fn alive_count(&self, team: usize) -> usize {
        self.ships.iter().filter(|s| s.team == team && s.alive).count()
    }

    /// The persistent scoreboard between these two models (loaded once both are known).
    fn load_scoreboard(&mut self) {
        let Some(lm) = self.lm.as_ref() else { return };
        let (Some(a), Some(b)) = (&lm.ready[0], &lm.ready[1]) else { return };
        let path = state_dir().join(format!("score-{a}-vs-{b}.txt"));
        if let Ok(text) = std::fs::read_to_string(&path) {
            for l in text.lines() {
                let f: Vec<u32> = l.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
                if f.len() < 2 {
                    continue;
                }
                match l.split_whitespace().next() {
                    Some("wins") => self.game.wins = [f[0], f[1]],
                    Some("kills") => self.game.total_kills = [f[0], f[1]],
                    Some("cows") => self.game.total_cows = [f[0], f[1]],
                    Some("games") => self.game.n = f[0].max(1),
                    _ => {}
                }
            }
        }
        self.game.score_path = Some(path);
    }

    fn save_scoreboard(&self) {
        let Some(p) = &self.game.score_path else { return };
        let g = &self.game;
        let s = format!(
            "# reverie: {} vs {} — games won, lifetime kills and cows\nwins {} {}\nkills {} {}\ncows {} {}\ngames {} {}\n",
            TEAM_NAME[0],
            TEAM_NAME[1],
            g.wins[0],
            g.wins[1],
            g.total_kills[0] + self.score[0],
            g.total_kills[1] + self.score[1],
            g.total_cows[0] + self.cows_taken[0],
            g.total_cows[1] + self.cows_taken[1],
            g.n,
            g.n
        );
        let _ = std::fs::create_dir_all(state_dir());
        let _ = std::fs::write(p, s);
    }

    /// Fitness of a team over the current evaluation window: kills − losses + ½ cows (+ win
    /// bonus), per minute of play.
    fn window_fitness(&self, team: usize, bonus: f32) -> f32 {
        let lm = self.lm.as_ref().unwrap();
        let b = lm.seg_base[team];
        let dk = self.score[team].saturating_sub(b[0]) as f32;
        let dl = self.game.losses[team].saturating_sub(b[1]) as f32;
        let dc = self.cows_taken[team].saturating_sub(b[2]) as f32;
        let minutes = (lm.seg_t / 60.0).max(0.25);
        (dk - dl + 0.5 * dc + bonus) / minutes
    }

    /// Close the evaluation window for both teams (genetic mode): score the active doctrines,
    /// move on to the next candidates, breed when a generation is complete.
    fn close_window(&mut self, winner: Option<usize>) {
        if self.lm.as_ref().is_none_or(|l| l.pop.is_none()) {
            return;
        }
        let fits = [0, 1].map(|t| {
            self.window_fitness(
                t,
                match winner {
                    Some(w) if w == t => 6.0,
                    Some(_) => -6.0,
                    None => 0.0,
                },
            )
        });
        let mut bred = [false; 2];
        {
            let lm = self.lm.as_mut().unwrap();
            let pop = lm.pop.as_mut().unwrap();
            for t in 0..2 {
                bred[t] = pop[t].score(fits[t], &mut self.rng);
            }
            lm.seg_t = 0.0;
            lm.seg_base = [[self.score[0], self.game.losses[0], self.cows_taken[0]], [self.score[1], self.game.losses[1], self.cows_taken[1]]];
        }
        for (t, &bred_now) in bred.iter().enumerate() {
            let (gen, d) = {
                let pop = &self.lm.as_ref().unwrap().pop.as_ref().unwrap()[t];
                (pop.gen, pop.active())
            };
            if bred_now {
                self.event(t, format!("evolution: generation {gen} bred; your doctrine is now: {}", d.describe()));
            } else {
                self.event(t, format!("evolution: new doctrine under test: {}", d.describe()));
            }
        }
    }

    /// Start the next game: the loser of the last one gets an extra saucer.
    fn start_game(&mut self) {
        self.game.total_kills[0] += self.score[0];
        self.game.total_kills[1] += self.score[1];
        self.game.total_cows[0] += self.cows_taken[0];
        self.game.total_cows[1] += self.cows_taken[1];
        self.score = [0; 2];
        self.cows_taken = [0; 2];
        self.game.losses = [0; 2];
        self.game.n += 1;
        self.game.regens = [self.game.regens_total; 2];
        self.bolts.clear();
        for i in 0..self.ships.len() {
            self.ships[i].alive = false;
            self.ships[i].respawn = REINFORCE; // spare slots wait the full delay, no instant refills
        }
        for team in 0..2 {
            let n = self.game.max_alive + if self.game.handicap == Some(team) { 1 } else { 0 };
            let slots: Vec<usize> = self.ships.iter().enumerate().filter(|(_, s)| s.team == team).map(|(i, _)| i).take(n).collect();
            for i in slots {
                self.deploy(i);
            }
        }
        if let Some(lm) = self.lm.as_mut() {
            lm.game_kills = [0; 2];
            lm.seg_t = 0.0;
            lm.seg_base = [[0; 3]; 2];
        }
        let text = format!("GAME {}", self.game.n);
        self.show_banner(&text, Rgb::WHITE, 3.0);
        self.mlog(&format!(
            "start: ZORB {} saucers, KRELL {} saucers, {} reinforcements each; games won {}-{}",
            self.alive_count(0),
            self.alive_count(1),
            self.game.regens_total,
            self.game.wins[0],
            self.game.wins[1]
        ));
        for team in 0..2 {
            let extra = if self.game.handicap == Some(team) { " (one extra saucer for losing the last game)" } else { "" };
            self.event(
                team,
                format!(
                    "game {} begins: {} saucers{extra}, {} reinforcements",
                    self.game.n,
                    self.game.max_alive + usize::from(self.game.handicap == Some(team)),
                    self.game.regens_total
                ),
            );
        }
        self.save_scoreboard();
    }

    /// Match bookkeeping (lm mode): reinforcements, game over, next game.
    #[allow(clippy::needless_range_loop)]
    fn fleet(&mut self, dt: f32) {
        if self.game.pause > 0.0 {
            self.game.pause -= dt;
            if self.game.pause <= 0.0 {
                self.game.pause = 0.0;
                self.start_game();
            }
            return;
        }
        if self.game.score_path.is_none() {
            self.load_scoreboard();
        }
        // genetic evaluation window
        if self.lm.as_ref().is_some_and(|l| l.pop.is_some()) {
            let lm = self.lm.as_mut().unwrap();
            lm.seg_t += dt;
            if lm.seg_t >= SEGMENT {
                self.close_window(None);
            }
        }
        for team in 0..2 {
            if !self.ships.iter().any(|s| s.team == team) {
                continue;
            }
            // game over: nothing in the air and no reinforcement will come (the budget is spent)
            if self.alive_count(team) == 0 && self.game.regens[team] == 0 {
                let winner = 1 - team;
                self.mlog(&format!(
                    "over: {} WINS (kills {}-{}, cows {}-{}, {} reinforcements left for the winner)",
                    TEAM_NAME[winner], self.score[0], self.score[1], self.cows_taken[0], self.cows_taken[1], self.game.regens[winner]
                ));
                self.game.wins[winner] += 1;
                self.game.handicap = Some(team);
                self.game.pause = GAME_PAUSE;
                let text = format!("{} WINS GAME {}", TEAM_NAME[winner], self.game.n);
                self.show_banner(&text, TEAM_COL[winner], GAME_PAUSE);
                self.event(team, format!("you lost game {}: no saucer left in the air (reinforcements left: {})", self.game.n, self.game.regens[team]));
                self.event(winner, format!("you won game {}: the enemy has no saucer left", self.game.n));
                if let Some(lm) = self.lm.as_mut() {
                    lm.cry_due[team] = true;
                }
                self.post_mortem_game(team);
                self.close_window(Some(winner));
                self.save_scoreboard();
                return;
            }
            // reinforcements: only up to max_alive on screen, only while the budget lasts
            let due: Vec<usize> = (0..self.ships.len()).filter(|&i| self.ships[i].team == team && !self.ships[i].alive).collect();
            for i in due {
                if self.alive_count(team) >= self.game.max_alive || self.game.regens[team] == 0 {
                    break;
                }
                self.ships[i].respawn -= dt;
                if self.ships[i].respawn <= 0.0 {
                    self.deploy(i);
                    self.game.regens[team] -= 1;
                    let (id, left) = (self.ships[i].id, self.game.regens[team]);
                    self.event(team, format!("reinforcement S{id} warped in ({left} left this game)"));
                    self.mlog(&format!("reinforcement: {} S{id} ({left} left, {} in the air)", TEAM_NAME[team], self.alive_count(team)));
                }
            }
        }
    }

    /// Genetic mode: an order the model gave, checked against the team's doctrine. Returns the
    /// order that is actually flown and whether it had to be corrected.
    fn doctrine(&self, team: usize) -> Doctrine {
        self.lm.as_ref().and_then(|l| l.pop.as_ref()).map(|p| p[team].active()).unwrap_or_else(Doctrine::default_doctrine)
    }

    /// The team's doctrine, checked against an order the model gave. Always on (with the default
    /// doctrine unless `evolve` is breeding one): small models drift into fleeing with healthy
    /// ships or beaming cows under fire, and the bounds keep both sides playing the same game.
    /// Returns the order that is actually flown and whether it had to be corrected.
    fn apply_doctrine(&self, i: usize, order: Order, fleeing: usize) -> (Order, bool) {
        let d = self.doctrine(self.ships[i].team);
        let me = &self.ships[i];
        let range = self.s * 16.0;
        let threat = self.nearest_enemy(i).map_or(f32::MAX, |b| b.1);
        let allowed_fleeing = ((d.courage * self.alive_count(me.team) as f32).ceil() as usize).max(1);
        // 1. a badly damaged ship with an enemy in range must flee
        if me.hp < d.flee_hp && threat < range && !matches!(order, Order::Flee) {
            return (Order::Flee, true);
        }
        match order {
            // 2. no fleeing at high hp, and not the whole fleet at once
            Order::Flee if me.hp > d.brave_hp || fleeing >= allowed_fleeing => (Order::Hunt, true),
            // 3. abducting is the model's own call: the doctrine only advises on it (what a
            //    commander does with cows says a lot about it), so it is never corrected
            Order::Abduct(_) => (order, false),
            // 4. focus fire: sometimes redirect to the weakest enemy in range
            Order::Attack(t) => {
                let weakest = self
                    .ships
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| o.alive && o.team != me.team && o.warp >= 1.0 && ((o.x - me.x).powi(2) + (o.y - me.y).powi(2)).sqrt() < range)
                    .min_by(|a, b| a.1.hp.partial_cmp(&b.1.hp).unwrap());
                match weakest {
                    Some((w, o)) if w != t && o.hp < 60.0 && (o.id as f32 * 0.37 + self.t).fract() < d.focus => (Order::Attack(w), true),
                    _ => (order, false),
                }
            }
            o => (o, false),
        }
    }

    // ------------------------------------------------------------ language-model commanders

    /// Everything one commander gets to see, as one JSON line. Coordinates are in
    /// percent of the field width so the numbers are small and comparable.
    fn observation(&self, team: usize, tick: u32) -> String {
        let k = 100.0 / self.w;
        let pc = |v: f32| (v * k).round() as i32;
        let lm = self.lm.as_ref().unwrap();
        let mut o = String::with_capacity(1024);
        let doctrine = self.doctrine(team).describe();
        let gen = lm.pop.as_ref().map_or(0, |p| p[team].gen);
        o.push_str(&format!(
            "{{\"t\":\"obs\",\"team\":{team},\"tick\":{tick},\"score\":[{},{}],\"cows_taken\":[{},{}],\"rounds\":[{},{}],\"round\":{},\"regens\":[{},{}],\"alive\":[{},{}],\"max_alive\":{},\"doctrine\":{},\"gen\":{gen},\"corrected\":{},\"fh\":{},\"ground\":{},\"range\":{},\"mine\":[",
            self.score[0],
            self.score[1],
            self.cows_taken[0],
            self.cows_taken[1],
            self.game.wins[0],
            self.game.wins[1],
            self.game.n,
            self.game.regens[0],
            self.game.regens[1],
            self.alive_count(0),
            self.alive_count(1),
            self.game.max_alive,
            json_str(&doctrine),
            lm.corrected[team],
            pc(self.h),
            pc(self.ground),
            pc(self.s * 16.0)
        ));
        let mut first = true;
        for (i, s) in self.ships.iter().enumerate() {
            if s.team != team || !s.alive {
                continue;
            }
            let (near, dist) = self.nearest_enemy(i).map_or((-1, -1), |(e, d)| (e as i32, pc(d)));
            let (cow, cowd) = self.nearest_cow(i).map_or((-1, -1), |(c, d)| (c as i32, pc(d)));
            if !first {
                o.push(',');
            }
            first = false;
            o.push_str(&format!(
                "{{\"id\":{},\"x\":{},\"y\":{},\"hp\":{},\"near\":{near},\"dist\":{dist},\"cow\":{cow},\"cowd\":{cowd},\"order\":{}}}",
                s.id,
                pc(s.x),
                pc(s.y),
                s.hp.round().max(1.0) as i32,
                json_str(&s.order.describe())
            ));
        }
        o.push_str("],\"foes\":[");
        first = true;
        for s in self.ships.iter() {
            if s.team == team || !s.alive || s.warp < 1.0 {
                continue;
            }
            let tg = s.target.filter(|&t| self.ships[t].team == team).map_or(-1, |t| t as i32);
            if !first {
                o.push(',');
            }
            first = false;
            o.push_str(&format!("{{\"id\":{},\"x\":{},\"y\":{},\"hp\":{},\"target\":{tg}}}", s.id, pc(s.x), pc(s.y), s.hp.round().max(1.0) as i32));
        }
        o.push_str("],\"cows\":[");
        first = true;
        for (ci, c) in self.cows.iter().enumerate() {
            let state = match self.ships.iter().find(|o| o.alive && o.abduct == Some(ci)) {
                Some(l) if l.team == team => format!("being lifted by your S{}", l.id),
                Some(l) => format!("being lifted by enemy E{}", l.id),
                None => "free".into(),
            };
            if !first {
                o.push(',');
            }
            first = false;
            o.push_str(&format!("{{\"id\":{ci},\"x\":{},\"state\":{}}}", pc(c.x), json_str(&state)));
        }
        o.push_str("],\"events\":[");
        o.push_str(&lm.events[team].iter().map(|e| json_str(e)).collect::<Vec<_>>().join(","));
        o.push_str(&format!("],\"foe_say\":{},\"cry\":{}}}", json_str(&lm.say[1 - team]), lm.cry_due[team]));
        o
    }

    fn lm_poll(&mut self) {
        let msgs = match self.lm.as_mut().and_then(|l| l.link.as_mut()) {
            Some(link) => link.poll(),
            None => return,
        };
        let mut to_apply: Vec<(usize, Vec<(usize, Order)>)> = vec![];
        let lm = self.lm.as_mut().unwrap();
        for m in msgs {
            match m {
                Msg::Status(s) => lm.status = format!("arena: {s}"),
                Msg::Ready { team, label, gb } => {
                    lm.ready[team] = Some(label);
                    lm.status = if lm.ready.iter().all(|r| r.is_some()) {
                        format!("arena: both commanders online, {gb:.1} GB")
                    } else {
                        format!("arena: {} online", TEAM_NAME[team])
                    };
                    lm.since[team] = lm.think; // ask right away
                }
                Msg::Orders { team, orders, say } => {
                    lm.pending[team] = false;
                    lm.wait[team] = 0.0;
                    if !say.is_empty() {
                        lm.say[team] = say;
                        lm.say_age[team] = 0.0;
                    }
                    lm.status.clear();
                    lm.corrected[team] = 0;
                    to_apply.push((team, orders));
                }
                Msg::Lesson { team, text } => {
                    lm.lesson[team] = text;
                    lm.lesson_age[team] = 0.0;
                }
                Msg::Error(e) => {
                    lm.status = format!("arena error: {e} (built-in pilots)");
                    lm.link = None;
                    lm.ready = [None, None];
                    break;
                }
                Msg::Exited => {
                    if lm.link.is_some() {
                        lm.status = "arena exited (see arena.log); built-in pilots".into();
                        lm.link = None;
                        lm.ready = [None, None];
                    }
                    break;
                }
            }
        }
        for (team, orders) in to_apply {
            let mut fleeing = self.ships.iter().filter(|s| s.team == team && s.alive && matches!(s.order, Order::Flee)).count();
            for (id, o) in orders {
                let Some(i) = self.ships.iter().position(|s| s.id == id && s.team == team && s.alive) else { continue };
                // an abduction in progress is seen through (the model re-plans every second,
                // a cow takes longer than that): keep it unless the ship is hurt with an enemy
                // in range, the cow is gone, or it has dragged on for 15 s
                if let Order::Abduct(c) = self.ships[i].order {
                    let valid = self.cow_free_for(c, i) && self.ships[i].order_t < 15.0;
                    let danger = self.ships[i].hp < 35.0 && self.nearest_enemy(i).is_some_and(|(_, d)| d < self.s * 16.0);
                    if valid && !danger {
                        continue;
                    }
                }
                // the menu has no patrol/guard; treat them as hunt
                let o = match o {
                    Order::Patrol | Order::Guard(_) => Order::Hunt,
                    o => o,
                };
                let was_fleeing = matches!(self.ships[i].order, Order::Flee);
                let (o, corrected) = self.apply_doctrine(i, o, fleeing.saturating_sub(usize::from(was_fleeing)));
                if corrected {
                    if let Some(lm) = self.lm.as_mut() {
                        lm.corrected[team] += 1;
                    }
                }
                fleeing = fleeing.saturating_sub(usize::from(was_fleeing)) + usize::from(matches!(o, Order::Flee));
                if self.ships[i].order != o {
                    self.ships[i].order_t = 0.0;
                }
                self.ships[i].order = o;
            }
        }
    }

    fn lm_send(&mut self, dt: f32) {
        if !self.lm_mode() {
            return;
        }
        let mut todo = vec![];
        {
            let lm = self.lm.as_mut().unwrap();
            for team in 0..2 {
                lm.since[team] += dt;
                lm.lesson_age[team] += dt;
                lm.say_age[team] += dt;
                if lm.pending[team] {
                    lm.wait[team] += dt;
                    if lm.wait[team] > 60.0 {
                        lm.pending[team] = false; // the sidecar is stuck; try again
                    }
                    continue;
                }
                if lm.ready[team].is_some() && lm.since[team] >= lm.think {
                    lm.tick[team] += 1;
                    todo.push((team, lm.tick[team]));
                }
            }
        }
        for (team, tick) in todo {
            // nothing to command between games (the cry request still goes through once)
            if self.game.pause > 0.0 && !self.lm.as_ref().unwrap().cry_due[team] {
                continue;
            }
            let obs = self.observation(team, tick);
            let lm = self.lm.as_mut().unwrap();
            if lm.link.as_mut().is_some_and(|l| l.send(&obs)) {
                lm.pending[team] = true;
                lm.wait[team] = 0.0;
                lm.since[team] = 0.0;
                lm.events[team].clear();
                lm.cry_due[team] = false;
            }
        }
    }

    // ------------------------------------------------------------ drawing helpers

    fn draw_saucer(&self, cv: &mut Canvas, o: &Ship) {
        let (t, s) = (self.t, self.s);
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
        // motion trail
        let speed = (o.vx * o.vx + o.vy * o.vy).sqrt() / self.max_speed();
        if o.trail.len() >= 2 && speed > 0.25 {
            let n = o.trail.len();
            for k in 1..n {
                let (x0, y0) = o.trail[k - 1];
                let (x1, y1) = o.trail[k];
                let f = k as f32 / n as f32;
                cv.line_add(x0, y0, x1, y1, tc.scale(0.16 * f * speed * a));
            }
        }
        let rot = (o.vx / self.max_speed()).clamp(-1.0, 1.0) * 0.3;
        let (sn, cs) = rot.sin_cos();
        let (rx, ry) = (s, s * 0.34);
        // engine glow, stronger when moving fast
        let pulse = 0.7 + 0.3 * (t * 5.0 + o.phase).sin();
        cv.glow(o.x, o.y + ry * 1.3, s * (0.9 + 0.5 * speed), tc.scale((0.24 + 0.2 * speed) * pulse * a));
        // hull: dark underside, bright upper deck, team stripe between
        cv.ellipse(o.x, o.y + ry * 0.15, rx, ry, rot, STEEL_LO.scale(0.8), a);
        let (hx, hy) = (o.x + sn * ry * 0.35, o.y - cs * ry * 0.35);
        cv.ellipse(hx, hy, rx * 0.92, ry * 0.62, rot, STEEL_HI.lerp(tc, 0.12), 0.9 * a);
        cv.ellipse(hx, hy + ry * 0.35, rx * 0.86, ry * 0.18, rot, tc.scale(0.75), 0.8 * a);
        // deck highlight (light from the upper left)
        cv.ellipse(hx - rx * 0.28, hy - ry * 0.18, rx * 0.4, ry * 0.2, rot, Rgb::WHITE, 0.22 * a);
        // dome with a specular
        let d = ry * 1.15;
        let (dx, dy) = (o.x + sn * d, o.y - cs * d);
        cv.ellipse(dx, dy, s * 0.46, s * 0.42, rot, tc.lerp(Rgb::WHITE, 0.3), 0.9 * a);
        cv.ellipse(dx + s * 0.05, dy + s * 0.1, s * 0.3, s * 0.2, rot, tc.scale(0.6), 0.5 * a);
        cv.splat(dx - s * 0.16, dy - s * 0.16, Rgb::WHITE, 0.85 * a);
        // chasing rim lights along the stripe
        let lights = 5;
        for k in 0..lights {
            let f = (k as f32 / (lights - 1) as f32) * 2.0 - 1.0;
            let lx = f * rx * 0.78;
            let ly = ry * 0.5;
            let (px, py) = (o.x + lx * cs - ly * sn, o.y + lx * sn + ly * cs);
            let on = ((t * 6.0 + k as f32 * 1.3 + o.phase).sin() * 0.5 + 0.5).powi(2);
            cv.splat_add(px, py, tc.lerp(Rgb::WHITE, 0.3).scale((0.3 + 0.9 * on) * a));
        }
        // shield shimmer when hit
        if o.flash > 0.0 {
            let f = o.flash;
            cv.ellipse(o.x, o.y, rx * 1.35, ry * 2.6, rot, tc.lerp(Rgb::WHITE, 0.6), 0.28 * f);
            cv.glow(o.x, o.y, s * 1.6, Rgb::WHITE.scale(0.3 * f));
        }
        // damage smoke tint
        if o.hp < 45.0 {
            cv.splat(o.x - rx * 0.4, o.y - ry * 0.3, Rgb::hex(0x2a2530), 0.35 * (1.0 - o.hp / 45.0));
        }
    }

    /// 3x5 pixel font for the title card and banners.
    fn glyph(c: char) -> [&'static str; 5] {
        match c {
            'A' => ["010", "101", "111", "101", "101"],
            'B' => ["110", "101", "110", "101", "110"],
            'C' => ["011", "100", "100", "100", "011"],
            'D' => ["110", "101", "101", "101", "110"],
            'E' => ["111", "100", "110", "100", "111"],
            'F' => ["111", "100", "110", "100", "100"],
            'G' => ["011", "100", "101", "101", "011"],
            'H' => ["101", "101", "111", "101", "101"],
            'I' => ["111", "010", "010", "010", "111"],
            'J' => ["001", "001", "001", "101", "010"],
            'K' => ["101", "101", "110", "101", "101"],
            'L' => ["100", "100", "100", "100", "111"],
            'M' => ["101", "111", "111", "101", "101"],
            'N' => ["110", "101", "101", "101", "101"],
            'O' => ["010", "101", "101", "101", "010"],
            'P' => ["110", "101", "110", "100", "100"],
            'Q' => ["010", "101", "101", "011", "001"],
            'R' => ["110", "101", "110", "101", "101"],
            'S' => ["011", "100", "010", "001", "110"],
            'T' => ["111", "010", "010", "010", "010"],
            'U' => ["101", "101", "101", "101", "111"],
            'V' => ["101", "101", "101", "101", "010"],
            'W' => ["101", "101", "111", "111", "101"],
            'X' => ["101", "101", "010", "101", "101"],
            'Y' => ["101", "101", "010", "010", "010"],
            'Z' => ["111", "001", "010", "100", "111"],
            '0' => ["111", "101", "101", "101", "111"],
            '1' => ["010", "110", "010", "010", "111"],
            '2' => ["111", "001", "111", "100", "111"],
            '3' => ["111", "001", "111", "001", "111"],
            '4' => ["101", "101", "111", "001", "001"],
            '5' => ["111", "100", "111", "001", "111"],
            '6' => ["111", "100", "111", "101", "111"],
            '7' => ["111", "001", "001", "001", "001"],
            '8' => ["111", "101", "111", "101", "111"],
            '9' => ["111", "101", "111", "001", "111"],
            '-' => ["000", "000", "111", "000", "000"],
            _ => ["000", "000", "000", "000", "000"],
        }
    }

    fn text_width(text: &str, fs: i32) -> i32 {
        text.len() as i32 * 4 * fs - fs
    }

    fn big_text(cv: &mut Canvas, text: &str, x: i32, y: i32, fs: i32, col: Rgb, a: f32) {
        let mut cx = x;
        for ch in text.chars() {
            let g = Self::glyph(ch.to_ascii_uppercase());
            for (r, row) in g.iter().enumerate() {
                for (c, bit) in row.bytes().enumerate() {
                    if bit == b'1' {
                        cv.rect(cx + c as i32 * fs, y + r as i32 * fs, fs, fs, col, a);
                    }
                }
            }
            cx += 4 * fs;
        }
    }

    fn big_text_centered(&self, cv: &mut Canvas, text: &str, y: i32, fs: i32, col: Rgb, a: f32) {
        let tw = Self::text_width(text, fs);
        let x = (self.w as i32 - tw) / 2;
        // soft dark plate behind the letters for legibility
        cv.rect(x - fs * 2, y - fs, tw + fs * 4, 5 * fs + fs * 2, Rgb::hex(0x04050d), 0.45 * a);
        Self::big_text(cv, text, x, y, fs, col, a);
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
        // the farm: barn on the left, ten cows spread evenly over the field
        let inset = (self.w * 0.06).max(self.s * 2.0);
        self.barns = [inset, self.w - inset];
        self.cows.clear();
        for i in 0..NCOWS {
            let x = self.w * (0.10 + 0.82 * (i as f32 + 0.5) / NCOWS as f32) + self.rng.range(-2.0, 2.0);
            let mut c = self.new_cow(x);
            c.goal = x;
            c.graze = self.rng.range(2.0, 12.0);
            self.cows.push(c);
        }
        self.cars.clear();
        self.clouds = (0..3)
            .map(|k| Cloud {
                x: self.w * (0.15 + 0.3 * k as f32) + self.rng.range(-8.0, 8.0),
                y: self.ground * self.rng.range(0.22, 0.42),
                rx: self.s * self.rng.range(2.2, 3.6),
                ry: self.s * self.rng.range(0.5, 0.8),
                v: self.rng.range(0.35, 0.7),
                seed: self.rng.next_u64() as u32,
            })
            .collect();
        if self.ships.is_empty() {
            let lm = self.lm.is_some();
            let per = if lm { LM_SLOTS } else { (w / 45).clamp(2, 5) };
            let mut id = 0;
            for team in 0..2 {
                for k in 0..per {
                    let mut s = self.spawn_ship(team, id);
                    // lm: only max_alive saucers start; the spare slot is for the loser's extra one
                    if lm && k >= self.game.max_alive {
                        s.alive = false;
                        s.respawn = REINFORCE;
                    }
                    self.ships.push(s);
                    id += 1;
                }
            }
        } else if ow > 1.0 {
            for s in self.ships.iter_mut() {
                s.x *= self.w / ow;
                s.y *= self.h / oh;
                s.trail.clear();
            }
        }
    }

    fn update(&mut self, rdt: f32) {
        let dt = rdt;
        self.title = (self.title - rdt).max(0.0);
        self.banner_t = (self.banner_t - rdt).max(0.0);
        self.t += dt;
        let s = self.s;
        self.lm_poll();
        let lm_mode = self.lm_mode();
        for i in 0..self.ships.len() {
            if self.ships[i].alive {
                if self.ships[i].warp < 1.0 {
                    self.ships[i].warp = (self.ships[i].warp + dt * 0.9).min(1.0);
                }
                if !self.commanded(self.ships[i].team) {
                    self.ships[i].order = self.builtin_order(i);
                }
                self.execute(i, dt);
                let sh = &mut self.ships[i];
                sh.flash = (sh.flash - dt * 4.0).max(0.0);
                sh.order_t += dt;
                if sh.hp < 35.0 {
                    sh.low_for += dt;
                }
                if !matches!(sh.order, Order::Abduct(_)) {
                    sh.bored -= dt;
                }
                if sh.hp < 45.0 && self.rng.chance(dt * 12.0) {
                    let (x, y) = (sh.x, sh.y);
                    let life = self.rng.range(0.8, 1.6);
                    self.parts.push(Part {
                        x,
                        y,
                        vx: self.rng.range(-2.0, 2.0),
                        vy: -self.rng.range(2.0, 5.0),
                        life,
                        max: life,
                        kind: PK::Smoke,
                        col: Rgb::hex(0x4a4658),
                    });
                }
            } else if !lm_mode {
                // built-in pilots: automatic reinforcements. In lm mode `fleet` decides.
                self.ships[i].respawn -= dt;
                if self.ships[i].respawn <= 0.0 {
                    self.deploy(i);
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
            let (bx, by, team, from) = (b.x, b.y, b.team, b.from);
            let mut dead = b.life <= 0.0 || bx < -5.0 || bx > self.w + 5.0 || by < -5.0;
            if by >= self.ground {
                dead = true;
                self.sparks(bx, self.ground, Rgb::hex(0xffc070), 5);
                self.rings.push(Ring { x: bx, y: self.ground, r: 0.5, speed: s * 2.0, life: 0.25, col: TEAM_COL[team].scale(0.5) });
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
                        let (ox, oy, ot, oid) = (o.x, o.y, o.team, o.id);
                        let killed = o.hp <= 0.0;
                        self.sparks(bx, by, TEAM_COL[team].lerp(Rgb::WHITE, 0.5), 7);
                        if killed {
                            self.post_mortem(j, from);
                            let o = &mut self.ships[j];
                            o.alive = false;
                            o.abduct = None;
                            o.target = None;
                            o.trail.clear();
                            o.respawn = if lm_mode { REINFORCE } else { self.rng.range(2.5, 4.5) };
                            self.score[team] += 1;
                            self.explode(ox, oy, TEAM_COL[ot]);
                            let shooter = self.ships[from].id;
                            self.mlog(&format!(
                                "kill: {} S{shooter} destroyed {} S{oid} ({} left in the air, {} reinforcements)",
                                TEAM_NAME[team],
                                TEAM_NAME[ot],
                                self.alive_count(ot),
                                self.game.regens[ot]
                            ));
                            self.event(team, format!("your S{shooter} destroyed enemy E{oid}"));
                            self.event(ot, format!("your S{oid} was destroyed by E{shooter}; a reinforcement warps in shortly"));
                            self.game.losses[ot] += 1;
                            if let Some(lm) = self.lm.as_mut() {
                                lm.cry_due[ot] = true;
                                lm.game_kills[team] += 1;
                            }
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
        // cows: walk to a goal, graze a while, pick a new goal
        let w = self.w;
        for ci in 0..self.cows.len() {
            let lifted = self.ships.iter().any(|o| o.alive && o.abduct == Some(ci));
            let c = &mut self.cows[ci];
            if c.lift > 0.0 && !lifted {
                c.lift = (c.lift - dt * 1.5).max(0.0); // the beam broke: fall back
                continue;
            }
            if c.lift > 0.0 {
                continue;
            }
            if c.graze > 0.0 {
                c.graze -= dt;
                if c.graze <= 0.0 {
                    c.goal = (c.x + self.rng.range(-0.12, 0.12) * w).clamp(w * 0.06, w * 0.95);
                    c.dir = if c.goal > c.x { 1.0 } else { -1.0 };
                }
            } else {
                let step = 1.1 * dt;
                if (c.goal - c.x).abs() <= step {
                    c.x = c.goal;
                    c.graze = self.rng.range(3.0, 14.0);
                } else {
                    c.x += c.dir * step;
                    c.walk += dt * 6.0;
                }
            }
        }
        // cars on the road
        if self.rng.chance(dt * 0.12) && self.cars.len() < 3 {
            let right = self.rng.chance(0.5);
            self.cars.push(Car { x: if right { -4.0 } else { w + 4.0 }, v: if right { 1.0 } else { -1.0 } * self.rng.range(9.0, 14.0) });
        }
        self.cars.retain_mut(|c| {
            c.x += c.v * dt;
            c.x > -8.0 && c.x < w + 8.0
        });
        for c in self.clouds.iter_mut() {
            c.x += c.v * dt;
            if c.x - c.rx > w {
                c.x = -c.rx;
            }
        }
        if self.parts.len() > 4000 {
            self.parts.drain(0..1000);
        }
        if lm_mode {
            self.fleet(rdt);
        }
        self.lm_send(rdt);
    }

    fn render(&mut self, cv: &mut Canvas) {
        cv.clear_overlays();
        let (w, h) = (cv.w, cv.h);
        let t = self.t;
        let s = self.s;
        let gi = self.ground as i32;
        // sky: gradient with ordered dithering (no banding in the dark tones)
        const BAYER: [[f32; 4]; 4] = [[0.0, 8.0, 2.0, 10.0], [12.0, 4.0, 14.0, 6.0], [3.0, 11.0, 1.0, 9.0], [15.0, 7.0, 13.0, 5.0]];
        let sky = [(0.0, Rgb::hex(0x04050d)), (0.55, Rgb::hex(0x121538)), (0.85, Rgb::hex(0x2c1f4a)), (1.0, Rgb::hex(0x5a2d52))];
        for y in 0..gi.min(h as i32) as usize {
            let c = gradient(&sky, y as f32 / self.ground);
            let row = &mut cv.px[y * w..(y + 1) * w];
            for (x, p) in row.iter_mut().enumerate() {
                let d = (BAYER[y & 3][x & 3] / 16.0 - 0.5) * (1.6 / 255.0);
                *p = Rgb::new(c.r + d, c.g + d, c.b + d);
            }
        }
        for y in gi.max(0) as usize..h {
            cv.px[y * w..(y + 1) * w].iter_mut().for_each(|p| *p = Rgb::hex(0x0a0d0c));
        }
        // aurora: a slow curtain high in the sky
        let a_top = 0.04 * self.ground;
        let a_bot = 0.5 * self.ground;
        let green = Rgb::hex(0x2fd68a);
        let violet = Rgb::hex(0x8a4fd0);
        for y in a_top as usize..a_bot as usize {
            let fy = (y as f32 - a_top) / (a_bot - a_top);
            let env = (fy * std::f32::consts::PI).sin().powf(1.4);
            for x in 0..w {
                let fx = x as f32;
                let n = vnoise(fx * 0.045 + t * 0.012, fy * 3.0 - t * 0.02, 21);
                let curtain = vnoise(fx * 0.12 - t * 0.03, fy * 1.5, 22);
                let k = ((n - 0.42) * 2.6).clamp(0.0, 1.0) * env * (0.5 + 0.5 * curtain);
                if k > 0.02 {
                    let col = green.lerp(violet, vnoise(fx * 0.02 + t * 0.005, 0.5, 23));
                    let p = &mut cv.px[y * w + x];
                    *p = p.lerp(Rgb::new(p.r + col.r * 0.55, p.g + col.g * 0.55, p.b + col.b * 0.55), k);
                }
            }
        }
        self.stars.draw(cv, t, 1.0);
        // moon
        let (mx, my, mr) = (self.w * 0.83, self.h * 0.16, s * 1.5);
        cv.glow(mx, my, mr * 4.0, Rgb::hex(0x3a3550).scale(0.6));
        cv.disc(mx, my, mr, Rgb::hex(0xf3ecd2), 1.0);
        cv.disc(mx - mr * 0.3, my - mr * 0.1, mr * 0.28, Rgb::hex(0xcfc6aa), 0.8);
        cv.disc(mx + mr * 0.35, my + mr * 0.35, mr * 0.2, Rgb::hex(0xd8cfb3), 0.7);
        // moonlit clouds
        for c in &self.clouds {
            let (x0, x1) = ((c.x - c.rx).floor() as i32, (c.x + c.rx).ceil() as i32);
            let (y0, y1) = ((c.y - c.ry).floor() as i32, (c.y + c.ry).ceil() as i32);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let ex = (x as f32 - c.x) / c.rx;
                    let ey = (y as f32 - c.y) / c.ry;
                    let d = ex * ex + ey * ey;
                    if d >= 1.0 {
                        continue;
                    }
                    let edge = vnoise(x as f32 * 0.35 + (c.seed % 1000) as f32, y as f32 * 0.6, c.seed);
                    let a = ((1.0 - d) * 1.4 * (0.45 + 0.55 * edge)).min(1.0) * 0.4;
                    // lit from the moon side, darker underneath
                    let lit = 0.5 + 0.5 * (-ey) * 0.7 + 0.2 * (ex * if c.x < mx { 1.0 } else { -1.0 });
                    let col = Rgb::hex(0x2a2d4c).lerp(Rgb::hex(0x9a94b8), lit.clamp(0.0, 1.0));
                    cv.blend(x, y, col, a);
                }
            }
        }
        // skyline
        let mut max_bh = 0;
        for b in &self.city {
            max_bh = max_bh.max(b.h);
            cv.rect(b.x, gi - b.h, b.w, b.h, Rgb::hex(0x0b0c19), 1.0);
            for &(wx, wy, ph) in &b.windows {
                let on = ((t * 0.05 + ph).sin() > -0.6) as i32 as f32;
                if on > 0.0 {
                    let flick = 0.8 + 0.2 * (t * 0.7 + ph * 3.0).sin();
                    cv.set(b.x + wx, gi - b.h + wy, Rgb::hex(0xffcf73).scale(0.55 * flick));
                }
            }
        }
        // field: grass with a road and a fence
        for y in gi..h as i32 {
            for x in 0..w as i32 {
                let n = vnoise(x as f32 * 0.3, y as f32 * 0.7, 3);
                cv.set(x, y, Rgb::hex(0x0d1a12).lerp(Rgb::hex(0x16291b), n));
            }
        }
        let road_y = gi + 5;
        for x in 0..w as i32 {
            cv.set(x, road_y, Rgb::hex(0x171a1c));
            cv.set(x, road_y + 1, Rgb::hex(0x131517));
            if (x / 3) % 2 == 0 {
                cv.blend(x, road_y, Rgb::hex(0x5a5a40), 0.35);
            }
        }
        for x in (1..w as i32).step_by(7) {
            cv.set(x, gi, Rgb::hex(0x3d2c1c));
            cv.set(x, gi + 1, Rgb::hex(0x2c2014));
            cv.blend(x + 1, gi, Rgb::hex(0x3d2c1c), 0.5);
        }
        // two barns, one per side
        for &barn_x in &self.barns {
            let bw = (s * 2.4) as i32;
            let bh = (s * 1.3) as i32;
            let bx = barn_x as i32 - bw / 2;
            cv.rect(bx, gi - bh, bw, bh, Rgb::hex(0x5a1e1e), 1.0);
            for r in 0..(bh / 2 + 1) {
                let inset = r * bw / (bh + 1);
                cv.rect(bx + inset / 2, gi - bh - (bh / 2 + 1) + r, bw - inset, 1, Rgb::hex(0x3b2a26), 1.0);
            }
            cv.rect(bx + bw / 2 - 1, gi - bh / 2, 2, bh / 2, Rgb::hex(0xffcf73).scale(0.55), 1.0);
            cv.set(bx + 1, gi - bh + 1, Rgb::hex(0x7a2a2a));
        }
        // dynamic lights: bolts, explosions and beams light the city and the field
        self.lights.clear();
        for b in &self.bolts {
            self.lights.push(Light { x: b.x, y: b.y, r: s * 3.5, col: TEAM_COL[b.team].scale(0.35) });
        }
        for r in &self.rings {
            self.lights.push(Light { x: r.x, y: r.y, r: r.r + s * 4.0, col: r.col.scale(0.5 * r.life) });
        }
        for o in &self.ships {
            if let (true, Some(_)) = (o.alive, o.abduct) {
                self.lights.push(Light { x: o.x, y: self.ground, r: s * 2.5, col: TEAM_COL[o.team].scale(0.15) });
            }
        }
        let lit_top = gi - max_bh - 1;
        for l in &self.lights {
            let (x0, x1) = ((l.x - l.r).floor() as i32, (l.x + l.r).ceil() as i32);
            let (y0, y1) = ((l.y - l.r).floor().max(lit_top as f32) as i32, ((l.y + l.r).ceil() as i32).min(h as i32 - 1));
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = ((x as f32 - l.x).powi(2) + (y as f32 - l.y).powi(2)).sqrt() / l.r;
                    if d < 1.0 {
                        let f = (1.0 - d) * (1.0 - d);
                        cv.add(x, y, l.col.scale(f));
                    }
                }
            }
        }
        // cows (with legs that walk and a tail that flicks)
        for (ci, c) in self.cows.iter().enumerate() {
            let lifter = self.ships.iter().find(|o| o.alive && o.abduct == Some(ci));
            let cy = match lifter {
                Some(o) => c.y + (o.y - c.y) * c.lift,
                None => c.y - (c.lift * 4.0),
            };
            let (x, y) = (c.x.round() as i32, cy.round() as i32);
            let (white, spot, dark) = (Rgb::hex(0xeeeae0), Rgb::hex(0x26262a), Rgb::hex(0x8c867c));
            let f = if c.dir > 0.0 { 1 } else { -1 };
            let stepping = c.graze <= 0.0 && c.lift <= 0.0;
            let leg = if stepping && c.walk.sin() > 0.0 { 1 } else { 0 };
            // 4x2 body with a spot, head, legs, tail
            for bx in 0..4 {
                cv.set(x + bx * f, y - 2, if bx == 1 { spot } else { white });
                cv.set(x + bx * f, y - 1, if bx == 2 { spot } else { white });
            }
            cv.set(x + 4 * f, y - 2, white);
            cv.set(x + 4 * f, y - 3 + if c.graze > 0.0 && (t * 0.8 + ci as f32).sin() > 0.6 { 1 } else { 0 }, dark);
            cv.set(x + leg, y, dark);
            cv.set(x + 3 * f - leg, y, dark);
            if (t * 1.3 + ci as f32 * 2.0).sin() > 0.85 {
                cv.set(x - f, y - 2, dark);
            }
        }
        // cars: headlights and taillights
        for c in &self.cars {
            let (hx, tx) = if c.v > 0.0 { (c.x + 1.0, c.x - 1.0) } else { (c.x - 1.0, c.x + 1.0) };
            cv.splat_add(hx, road_y as f32 + 0.5, Rgb::hex(0xfff2c0).scale(0.8));
            cv.splat_add(hx + c.v.signum() * 1.5, road_y as f32 + 0.5, Rgb::hex(0xfff2c0).scale(0.3));
            cv.splat_add(tx, road_y as f32 + 0.5, Rgb::hex(0xff3030).scale(0.6));
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
                        let half = s * 0.3 + s * 0.9 * f;
                        let pulse = 0.75 + 0.25 * ((t * 6.0) - f * 8.0).sin();
                        for xx in (o.x - half) as i32..=(o.x + half) as i32 {
                            let e = 1.0 - ((xx as f32 - o.x).abs() / half).powi(2);
                            cv.add(xx, yy as i32, TEAM_COL[o.team].scale(0.028 * e * pulse));
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
                    let c = if k > 0.7 {
                        Rgb::hex(0xfff3c4)
                    } else if k > 0.45 {
                        Rgb::hex(0xffb347)
                    } else if k > 0.2 {
                        Rgb::hex(0xff5a36)
                    } else {
                        Rgb::hex(0x6a2a3a)
                    };
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
        let ships: Vec<Ship> = self.ships.iter().filter(|o| o.alive).cloned().collect();
        for o in &ships {
            self.draw_saucer(cv, o);
        }
        // bolts
        for b in &self.bolts {
            let tc = TEAM_COL[b.team];
            let (tx, ty) = (b.x - b.vx * 0.035, b.y - b.vy * 0.035);
            cv.line_add(tx, ty, b.x, b.y, tc.scale(0.9));
            cv.splat_add(b.x, b.y, Rgb::WHITE.scale(0.8));
        }
        // title card and banners
        let fs = ((self.w / 70.0) as i32).max(1);
        if self.title > 0.0 {
            let a = (self.title / 0.8).min(1.0) * ((4.5 - self.title) / 0.5).min(1.0);
            let y = (self.ground * 0.3) as i32;
            let wz = Self::text_width("ZORB", fs);
            let wv = Self::text_width("VS", fs);
            let wk = Self::text_width("KRELL", fs);
            let total = wz + wv + wk + fs * 8;
            let x = (self.w as i32 - total) / 2;
            cv.rect(x - fs * 2, y - fs, total + fs * 4, 7 * fs, Rgb::hex(0x04050d), 0.5 * a);
            Self::big_text(cv, "ZORB", x, y, fs, TEAM_COL[0], a);
            Self::big_text(cv, "VS", x + wz + fs * 4, y, fs, Rgb::hex(0xd8d8e8), a);
            Self::big_text(cv, "KRELL", x + wz + wv + fs * 8, y, fs, TEAM_COL[1], a);
        } else if self.banner_t > 0.0 {
            let a = (self.banner_t / 0.6).min(1.0);
            let text = self.banner.clone();
            self.big_text_centered(cv, &text, (self.ground * 0.3) as i32, fs, self.banner_col, a);
        }
        // HUD: a dim plate over the top rows, then the scoreboard
        let cols = cv.cols as i32;
        cv.rect(0, 0, w as i32, if self.lm.as_ref().is_some_and(|l| l.pop.is_some()) && cols >= 110 { 6 } else { 4 }, Rgb::hex(0x04050d), 0.5);
        let short = cols < 110;
        let board = |team: usize, s: &Ufo| {
            if s.lm.is_none() {
                format!("{} {}", TEAM_NAME[team], s.score[team])
            } else if short {
                format!("{} {}k {}c G{}", TEAM_NAME[team], s.score[team], s.cows_taken[team], s.game.wins[team])
            } else {
                format!(
                    "{} {} kills {} cows  games won {}  reinf {}/{}",
                    TEAM_NAME[team], s.score[team], s.cows_taken[team], s.game.wins[team], s.game.regens[team], s.game.regens_total
                )
            }
        };
        let hud = board(0, self);
        let hud2 = board(1, self);
        match &self.lm {
            None => {
                cv.text(2, 0, &hud, TEAM_COL[0].scale(0.7));
                cv.text(cols - 2 - hud2.len() as i32, 0, &hud2, TEAM_COL[1].scale(0.7));
            }
            Some(lm) => {
                let grey = Rgb::hex(0x8a90a8).scale(0.7);
                let dim = Rgb::hex(0x8a90a8).scale(0.45);
                let l0 = lm.ready[0].clone().unwrap_or_else(|| "built-in".into());
                let l1 = lm.ready[1].clone().unwrap_or_else(|| "built-in".into());
                let (t0, t1) = (if lm.pending[0] { " .." } else { "" }, if lm.pending[1] { ".. " } else { "" });
                cv.text(2, 0, &hud, TEAM_COL[0].scale(0.7));
                cv.text(2, 1, &format!("{l0}{t0}"), grey);
                cv.text(cols - 2 - hud2.len() as i32, 0, &hud2, TEAM_COL[1].scale(0.7));
                let right = format!("{t1}{l1}");
                cv.text(cols - 2 - right.len() as i32, 1, &right, grey);
                if !short {
                    let mid = match &lm.pop {
                        Some(p) => format!("game {}  evolve gen {}/{}", self.game.n, p[0].gen, p[1].gen),
                        None => format!("game {}", self.game.n),
                    };
                    cv.text((cols - mid.len() as i32) / 2, 0, &mid, dim);
                    if let Some(p) = &lm.pop {
                        let d0 = format!("doctrine: {}", p[0].active().short());
                        let d1 = format!("doctrine: {}", p[1].active().short());
                        cv.text(2, 2, &d0, TEAM_COL[0].scale(0.35));
                        cv.text(cols - 2 - d1.len() as i32, 2, &d1, TEAM_COL[1].scale(0.35));
                    }
                }
                let maxw = (cols - 4).max(8) as usize;
                let clip = |s: &str| s.chars().take(maxw).collect::<String>();
                let mut row = if lm.pop.is_some() && !short { 3 } else { 2 };
                if !lm.status.is_empty() {
                    cv.text(2, row, &clip(&lm.status), dim);
                    row += 1;
                }
                // a battle cry (shouted when one of the team's saucers dies) shows for 20 s
                for team in 0..2 {
                    if !lm.say[team].is_empty() && lm.say_age[team] < 20.0 {
                        cv.text(2, row, &clip(&format!("{}> {}", TEAM_NAME[team], lm.say[team])), TEAM_COL[team].scale(0.5));
                        row += 1;
                    }
                }
                // a fresh lesson shows for 12 s
                for team in 0..2 {
                    if !lm.lesson[team].is_empty() && lm.lesson_age[team] < 12.0 {
                        cv.text(2, row, &clip(&format!("{} learned: {}", TEAM_NAME[team], lm.lesson[team])), TEAM_COL[team].lerp(grey, 0.5).scale(0.75));
                        row += 1;
                    }
                }
            }
        }
    }
}
