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
    pub garden_speed: f32,
    pub day_cycle_seconds: f32,
    pub character: String,
}

pub const DEFAULT_TOML: &str = r#"# reverie — ~/.config/reverie/config.toml

# Seconds your shell must sit idle at the prompt before the screensaver starts.
idle_seconds = 300

# Frames per second while animating, and while the window is unfocused.
fps = 30
unfocused_fps = 12

# Scenes to rotate through: ufo, galaxy, garden, meadow, or portrait:<sprite-name>
scenes = ["ufo", "galaxy", "garden", "meadow"]
rotate_minutes = 4

# At most this many terminals animate at once (others stay quiet).
max_instances = 3

# "auto", "truecolor" or "256"
color = "auto"

# Colour change (0-255 per channel) ignored between frames. Higher = fewer bytes
# for the terminal to draw = lower CPU. 3-8 is invisible in practice.
tolerance = 5

# Garden: growth speed multiplier (1.0 = a plant matures in ~6 min of idle time).
garden_speed = 1.0

# Meadow: seconds for a full day -> sunset -> night -> dawn cycle.
day_cycle_seconds = 300

# Meadow character: "builtin" or the name of a sprite made with `reverie import`.
character = "builtin"
"#;

impl Default for Config {
    fn default() -> Self {
        Config {
            idle_seconds: 300,
            fps: 30,
            unfocused_fps: 12,
            scenes: vec!["ufo".into(), "galaxy".into(), "garden".into(), "meadow".into()],
            rotate_minutes: 4.0,
            max_instances: 3,
            color: "auto".into(),
            tolerance: 5,
            garden_speed: 1.0,
            day_cycle_seconds: 300.0,
            character: "builtin".into(),
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
                "garden_speed" => set_num(&mut self.garden_speed, v),
                "day_cycle_seconds" => set_num(&mut self.day_cycle_seconds, v),
                "color" => self.color = unquote(v),
                "character" => self.character = unquote(v),
                "scenes" => {
                    let list: Vec<String> = v.trim_start_matches('[').trim_end_matches(']').split(',').map(unquote).filter(|s| !s.is_empty()).collect();
                    if !list.is_empty() {
                        self.scenes = list;
                    }
                }
                _ => {}
            }
        }
        self.fps = self.fps.clamp(1, 120);
        self.unfocused_fps = self.unfocused_fps.clamp(1, 120);
        self.idle_seconds = self.idle_seconds.max(5);
        self.tolerance = self.tolerance.clamp(0, 32);
    }

    pub fn truecolor(&self) -> bool {
        match self.color.as_str() {
            "truecolor" | "24bit" => true,
            "256" => false,
            _ => {
                let ct = std::env::var("COLORTERM").unwrap_or_default();
                // VTE-based terminals (GNOME Terminal, Ptyxis) set VTE_VERSION and do truecolor.
                ct.contains("truecolor") || ct.contains("24bit") || std::env::var("VTE_VERSION").is_ok() || std::env::var("TERM").map(|t| t.contains("direct") || t.contains("kitty") || t.contains("ghostty")).unwrap_or(false)
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
    let uid = unsafe { libc::getuid() };
    let d = base.join(format!("reverie-{uid}"));
    let _ = std::fs::create_dir_all(&d);
    unsafe {
        let c = std::ffi::CString::new(d.to_string_lossy().as_bytes()).unwrap();
        libc::chmod(c.as_ptr(), 0o700);
    }
    d
}
