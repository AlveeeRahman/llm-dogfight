//! Canvas -> terminal bytes. This is where performance on VTE (GNOME Terminal/Ptyxis)
//! is won or lost: the terminal's parse+render cost scales with bytes, so we
//!
//! * only emit cells that changed since the last frame the terminal actually saw,
//! * treat colour drift below `tol` as "unchanged" (error stays bounded because we
//!   compare against what we last *emitted*, not against the previous frame),
//! * track the terminal's SGR + cursor state to skip redundant escapes,
//! * wrap each frame in synchronized-update mode (DEC 2026) to avoid tearing where
//!   supported (ignored harmlessly elsewhere).
use crate::canvas::Canvas;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
}

const BLANK: Cell = Cell { ch: '\u{1}', fg: [0; 3], bg: [0; 3] };

pub struct Encoder {
    prev: Vec<Cell>,
    cols: usize,
    rows: usize,
    pub tol: i32,
    pub truecolor: bool,
    full: bool,
    sfg: Option<[u8; 3]>,
    sbg: Option<[u8; 3]>,
}

impl Encoder {
    pub fn new(truecolor: bool, tol: i32) -> Self {
        Encoder { prev: vec![], cols: 0, rows: 0, tol, truecolor, full: true, sfg: None, sbg: None }
    }

    pub fn invalidate(&mut self) {
        self.full = true;
    }

    #[inline]
    fn build(cv: &Canvas, c: usize, r: usize) -> Cell {
        let i = r * cv.cols + c;
        let top = cv.px[(2 * r) * cv.w + c].to_u8();
        let bot = cv.px[(2 * r + 1) * cv.w + c].to_u8();
        let (g, gc) = cv.glyphs[i];
        if g != '\0' {
            return Cell { ch: g, fg: gc.to_u8(), bg: avg(top, bot) };
        }
        let d = cv.dots[i];
        if d != 0 {
            return Cell { ch: char::from_u32(0x2800 + d as u32).unwrap_or(' '), fg: cv.dot_col[i].to_u8(), bg: avg(top, bot) };
        }
        if close(top, bot, 1) {
            return Cell { ch: ' ', fg: top, bg: top };
        }
        Cell { ch: '▀', fg: top, bg: bot }
    }

    #[inline]
    fn same(&self, a: &Cell, b: &Cell) -> bool {
        if a.ch != b.ch {
            return false;
        }
        if !close(a.bg, b.bg, self.tol) {
            return false;
        }
        a.ch == ' ' || close(a.fg, b.fg, self.tol)
    }

    /// Encode `cv` into `out` (appends). Returns number of cells emitted.
    pub fn encode(&mut self, cv: &Canvas, out: &mut Vec<u8>) -> usize {
        if cv.cols != self.cols || cv.rows != self.rows {
            self.cols = cv.cols;
            self.rows = cv.rows;
            self.prev = vec![BLANK; cv.cols * cv.rows];
            self.full = true;
        }
        out.extend_from_slice(b"\x1b[?2026h");
        if self.full {
            out.extend_from_slice(b"\x1b[0m\x1b[H\x1b[2J");
            self.sfg = None;
            self.sbg = None;
        }
        let mut emitted = 0;
        let mut cur: Option<(usize, usize)> = None;
        for r in 0..self.rows {
            for c in 0..self.cols {
                let cell = Self::build(cv, c, r);
                let i = r * self.cols + c;
                if !self.full && self.same(&self.prev[i], &cell) {
                    continue;
                }
                // cursor
                match cur {
                    Some((cc, cr)) if cr == r && cc == c => {}
                    Some((cc, cr)) if cr == r && c > cc && c - cc < 4 => {
                        // cheaper to re-emit the skipped cells? only if SGR matches; just use CUF
                        out.extend_from_slice(b"\x1b[");
                        push_num(out, (c - cc) as u32);
                        out.push(b'C');
                    }
                    _ => {
                        out.extend_from_slice(b"\x1b[");
                        push_num(out, (r + 1) as u32);
                        out.push(b';');
                        push_num(out, (c + 1) as u32);
                        out.push(b'H');
                    }
                }
                // colours
                let need_fg = cell.ch != ' ' && self.sfg != Some(cell.fg);
                let need_bg = self.sbg != Some(cell.bg);
                if need_fg || need_bg {
                    out.extend_from_slice(b"\x1b[");
                    if need_fg {
                        self.push_color(out, 38, cell.fg);
                        self.sfg = Some(cell.fg);
                    }
                    if need_bg {
                        if need_fg {
                            out.push(b';');
                        }
                        self.push_color(out, 48, cell.bg);
                        self.sbg = Some(cell.bg);
                    }
                    out.push(b'm');
                }
                let mut buf = [0u8; 4];
                out.extend_from_slice(cell.ch.encode_utf8(&mut buf).as_bytes());
                self.prev[i] = cell;
                emitted += 1;
                cur = if c + 1 < self.cols { Some((c + 1, r)) } else { None };
            }
        }
        out.extend_from_slice(b"\x1b[?2026l");
        self.full = false;
        emitted
    }

    fn push_color(&self, out: &mut Vec<u8>, base: u32, c: [u8; 3]) {
        push_num(out, base);
        if self.truecolor {
            out.extend_from_slice(b";2;");
            push_num(out, c[0] as u32);
            out.push(b';');
            push_num(out, c[1] as u32);
            out.push(b';');
            push_num(out, c[2] as u32);
        } else {
            out.extend_from_slice(b";5;");
            push_num(out, to_256(c) as u32);
        }
    }
}

#[inline]
fn avg(a: [u8; 3], b: [u8; 3]) -> [u8; 3] {
    [((a[0] as u16 + b[0] as u16) / 2) as u8, ((a[1] as u16 + b[1] as u16) / 2) as u8, ((a[2] as u16 + b[2] as u16) / 2) as u8]
}
#[inline]
fn close(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
    (a[0] as i32 - b[0] as i32).abs() <= tol && (a[1] as i32 - b[1] as i32).abs() <= tol && (a[2] as i32 - b[2] as i32).abs() <= tol
}

#[inline]
fn push_num(out: &mut Vec<u8>, v: u32) {
    if v < 10 {
        out.push(b'0' + v as u8);
    } else if v < 100 {
        out.push(b'0' + (v / 10) as u8);
        out.push(b'0' + (v % 10) as u8);
    } else if v < 1000 {
        out.push(b'0' + (v / 100) as u8);
        out.push(b'0' + ((v / 10) % 10) as u8);
        out.push(b'0' + (v % 10) as u8);
    } else {
        out.extend_from_slice(v.to_string().as_bytes());
    }
}

/// xterm 256-colour: 6x6x6 cube + 24 greys, whichever is closer.
pub fn to_256(c: [u8; 3]) -> u8 {
    const LV: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let idx = |v: u8| -> usize {
        let v = v as i32;
        let mut best = 0;
        for (i, l) in LV.iter().enumerate() {
            if (v - l).abs() < (v - LV[best]).abs() {
                best = i;
            }
        }
        best
    };
    let (r, g, b) = (idx(c[0]), idx(c[1]), idx(c[2]));
    let cube = [LV[r], LV[g], LV[b]];
    let d_cube: i32 = (0..3).map(|k| (cube[k] - c[k] as i32).pow(2)).sum();
    let avg = (c[0] as i32 + c[1] as i32 + c[2] as i32) / 3;
    let gi = ((avg - 8).max(0) / 10).min(23);
    let gv = 8 + gi * 10;
    let d_grey: i32 = (0..3).map(|k| (gv - c[k] as i32).pow(2)).sum();
    if d_grey < d_cube { (232 + gi) as u8 } else { (16 + 36 * r + 6 * g + b) as u8 }
}
