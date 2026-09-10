//! Raw terminal handling with a hard guarantee: whatever happens (key, SIGTERM,
//! SIGHUP, panic) the user's terminal is restored exactly — termios, cursor,
//! alt-screen, colours, autowrap, focus reporting.
use std::io;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

pub static RESIZED: AtomicBool = AtomicBool::new(false);
pub static QUIT: AtomicBool = AtomicBool::new(false);
pub static CONT: AtomicBool = AtomicBool::new(false);
static TTY_FD: AtomicI32 = AtomicI32::new(-1);
static SAVED: Mutex<Option<libc::termios>> = Mutex::new(None);

const ENTER: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[?7l\x1b[?1004h\x1b[0m\x1b[2J";
const LEAVE: &[u8] = b"\x1b[?2026l\x1b[?1004l\x1b[0m\x1b[?7h\x1b[?25h\x1b[?1049l";

pub enum Input {
    None,
    Key,
    FocusIn,
    FocusOut,
}

pub struct Term {
    fd: i32,
    active: bool,
}

extern "C" fn on_signal(sig: libc::c_int) {
    match sig {
        libc::SIGWINCH => RESIZED.store(true, Ordering::SeqCst),
        libc::SIGCONT => CONT.store(true, Ordering::SeqCst),
        _ => QUIT.store(true, Ordering::SeqCst),
    }
}

fn install_handlers() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0; // no SA_RESTART: we want poll() to wake up
        for s in [libc::SIGWINCH, libc::SIGCONT, libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGQUIT] {
            libc::sigaction(s, &sa, std::ptr::null_mut());
        }
        // Writes to a closed terminal must not kill us before we restore.
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
}

/// Restore from static state. Safe to call from a panic hook; idempotent.
pub fn emergency_restore() {
    let fd = TTY_FD.swap(-1, Ordering::SeqCst);
    if fd < 0 {
        return;
    }
    raw_write(fd, LEAVE);
    if let Ok(g) = SAVED.lock() {
        if let Some(t) = g.as_ref() {
            unsafe {
                libc::tcsetattr(fd, libc::TCSANOW, t);
            }
        }
    }
}

fn raw_write(fd: i32, mut buf: &[u8]) -> bool {
    while !buf.is_empty() {
        let n = unsafe { libc::write(fd, buf.as_ptr() as *const libc::c_void, buf.len()) };
        if n < 0 {
            let e = io::Error::last_os_error();
            match e.raw_os_error() {
                Some(libc::EINTR) => continue,
                Some(libc::EAGAIN) => {
                    let mut p = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
                    unsafe { libc::poll(&mut p, 1, 50) };
                    continue;
                }
                _ => return false,
            }
        }
        buf = &buf[n as usize..];
    }
    true
}

impl Term {
    pub fn open() -> io::Result<Term> {
        let path = b"/dev/tty\0";
        let fd = unsafe { libc::open(path.as_ptr() as *const libc::c_char, libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOCTTY) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Term { fd, active: false })
    }

    pub fn enter(&mut self) -> io::Result<()> {
        let mut t: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(self.fd, &mut t) } != 0 {
            return Err(io::Error::last_os_error());
        }
        *SAVED.lock().unwrap() = Some(t);
        TTY_FD.store(self.fd, Ordering::SeqCst);
        install_handlers();
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            emergency_restore();
            prev_hook(info);
        }));
        self.set_raw(&t);
        raw_write(self.fd, ENTER);
        self.active = true;
        Ok(())
    }

    fn set_raw(&self, orig: &libc::termios) {
        let mut t = *orig;
        t.c_iflag &= !(libc::IGNBRK | libc::BRKINT | libc::PARMRK | libc::ISTRIP | libc::INLCR | libc::IGNCR | libc::ICRNL | libc::IXON);
        t.c_oflag &= !libc::OPOST;
        t.c_lflag &= !(libc::ECHO | libc::ECHONL | libc::ICANON | libc::ISIG | libc::IEXTEN);
        t.c_cflag &= !(libc::CSIZE | libc::PARENB);
        t.c_cflag |= libc::CS8;
        t.c_cc[libc::VMIN] = 0;
        t.c_cc[libc::VTIME] = 0;
        unsafe {
            libc::tcsetattr(self.fd, libc::TCSANOW, &t);
        }
    }

    /// After SIGCONT (e.g. resumed from a stop) re-apply raw mode + alt screen.
    pub fn reassert(&self) {
        if let Some(t) = *SAVED.lock().unwrap() {
            self.set_raw(&t);
        }
        raw_write(self.fd, ENTER);
    }

    pub fn size(&self) -> (usize, usize) {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(self.fd, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            (ws.ws_col as usize, ws.ws_row as usize)
        } else {
            (80, 24)
        }
    }

    pub fn write_all(&self, buf: &[u8]) -> bool {
        raw_write(self.fd, buf)
    }

    /// Wait up to `timeout_ms` for input. Focus reports are not "keys".
    pub fn poll_input(&self, timeout_ms: i32) -> Input {
        let mut p = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
        let r = unsafe { libc::poll(&mut p, 1, timeout_ms.max(0)) };
        if r <= 0 {
            return Input::None;
        }
        if p.revents & (libc::POLLHUP | libc::POLLERR) != 0 {
            QUIT.store(true, Ordering::SeqCst);
            return Input::None;
        }
        let mut buf = [0u8; 512];
        let n = unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n <= 0 {
            return Input::None;
        }
        let bytes = &buf[..n as usize];
        let mut rest = bytes;
        let mut focus = Input::None;
        while !rest.is_empty() {
            if rest.starts_with(b"\x1b[I") {
                focus = Input::FocusIn;
                rest = &rest[3..];
            } else if rest.starts_with(b"\x1b[O") {
                focus = Input::FocusOut;
                rest = &rest[3..];
            } else {
                return Input::Key;
            }
        }
        focus
    }

    pub fn leave(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        // Swallow the rest of a multi-byte key (arrows, F-keys) so nothing leaks to the shell.
        unsafe {
            libc::usleep(15_000);
            libc::tcflush(self.fd, libc::TCIFLUSH);
        }
        emergency_restore();
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        self.leave();
        unsafe {
            libc::close(self.fd);
        }
    }
}
