//! Idle detection, designed around a measured fact: bash defers USR1/USR2 traps
//! while idle in readline, but services SIGALRM immediately (bash 5.2, zsh 5.9 and
//! fish 3.7 all verified). So:
//!
//!   watcher (1 tiny process per shell) --SIGALRM--> shell --runs--> `reverie run --idle-trigger`
//!
//! The watcher fires only when (a) the shell itself owns the terminal foreground
//! (tpgid == shell pgrp, i.e. no command running) and is sleeping in read, and
//! (b) the tty has had no input for `idle_seconds` (tty atime; kernel granularity 8 s).
//! A marker file proves the SIGALRM came from us, so a stray/late alarm is a no-op.
use crate::config::{runtime_dir, state_dir, Config};
use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn marker(pid: i32) -> PathBuf {
    runtime_dir().join(format!("fire-{pid}"))
}

/// True iff our watcher asked for this run within the last 5 s.
pub fn claim_trigger(pid: i32) -> bool {
    let m = marker(pid);
    let fresh = fs::read_to_string(&m).ok().and_then(|s| s.trim().parse::<u64>().ok()).map(|t| now().saturating_sub(t) <= 5).unwrap_or(false);
    let _ = fs::remove_file(&m);
    fresh && !paused()
}

/// flock-based slot so at most `max` terminals animate at once.
pub fn acquire_slot(max: u32) -> Option<fs::File> {
    for i in 0..max.max(1) {
        let p = runtime_dir().join(format!("slot-{i}"));
        if let Ok(f) = fs::OpenOptions::new().create(true).truncate(false).write(true).open(&p) {
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Some(f);
            }
        }
    }
    None
}

pub fn pause_file() -> PathBuf {
    state_dir().join("paused-until")
}
pub fn paused() -> bool {
    fs::read_to_string(pause_file()).ok().and_then(|s| s.trim().parse::<u64>().ok()).map(|t| t > now()).unwrap_or(false)
}
pub fn set_pause(minutes: Option<u64>) {
    let _ = fs::create_dir_all(state_dir());
    let until = match minutes {
        Some(m) => now() + m * 60,
        None => u64::MAX / 2,
    };
    let _ = fs::write(pause_file(), until.to_string());
}
pub fn resume() {
    let _ = fs::remove_file(pause_file());
}

struct ProcStat {
    state: char,
    pgrp: i32,
    tpgid: i32,
}

fn proc_stat(pid: i32) -> Option<ProcStat> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &s[s.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    Some(ProcStat { state: f.first()?.chars().next()?, pgrp: f.get(2)?.parse().ok()?, tpgid: f.get(5)?.parse().ok()? })
}

fn tty_of(pid: i32) -> Option<PathBuf> {
    let p = fs::read_link(format!("/proc/{pid}/fd/0")).ok()?;
    let s = p.to_string_lossy();
    if s.starts_with("/dev/pts/") || s.starts_with("/dev/tty") {
        Some(p)
    } else {
        None
    }
}

fn atime(p: &PathBuf) -> u64 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(p).map(|m| m.atime().max(0) as u64).unwrap_or(0)
}

fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 || *libc::__errno_location() == libc::EPERM }
}

fn daemonize() -> bool {
    unsafe {
        match libc::fork() {
            -1 => return false,
            0 => {}
            _ => libc::_exit(0),
        }
        libc::setsid();
        let null = libc::open(b"/dev/null\0".as_ptr() as *const libc::c_char, libc::O_RDWR);
        if null >= 0 {
            libc::dup2(null, 0);
            libc::dup2(null, 1);
            libc::dup2(null, 2);
            if null > 2 {
                libc::close(null);
            }
        }
        libc::chdir(b"/\0".as_ptr() as *const libc::c_char);
    }
    true
}

pub fn watch(pid: i32, idle_override: Option<u64>, daemon: bool) -> i32 {
    let Some(tty) = tty_of(pid) else {
        eprintln!("reverie watch: pid {pid} has no terminal on stdin");
        return 1;
    };
    if daemon && !daemonize() {
        return 1;
    }
    // one watcher per shell: replace any older one
    let pidfile = runtime_dir().join(format!("watch-{pid}"));
    if let Ok(old) = fs::read_to_string(&pidfile).map(|s| s.trim().parse::<i32>().unwrap_or(0)) {
        if old > 0 && old != std::process::id() as i32 {
            let is_ours = fs::read_to_string(format!("/proc/{old}/cmdline")).map(|c| c.contains("reverie") && c.contains("watch")).unwrap_or(false);
            if is_ours {
                unsafe { libc::kill(old, libc::SIGTERM) };
            }
        }
    }
    let _ = fs::write(&pidfile, std::process::id().to_string());
    let mut cfg = Config::load();
    let mut cfg_check = now();
    let mut last_fire = 0u64;
    loop {
        if !alive(pid) {
            break;
        }
        if now().saturating_sub(cfg_check) > 60 {
            cfg = Config::load();
            cfg_check = now();
        }
        let idle = idle_override.unwrap_or(cfg.idle_seconds).max(2);
        let mut sleep = 5u64;
        if !paused() {
            if let Some(st) = proc_stat(pid) {
                let at_prompt = st.tpgid == st.pgrp && st.state == 'S';
                if at_prompt {
                    let since = now().saturating_sub(atime(&tty).max(last_fire));
                    if since >= idle {
                        let _ = fs::write(marker(pid), now().to_string());
                        unsafe { libc::kill(pid, libc::SIGALRM) };
                        last_fire = now();
                        sleep = 2;
                    } else {
                        sleep = (idle - since).clamp(1, 10);
                    }
                }
            }
        } else {
            sleep = 30;
        }
        std::thread::sleep(Duration::from_secs(sleep));
    }
    let _ = fs::remove_file(&pidfile);
    let _ = fs::remove_file(marker(pid));
    0
}

/// Stop every watcher belonging to this user (used by `uninstall`).
pub fn stop_watchers() -> usize {
    let me = std::process::id() as i32;
    let uid = unsafe { libc::getuid() };
    let mut n = 0;
    if let Ok(rd) = fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else { continue };
            if pid == me {
                continue;
            }
            use std::os::unix::fs::MetadataExt;
            if fs::metadata(e.path()).map(|m| m.uid() != uid).unwrap_or(true) {
                continue;
            }
            let cmd = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            let parts: Vec<&[u8]> = cmd.split(|&b| b == 0).collect();
            let is_rev = parts.first().map(|p| p.ends_with(b"reverie")).unwrap_or(false);
            if is_rev && parts.get(1) == Some(&&b"watch"[..]) {
                unsafe { libc::kill(pid, libc::SIGTERM) };
                n += 1;
            }
        }
    }
    n
}
