//! Link to the arena sidecar (agents/arena.py): two small language models, one per
//! saucer team, each issuing orders for its ships. The Rust side stays tiny: it spawns
//! `python3 arena.py`, writes one JSON observation per decision and reads back plain
//! `ORDERS ...` lines from a reader thread. No GPU code lives in the binary.
use crate::config::{data_dir, state_dir, Config};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

pub const SCRIPT: &str = include_str!("../agents/arena.py");

/// What a ship is currently trying to do. Executed by the steering behaviours in ufo.rs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    Attack(usize),
    Hunt,
    Flee,
    Abduct(usize),
    Guard(usize),
    Patrol,
}

impl Order {
    pub fn describe(&self) -> String {
        match self {
            Order::Attack(t) => format!("attack E{t}"),
            Order::Hunt => "hunt".into(),
            Order::Flee => "flee".into(),
            Order::Abduct(c) => format!("abduct C{c}"),
            Order::Guard(a) => format!("guard S{a}"),
            Order::Patrol => "patrol".into(),
        }
    }
    fn parse(tok: &str) -> Option<(usize, Order)> {
        let mut it = tok.split(':');
        let id: usize = it.next()?.strip_prefix('S')?.parse().ok()?;
        let verb = it.next()?;
        let arg: Option<usize> = it.next().and_then(|a| a.parse().ok());
        let o = match (verb, arg) {
            ("attack", Some(t)) => Order::Attack(t),
            ("attack", None) | ("hunt", _) => Order::Hunt,
            ("flee", _) => Order::Flee,
            ("abduct", Some(c)) => Order::Abduct(c),
            ("guard", Some(a)) => Order::Guard(a),
            ("patrol", _) | ("abduct", None) | ("guard", None) => Order::Patrol,
            _ => return None,
        };
        Some((id, o))
    }
}

pub enum Msg {
    Status(String),
    Ready { team: usize, label: String, gb: f32 },
    Orders { team: usize, orders: Vec<(usize, Order)>, say: String, deploy: usize },
    /// the commander wrote a lesson after a loss (evolve mode)
    Lesson { team: usize, text: String },
    Error(String),
    Exited,
}

pub struct Arena {
    child: Child,
    stdin: Option<ChildStdin>,
    rx: Receiver<String>,
    exited: bool,
}

pub fn script_path() -> PathBuf {
    data_dir().join("arena.py")
}

/// Write the embedded sidecar next to the user's data (only when it changed), so a
/// `cargo install` is all that's needed to ship a new version of it.
pub fn install_script() -> std::io::Result<PathBuf> {
    let p = script_path();
    if std::fs::read_to_string(&p).map(|s| s != SCRIPT).unwrap_or(true) {
        std::fs::create_dir_all(data_dir())?;
        std::fs::write(&p, SCRIPT)?;
    }
    Ok(p)
}

pub fn log_path() -> PathBuf {
    state_dir().join("arena.log")
}

/// Where the commanders keep what they learned (one file per team and model).
pub fn lessons_dir() -> PathBuf {
    state_dir().join("lessons")
}

/// `python3 arena.py --model-a .. --model-b .. --backend .. --vram-gb .. --quant ..` + extra args.
pub fn command(cfg: &Config, extra: &[&str]) -> std::io::Result<Command> {
    let script = install_script()?;
    let mut cmd = Command::new(&cfg.lm_python);
    cmd.arg(script)
        .args(["--model-a", &cfg.lm_model_a, "--model-b", &cfg.lm_model_b, "--backend", &cfg.lm_backend, "--quant", &cfg.lm_quant])
        .arg("--vram-gb")
        .arg(format!("{}", cfg.lm_vram_gb))
        .arg(if cfg.lm_evolve { "--evolve" } else { "--no-evolve" })
        .arg("--memory")
        .arg(lessons_dir())
        .args(extra)
        .env("PYTHONUNBUFFERED", "1")
        .env("TRANSFORMERS_VERBOSITY", "error")
        .env("HF_HUB_DISABLE_PROGRESS_BARS", "1")
        .env("TOKENIZERS_PARALLELISM", "false");
    Ok(cmd)
}

impl Arena {
    pub fn spawn(cfg: &Config) -> Result<Arena, String> {
        let mut cmd = command(cfg, &[]).map_err(|e| format!("cannot write arena.py: {e}"))?;
        let _ = std::fs::create_dir_all(state_dir());
        let log = std::fs::File::create(log_path()).map_err(|e| format!("cannot open arena.log: {e}"))?;
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(log);
        let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", cfg.lm_python))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Arena { child, stdin, rx, exited: false })
    }

    /// One observation line (JSON). Returns false once the sidecar is gone.
    pub fn send(&mut self, line: &str) -> bool {
        match self.stdin.as_mut() {
            Some(w) => w.write_all(line.as_bytes()).and_then(|_| w.write_all(b"\n")).is_ok(),
            None => false,
        }
    }

    pub fn poll(&mut self) -> Vec<Msg> {
        let mut out = vec![];
        loop {
            match self.rx.try_recv() {
                Ok(l) => {
                    if let Some(m) = parse(&l) {
                        out.push(m);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.exited {
                        self.exited = true;
                        out.push(Msg::Exited);
                    }
                    break;
                }
            }
        }
        if !self.exited {
            if let Ok(Some(_)) = self.child.try_wait() {
                // let the reader drain first; the disconnect above reports it next frame
            }
        }
        out
    }
}

impl Drop for Arena {
    fn drop(&mut self) {
        if let Some(mut w) = self.stdin.take() {
            let _ = w.write_all(b"{\"t\":\"quit\"}\n");
        }
        for _ in 0..30 {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse(line: &str) -> Option<Msg> {
    let line = line.trim();
    let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
    match kind {
        "STATUS" => Some(Msg::Status(rest.to_string())),
        "ERROR" => Some(Msg::Error(rest.to_string())),
        "LESSON" => {
            let (team, text) = rest.split_once(' ').unwrap_or((rest, ""));
            let team: usize = team.parse().ok()?;
            (team < 2).then_some(Msg::Lesson { team, text: text.chars().filter(|c| (' '..='~').contains(c)).take(120).collect() })
        }
        "READY" => {
            let mut it = rest.split_whitespace();
            let team: usize = it.next()?.parse().ok()?;
            let label = it.next()?.to_string();
            let gb: f32 = it.next().and_then(|g| g.parse().ok()).unwrap_or(0.0);
            (team < 2).then_some(Msg::Ready { team, label, gb })
        }
        "ORDERS" => {
            let (head, say) = rest.split_once('|').unwrap_or((rest, ""));
            let mut it = head.split_whitespace();
            let team: usize = it.next()?.parse().ok()?;
            let _tick = it.next()?;
            let mut deploy = 0;
            let mut orders = vec![];
            for tok in it {
                if let Some(n) = tok.strip_prefix("D:") {
                    deploy = n.parse().unwrap_or(0);
                } else if let Some(o) = Order::parse(tok) {
                    orders.push(o);
                }
            }
            (team < 2).then_some(Msg::Orders { team, orders, say: sanitize(say), deploy })
        }
        _ => None,
    }
}

/// Only printable ASCII reaches the glyph layer (cell widths stay predictable).
fn sanitize(s: &str) -> String {
    s.chars().filter(|c| (' '..='~').contains(c)).take(40).collect::<String>().trim().to_string()
}

/// Minimal JSON string escaping for the observation lines.
pub fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push(' '),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}
