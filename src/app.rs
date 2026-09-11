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
    /// `--perf FILE` (or DOGFIGHT_PERF=FILE): one line per second of frame timings
    pub perf: Option<String>,
    pub scene: Option<String>,
    /// `--pilots builtin|lm` overrides `ufo_pilots` from the config
    pub pilots: Option<String>,
    /// `cuda` | `mlx` overrides `lm_backend`
    pub backend: Option<String>,
    /// `--lessons on|off` overrides `lm_evolve`
    pub evolve: Option<bool>,
    /// the `evolve` word: genetic doctrine evolution
    pub genetic: Option<bool>,
    pub fps: Option<u32>,
    pub seed: Option<u64>,
    pub idle_trigger: Option<i32>,
    pub duration: Option<f32>,
}

/// Per-second frame statistics, written to a file so a run can be watched for lag without
/// touching the terminal: fps achieved, update/render/encode/write ms (mean/max), KB per
/// frame, the longest gap between frames and this process's CPU share.
struct Perf {
    file: Option<std::fs::File>,
    t0: Instant,
    sec_start: Instant,
    frames: u32,
    sum: [f64; 4],
    max: [f64; 4],
    bytes: usize,
    gap_max: f64,
    cpu_last: f64,
    notes: Vec<String>,
}

impl Perf {
    fn new(path: Option<&str>) -> Perf {
        let file = path.and_then(|p| std::fs::File::create(p).ok());
        Perf {
            file,
            t0: Instant::now(),
            sec_start: Instant::now(),
            frames: 0,
            sum: [0.0; 4],
            max: [0.0; 4],
            bytes: 0,
            gap_max: 0.0,
            cpu_last: cpu_seconds(),
            notes: vec![],
        }
    }
    fn on(&self) -> bool {
        self.file.is_some()
    }
    fn note(&mut self, s: &str) {
        if self.on() {
            self.notes.push(s.to_string());
        }
    }
    fn frame(&mut self, ms: [f64; 4], bytes: usize, gap_ms: f64) {
        if !self.on() {
            return;
        }
        self.frames += 1;
        for (k, m) in ms.iter().enumerate() {
            self.sum[k] += m;
            self.max[k] = self.max[k].max(*m);
        }
        self.bytes += bytes;
        self.gap_max = self.gap_max.max(gap_ms);
        if self.sec_start.elapsed().as_secs_f64() >= 1.0 {
            self.flush();
        }
    }
    fn flush(&mut self) {
        use std::io::Write;
        let Some(f) = self.file.as_mut() else { return };
        let n = self.frames.max(1) as f64;
        let cpu = cpu_seconds();
        let share = (cpu - self.cpu_last) / self.sec_start.elapsed().as_secs_f64().max(0.001) * 100.0;
        let _ = writeln!(
            f,
            "t={:5.1}s fps={:3} update={:.2}/{:.2} render={:.2}/{:.2} encode={:.2}/{:.2} write={:.2}/{:.2} kb/frame={:.1} gap_max={:.0}ms cpu={:.0}%{}",
            self.t0.elapsed().as_secs_f64(),
            self.frames,
            self.sum[0] / n,
            self.max[0],
            self.sum[1] / n,
            self.max[1],
            self.sum[2] / n,
            self.max[2],
            self.sum[3] / n,
            self.max[3],
            self.bytes as f64 / n / 1024.0,
            self.gap_max,
            share,
            if self.notes.is_empty() { String::new() } else { format!("  [{}]", self.notes.join("; ")) }
        );
        self.frames = 0;
        self.sum = [0.0; 4];
        self.max = [0.0; 4];
        self.bytes = 0;
        self.gap_max = 0.0;
        self.notes.clear();
        self.cpu_last = cpu;
        self.sec_start = Instant::now();
    }
}

/// CPU seconds used by this process (Linux /proc; 0 elsewhere).
fn cpu_seconds() -> f64 {
    std::fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|s| {
            let rest = &s[s.rfind(')')? + 2..];
            let f: Vec<&str> = rest.split_whitespace().collect();
            let ut: f64 = f.get(11)?.parse().ok()?;
            let st: f64 = f.get(12)?.parse().ok()?;
            Some((ut + st) / 100.0)
        })
        .unwrap_or(0.0)
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
    if let Some(g) = opts.genetic {
        cfg.lm_genetic = g;
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
            eprintln!("dogfight: no terminal: {e}");
            return 1;
        }
    };
    if let Err(e) = term.enter() {
        eprintln!("dogfight: cannot configure terminal: {e}");
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
            eprintln!("dogfight: no valid scene in {:?} (try `dogfight list`)", list);
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
    let perf_env = std::env::var("DOGFIGHT_PERF").ok();
    let mut perf = Perf::new(opts.perf.as_deref().or(perf_env.as_deref()));
    // lag guard: a terminal that cannot drain our bytes makes write() block; when that keeps
    // happening, halve the frame rate for a few seconds instead of letting the animation stutter
    let mut slow_writes = 0u32;
    let mut lag_until = Instant::now();
    let mut lag_events = 0u32;
    // ... and raise the encoder's colour tolerance: fewer changed cells = fewer bytes, which is
    // the actual bottleneck; it relaxes back to the configured value once writes are fast again
    let mut tol = cfg.tolerance;
    let mut calm_since = Instant::now();
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
        let t_upd = Instant::now();
        cur.update(dt);
        let t_ren = Instant::now();
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
        let t_enc = Instant::now();
        out.clear();
        enc.encode(&cv, &mut out);
        let t_wr = Instant::now();
        if !term.write_all(&out) {
            break;
        }
        let t_end = Instant::now();
        let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1000.0;
        perf.frame([ms(t_upd, t_ren), ms(t_ren, t_enc), ms(t_enc, t_wr), ms(t_wr, t_end)], out.len(), dt as f64 * 1000.0);
        let period_ms = 1000.0 / fps_focused as f64;
        if ms(t_wr, t_end) > period_ms * 0.5 {
            slow_writes += 1;
            calm_since = Instant::now();
            if slow_writes >= 4 && Instant::now() >= lag_until {
                lag_until = Instant::now() + Duration::from_secs(5);
                lag_events += 1;
                tol = (tol + 4).min(24);
                enc.set_tolerance(tol);
                perf.note(&format!("lag guard #{lag_events}: terminal is slow, fps halved for 5 s, tolerance {tol}"));
            }
        } else {
            slow_writes = slow_writes.saturating_sub(1);
            if tol > cfg.tolerance && calm_since.elapsed() > Duration::from_secs(8) {
                tol = (tol - 2).max(cfg.tolerance);
                enc.set_tolerance(tol);
                calm_since = Instant::now();
                perf.note(&format!("terminal keeping up, tolerance back to {tol}"));
            }
        }
        if let Some(d) = opts.duration {
            if started.elapsed().as_secs_f32() > d {
                break;
            }
        }
        // wait for the next frame, reacting to input
        let mut fps = if focused { fps_focused } else { cfg.unfocused_fps.min(fps_focused) };
        if Instant::now() < lag_until {
            fps = (fps / 2).max(15);
        }
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
    perf.flush();
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
