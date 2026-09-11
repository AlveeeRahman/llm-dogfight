#!/usr/bin/env python3
"""LOCKED correctness harness.

Drives a real interactive bash through a pseudo-terminal and checks the [correctness] contract
in eval/thresholds.toml: the battle starts, any key returns the prompt within 300 ms, the screen
and termios are restored, and SIGTERM restores them too. No partial credit.

  python3 eval/test_terminal.py [--record out.raw]
"""
import os, pty, sys, time, select, signal, struct, fcntl, termios, tempfile, argparse, json

COLS, ROWS = 110, 32
ENTER_ALT, LEAVE_ALT = b"\x1b[?1049h", b"\x1b[?1049l"
RESTORE = [b"\x1b[?25h", b"\x1b[?1049l", b"\x1b[0m", b"\x1b[?7h"]

class Shell:
    def __init__(self, shell, env, record=None):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            os.execvpe("bash", ["bash", "--rcfile", env["RCFILE"], "-i"], env)
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
    def children(self, name=b"dogfight"):
        """PIDs of this shell's own child processes whose command is `name`. Only processes we
        started through this pty are ever signalled; nothing else on the machine is touched."""
        pids = []
        try:
            kids = open(f"/proc/{self.pid}/task/{self.pid}/children").read().split()
        except OSError:  # kernel without CONFIG_PROC_CHILDREN: fall back to the parent-pid field
            kids = []
            for p in os.listdir("/proc"):
                if p.isdigit():
                    try:
                        if open(f"/proc/{p}/stat").read().rsplit(")", 1)[1].split()[1] == str(self.pid): kids.append(p)
                    except OSError: pass
        for p in kids:
            try:
                if open(f"/proc/{p}/cmdline", "rb").read().split(b"\0", 1)[0] == name: pids.append(int(p))
            except OSError: pass
        return pids
    def close(self):
        # terminate what we started (the shell's dogfight children first, then the shell), then
        # reap, so a failing check never leaves a saver holding the pty
        for p in self.children():
            try: os.kill(p, signal.SIGTERM)
            except OSError: pass
        try: os.kill(self.pid, signal.SIGKILL)
        except OSError: pass
        try: os.waitpid(self.pid, 0)
        except OSError: pass
        try: os.close(self.fd)
        except OSError: pass
        if self.rec: self.rec.close()

def make_env(tmp):
    cfg = os.path.join(tmp, "cfg", "dogfight"); os.makedirs(cfg, exist_ok=True)
    open(os.path.join(cfg, "config.toml"), "w").write("fps = 30\n")
    env = dict(os.environ, TERM="xterm-256color", COLORTERM="truecolor", HOME=tmp,
               XDG_CONFIG_HOME=os.path.join(tmp, "cfg"), XDG_STATE_HOME=os.path.join(tmp, "state"),
               XDG_DATA_HOME=os.path.join(tmp, "data"), XDG_RUNTIME_DIR=os.path.join(tmp, "run"))
    os.makedirs(env["XDG_RUNTIME_DIR"], exist_ok=True)
    rc = os.path.join(tmp, "bashrc"); open(rc, "w").write("PS1='demo:~$ '\n")
    env["RCFILE"] = rc
    return env

results = {}
def check(name, ok, detail=""):
    results[name] = ok
    print(f"  {'PASS' if ok else 'FAIL'}  {name}  {detail}")

def test_run_and_restore(tmp):
    print("[run/restore]")
    env = make_env(tmp)
    sh = Shell("bash", env)
    try:
        _run_and_restore(sh, tmp)
    finally:
        sh.close()

def _run_and_restore(sh, tmp):
    sh.pump(1.5, b"demo:~$ ")
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
    targets = sh.children()          # the dogfight we just started, and nothing else
    check("sigterm_target_is_our_child", len(targets) == 1, f"pids {targets}")
    for p in targets:
        os.kill(p, signal.SIGTERM)
    sh.pump(4.0, b"TERM_DONE"); sh.pump(0.5)
    b2 = open(os.path.join(tmp, "after2.txt")).read() if os.path.exists(os.path.join(tmp, "after2.txt")) else "!"
    tail2 = sh.out[sh.out.rfind(ENTER_ALT):]
    check("restore_after_sigterm", b"TERM_DONE" in sh.out and b2 == a and all(x in tail2 for x in RESTORE))

if __name__ == "__main__":
    ap = argparse.ArgumentParser(); ap.add_argument("--record")
    a = ap.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        os.chdir(tmp)
        test_run_and_restore(tmp)
    ok = all(results.values())
    print(f"\n{sum(results.values())}/{len(results)} checks passed -> {'ALL PASS' if ok else 'FAILURES'}")
    sys.exit(0 if ok else 1)
