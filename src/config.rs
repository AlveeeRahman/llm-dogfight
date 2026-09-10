//! Config lives in ~/.config/reverie/config.toml. We parse a small TOML subset
//! (key = value, strings, numbers, bools, string arrays, comments) with no deps.
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub idle_seconds: u64,
    pub fps: u32,
    pub unfocused_fps: u32,
    pub scenes: Vec<String>,
    pub rotate_minutes: f32,
    pub max_instances: u32,
    pub color: String,
    pub tolerance: i32,
    /// "builtin" (heuristic pilots, no GPU) or "lm" (two language models via agents/arena.py)
    pub ufo_pilots: String,
    pub lm_backend: String,
    pub lm_model_a: String,
    pub lm_model_b: String,
    pub lm_vram_gb: f32,
    pub lm_quant: String,
    pub lm_python: String,
    pub lm_think_seconds: f32,
    /// commanders write a lesson after each loss and keep it in their prompts (and on disk)
    pub lm_evolve: bool,
    /// genetic doctrine evolution (`reverie run ufo-battle evolve`)
    pub lm_genetic: bool,
    /// reinforcements per team per game; a team that cannot field a saucer loses the game
    pub lm_regens: u32,
    /// saucers per team on screen (the previous game's loser starts with one extra)
    pub lm_max_alive: u32,
}

pub const DEFAULT_TOML: &str = r#"# reverie — ~/.config/reverie/config.toml

# Seconds your shell must sit idle at the prompt before the screensaver starts.
idle_seconds = 300

# Frames per second while animating, and while the window is unfocused.
fps = 30
unfocused_fps = 12

# Idle-screensaver mode only (`reverie install`): "ufo" (built-in pilots, no GPU) and/or "ufo-battle".
scenes = ["ufo"]
rotate_minutes = 4

# At most this many terminals animate at once (others stay quiet).
max_instances = 3

# "auto", "truecolor" or "256"
color = "auto"

# Colour change (0-255 per channel) ignored between frames. Higher = fewer bytes
# for the terminal to draw = lower CPU. 3-8 is invisible in practice.
tolerance = 5

# Who flies the "ufo" scene: "builtin" (heuristic pilots, no GPU) or "lm" (same as the
# "ufo-battle" scene: two small language models command the two teams; needs python3 with
# torch+transformers on a CUDA GPU, or mlx-lm on Apple silicon). Try `reverie run cuda ufo-battle`;
# check the setup with `reverie arena check --load`.
ufo_pilots = "builtin"

# Language-model commanders. Defaults fit an 8 GB CUDA card (about 5 GB together).
lm_backend = "auto"                                 # auto = cuda, or mlx on Apple silicon; or `reverie mlx` / `reverie cuda`
lm_model_a = "Qwen/Qwen3-0.6B"                      # team ZORB
lm_model_b = "HuggingFaceTB/SmolLM2-1.7B-Instruct"  # team KRELL
lm_vram_gb = 6                                      # CUDA memory cap for both models
lm_quant = "none"                                   # cuda: none | 8bit | 4bit (bitsandbytes) for bigger models
lm_python = "python3"                               # interpreter that has the ML packages
lm_think_seconds = 1.0                              # minimum pause between a team's orders
lm_evolve = true                                    # learn from every destroyed saucer (lessons persist in
                                                    # ~/.local/state/reverie/lessons; `reverie reset model` clears)
lm_genetic = false                                  # evolve each team's bounded doctrine with a genetic algorithm
                                                    # (same as the `evolve` word: `reverie run ufo-battle evolve`)
lm_max_alive = 4                                    # saucers per team on screen (a game's loser restarts with +1)
lm_regens = 20                                      # reinforcements per team per game; then it is a fight to the death
"#;

impl Default for Config {
    fn default() -> Self {
        Config {
            idle_seconds: 300,
            fps: 30,
            unfocused_fps: 12,
            scenes: vec!["ufo".into()],
            rotate_minutes: 4.0,
            max_instances: 3,
            color: "auto".into(),
            tolerance: 5,
            ufo_pilots: "builtin".into(),
            lm_backend: "auto".into(),
            lm_model_a: "Qwen/Qwen3-0.6B".into(),
            lm_model_b: "HuggingFaceTB/SmolLM2-1.7B-Instruct".into(),
            lm_vram_gb: 6.0,
            lm_quant: "none".into(),
            lm_python: "python3".into(),
            lm_think_seconds: 1.0,
            lm_evolve: true,
            lm_genetic: false,
            lm_regens: 20,
            lm_max_alive: 4,
        }
    }
}

impl Config {
    pub fn load() -> Config {
        let mut c = Config::default();
        if let Ok(text) = std::fs::read_to_string(config_path()) {
            c.apply(&text);
        }
        c
    }

    pub fn apply(&mut self, text: &str) {
        for raw in text.lines() {
            let line = strip_comment(raw).trim();
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "idle_seconds" => set_num(&mut self.idle_seconds, v),
                "fps" => set_num(&mut self.fps, v),
                "unfocused_fps" => set_num(&mut self.unfocused_fps, v),
                "rotate_minutes" => set_num(&mut self.rotate_minutes, v),
                "max_instances" => set_num(&mut self.max_instances, v),
                "tolerance" => set_num(&mut self.tolerance, v),
                "lm_vram_gb" => set_num(&mut self.lm_vram_gb, v),
                "lm_think_seconds" => set_num(&mut self.lm_think_seconds, v),
                "color" => self.color = unquote(v),
                "ufo_pilots" | "pilots" => self.ufo_pilots = unquote(v),
                "lm_backend" => self.lm_backend = unquote(v),
                "lm_model_a" => self.lm_model_a = unquote(v),
                "lm_model_b" => self.lm_model_b = unquote(v),
                "lm_quant" => self.lm_quant = unquote(v),
                "lm_python" => self.lm_python = unquote(v),
                "lm_evolve" | "lm_lessons" => self.lm_evolve = !matches!(unquote(v).to_lowercase().as_str(), "false" | "0" | "no" | "off"),
                "lm_genetic" => self.lm_genetic = !matches!(unquote(v).to_lowercase().as_str(), "false" | "0" | "no" | "off"),
                "lm_regens" => set_num(&mut self.lm_regens, v),
                "lm_max_alive" => set_num(&mut self.lm_max_alive, v),
                "scenes" => {
                    let list: Vec<String> = v.trim_start_matches('[').trim_end_matches(']').split(',').map(unquote).filter(|s| !s.is_empty()).collect();
                    if !list.is_empty() {
                        self.scenes = list;
                    }
                }
                _ => {} // unknown keys (including v0.1's garden/meadow keys) are ignored
            }
        }
        self.fps = self.fps.clamp(1, 120);
        self.unfocused_fps = self.unfocused_fps.clamp(1, 120);
        self.idle_seconds = self.idle_seconds.max(5);
        self.tolerance = self.tolerance.clamp(0, 32);
        self.lm_vram_gb = self.lm_vram_gb.clamp(1.0, 512.0);
        self.lm_max_alive = self.lm_max_alive.clamp(1, 4);
        self.lm_regens = self.lm_regens.min(500);
        if !matches!(self.ufo_pilots.as_str(), "builtin" | "lm") {
            self.ufo_pilots = "builtin".into();
        }
        if !matches!(self.lm_backend.as_str(), "auto" | "cuda" | "mlx") {
            self.lm_backend = "auto".into();
        }
        if !matches!(self.lm_quant.as_str(), "none" | "8bit" | "4bit") {
            self.lm_quant = "none".into();
        }
    }

    pub fn truecolor(&self) -> bool {
        match self.color.as_str() {
            "truecolor" | "24bit" => true,
            "256" => false,
            _ => {
                let ct = std::env::var("COLORTERM").unwrap_or_default();
                // VTE-based terminals (GNOME Terminal, Ptyxis) set VTE_VERSION and do truecolor.
                ct.contains("truecolor")
                    || ct.contains("24bit")
                    || std::env::var("VTE_VERSION").is_ok()
                    || std::env::var("TERM").map(|t| t.contains("direct") || t.contains("kitty") || t.contains("ghostty")).unwrap_or(false)
            }
        }
    }
}

fn strip_comment(s: &str) -> &str {
    let mut in_q = false;
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_q = !in_q,
            '#' if !in_q => return &s[..i],
            _ => {}
        }
    }
    s
}
fn unquote(s: &str) -> String {
    s.trim().trim_matches('"').trim_matches('\'').to_string()
}
fn set_num<T: std::str::FromStr>(dst: &mut T, v: &str) {
    if let Ok(x) = v.trim().parse::<T>() {
        *dst = x;
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
}
fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home().join(fallback),
    }
}
pub fn config_path() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("reverie").join("config.toml")
}
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("reverie")
}
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("reverie")
}
/// Per-user runtime dir shared with the shell snippets:
/// `${XDG_RUNTIME_DIR:-/tmp}/reverie-$UID`
pub fn runtime_dir() -> PathBuf {
    let base = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from("/tmp"),
    };
    // SAFETY: getuid never fails and has no preconditions.
    let uid = unsafe { libc::getuid() };
    let d = base.join(format!("reverie-{uid}"));
    let _ = std::fs::create_dir_all(&d);
    if let Ok(c) = std::ffi::CString::new(d.to_string_lossy().as_bytes()) {
        // SAFETY: `c` is a valid NUL-terminated path that outlives the call.
        unsafe { libc::chmod(c.as_ptr(), 0o700) };
    }
    d
}
