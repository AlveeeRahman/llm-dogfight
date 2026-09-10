//! The screensaver loop: pace frames, rotate scenes with a crossfade, exit on any key.
use crate::canvas::Canvas;
use crate::config::Config;
use crate::encode::Encoder;
use crate::idle;
use crate::scenes::{self, Scene};
use crate::term::{Input, Term, CONT, QUIT, RESIZED};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub struct RunOpts {
    pub scene: Option<String>,
    /// `--pilots builtin|lm` overrides `ufo_pilots` from the config
    pub pilots: Option<String>,
    /// `cuda` | `mlx` overrides `lm_backend`
    pub backend: Option<String>,
    /// `--evolve on|off` overrides `lm_evolve`
    pub evolve: Option<bool>,
    pub fps: Option<u32>,
    pub seed: Option<u64>,
    pub idle_trigger: Option<i32>,
    pub duration: Option<f32>,
}

fn clock_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(7)
}

pub fn run(cfg: &Config, opts: RunOpts) -> i32 {
    let mut cfg = cfg.clone();
    if let Some(p) = &opts.pilots {
        cfg.ufo_pilots = p.clone();
    }
    if let Some(e) = opts.evolve {
        cfg.lm_evolve = e;
    }
    if let Some(b) = &opts.backend {
        cfg.lm_backend = b.clone();
    }
    let cfg = &cfg;
    if let Some(pid) = opts.idle_trigger {
        if !idle::claim_trigger(pid) {
            return 0; // stale or foreign SIGALRM: do nothing, silently
        }
    }
    let slot = idle::acquire_slot(cfg.max_instances);
    if slot.is_none() && opts.idle_trigger.is_some() {
        return 0; // enough terminals are already animating
    }
    let mut term = match Term::open() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("reverie: no terminal: {e}");
            return 1;
        }
    };
    if let Err(e) = term.enter() {
        eprintln!("reverie: cannot configure terminal: {e}");
        return 1;
    }
    let seed = opts.seed.unwrap_or_else(clock_seed);
    let mut rng = crate::rng::Rng::new(seed);
    let list: Vec<String> = match &opts.scene {
        Some(s) => vec![s.clone()],
        None => cfg.scenes.clone(),
    };
    let (cols, rows) = term.size();
    let mut cv = Canvas::new(cols, rows);
    let mut cv2 = Canvas::new(cols, rows);
    let mut enc = Encoder::new(cfg.truecolor(), cfg.tolerance);
    let mut idx = rng.below(list.len());
    let mut cur: Box<dyn Scene> = match first_valid(&list, &mut idx, seed, cv.w, cv.h, cfg) {
        Some(s) => s,
        None => {
            term.leave();
            eprintln!("reverie: no valid scene in {:?} (try `reverie list`)", list);
            return 2;
        }
    };
    let mut next: Option<Box<dyn Scene>> = None;
    let mut fade = 0.0f32;
    let rotate = (cfg.rotate_minutes * 60.0).max(10.0);
    let mut since_switch = 0.0f32;
    let fps_focused = opts.fps.unwrap_or(cfg.fps).clamp(1, 120);
    let mut focused = true;
    let started = Instant::now();
    let mut last = Instant::now();
    let mut out: Vec<u8> = Vec::with_capacity(1 << 16);
    loop {
        if QUIT.load(Ordering::SeqCst) {
            break;
        }
        if RESIZED.swap(false, Ordering::SeqCst) {
            let (c, r) = term.size();
            cv.resize(c, r);
            cv2.resize(c, r);
            cur.resize(cv.w, cv.h);
            if let Some(n) = next.as_mut() {
                n.resize(cv.w, cv.h);
            }
            enc.invalidate();
        }
        if CONT.swap(false, Ordering::SeqCst) {
            term.reassert();
            enc.invalidate();
        }
        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.1);
        last = now;
        since_switch += dt;

        // scene rotation with a 1.5 s crossfade
        if list.len() > 1 && next.is_none() && since_switch > rotate {
            let mut j = (idx + 1) % list.len();
            if let Some(s) = first_valid(&list, &mut j, rng.next_u64(), cv.w, cv.h, cfg) {
                idx = j;
                next = Some(s);
                fade = 0.0;
            }
            since_switch = 0.0;
        }
        cur.update(dt);
        cur.render(&mut cv);
        if let Some(n) = next.as_mut() {
            fade += dt / 1.5;
            n.update(dt);
            n.render(&mut cv2);
            let k = fade.min(1.0);
            for (a, b) in cv.px.iter_mut().zip(cv2.px.iter()) {
                *a = a.lerp(*b, k);
            }
            if k > 0.5 {
                cv.glyphs.copy_from_slice(&cv2.glyphs);
                cv.dots.copy_from_slice(&cv2.dots);
                cv.dot_col.copy_from_slice(&cv2.dot_col);
            }
            if fade >= 1.0 {
                cur.save();
                cur = next.take().unwrap();
            }
        }
        out.clear();
        enc.encode(&cv, &mut out);
        if !term.write_all(&out) {
            break;
        }
        if let Some(d) = opts.duration {
            if started.elapsed().as_secs_f32() > d {
                break;
            }
        }
        // wait for the next frame, reacting to input
        let fps = if focused { fps_focused } else { cfg.unfocused_fps.min(fps_focused) };
        let deadline = now + Duration::from_secs_f64(1.0 / fps as f64);
        let mut quit = false;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match term.poll_input(left.as_millis().max(1) as i32) {
                Input::Key => {
                    quit = true;
                    break;
                }
                Input::FocusIn => focused = true,
                Input::FocusOut => focused = false,
                Input::None => {
                    if QUIT.load(Ordering::SeqCst) || RESIZED.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        }
        if quit {
            break;
        }
    }
    cur.save();
    if let Some(mut n) = next {
        n.save();
    }
    term.leave();
    drop(slot);
    0
}

fn first_valid(list: &[String], idx: &mut usize, seed: u64, w: usize, h: usize, cfg: &Config) -> Option<Box<dyn Scene>> {
    for k in 0..list.len() {
        let j = (*idx + k) % list.len();
        if let Some(s) = scenes::make(&list[j], seed, w, h, cfg, true) {
            *idx = j;
            return Some(s);
        }
    }
    None
}
