//! Idle detection, designed around a measured fact: bash defers USR1/USR2 traps
//! while idle in readline, but services SIGALRM immediately (bash 5.2, zsh 5.9 and
//! fish 3.7 all verified). So:
//!
//!   watcher (1 tiny process per shell) --SIGALRM--> shell --runs--> `dogfight run --idle-trigger`
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
            // SAFETY: `f` is an open file we own; flock on its descriptor has no memory preconditions.
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
    pgrp: i32,
    tpgid: i32,
    sleeping: bool,
}

/// Linux: /proc/<pid>/stat (state, pgrp, tpgid). Zero syscalls beyond one read.
#[cfg(target_os = "linux")]
fn proc_stat(pid: i32) -> Option<ProcStat> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &s[s.rfind(')')? + 2..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    Some(ProcStat { sleeping: f.first()?.starts_with('S'), pgrp: f.get(2)?.parse().ok()?, tpgid: f.get(5)?.parse().ok()? })
}

/// macOS/BSD: `ps` knows the tty's foreground process group (tpgid). One exec per poll
/// (every 1-10 s) is fine for a watcher that otherwise sleeps.
#[cfg(not(target_os = "linux"))]
fn proc_stat(pid: i32) -> Option<ProcStat> {
    let out = std::process::Command::new("ps").args(["-o", "pgid=,tpgid=,stat=", "-p", &pid.to_string()]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let f: Vec<&str> = s.split_whitespace().collect();
    Some(ProcStat {
        pgrp: f.first()?.parse().ok()?,
        tpgid: f.get(1)?.parse().ok()?,
        sleeping: f.get(2).map_or(true, |st| st.starts_with('S') || st.starts_with('I')),
    })
}

/// The shell's terminal: `--tty` from the shell snippet, else (Linux) its stdin link.
fn tty_of(pid: i32, hint: Option<&str>) -> Option<PathBuf> {
    if let Some(t) = hint.filter(|t| t.starts_with("/dev/")) {
        return Some(PathBuf::from(t));
    }
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
    // SAFETY: signal 0 only probes for existence and permission; nothing is delivered.
    let ok = unsafe { libc::kill(pid, 0) == 0 };
    ok || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Is `pid` one of our `dogfight watch` processes? (before replacing it)
fn is_watcher(pid: i32) -> bool {
    #[cfg(target_os = "linux")]
    {
        fs::read_to_string(format!("/proc/{pid}/cmdline")).map(|c| c.contains("dogfight") && c.contains("watch")).unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::process::Command::new("ps")
            .args(["-o", "command=", "-p", &pid.to_string()])
            .output()
            .map(|o| {
                let c = String::from_utf8_lossy(&o.stdout);
                c.contains("dogfight") && c.contains("watch")
            })
            .unwrap_or(false)
    }
}

fn daemonize() -> bool {
    // SAFETY: called once at startup from `watch`, before any thread exists, so fork is
    // sound; the child only calls async-signal-safe functions (setsid, open, dup2, close,
    // chdir) with valid arguments before returning to Rust.
    unsafe {
        match libc::fork() {
            -1 => return false,
            0 => {}
            _ => libc::_exit(0),
        }
        libc::setsid();
        let null = libc::open(c"/dev/null".as_ptr(), libc::O_RDWR);
        if null >= 0 {
            libc::dup2(null, 0);
            libc::dup2(null, 1);
            libc::dup2(null, 2);
            if null > 2 {
                libc::close(null);
            }
        }
        libc::chdir(c"/".as_ptr());
    }
    true
}

pub fn watch(pid: i32, tty_hint: Option<&str>, idle_override: Option<u64>, daemon: bool) -> i32 {
    let Some(tty) = tty_of(pid, tty_hint) else {
        eprintln!("dogfight watch: pid {pid} has no terminal (pass --tty \"$(tty)\")");
        return 1;
    };
    if daemon && !daemonize() {
        return 1;
    }
    // one watcher per shell: replace any older one
    let pidfile = runtime_dir().join(format!("watch-{pid}"));
    if let Ok(old) = fs::read_to_string(&pidfile).map(|s| s.trim().parse::<i32>().unwrap_or(0)) {
        if old > 0 && old != std::process::id() as i32 && is_watcher(old) {
            // SAFETY: sending a signal has no memory preconditions; `old` was verified to be our own watcher.
            unsafe { libc::kill(old, libc::SIGTERM) };
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
                let at_prompt = st.tpgid == st.pgrp && st.sleeping;
                if at_prompt {
                    let since = now().saturating_sub(atime(&tty).max(last_fire));
                    if since >= idle {
                        let _ = fs::write(marker(pid), now().to_string());
                        // SAFETY: sending a signal has no memory preconditions; `pid` is the shell that started us.
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

/// Stop every watcher belonging to this user (used by `uninstall`): the pidfiles in the
/// runtime dir, plus (Linux) a /proc sweep for watchers from another XDG_RUNTIME_DIR.
pub fn stop_watchers() -> usize {
    let me = std::process::id() as i32;
    let mut seen = std::collections::HashSet::new();
    let mut n = 0;
    if let Ok(rd) = fs::read_dir(runtime_dir()) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix("watch-") else { continue };
            let pid: i32 = fs::read_to_string(e.path()).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            if pid > 0 && pid != me && is_watcher(pid) && seen.insert(pid) {
                // SAFETY: sending a signal has no memory preconditions; `pid` was verified to be a watcher.
                unsafe { libc::kill(pid, libc::SIGTERM) };
                n += 1;
            }
            let _ = rest;
            let _ = fs::remove_file(e.path());
        }
    }
    #[cfg(target_os = "linux")]
    {
        // SAFETY: getuid never fails and has no preconditions.
        let uid = unsafe { libc::getuid() };
        if let Ok(rd) = fs::read_dir("/proc") {
            for e in rd.flatten() {
                let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else { continue };
                if pid == me || seen.contains(&pid) {
                    continue;
                }
                use std::os::unix::fs::MetadataExt;
                if fs::metadata(e.path()).map(|m| m.uid() != uid).unwrap_or(true) {
                    continue;
                }
                let cmd = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
                let parts: Vec<&[u8]> = cmd.split(|&b| b == 0).collect();
                let is_rev = parts.first().map(|p| p.ends_with(b"dogfight")).unwrap_or(false);
                if is_rev && parts.get(1) == Some(&&b"watch"[..]) {
                    // SAFETY: sending a signal has no memory preconditions; the cmdline was checked to be `dogfight watch`.
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                    n += 1;
                }
            }
        }
    }
    n
}
