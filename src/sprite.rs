//! `.rvs` sprite: tiny binary format for imported characters.
//!   "RVS1" u16 w, u16 h, u16 frames, u16 flags, i16 eye1x, eye1y, eye2x, eye2y,
//!   then per frame: u16 delay_ms + w*h RGBA bytes.
use crate::config::data_dir;
use std::path::PathBuf;

pub struct Sprite {
    pub w: usize,
    pub h: usize,
    pub frames: Vec<Vec<[u8; 4]>>,
    pub delays: Vec<u16>,
    pub eyes: Option<[i16; 4]>,
}

pub fn sprite_dir() -> PathBuf {
    data_dir().join("sprites")
}

pub fn sprite_path(name: &str) -> PathBuf {
    let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    sprite_dir().join(format!("{safe}.rvs"))
}

impl Sprite {
    pub fn load(name: &str) -> Option<Sprite> {
        let b = std::fs::read(sprite_path(name)).ok()?;
        if b.len() < 20 || &b[0..4] != b"RVS1" {
            return None;
        }
        let u16at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let i16at = |i: usize| i16::from_le_bytes([b[i], b[i + 1]]);
        let (w, h, n) = (u16at(4) as usize, u16at(6) as usize, u16at(8) as usize);
        let eyes = [i16at(12), i16at(14), i16at(16), i16at(18)];
        let eyes = if eyes[0] >= 0 { Some(eyes) } else { None };
        let mut off = 20;
        let (mut frames, mut delays) = (vec![], vec![]);
        for _ in 0..n {
            if off + 2 + w * h * 4 > b.len() {
                return None;
            }
            delays.push(u16at(off));
            off += 2;
            let px = b[off..off + w * h * 4].chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
            frames.push(px);
            off += w * h * 4;
        }
        if frames.is_empty() {
            return None;
        }
        Some(Sprite { w, h, frames, delays, eyes })
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut b = Vec::with_capacity(20 + self.frames.len() * (2 + self.w * self.h * 4));
        b.extend_from_slice(b"RVS1");
        for v in [self.w as u16, self.h as u16, self.frames.len() as u16, 0u16] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let e = self.eyes.unwrap_or([-1; 4]);
        for v in e {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for (f, d) in self.frames.iter().zip(&self.delays) {
            b.extend_from_slice(&d.to_le_bytes());
            for p in f {
                b.extend_from_slice(p);
            }
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, b)
    }

    pub fn list() -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(sprite_dir())
            .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".rvs")).map(String::from)).collect())
            .unwrap_or_default();
        v.sort();
        v
    }
}
