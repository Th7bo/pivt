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

// ───────────────────────── flow field (noise ASCII) ─────────────────────────

const RAMP: &[u8] = b" .,:;-=+*x#%@";

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn lat(ix: i32, iy: i32) -> f32 {
    let h = hash2((ix & 0xffff) as u16, (iy & 0xffff) as u16);
    (h & 0xffff) as f32 / 65535.0
}
/// 2D value noise in 0..1.
fn vnoise(x: f32, y: f32) -> f32 {
    let x0 = x.floor();
    let y0 = y.floor();
    let xf = smooth(x - x0);
    let yf = smooth(y - y0);
    let (ix, iy) = (x0 as i32, y0 as i32);
    let v00 = lat(ix, iy);
    let v10 = lat(ix + 1, iy);
    let v01 = lat(ix, iy + 1);
    let v11 = lat(ix + 1, iy + 1);
    lerp(lerp(v00, v10, xf), lerp(v01, v11, xf), yf)
}
/// Fractal brownian motion (3 octaves).
fn fbm(x: f32, y: f32) -> f32 {
    let mut v = 0.0;
    let mut amp = 0.5;
    let mut fx = x;
    let mut fy = y;
    for _ in 0..3 {
        v += amp * vnoise(fx, fy);
        fx *= 2.0;
        fy *= 2.0;
        amp *= 0.5;
    }
    v
}

struct FlowCfg {
    peak: f32,    // brightness ceiling (0..1) — subtle vs bold
    gain: f32,    // contrast
    cutoff: f32,  // below this the cell stays blank (higher = sparser)
    colors: [Color; 4], // ramp colors dim→bright
}

/// Flowing ASCII noise field. `mouse` (cell coords) adds a ripple.
fn flow(f: &mut Frame, area: Rect, tick: u64, mouse: Option<(u16, u16)>, cfg: &FlowCfg) {
    let t = tick as f32 * 0.05;
    // char cells are ~2:1 tall, compensate so the flow looks round
    let aspect = 0.5f32;
    let (mcx, mcy) = match mouse {
        Some((mx, my)) => (mx as f32, my as f32),
        None => (f32::NAN, f32::NAN),
    };
    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let fx = (x - area.left()) as f32;
            let fy = (y - area.top()) as f32;
            // scrolling fbm + a slow transverse wave
            let mut v = fbm(fx * 0.09 + t * 0.6, fy * 0.16 - t * 0.25);
            v += 0.12 * (fx * 0.10 + t).sin() * (fy * 0.18 - t * 0.5).cos();
            // mouse ripple
            if !mcx.is_nan() {
                let dx = fx - mcx;
                let dy = (fy - mcy) / aspect;
                let d = (dx * dx + dy * dy).sqrt();
                v += (d * 0.45 - t * 7.0).sin() * (-d * 0.09).exp() * 0.5;
            }
            v = ((v - 0.35) * cfg.gain).clamp(0.0, 1.0) * cfg.peak;
            if v < cfg.cutoff {
                continue; // leave empty cells blank
            }
            let ri = ((v * (RAMP.len() as f32 - 1.0)) as usize).clamp(1, RAMP.len() - 1);
            let glyph = RAMP[ri] as char;
            let ci = (v * 3.999) as usize;
            let color = cfg.colors[ci.min(3)];
            if let Some(cell) = buf.cell_mut((x, y)) {
                cell.set_char(glyph).set_fg(color);
            }
        }
    }
}

/// Subtle flow behind the live UI.
pub fn draw_flow_app(f: &mut Frame, area: Rect, tick: u64, mouse: Option<(u16, u16)>) {
    flow(f, area, tick, mouse, &FlowCfg {
        peak: 0.5,
        gain: 1.5,
        cutoff: 0.2,
        colors: [STAR_FAINT, STAR_DIM, STAR_DIM, STAR_MID],
    });
}

/// Bold flow for the splash.
pub fn draw_flow_splash(f: &mut Frame, area: Rect, tick: u64, mouse: Option<(u16, u16)>) {
    flow(f, area, tick, mouse, &FlowCfg {
        peak: 1.0,
        gain: 1.8,
        cutoff: 0.05,
        colors: [STAR_DIM, STAR_MID, STAR_HI, ACCENT],
    });
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
