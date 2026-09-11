#![warn(clippy::undocumented_unsafe_blocks)]
mod app;
mod arena;
mod canvas;
mod config;
mod encode;
mod evolve;
mod math;
mod rng;
mod scenes;
mod term;

use config::Config;
use std::time::Instant;

const HELP: &str = "LLM Dogfight — a UFO dogfight in your terminal, flown by two local language models

USAGE
  dogfight                         start the battle (auto-detects CUDA, or MLX on Apple silicon); any key exits
  dogfight cuda | mlx | cpu         same, with the backend chosen by hand (cpu: no GPU, tiny models, slow but real)
  dogfight evolve                  ... and evolve each team's bounded doctrine with a genetic algorithm
  dogfight ufo                     the built-in pilots, no models, no GPU
      options: [--zorb MODEL] [--krell MODEL] [--lessons on|off] [--fps N] [--seed N] [--duration SECS] [--perf FILE] [--shot SECS:FILE]
  dogfight models                  the model catalogue (aliases, sizes, what fits 8 GB); --zorb/--krell take an alias or any HF id
  dogfight run [cuda|mlx] [ufo|ufo-battle] [evolve]   the long form of the above
  dogfight arena check [--load]    verify python/torch/CUDA (or mlx) and the models; --load times them
  dogfight arena pull              download the two models (~5 GB) ahead of the first battle
  dogfight arena lessons           what the commanders learned, and their evolved doctrines
  dogfight reset model | score | all
                                  forget lessons + doctrines / games won and lost / both
  dogfight remove [--yes] [--keep-models]
                                  uninstall everything: config, memories, scores, logs, the
                                  downloaded models, and this binary
  dogfight status                  show config, integration and watchers
  dogfight config                  print the default config (copy to ~/.config/dogfight/config.toml)

  dogfight bench [--scene NAME] [--size 200x55] [--frames 600] [--seed 42]
  dogfight snapshot --scene NAME [--size 120x36] [--frames 300] [--seed 42] --out FILE
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
                } else if it.peek().map(|n| !n.starts_with("--")).unwrap_or(false)
                    && !matches!(k, "daemon" | "help" | "version" | "json" | "load" | "yes" | "models" | "keep-models")
                {
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

/// `dogfight arena [check|pull]`: the sidecar's own commands, with the user's config applied.
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
                println!("no lessons yet ({}); they appear after the first losses in `dogfight arena`", dir.display());
            }
            for team in ["ZORB", "KRELL"] {
                let p = evolve::doctrine_path(team);
                if let Ok(text) = std::fs::read_to_string(&p) {
                    println!("{team} doctrine population ({}):", p.display());
                    for l in text.lines().filter(|l| !l.starts_with('#')) {
                        println!("  {l}");
                    }
                }
            }
            return 0;
        }
        Some("forget") => return reset_files(true, false),
        Some(other) => {
            eprintln!("dogfight arena: unknown subcommand '{other}' (check | pull | lessons)");
            return 2;
        }
        None => unreachable!(),
    };
    let mut cmd = match arena::command(cfg, &extra) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("dogfight arena: {e}");
            return 1;
        }
    };
    println!(
        "models: {} vs {}  backend {}  {}  evolve {}  ({})",
        cfg.lm_model_a,
        cfg.lm_model_b,
        cfg.lm_backend,
        vram_desc(cfg),
        if cfg.lm_evolve { "on" } else { "off" },
        config::config_path().display()
    );
    match cmd.status() {
        Ok(st) => st.code().unwrap_or(1),
        Err(e) => {
            eprintln!("dogfight arena: cannot run {}: {e}\n  cuda: pip install torch transformers    mac: pip install mlx-lm", cfg.lm_python);
            1
        }
    }
}

/// Popular small instruct models, all ungated (no licence click-through, no token), with bf16
/// sizes from the Hugging Face API on 2026-09-10. `dogfight models` prints it; `--zorb` / `--krell`
/// accept an alias or any HF id.
const MODELS: &[(&str, &str, f32, &str)] = &[
    ("qwen3-0.6b", "Qwen/Qwen3-0.6B", 1.5, "default for ZORB; fast and decisive, rarely breaks the order format"),
    ("qwen3-1.7b", "Qwen/Qwen3-1.7B", 4.1, "the strongest Qwen that fits 8 GB next to a small partner"),
    ("qwen2.5-0.5b", "Qwen/Qwen2.5-0.5B-Instruct", 1.0, "previous Qwen generation; compact and quick"),
    ("qwen2.5-1.5b", "Qwen/Qwen2.5-1.5B-Instruct", 3.1, "previous Qwen generation; compact and quick"),
    ("smollm2-360m", "HuggingFaceTB/SmolLM2-360M-Instruct", 0.7, "default for KRELL; the smallest model that still plays, sometimes skips the order format"),
    ("smollm2-1.7b", "HuggingFaceTB/SmolLM2-1.7B-Instruct", 3.4, "the original KRELL; follows instructions closely, plays cautiously"),
    ("gemma3-1b", "unsloth/gemma-3-1b-it", 2.0, "Google's Gemma 3 1B, ungated mirror; the most even small matchup against qwen3-0.6b"),
    ("lfm2-1.2b", "LiquidAI/LFM2-1.2B", 2.3, "Liquid AI, designed for on-device inference"),
    ("lfm2-700m", "LiquidAI/LFM2-700M", 1.5, "Liquid AI, designed for on-device inference; often skips the order format"),
    ("olmo2-1b", "allenai/OLMo-2-0425-1B-Instruct", 3.0, "fully open training recipe; fond of abducting cows"),
    ("falcon3-1b", "tiiuae/Falcon3-1B-Instruct", 3.3, "TII's Falcon 3, 1B instruct build"),
    ("danube3-500m", "h2oai/h2o-danube3-500m-chat", 1.0, "under 1B: very fast, often skips the order format"),
    ("deepseek-r1-1.5b", "deepseek-ai/DeepSeek-R1-Distill-Qwen-1.5B", 3.6, "reasoning model that thinks out loud; orders arrive late"),
    ("granite3.3-2b", "ibm-granite/granite-3.3-2b-instruct", 5.1, "strongest of the small class; 12 GB card, or lm_quant = \"8bit\" on 8 GB"),
    ("smollm3-3b", "HuggingFaceTB/SmolLM3-3B", 6.2, "strongest of the small class; 12 GB card, or lm_quant = \"8bit\" on 8 GB"),
    ("qwen2.5-3b", "Qwen/Qwen2.5-3B-Instruct", 6.2, "strongest of the small class; 12 GB card, or lm_quant = \"8bit\" on 8 GB"),
    // bigger cards and unified memory
    ("qwen3-4b", "Qwen/Qwen3-4B", 8.0, "the best mid-size commander; 12 GB+ card"),
    ("phi4-mini", "microsoft/Phi-4-mini-instruct", 7.7, "the best mid-size commander; 12 GB+ card"),
    ("olmo2-7b", "allenai/OLMo-2-1124-7B-Instruct", 14.6, "20 GB+ card"),
    ("qwen2.5-7b", "Qwen/Qwen2.5-7B-Instruct", 15.2, "20 GB+ card"),
    ("qwen3-8b", "Qwen/Qwen3-8B", 16.4, "24 GB+ card, or mlx-community/Qwen3-8B-4bit (4.6 GB) on a Mac"),
    ("granite3.3-8b", "ibm-granite/granite-3.3-8b-instruct", 16.3, "24 GB+ card"),
    ("qwen3-14b", "Qwen/Qwen3-14B", 29.5, "48 GB card, or 8bit on 24 GB, or mlx-community/Qwen3-14B-4bit (8.3 GB)"),
];

/// GPU memory in GB, or unified memory on Apple silicon: (gigabytes, description). None when
/// nothing can be measured (no nvidia-smi, not a Mac).
fn machine_memory() -> Option<(f32, String)> {
    if let Ok(o) = std::process::Command::new("nvidia-smi").args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]).output() {
        let text = String::from_utf8_lossy(&o.stdout);
        let gpus: Vec<(String, f32)> = text
            .lines()
            .filter_map(|l| {
                let (name, mem) = l.rsplit_once(',')?;
                Some((name.trim().to_string(), mem.trim().parse::<f32>().ok()? / 1024.0))
            })
            .collect();
        if let Some((name, gb)) = gpus.first() {
            let more = if gpus.len() > 1 { format!(" x{} (each model gets its own GPU)", gpus.len()) } else { String::new() };
            return Some((*gb, format!("{name}{more}, {gb:.0} GB GPU memory")));
        }
    }
    if cfg!(target_os = "macos") {
        if let Ok(o) = std::process::Command::new("sysctl").args(["-n", "hw.memsize"]).output() {
            if let Ok(bytes) = String::from_utf8_lossy(&o.stdout).trim().parse::<f64>() {
                let gb = (bytes / 1024f64.powi(3)) as f32;
                return Some((gb, format!("Apple silicon, {gb:.0} GB unified memory")));
            }
        }
    }
    None
}

/// An alias from the catalogue, or a Hugging Face id (`org/name[@rev]`). Anything else is a
/// typo: refuse it here rather than let the sidecar fail and the battle fall back silently.
fn resolve_model(name: &str) -> String {
    let key = name.to_lowercase();
    if let Some(m) = MODELS.iter().find(|m| m.0 == key) {
        return m.1.to_string();
    }
    if name.contains('/') {
        return name.to_string();
    }
    eprintln!("dogfight: unknown model alias '{name}'. Use an alias from `dogfight models` or a Hugging Face id like org/name.");
    let close: Vec<&str> = MODELS.iter().map(|m| m.0).filter(|a| a.starts_with(&key[..key.len().min(4)])).collect();
    if !close.is_empty() {
        eprintln!("         did you mean: {}", close.join(", "));
    }
    std::process::exit(2);
}

fn vram_desc(cfg: &Config) -> String {
    if cfg.lm_vram_gb <= 0.0 {
        "GPU cap auto (card memory - 2 GB)".to_string()
    } else {
        format!("GPU cap {} GB", cfg.lm_vram_gb)
    }
}

fn models_cmd(cfg: &Config) -> i32 {
    // what this machine can hold: the sidecar's cap (card - 2 GB, or lm_vram_gb), less ~0.8 GB
    // of activations and CUDA context; on a Mac, unified memory less what the OS needs
    let machine = machine_memory();
    let budget = match (&machine, cfg.lm_vram_gb) {
        (_, v) if v > 0.0 => v - 0.8,
        (Some((gb, d)), _) if d.contains("unified") => gb * 0.7,
        (Some((gb, _)), _) => (gb - 2.0).max(4.0) - 0.8,
        (None, _) => 8.0 - 2.0 - 0.8,
    };
    match &machine {
        Some((_, d)) => println!("this machine: {d}; room for about {budget:.0} GB of models at once"),
        None => println!("this machine: no GPU detected; verdicts below assume an 8 GB card"),
    }
    println!();
    println!("alias              hugging face id                               bf16  notes");
    for (alias, id, gb, note) in MODELS {
        let pair = if id == &cfg.lm_model_a { cfg_size(&cfg.lm_model_b) } else { cfg_size(&cfg.lm_model_a) };
        let fits = if gb + pair <= budget {
            "fits next to your other model"
        } else {
            "too big next to your other model (try lm_quant = \"8bit\", or a 4-bit MLX repo)"
        };
        let mark = if id == &cfg.lm_model_a || id == &cfg.lm_model_b { "*" } else { " " };
        println!("{mark}{:<17} {:<44} {:>4.1}GB  {}{}{}", alias, id, gb, note, if note.is_empty() { "" } else { "; " }, fits);
    }
    // the strongest balanced pair that fits: maximise the smaller model, then the total,
    // from different families so the fight is between two styles
    let mut best: Option<(&str, &str, f32, f32)> = None;
    for (i, a) in MODELS.iter().enumerate() {
        for b in MODELS.iter().skip(i + 1) {
            let (total, small) = (a.2 + b.2, a.2.min(b.2));
            let same_family = a.1.split('/').next() == b.1.split('/').next();
            let better = best.is_none_or(|(_, _, s, t)| small > s || (small == s && total > t));
            if total <= budget && !same_family && better {
                best = Some((a.0, b.0, small, total));
            }
        }
    }
    if let Some((a, b, _, t)) = best {
        println!("\nstrongest balanced pair for this machine: dogfight --zorb {a} --krell {b}   ({t:.1} GB)");
    }
    println!("\n* = current pair ({} vs {}).", cfg.lm_model_a, cfg.lm_model_b);
    println!("all of these are ungated (no licence click-through, no token).");
    println!("swap: dogfight --zorb lfm2-1.2b --krell gemma3-1b          (alias or any Hugging Face id, `id@revision` to pin)");
    println!("keep: lm_model_a / lm_model_b in {}", config::config_path().display());
    println!("then: dogfight arena pull   (download)   dogfight arena check --load   (measure peak memory)");
    println!("mlx: the same ids work through mlx-lm; mlx-community/<name>-4bit repos are smaller and faster.");
    0
}

fn cfg_size(id: &str) -> f32 {
    MODELS.iter().find(|m| m.1 == id).map(|m| m.2).unwrap_or(3.0)
}

/// `dogfight remove [--yes] [--keep-models]`: the whole footprint — config, state, data,
/// runtime dir, the models dogfight downloaded — then the binary.
fn remove_cmd(cfg: &Config, a: &Args) -> i32 {
    let keep_models = a.has("keep-models");
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from).unwrap_or_default();
    let exe = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok());
    let cargo_bin = home.join(".cargo/bin/dogfight");
    let mut dirs: Vec<std::path::PathBuf> = vec![config::state_dir(), config::data_dir(), config::runtime_dir()];
    if let Some(c) = config::config_path().parent() {
        dirs.insert(0, c.to_path_buf());
    }
    let hf_hub = std::env::var_os("HF_HOME").map(|h| std::path::PathBuf::from(h).join("hub")).unwrap_or_else(|| home.join(".cache/huggingface/hub"));
    // only models dogfight itself would have downloaded: the catalogue and the configured pair
    let mut ids: Vec<String> = MODELS.iter().map(|m| m.1.to_string()).collect();
    for m in [&cfg.lm_model_a, &cfg.lm_model_b] {
        let id = m.split('@').next().unwrap_or(m).to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let models: Vec<std::path::PathBuf> = ids.iter().map(|m| hf_hub.join(format!("models--{}", m.replace('/', "--")))).filter(|p| p.exists()).collect();
    let mut bins: Vec<std::path::PathBuf> = vec![];
    if cargo_bin.exists() {
        bins.push(cargo_bin.clone());
    }
    if let Some(e) = &exe {
        if !bins.iter().any(|b| b.canonicalize().ok().as_ref() == Some(e)) {
            bins.push(e.clone());
        }
    }
    println!("dogfight remove will delete:");
    for d in &dirs {
        if d.exists() {
            println!("  {}", d.display());
        }
    }
    if keep_models {
        println!("  (kept: {} downloaded model dir(s) in {}, --keep-models)", models.len(), hf_hub.display());
    } else {
        for m in &models {
            println!("  {} ({} MB, downloaded model)", m.display(), dir_size_mb(m));
        }
    }
    for b in &bins {
        println!("  {} (the program itself)", b.display());
    }
    println!("  (anything else in {} is not dogfight's and stays)", hf_hub.display());
    if !a.has("yes") {
        print!("Remove all of this? [y/N] ");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("nothing removed");
            return 1;
        }
    }
    let gone = |path: &std::path::Path, dir: bool| match if dir { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) } {
        Ok(()) => println!("removed {}", path.display()),
        Err(e) => eprintln!("could not remove {}: {e}", path.display()),
    };
    for d in dirs.iter().filter(|d| d.exists()) {
        gone(d, true);
    }
    if !keep_models {
        for m in &models {
            gone(m, true);
        }
    }
    for b in &bins {
        gone(b, false);
    }
    println!("dogfight is gone{}.", if keep_models { " (models kept)" } else { "" });
    0
}

fn dir_size_mb(p: &std::path::Path) -> u64 {
    fn walk(p: &std::path::Path) -> u64 {
        std::fs::read_dir(p)
            .map(|rd| rd.flatten().map(|e| e.metadata().map(|m| if m.is_dir() { walk(&e.path()) } else { m.len() }).unwrap_or(0)).sum())
            .unwrap_or(0)
    }
    walk(p) / (1024 * 1024)
}

/// `dogfight reset model|score|all`
fn reset_cmd(a: &Args) -> i32 {
    match a.pos.get(1).map(String::as_str) {
        Some("model") => reset_files(true, false),
        Some("score") => reset_files(false, true),
        Some("all") => reset_files(true, true),
        _ => {
            eprintln!("usage: dogfight reset model|score|all\n  model  forget the commanders' lessons and evolved doctrines\n  score  reset games won/lost (and lifetime kills/cows) between the models");
            2
        }
    }
}

fn reset_files(model: bool, score: bool) -> i32 {
    let mut n = 0;
    if model {
        if let Ok(rd) = std::fs::read_dir(arena::lessons_dir()) {
            n += rd.flatten().filter(|e| std::fs::remove_file(e.path()).is_ok()).count();
        }
        for team in ["ZORB", "KRELL"] {
            if std::fs::remove_file(evolve::doctrine_path(team)).is_ok() {
                n += 1;
            }
        }
        println!("model memory reset: {n} file(s) removed (lessons + doctrines)");
    }
    if score {
        let mut m = 0;
        if let Ok(rd) = std::fs::read_dir(config::state_dir()) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with("score-") && std::fs::remove_file(e.path()).is_ok() {
                    m += 1;
                }
            }
        }
        println!("score reset: {m} scoreboard file(s) removed");
    }
    0
}

fn main() {
    // `dogfight status | head` must not panic on a closed pipe
    // SAFETY: restoring the default disposition of SIGPIPE has no preconditions.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
    let a = Args::parse();
    if a.has("version") {
        println!("dogfight {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if a.has("help") {
        print!("{HELP}");
        return;
    }
    let mut cfg = Config::load();
    // `--zorb MODEL` / `--krell MODEL`: pick the commanders for this run (alias or Hugging Face id)
    if let Some(m) = a.get("zorb") {
        cfg.lm_model_a = resolve_model(m);
    }
    if let Some(m) = a.get("krell") {
        cfg.lm_model_b = resolve_model(m);
    }
    if let Some(b) = a.get("backend") {
        if matches!(b, "auto" | "cuda" | "mlx" | "cpu") {
            cfg.lm_backend = b.to_string();
        }
    }
    let cfg = cfg;
    // no arguments: the battle itself (falls back to the built-in pilots if the sidecar can't start)
    let cmd = a.pos.first().map(|s| s.as_str()).unwrap_or("ufo-battle");
    let on = |v: &str| !matches!(v.to_lowercase().as_str(), "off" | "false" | "0" | "no");
    // `--shot 75:battle.ansi`: save the live frame at second 75 (render it with eval/ansi2png.py)
    let shot: Option<(f32, String)> = a.get("shot").and_then(|v| {
        let (secs, path) = v.split_once(':')?;
        Some((secs.parse().ok()?, path.to_string()))
    });
    let evolve = a.get("lessons").or_else(|| a.get("evolve")).map(on);
    // `dogfight run [cuda|mlx] [ufo|ufo-battle] [evolve]`: positional words pick backend, scene, GA
    let mut scene = a.get("scene").map(String::from);
    let mut pilots = a.get("pilots").map(String::from);
    let mut backend = a.get("backend").map(String::from);
    let mut genetic = a.get("genetic").map(on);
    for w in a.pos.iter().skip(1) {
        match w.as_str() {
            "cuda" | "mlx" | "cpu" => backend = Some(w.clone()),
            "ufo-battle" | "battle" | "lm" => {
                scene = Some("ufo".into());
                pilots = Some("lm".into());
            }
            "evolve" | "genetic" => genetic = Some(true),
            "check" | "pull" | "lessons" | "forget" | "model" | "score" | "all" | "remove" | "models" => {}
            other => scene = Some(other.to_string()),
        }
    }
    let run_opts = |scene: Option<String>, pilots: Option<String>, backend: Option<String>| app::RunOpts {
        perf: a.get("perf").map(String::from),
        scene,
        pilots,
        backend,
        evolve,
        genetic,
        fps: a.num("fps"),
        seed: a.num("seed"),
        duration: a.num("duration"),
        shot: shot.clone(),
    };
    let code = match cmd {
        // `dogfight` / `dogfight cuda` / `dogfight mlx` / `dogfight evolve`: the battle. `dogfight ufo`: the baseline.
        "run" | "preview" | "demo" | "play" => app::run(&cfg, run_opts(scene, pilots, backend)),
        "cuda" | "mlx" | "cpu" | "evolve" | "ufo-battle" | "battle" => {
            let mut backend = backend;
            let mut genetic = genetic;
            for w in a.pos.iter() {
                match w.as_str() {
                    "cuda" | "mlx" | "cpu" => backend = Some(w.clone()),
                    "evolve" => genetic = Some(true),
                    _ => {}
                }
            }
            app::run(
                &cfg,
                app::RunOpts {
                    perf: a.get("perf").map(String::from),
                    scene: Some("ufo".into()),
                    pilots: Some("lm".into()),
                    backend,
                    evolve,
                    genetic,
                    fps: a.num("fps"),
                    seed: a.num("seed"),
                    duration: a.num("duration"),
                    shot: shot.clone(),
                },
            )
        }
        "ufo" => app::run(&cfg, run_opts(Some("ufo".into()), Some("builtin".into()), None)),
        "arena" if a.pos.len() == 1 || matches!(a.pos[1].as_str(), "cuda" | "mlx" | "cpu" | "evolve") => {
            app::run(&cfg, run_opts(Some("ufo".into()), Some("lm".into()), backend))
        }
        "arena" => arena_cmd(&cfg, &a),
        "reset" => reset_cmd(&a),
        "models" => models_cmd(&cfg),
        "remove" => remove_cmd(&cfg, &a),
        "list" => {
            for (n, d) in scenes::NAMES {
                println!("  {n:<8} {d}");
            }
            println!("\nbackend for ufo-battle: {} (config lm_backend; or `dogfight run cuda|mlx ufo-battle`)", cfg.lm_backend);
            0
        }
        "config" => {
            print!("{}", config::DEFAULT_TOML);
            0
        }
        "status" => {
            println!("config:      {}{}", config::config_path().display(), if config::config_path().exists() { "" } else { " (defaults)" });
            println!("fps:         {} (unfocused {})", cfg.fps, cfg.unfocused_fps);
            println!("colour:      {}", if cfg.truecolor() { "truecolor" } else { "256" });
            println!("pilots:      {}", cfg.ufo_pilots);
            println!(
                "lm:          {} vs {} ({} backend, {}, {}, evolve {})",
                cfg.lm_model_a,
                cfg.lm_model_b,
                cfg.lm_backend,
                vram_desc(&cfg),
                cfg.lm_python,
                if cfg.lm_evolve { "on" } else { "off" }
            );
            println!("state:       {}", config::state_dir().display());
            println!("arena log:   {}", arena::log_path().display());
            0
        }
        "bench" => bench(&cfg, &a),
        "snapshot" => snapshot(&cfg, &a),
        other => {
            eprintln!("dogfight: unknown command '{other}'\n");
            eprint!("{HELP}");
            2
        }
    };
    std::process::exit(code);
}
