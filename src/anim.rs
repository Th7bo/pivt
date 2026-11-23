use crate::theme::*;
use ratatui::prelude::*;

// Deterministic spatial hash so stars are stable across frames & resizes
// without storing them.
fn hash2(x: u16, y: u16) -> u32 {
    let mut h = (x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1e995);
    h ^= h >> 15;
    h
}

struct FieldCfg {
    density: u32,  // higher = sparser
    bright: bool,  // louder palette (splash)
    calm: bool,    // fewer twinklers, dimmer peaks (in-app)
    drift_div: i64, // >0 = starfield slowly scrolls down every `drift_div` ticks
    speed: u64,    // twinkle speed divisor
}

fn star_at(x: u16, y: u16, tick: u64, cfg: &FieldCfg) -> Option<(char, Color)> {
    // vertical drift: sample the field a bit lower each tick
    let yv = if cfg.drift_div > 0 {
        (y as i64 + tick as i64 / cfg.drift_div) as u16
    } else {
        y
    };
    let h = hash2(x, yv);
    if h % cfg.density != 0 {
        return None;
    }
    let phase = (h >> 5) & 0x1f;
    let twinkle_mask = if cfg.calm { 0b111 } else { 0b11 }; // calm: ~1/8 animate
    let twinkler = (h >> 3) & twinkle_mask == 0;
    let step = ((tick / cfg.speed) as u32 + phase) % 8;

    let (faint, mid, hi, pop) = if cfg.bright {
        (STAR_DIM, STAR_MID, STAR_HI, ACCENT)
    } else if cfg.calm {
        (STAR_FAINT, STAR_FAINT, STAR_DIM, STAR_MID)
    } else {
        (STAR_FAINT, STAR_DIM, STAR_MID, STAR_HI)
    };

    if !twinkler {
        let (g, c) = match h % 3 {
            0 => ('.', faint),
            1 => ('·', faint),
            _ => ('˙', mid),
        };
        return Some((g, c));
    }
    let (g, c) = match step {
        0 | 7 => ('·', faint),
        1 | 6 => ('.', mid),
        2 | 5 => ('+', mid),
        3 => ('*', hi),
        _ => (if h & 1 == 0 { '✦' } else { '✧' }, pop),
    };
    Some((g, c))
}

fn paint(f: &mut Frame, area: Rect, tick: u64, cfg: &FieldCfg) {
    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some((g, c)) = star_at(x, y, tick, cfg) {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(g).set_fg(c);
                }
            }
        }
    }
}

/// Calm, slowly-drifting starfield behind the live UI.
pub fn draw_field_app(f: &mut Frame, area: Rect, tick: u64) {
    paint(f, area, tick, &FieldCfg { density: 42, bright: false, calm: true, drift_div: 16, speed: 9 });
}

/// Loud, twinkly starfield for the splash.
pub fn draw_field_splash(f: &mut Frame, area: Rect, tick: u64) {
    paint(f, area, tick, &FieldCfg { density: 7, bright: true, calm: false, drift_div: 0, speed: 4 });
}

/// A shooting star that streaks across. `period`: ticks between streaks.
pub fn draw_shooting_star(f: &mut Frame, area: Rect, tick: u64, period: u64, bright: bool) {
    if area.width < 10 || area.height < 4 {
        return;
    }
    let t = tick % period;
    if t > 10 {
        return;
    }
    let seed = hash2(7, (tick / period) as u16 + 1);
    let y = area.top() + 1 + (seed % (area.height.saturating_sub(2)) as u32) as u16;
    let head = area.left() as i32 + (t as i32 * (area.width as i32)) / 11;
    let buf = f.buffer_mut();
    let trail = ['═', '━', '─', '·'];
    for (i, g) in trail.iter().enumerate() {
        let x = head - i as i32;
        if x < area.left() as i32 || x >= area.right() as i32 {
            continue;
        }
        let col = if i == 0 { STAR_HI } else { STAR_DIM };
        if let Some(cell) = buf.cell_mut((x as u16, y)) {
            cell.set_char(*g).set_fg(col);
        }
    }
    if head >= area.left() as i32 && head < area.right() as i32 {
        if let Some(cell) = buf.cell_mut((head as u16, y)) {
            cell.set_char('✦').set_fg(if bright { ACCENT } else { STAR_HI });
        }
    }
}

/// Pulsing accent for the selected-row gutter mark.
pub fn pulse(tick: u64) -> Color {
    // ping-pong 0..6..0 over 12 ticks, lerp ACCENT -> bright
    let p = (tick % 12) as i32;
    let d = (p - 6).unsigned_abs() as u16; // 0..6
    let t = 6 - d; // bright at mid
    let lerp = |a: u8, b: u8| (a as u16 + ((b as i16 - a as i16) * t as i16 / 6) as u16).min(255) as u8;
    Color::Rgb(lerp(0x2d, 0xd4), lerp(0xd4, 0xf5), lerp(0xbf, 0xff))
}

// Big block banner for the splash.
pub const BANNER: &[&str] = &[
    "██████╗ ██╗██╗   ██╗████████╗",
    "██╔══██╗██║██║   ██║╚══██╔══╝",
    "██████╔╝██║██║   ██║   ██║   ",
    "██╔═══╝ ██║╚██╗ ██╔╝   ██║   ",
    "██║     ██║ ╚████╔╝    ██║   ",
    "╚═╝     ╚═╝  ╚═══╝     ╚═╝   ",
];
