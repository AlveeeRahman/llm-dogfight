#!/usr/bin/env python3
"""LOCKED correctness harness.

Drives real interactive shells through a pseudo-terminal and checks the
[correctness] contract in eval/thresholds.toml. No partial credit.

  python3 eval/test_terminal.py [--record out.raw] [--shells bash,zsh,fish]
"""
import os, pty, sys, time, select, signal, struct, fcntl, termios, tempfile, argparse, json

COLS, ROWS = 110, 32
ENTER_ALT, LEAVE_ALT = b"\x1b[?1049h", b"\x1b[?1049l"
RESTORE = [b"\x1b[?25h", b"\x1b[?1049l", b"\x1b[0m", b"\x1b[?7h"]

class Shell:
    def __init__(self, shell, env, record=None):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            if shell == "bash":
                os.execvpe("bash", ["bash", "--rcfile", env["RCFILE"], "-i"], env)
            elif shell == "zsh":
                os.execvpe("zsh", ["zsh", "-i"], env)
            else:
                os.execvpe("fish", ["fish", "-i", "-C", f"source {env['RCFILE']}"], env)
        fcntl.ioctl(self.fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
        self.out = b""
        self.rec = open(record, "wb") if record else None
    def pump(self, secs, until=None):
        end = time.time() + secs
        while time.time() < end:
            r, _, _ = select.select([self.fd], [], [], 0.02)
            if r:
                try:
                    d = os.read(self.fd, 1 << 16)
                except OSError:
                    return False
                self.out += d
                if self.rec: self.rec.write(d)
                if until and until in self.out[-(len(until) + 65536):]:
                    return True
        return False
    def send(self, b): os.write(self.fd, b)
    def close(self):
        try: os.kill(self.pid, signal.SIGKILL)
        except OSError: pass
        if self.rec: self.rec.close()

def make_env(tmp, shell, idle):
    cfg = os.path.join(tmp, "cfg", "dogfight"); os.makedirs(cfg, exist_ok=True)
    open(os.path.join(cfg, "config.toml"), "w").write(
        f'idle_seconds = {idle}\nscenes = ["ufo"]\nrotate_minutes = 0.1\nfps = 30\n')
    env = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor", HOME=tmp,
               XDG_CONFIG_HOME=os.path.join(tmp, "cfg"), XDG_STATE_HOME=os.path.join(tmp, "state"),
               XDG_DATA_HOME=os.path.join(tmp, "data"), XDG_RUNTIME_DIR=os.path.join(tmp, "run"))
    os.makedirs(env["XDG_RUNTIME_DIR"], exist_ok=True)
    if shell == "bash":
        rc = os.path.join(tmp, "bashrc"); open(rc, "w").write("PS1='demo:~$ '\n" + os.popen("dogfight init bash").read())
        env["RCFILE"] = rc
    elif shell == "zsh":
        zd = os.path.join(tmp, "zdot"); os.makedirs(zd, exist_ok=True)
        open(os.path.join(zd, ".zshrc"), "w").write("PS1='demo:~$ '\n" + os.popen("dogfight init zsh").read())
        env["ZDOTDIR"] = zd
    else:
        rc = os.path.join(tmp, "rc.fish")
        open(rc, "w").write("function fish_prompt; echo -n 'demo:~$ '; end\n" + os.popen("dogfight init fish").read())
        env["RCFILE"] = rc
    return env

results = {}
RSS = []
def check(name, ok, detail=""):
    results[name] = ok
    print(f"  {'PASS' if ok else 'FAIL'}  {name}  {detail}")

def test_run_and_restore(tmp):
    print("[run/restore]")
    env = make_env(tmp, "bash", 300)
    sh = Shell("bash", env); sh.pump(1.5, b"demo:~$ ")
    sh.send(b"stty -g > before.txt; dogfight run --scene ufo; echo RC_$?; stty -g > after.txt\r")
    started = sh.pump(3.0, ENTER_ALT)
    sh.pump(1.5)
    t0 = time.time(); sh.send(b"x")
    back = sh.pump(3.0, b"RC_0")
    dt_ms = (time.time() - t0) * 1000
    sh.pump(0.5)
    check("saver_starts", started)
    check("exit_after_key_ms<=300", back and dt_ms <= 300, f"{dt_ms:.0f} ms")
    tail = sh.out[sh.out.rfind(ENTER_ALT):]
    check("restore_screen", all(s in tail for s in RESTORE))
    a = open(os.path.join(tmp, "before.txt")).read() if os.path.exists(os.path.join(tmp, "before.txt")) else "?"
    b = open(os.path.join(tmp, "after.txt")).read() if os.path.exists(os.path.join(tmp, "after.txt")) else "!"
    check("restore_termios", a == b and a != "?")
    # SIGTERM path: dogfight in the FOREGROUND, SIGTERM from outside (as a logout/kill would).
    # (A backgrounded `dogfight &` is stopped by SIGTTOU when it touches the tty — that tested
    #  job control, not dogfight. Fixed 2026-09-10.)
    sh.send(b"dogfight run --scene ufo; echo TERM_DONE; stty -g > after2.txt\r")
    sh.pump(2.0, ENTER_ALT); sh.pump(1.0)
    for p in os.listdir("/proc"):
        if p.isdigit():
            try:
                if open(f"/proc/{p}/cmdline", "rb").read().startswith(b"dogfight\0run"):
                    os.kill(int(p), signal.SIGTERM)
            except OSError: pass
    sh.pump(4.0, b"TERM_DONE"); sh.pump(0.5)
    b2 = open(os.path.join(tmp, "after2.txt")).read() if os.path.exists(os.path.join(tmp, "after2.txt")) else "!"
    tail2 = sh.out[sh.out.rfind(ENTER_ALT):]
    check("restore_after_sigterm", b"TERM_DONE" in sh.out and b2 == a and all(x in tail2 for x in RESTORE))
    sh.close()

def test_idle(tmp, shell, record=None):
    print(f"[idle: {shell}]")
    idle = 6
    env = make_env(tmp, shell, idle)
    sh = Shell(shell, env, record)
    sh.pump(3.0 if shell == "fish" else 1.5, b"demo:~$ ")
    # silent while a command runs
    mark = len(sh.out)
    sh.send(b"sleep 16\r")
    sh.pump(16.5)
    check(f"{shell}:idle_silent_during_cmd", ENTER_ALT not in sh.out[mark:])
    # half-typed line, then walk away
    sh.send(b"echo PARTIAL_OK")
    t0 = time.time()
    fired = sh.pump(idle + 20, ENTER_ALT)
    check(f"{shell}:idle_fires_at_prompt", fired, f"after {time.time() - t0:.1f}s (idle={idle}s, tty atime has 8s granularity)")
    if record:
        sh.pump(34.0)  # let the demo play through several scene crossfades
    else:
        sh.pump(2.0)
    sh.send(b"q")
    sh.pump(1.0, LEAVE_ALT)
    sh.pump(0.5)
    sh.send(b"\r")
    sh.pump(1.5)
    tail = sh.out[sh.out.rfind(LEAVE_ALT):]
    import re
    plain = re.sub(rb"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07]*\x07|\x1b\(B", b"", tail).replace(b"\r", b"")
    check(f"{shell}:partial_line_survives", b"\nPARTIAL_OK" in plain or plain.strip().startswith(b"PARTIAL_OK") or b"PARTIAL_OK\n" in plain)
    check(f"{shell}:wake_key_not_leaked", b"PARTIAL_OKq" not in plain and b"qecho" not in plain)
    RSS.append(watcher_rss())
    sh.close()

def watcher_rss():
    best = 0
    for p in os.listdir("/proc"):
        if not p.isdigit(): continue
        try:
            cmd = open(f"/proc/{p}/cmdline", "rb").read().split(b"\0")
            if cmd[0].endswith(b"dogfight") and len(cmd) > 1 and cmd[1] == b"watch":
                for l in open(f"/proc/{p}/status"):
                    if l.startswith("VmRSS"): best = max(best, int(l.split()[1]) / 1024)
        except OSError: pass
    return best

if __name__ == "__main__":
    ap = argparse.ArgumentParser(); ap.add_argument("--record"); ap.add_argument("--shells", default="bash,zsh,fish")
    a = ap.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        os.chdir(tmp)
        test_run_and_restore(tmp)
    for i, sh in enumerate(a.shells.split(",")):
        with tempfile.TemporaryDirectory() as tmp:
            os.chdir(tmp)
            test_idle(tmp, sh, a.record if (a.record and i == 0) else None)
    rss = max(RSS) if RSS else 0
    check("watcher_rss_mb<=4", 0 < rss <= 4.0, f"{rss:.2f} MB")
    os.system("pkill -f 'dogfight watch' 2>/dev/null")
    ok = all(results.values())
    print(f"\n{sum(results.values())}/{len(results)} checks passed -> {'ALL PASS' if ok else 'FAILURES'}")
    json.dump(results, open("/tmp/dogfight_correctness.json", "w"))
    sys.exit(0 if ok else 1)
