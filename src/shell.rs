//! Shell integration. Human-controlled surface: rc files are only modified by the
//! explicit `reverie install` command, with a timestamped backup and a marked block
//! that `reverie uninstall` removes cleanly.
use std::fs;
use std::path::PathBuf;

const BEGIN: &str = "# >>> reverie >>>";
const END: &str = "# <<< reverie <<<";

pub fn snippet(shell: &str) -> Option<&'static str> {
    Some(match shell {
        "bash" => {
            r#"# reverie: terminal screensaver when the prompt sits idle
if [[ $- == *i* ]] && [ -t 0 ] && [ -z "${REVERIE_DISABLE-}" ] && command -v reverie >/dev/null 2>&1; then
  # SIGWINCH afterwards makes readline repaint the half-typed line (verified in a pty)
  __reverie_alrm() { command reverie run --idle-trigger "$$"; kill -WINCH $$ 2>/dev/null; }
  trap '__reverie_alrm' ALRM
  command reverie watch --pid "$$" --tty "$(tty 2>/dev/null)" --daemon
fi
"#
        }
        "zsh" => {
            r#"# reverie: terminal screensaver when the prompt sits idle
if [[ -o interactive ]] && [[ -t 0 ]] && [[ -z ${REVERIE_DISABLE-} ]] && (( $+commands[reverie] )); then
  (( $+functions[TRAPALRM] )) && functions[__reverie_prev_alrm]=$functions[TRAPALRM]
  TRAPALRM() {
    if [[ -e ${XDG_RUNTIME_DIR:-/tmp}/reverie-$UID/fire-$$ ]]; then
      command reverie run --idle-trigger $$
      zle && zle reset-prompt
    elif (( $+functions[__reverie_prev_alrm] )); then
      __reverie_prev_alrm
    fi
    return 0
  }
  command reverie watch --pid $$ --tty "$(tty 2>/dev/null)" --daemon
fi
"#
        }
        "fish" => {
            r#"# reverie: terminal screensaver when the prompt sits idle
if status is-interactive; and isatty stdin; and not set -q REVERIE_DISABLE; and command -q reverie
    function __reverie_alrm --on-signal SIGALRM
        command reverie run --idle-trigger $fish_pid
        commandline -f repaint
    end
    command reverie watch --pid $fish_pid --tty (tty 2>/dev/null) --daemon
end
"#
        }
        _ => return None,
    })
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn detect_shell() -> String {
    std::env::var("SHELL").ok().and_then(|s| s.rsplit('/').next().map(String::from)).unwrap_or_else(|| "bash".into())
}

fn rc_path(shell: &str) -> Option<PathBuf> {
    Some(match shell {
        "bash" => home().join(".bashrc"),
        "zsh" => std::env::var_os("ZDOTDIR").map(PathBuf::from).unwrap_or_else(home).join(".zshrc"),
        "fish" => std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config")).join("fish/conf.d/reverie.fish"),
        _ => return None,
    })
}

fn line_for(shell: &str) -> String {
    match shell {
        "fish" => "reverie init fish | source".to_string(),
        s => format!("eval \"$(reverie init {s})\""),
    }
}

pub fn install(shell: &str) -> Result<String, String> {
    let rc = rc_path(shell).ok_or_else(|| format!("unsupported shell '{shell}' (bash, zsh, fish)"))?;
    let existing = fs::read_to_string(&rc).unwrap_or_default();
    if existing.contains(BEGIN) {
        return Ok(format!("already installed in {}", rc.display()));
    }
    if let Some(dir) = rc.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut msg = String::new();
    if !existing.is_empty() {
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let bak = rc.with_file_name(format!("{}.reverie-backup-{ts}", rc.file_name().unwrap().to_string_lossy()));
        fs::copy(&rc, &bak).map_err(|e| format!("backup failed: {e}"))?;
        msg += &format!("backup: {}\n", bak.display());
    }
    let block = format!("{}{BEGIN}\n{}\n{END}\n", if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" }, line_for(shell));
    let mut new = existing;
    new.push_str(&block);
    fs::write(&rc, new).map_err(|e| e.to_string())?;
    msg += &format!("added reverie to {}\nopen a new terminal (or run: {}) to activate", rc.display(), line_for(shell));
    Ok(msg)
}

pub fn uninstall() -> String {
    let mut msg = String::new();
    for sh in ["bash", "zsh", "fish"] {
        let Some(rc) = rc_path(sh) else { continue };
        let Ok(text) = fs::read_to_string(&rc) else { continue };
        if !text.contains(BEGIN) {
            continue;
        }
        if sh == "fish" {
            let _ = fs::remove_file(&rc);
        } else {
            let mut out = String::new();
            let mut skipping = false;
            for line in text.lines() {
                if line.trim() == BEGIN {
                    skipping = true;
                    continue;
                }
                if line.trim() == END {
                    skipping = false;
                    continue;
                }
                if !skipping {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            let _ = fs::write(&rc, out);
        }
        msg += &format!("removed from {}\n", rc.display());
    }
    let n = crate::idle::stop_watchers();
    msg += &format!("stopped {n} watcher(s)");
    msg
}

pub fn installed() -> Vec<String> {
    ["bash", "zsh", "fish"].iter().filter(|s| rc_path(s).and_then(|p| fs::read_to_string(p).ok()).map(|t| t.contains(BEGIN)).unwrap_or(false)).map(|s| s.to_string()).collect()
}
