//! Genetic evolution of a team's *doctrine*: a handful of bounded tactical parameters that
//! (a) are spelled out in the commander's prompt and (b) are enforced on every order the model
//! gives (`Doctrine::check`). The language model can only explore inside these heuristic
//! bounds — the same idea as AutoSafe's threat model + safe-action sampling, applied to a
//! dogfight instead of tool use: unsafe/losing behaviour is defined up front, the model's
//! actions are corrected against it, and what gets evolved is the bound set, not the model.
//!
//! One candidate doctrine is active per team at a time. It is scored over an evaluation
//! window (a game, or `SEGMENT` seconds of one) by kills − losses + ½ cows (+ a win bonus),
//! per minute. When every candidate of the population has a score, the top half survives,
//! the rest is replaced by uniform crossover of two survivors plus Gaussian mutation, clamped
//! to the bounds. Populations persist in `~/.local/state/dogfight/doctrine-<TEAM>.txt`.
use crate::config::state_dir;
use crate::rng::Rng;
use std::path::PathBuf;

pub const POP: usize = 6;
/// seconds of play per evaluation window (a game ending also closes the window)
pub const SEGMENT: f32 = 90.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Doctrine {
    /// ships below this hp must flee when an enemy is within laser range
    pub flee_hp: f32,
    /// ships above this hp may not flee (no full-health cowardice)
    pub brave_hp: f32,
    /// a cow may be abducted only when it is within this distance (percent of field width)
    pub abduct_dist: f32,
    /// ... and no enemy is within this many laser ranges of the ship
    pub abduct_clear: f32,
    /// probability that an attack order is redirected at the weakest enemy in range
    pub focus: f32,
    /// at most this fraction of the fleet may flee at the same time
    pub courage: f32,
}

/// (min, max) per parameter — the heuristic bounds the evolver can never leave.
pub const BOUNDS: [(f32, f32); 6] = [(15.0, 60.0), (40.0, 100.0), (10.0, 60.0), (0.1, 0.6), (0.0, 1.0), (0.2, 1.0)];

impl Doctrine {
    pub fn default_doctrine() -> Doctrine {
        Doctrine { flee_hp: 35.0, brave_hp: 70.0, abduct_dist: 30.0, abduct_clear: 0.3, focus: 0.5, courage: 0.5 }
    }
    fn to_vec(self) -> [f32; 6] {
        [self.flee_hp, self.brave_hp, self.abduct_dist, self.abduct_clear, self.focus, self.courage]
    }
    fn from_vec(v: [f32; 6]) -> Doctrine {
        let mut d = Doctrine { flee_hp: v[0], brave_hp: v[1], abduct_dist: v[2], abduct_clear: v[3], focus: v[4], courage: v[5] };
        d.clamp();
        d
    }
    fn clamp(&mut self) {
        let mut v = self.to_vec();
        for (x, (lo, hi)) in v.iter_mut().zip(BOUNDS.iter()) {
            *x = x.clamp(*lo, *hi);
        }
        if v[1] < v[0] + 10.0 {
            v[1] = (v[0] + 10.0).min(BOUNDS[1].1);
        }
        *self = Doctrine { flee_hp: v[0], brave_hp: v[1], abduct_dist: v[2], abduct_clear: v[3], focus: v[4], courage: v[5] };
    }
    pub fn random(rng: &mut Rng) -> Doctrine {
        let mut v = [0.0; 6];
        for (x, (lo, hi)) in v.iter_mut().zip(BOUNDS.iter()) {
            *x = rng.range(*lo, *hi);
        }
        Doctrine::from_vec(v)
    }
    pub fn crossover(a: &Doctrine, b: &Doctrine, rng: &mut Rng) -> Doctrine {
        let (va, vb) = (a.to_vec(), b.to_vec());
        let mut v = [0.0; 6];
        for k in 0..6 {
            v[k] = if rng.chance(0.5) { va[k] } else { vb[k] };
        }
        Doctrine::from_vec(v)
    }
    pub fn mutate(&self, rng: &mut Rng, strength: f32) -> Doctrine {
        let mut v = self.to_vec();
        for (x, (lo, hi)) in v.iter_mut().zip(BOUNDS.iter()) {
            if rng.chance(0.6) {
                *x += rng.gauss() * (hi - lo) * strength;
            }
        }
        Doctrine::from_vec(v)
    }
    /// What the commander is told (and what `check` enforces).
    pub fn describe(&self) -> String {
        format!(
            "flee below {:.0} hp when an enemy is in laser range; never flee above {:.0} hp; go for a cow when it is within {:.0} and no enemy is within {:.1}x laser range (your call); focus fire on the weakest enemy {:.0}% of the time; at most {:.0}% of the fleet may flee at once",
            self.flee_hp,
            self.brave_hp,
            self.abduct_dist,
            self.abduct_clear,
            self.focus * 100.0,
            self.courage * 100.0
        )
    }
    pub fn short(&self) -> String {
        format!(
            "flee<{:.0} brave>{:.0} cow<{:.0}/{:.1}r focus {:.0}% courage {:.0}%",
            self.flee_hp,
            self.brave_hp,
            self.abduct_dist,
            self.abduct_clear,
            self.focus * 100.0,
            self.courage * 100.0
        )
    }
    fn line(&self, fit: f32, evals: u32) -> String {
        let v = self.to_vec();
        format!("{} {} {} {} {} {} {} {}", v[0], v[1], v[2], v[3], v[4], v[5], fit, evals)
    }
}

struct Cand {
    d: Doctrine,
    fit: f32,
    evals: u32,
}

pub struct Population {
    team: usize,
    cands: Vec<Cand>,
    active: usize,
    pub gen: u32,
    path: PathBuf,
}

pub fn doctrine_path(team_name: &str) -> PathBuf {
    state_dir().join(format!("doctrine-{team_name}.txt"))
}

impl Population {
    /// Load the team's population or seed a fresh one (the default doctrine plus randoms).
    pub fn load_or_new(team: usize, team_name: &str, rng: &mut Rng) -> Population {
        let path = doctrine_path(team_name);
        let mut p = Population { team, cands: vec![], active: 0, gen: 0, path };
        if let Ok(text) = std::fs::read_to_string(&p.path) {
            for l in text.lines() {
                let f: Vec<f32> = l.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                if l.starts_with("gen ") {
                    p.gen = f.first().map(|g| *g as u32).unwrap_or(0);
                } else if l.starts_with("active ") {
                    p.active = f.first().map(|g| *g as usize).unwrap_or(0);
                } else if f.len() == 8 {
                    p.cands.push(Cand { d: Doctrine::from_vec([f[0], f[1], f[2], f[3], f[4], f[5]]), fit: f[6], evals: f[7] as u32 });
                }
            }
        }
        if p.cands.len() != POP {
            p.cands.clear();
            p.cands.push(Cand { d: Doctrine::default_doctrine(), fit: 0.0, evals: 0 });
            while p.cands.len() < POP {
                p.cands.push(Cand { d: Doctrine::random(rng), fit: 0.0, evals: 0 });
            }
            p.active = 0;
            p.gen = 0;
        }
        p.active = p.active.min(POP - 1);
        p
    }

    pub fn active(&self) -> Doctrine {
        self.cands[self.active].d
    }

    /// Close the active candidate's evaluation window with its fitness (per minute), move to the
    /// next unevaluated candidate, and breed a new generation when everyone has been scored.
    /// Returns true when a new generation was bred.
    pub fn score(&mut self, fitness: f32, rng: &mut Rng) -> bool {
        let c = &mut self.cands[self.active];
        // running mean so a candidate evaluated twice isn't judged on one lucky window
        c.fit = if c.evals == 0 { fitness } else { (c.fit * c.evals as f32 + fitness) / (c.evals + 1) as f32 };
        c.evals += 1;
        let mut bred = false;
        match (0..POP).map(|k| (self.active + 1 + k) % POP).find(|&k| self.cands[k].evals == 0) {
            Some(k) => self.active = k,
            None => {
                self.breed(rng);
                bred = true;
            }
        }
        self.save();
        bred
    }

    fn breed(&mut self, rng: &mut Rng) {
        self.cands.sort_by(|a, b| b.fit.partial_cmp(&a.fit).unwrap_or(std::cmp::Ordering::Equal));
        let keep = POP / 2;
        let parents: Vec<Doctrine> = self.cands[..keep].iter().map(|c| c.d).collect();
        for k in keep..POP {
            let a = parents[rng.below(keep)];
            let b = parents[rng.below(keep)];
            let child = Doctrine::crossover(&a, &b, rng).mutate(rng, 0.12);
            self.cands[k] = Cand { d: child, fit: 0.0, evals: 0 };
        }
        // the survivors keep their score but are re-evaluated next generation too
        for c in self.cands[..keep].iter_mut() {
            c.evals = 0;
        }
        // the champion goes first so the next game starts from the best-known doctrine
        self.active = 0;
        self.gen += 1;
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(state_dir());
        let mut s = format!("# dogfight doctrine population, team {}\ngen {}\nactive {}\n", self.team, self.gen, self.active);
        for c in &self.cands {
            s.push_str(&c.d.line(c.fit, c.evals));
            s.push('\n');
        }
        let _ = std::fs::write(&self.path, s);
    }
}
