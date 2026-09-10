mod app;
mod arena;
mod canvas;
mod config;
mod encode;
mod idle;
mod math;
mod rng;
mod scenes;
mod shell;
mod term;

use config::Config;
use std::time::Instant;

const HELP: &str = "reverie — a tiny terminal screensaver (UFO dogfights over a sleeping city)

USAGE
  reverie                         preview now (any key exits)
  reverie run ufo                 the baseline: built-in pilots, no GPU
  reverie run cuda ufo-battle     two small language models command the teams (CUDA, ~5 GB)
  reverie run mlx ufo-battle      same on Apple silicon (mlx-lm)
      options: [--evolve on|off] [--fps N] [--seed N] [--duration SECS]
  reverie arena [cuda|mlx]        same as `run ... ufo-battle`
  reverie arena check [--load]    verify python/torch/CUDA (or mlx) and the models; --load times them
  reverie arena pull              download the two models
  reverie arena lessons | forget  show / erase what the commanders learned from their losses
  reverie list                    list scenes
  reverie install [--shell bash|zsh|fish]
                                  start automatically when your prompt sits idle
  reverie uninstall               remove shell integration, stop watchers
  reverie pause [MINUTES] | resume
  reverie status                  show config, integration and watchers
  reverie config                  print the default config (copy to ~/.config/reverie/config.toml)

  reverie bench [--scene NAME] [--size 200x55] [--frames 600] [--seed 42]
  reverie snapshot --scene NAME [--size 120x36] [--frames 300] [--seed 42] --out FILE
  reverie init bash|zsh|fish      print the shell snippet (used by install)
  reverie watch --pid PID [--tty /dev/ttys001] [--idle SECS] [--daemon]
";

struct Args {
    pos: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}
impl Args {
    fn parse() -> Args {
        let mut pos = vec![];
        let mut flags = vec![];
        let mut it = std::env::args().skip(1).peekable();
        while let Some(a) = it.next() {
            if let Some(k) = a.strip_prefix("--") {
                if let Some((k, v)) = k.split_once('=') {
                    flags.push((k.to_string(), Some(v.to_string())));
                } else if it.peek().map(|n| !n.starts_with("--")).unwrap_or(false) && !matches!(k, "daemon" | "help" | "version" | "json" | "load") {
                    flags.push((k.to_string(), it.next()));
                } else {
                    flags.push((k.to_string(), None));
                }
            } else if a == "-h" {
                flags.push(("help".into(), None));
            } else {
                pos.push(a);
            }
        }
        Args { pos, flags }
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.flags.iter().find(|f| f.0 == k).and_then(|f| f.1.as_deref())
    }
    fn has(&self, k: &str) -> bool {
        self.flags.iter().any(|f| f.0 == k)
    }
    fn num<T: std::str::FromStr>(&self, k: &str) -> Option<T> {
        self.get(k).and_then(|v| v.parse().ok())
    }
}

fn size_arg(a: &Args, def: (usize, usize)) -> (usize, usize) {
    a.get("size").and_then(|s| s.split_once('x')).and_then(|(c, r)| Some((c.parse().ok()?, r.parse().ok()?))).unwrap_or(def)
}

fn bench(cfg: &Config, a: &Args) -> i32 {
    let (cols, rows) = size_arg(a, (200, 55));
    let frames: usize = a.num("frames").unwrap_or(600);
    let seed: u64 = a.num("seed").unwrap_or(42);
    let names: Vec<String> = match a.get("scene") {
        Some(s) => vec![s.to_string()],
        None => vec!["ufo".into()],
    };
    for name in names {
        let mut cv = canvas::Canvas::new(cols, rows);
        let mut enc = encode::Encoder::new(true, cfg.tolerance);
        let Some(mut s) = scenes::make(&name, seed, cv.w, cv.h, cfg, false) else {
            eprintln!("unknown scene {name}");
            return 2;
        };
        let mut out = Vec::with_capacity(1 << 18);
        let (mut ms, mut kb) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
        let mut first_kb = 0.0;
        for f in 0..=frames {
            let t0 = Instant::now();
            s.update(1.0 / 30.0);
            s.render(&mut cv);
            out.clear();
            enc.encode(&cv, &mut out);
            let dt = t0.elapsed().as_secs_f64() * 1000.0;
            if f == 0 {
                first_kb = out.len() as f64 / 1024.0;
            } else {
                ms.push(dt);
                kb.push(out.len() as f64 / 1024.0);
            }
        }
        let stat = |v: &mut Vec<f64>| {
            let mean = v.iter().sum::<f64>() / v.len() as f64;
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            (mean, v[(v.len() as f64 * 0.95) as usize - 1], v[v.len() - 1])
        };
        let (m_ms, p_ms, x_ms) = stat(&mut ms);
        let (m_kb, p_kb, _) = stat(&mut kb);
        println!(
            "{{\"scene\":\"{name}\",\"cols\":{cols},\"rows\":{rows},\"frames\":{frames},\"frame_mean_ms\":{m_ms:.3},\"frame_p95_ms\":{p_ms:.3},\"frame_max_ms\":{x_ms:.3},\"bytes_mean_kb\":{m_kb:.2},\"bytes_p95_kb\":{p_kb:.2},\"first_frame_kb\":{first_kb:.1},\"peak_rss_mb\":{:.1}}}",
            rss_mb()
        );
    }
    0
}

fn rss_mb() -> f32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmHWM")).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<f32>().ok()))
        .map(|kb| kb / 1024.0)
        .unwrap_or(0.0)
}

fn snapshot(cfg: &Config, a: &Args) -> i32 {
    let (cols, rows) = size_arg(a, (120, 36));
    let frames: usize = a.num("frames").unwrap_or(300);
    let seed: u64 = a.num("seed").unwrap_or(42);
    let name = a.get("scene").unwrap_or("ufo");
    let Some(path) = a.get("out") else {
        eprintln!("snapshot needs --out FILE");
        return 2;
    };
    let mut cv = canvas::Canvas::new(cols, rows);
    let Some(mut s) = scenes::make(name, seed, cv.w, cv.h, cfg, false) else {
        eprintln!("unknown scene {name}");
        return 2;
    };
    for _ in 0..frames {
        s.update(1.0 / 30.0);
    }
    s.render(&mut cv);
    let mut enc = encode::Encoder::new(true, 0);
    let mut out = Vec::new();
    enc.encode(&cv, &mut out);
    if let Err(e) = std::fs::write(path, &out) {
        eprintln!("{path}: {e}");
        return 1;
    }
    0
}

/// `reverie arena [check|pull]`: the sidecar's own commands, with the user's config applied.
fn arena_cmd(cfg: &Config, a: &Args) -> i32 {
    let extra: Vec<&str> = match a.pos.get(1).map(String::as_str) {
        Some("check") => {
            if a.has("load") {
                vec!["--check", "--load"]
            } else {
                vec!["--check"]
            }
        }
        Some("pull") => vec!["--pull"],
        Some("lessons") => {
            let dir = arena::lessons_dir();
            let mut any = false;
            if let Ok(rd) = std::fs::read_dir(&dir) {
                let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
                files.sort();
                for f in files {
                    let text = std::fs::read_to_string(&f).unwrap_or_default();
                    println!("{}:", f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
                    for (i, l) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
                        println!("  {}. {l}", i + 1);
                        any = true;
                    }
                }
            }
            if !any {
                println!("no lessons yet ({}); they appear after the first losses in `reverie arena`", dir.display());
            }
            return 0;
        }
        Some("forget") => {
            let dir = arena::lessons_dir();
            let n = std::fs::read_dir(&dir).map(|rd| rd.flatten().filter(|e| std::fs::remove_file(e.path()).is_ok()).count()).unwrap_or(0);
            println!("forgot {n} lesson file(s) in {}", dir.display());
            return 0;
        }
        Some(other) => {
            eprintln!("reverie arena: unknown subcommand '{other}' (check | pull | lessons | forget)");
            return 2;
        }
        None => unreachable!(),
    };
    let mut cmd = match arena::command(cfg, &extra) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("reverie arena: {e}");
            return 1;
        }
    };
    println!("models: {} vs {}  backend {}  budget {} GB  evolve {}  ({})", cfg.lm_model_a, cfg.lm_model_b, cfg.lm_backend, cfg.lm_vram_gb, if cfg.lm_evolve { "on" } else { "off" }, config::config_path().display());
    match cmd.status() {
        Ok(st) => st.code().unwrap_or(1),
        Err(e) => {
            eprintln!("reverie arena: cannot run {}: {e}\n  cuda: pip install torch transformers    mac: pip install mlx-lm", cfg.lm_python);
            1
        }
    }
}

fn main() {
    // `reverie status | head` must not panic on a closed pipe
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let a = Args::parse();
    if a.has("version") {
        println!("reverie {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if a.has("help") {
        print!("{HELP}");
        return;
    }
    let cfg = Config::load();
    let cmd = a.pos.first().map(|s| s.as_str()).unwrap_or("run");
    let evolve = a.get("evolve").map(|v| !matches!(v.to_lowercase().as_str(), "off" | "false" | "0" | "no"));
    // `reverie run [cuda|mlx] [ufo|ufo-battle]`: positional words pick the backend and the scene
    let mut scene = a.get("scene").map(String::from);
    let mut pilots = a.get("pilots").map(String::from);
    let mut backend = a.get("backend").map(String::from);
    for w in a.pos.iter().skip(1) {
        match w.as_str() {
            "cuda" | "mlx" => backend = Some(w.clone()),
            "ufo-battle" | "battle" | "lm" => {
                scene = Some("ufo".into());
                pilots = Some("lm".into());
            }
            other => scene = Some(other.to_string()),
        }
    }
    let run_opts = |scene: Option<String>, pilots: Option<String>, backend: Option<String>| app::RunOpts {
        scene,
        pilots,
        backend,
        evolve,
        fps: a.num("fps"),
        seed: a.num("seed"),
        idle_trigger: a.num("idle-trigger"),
        duration: a.num("duration"),
    };
    let code = match cmd {
        "run" | "preview" | "demo" => app::run(&cfg, run_opts(scene, pilots, backend)),
        "arena" if a.pos.len() == 1 || matches!(a.pos[1].as_str(), "cuda" | "mlx") => app::run(&cfg, run_opts(Some("ufo".into()), Some("lm".into()), backend)),
        "arena" => arena_cmd(&cfg, &a),
        "watch" => match a.num::<i32>("pid") {
            Some(pid) => idle::watch(pid, a.get("tty"), a.num("idle"), a.has("daemon")),
            None => {
                eprintln!("watch needs --pid");
                2
            }
        },
        "init" => match a.pos.get(1).and_then(|s| shell::snippet(s)) {
            Some(s) => {
                print!("{s}");
                0
            }
            None => {
                eprintln!("usage: reverie init bash|zsh|fish");
                2
            }
        },
        "install" => {
            let sh = a.get("shell").map(String::from).unwrap_or_else(shell::detect_shell);
            match shell::install(&sh) {
                Ok(m) => {
                    println!("{m}");
                    println!("idle after {} s (change idle_seconds in {})", cfg.idle_seconds, config::config_path().display());
                    0
                }
                Err(e) => {
                    eprintln!("reverie: {e}");
                    1
                }
            }
        }
        "uninstall" => {
            println!("{}", shell::uninstall());
            0
        }
        "pause" => {
            let m: Option<u64> = a.pos.get(1).and_then(|v| v.parse().ok());
            idle::set_pause(m);
            match m {
                Some(m) => println!("paused for {m} min"),
                None => println!("paused until `reverie resume`"),
            }
            0
        }
        "resume" => {
            idle::resume();
            println!("resumed");
            0
        }
        "list" => {
            for (n, d) in scenes::NAMES {
                println!("  {n:<8} {d}");
            }
            println!("\nbackend for ufo-battle: {} (config lm_backend; or `reverie run cuda|mlx ufo-battle`)", cfg.lm_backend);
            0
        }
        "config" => {
            print!("{}", config::DEFAULT_TOML);
            0
        }
        "status" => {
            println!("config:      {}{}", config::config_path().display(), if config::config_path().exists() { "" } else { " (defaults)" });
            println!("idle:        {} s, fps {}, scenes {:?}", cfg.idle_seconds, cfg.fps, cfg.scenes);
            println!("colour:      {}", if cfg.truecolor() { "truecolor" } else { "256" });
            println!("pilots:      {}", cfg.ufo_pilots);
            println!("lm:          {} vs {} ({} backend, {} GB, {}, evolve {})", cfg.lm_model_a, cfg.lm_model_b, cfg.lm_backend, cfg.lm_vram_gb, cfg.lm_python, if cfg.lm_evolve { "on" } else { "off" });
            let inst = shell::installed();
            println!("installed:   {}", if inst.is_empty() { "no (run `reverie install`)".to_string() } else { inst.join(", ") });
            println!("paused:      {}", idle::paused());
            println!("state:       {}", config::state_dir().display());
            println!("arena log:   {}", arena::log_path().display());
            0
        }
        "bench" => bench(&cfg, &a),
        "snapshot" => snapshot(&cfg, &a),
        other => {
            eprintln!("reverie: unknown command '{other}'\n");
            eprint!("{HELP}");
            2
        }
    };
    std::process::exit(code);
}
