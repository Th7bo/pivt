//! Background effects and motion: starfield, flow field, matrix rain, snow,
//! plasma, shooting stars, the selection pulse, the flag burst and the
//! splash banner.
//!
//! Every effect is a pure function of a millisecond clock and the resolved
//! [`Fx`] settings (built from `[look]` by [`Fx::from_prefs`]), so a frozen
//! clock gives identical frames. Effects paint straight into the buffer and
//! leave cells they don't use alone (widgets drawn later cover the rest).
//! Per-frame cost: palettes are resolved once per frame, per-row terms once
//! per row, and nothing allocates per cell.

use crate::prefs::{
    self, Background, BurstStyle, Drift, FlowColor, MatrixCharset, Prefs, Reveal, ShootDir, SplashBanner, SplashBg,
    SplashFont, StarColor,
};
use crate::theme::theme;
use ratatui::prelude::*;

/// Deterministic spatial hash, so effects are stable across frames and
/// resizes without storing anything.
fn hash3(x: u32, y: u32, z: u32) -> u32 {
    let mut h = x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663) ^ z.wrapping_mul(83492791);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1e995);
    h ^= h >> 15;
    h
}
fn hash2(x: u16, y: u16) -> u32 {
    hash3(x as u32, y as u32, 0)
}

/// A background effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bg {
    Stars,
    Flow,
    Matrix,
    Snow,
    Plasma,
}

impl Bg {
    #[cfg(test)]
    pub const ALL: [Bg; 5] = [Bg::Stars, Bg::Flow, Bg::Matrix, Bg::Snow, Bg::Plasma];

    /// The in-app background (`None` = off).
    pub fn of(b: Background) -> Option<Bg> {
        match b {
            Background::Stars => Some(Bg::Stars),
            Background::Flow => Some(Bg::Flow),
            Background::Matrix => Some(Bg::Matrix),
            Background::Snow => Some(Bg::Snow),
            Background::Plasma => Some(Bg::Plasma),
            Background::Off => None,
        }
    }

    /// The splash background: `splash_bg`, or `background` for `same`.
    pub fn of_splash(s: SplashBg, b: Background) -> Option<Bg> {
        match s {
            SplashBg::Same => Bg::of(b),
            SplashBg::Stars => Some(Bg::Stars),
            SplashBg::Flow => Some(Bg::Flow),
            SplashBg::Matrix => Some(Bg::Matrix),
            SplashBg::Snow => Some(Bg::Snow),
            SplashBg::Plasma => Some(Bg::Plasma),
            SplashBg::Off => None,
        }
    }

    #[cfg(test)]
    pub fn name(self) -> &'static str {
        match self {
            Bg::Stars => "stars",
            Bg::Flow => "flow",
            Bg::Matrix => "matrix",
            Bg::Snow => "snow",
            Bg::Plasma => "plasma",
        }
    }
}

/// One banner glyph and its display width.
pub type Glyph = (char, u8);

/// Animation settings resolved from `[look]` (see [`crate::prefs`]): ranges
/// turned into speeds and factors, glyph sets into char lists, the splash
/// banner loaded. Rebuilt whenever a preference changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Fx {
    /// Master switch (off = a still picture).
    pub animate: bool,
    // starfield
    /// 1 (sparse) ..= 10 (dense); 5 is the original look.
    pub density: u16,
    /// Star glyphs dim → bright; `None` = the classic set.
    pub stars: Option<Vec<char>>,
    pub twinkle: u16,
    pub twinklers: u16,
    pub drift: Drift,
    pub drift_speed: u16,
    pub brightness: u16,
    pub parallax: bool,
    pub star_color: StarColor,
    // shooting stars
    pub shoot: bool,
    pub shoot_every_ms: u64,
    pub shoot_dir: ShootDir,
    pub shoot_len: u16,
    pub shoot_speed: u16,
    pub trail: Vec<char>,
    pub shoot_color: String,
    // flow field
    /// Flow ramp, index 0 = blank.
    pub ramp: Vec<char>,
    pub flow_zoom: f32,
    pub flow_speed: f32,
    /// Noise offset per second of flow time (the drift heading).
    pub flow_dir: (f32, f32),
    pub flow_octaves: u8,
    pub flow_wave: f32,
    pub flow_gain: f32,
    pub flow_cut: f32,
    pub flow_color: FlowColor,
    pub ripple_amp: f32,
    pub ripple_decay: f32,
    pub ripple_speed: f32,
    // matrix / snow / plasma
    pub matrix_glyphs: Vec<char>,
    pub matrix_speed: u16,
    pub matrix_density: u16,
    pub snow_density: u16,
    pub snow_speed: u16,
    pub snow_wind: i16,
    pub plasma_ramp: Vec<char>,
    pub plasma_zoom: f32,
    pub plasma_speed: f32,
    pub plasma_density: u16,
    // selection pulse
    pub pulse: bool,
    pub pulse_ms: u64,
    pub pulse_from: String,
    pub pulse_to: String,
    // flag burst
    pub burst: bool,
    pub burst_style: BurstStyle,
    pub burst_ms: u64,
    pub burst_flash: bool,
    // splash
    /// Banner rows (glyph + width), clipped to [`ART_MAX_ROWS`] × [`ART_MAX_COLS`].
    pub banner: Vec<Vec<Glyph>>,
    /// Widest banner row, in cells.
    pub banner_w: u16,
    pub subtitle: String,
    pub reveal: Reveal,
    /// Reveal duration (0 = instant).
    pub reveal_ms: u64,
}

impl Default for Fx {
    fn default() -> Self {
        Fx::from_prefs(&Prefs::default()).0
    }
}

/// `5` → 1.0, scaled linearly (speed knobs).
fn per5(v: u16) -> f32 {
    v as f32 / 5.0
}
/// `5` → 1.0, ×1.25 per step (zoom knobs; higher = bigger features).
fn zoom(v: u16) -> f32 {
    1.25f32.powi(5 - v.clamp(1, 10) as i32)
}

impl Fx {
    /// Resolve the prefs. The second value is a warning when a custom
    /// splash banner couldn't be used (the built-in one is shown instead).
    pub fn from_prefs(p: &Prefs) -> (Fx, Option<String>) {
        let (banner, warning) = banner_for(p);
        let banner_w = banner.iter().map(|r| r.iter().map(|g| g.1 as u16).sum::<u16>()).max().unwrap_or(0);
        let angle = (p.flow_angle as f32).to_radians();
        let reveal_ms = match p.splash_reveal {
            Reveal::Instant => 0,
            _ => 750 * 5 / p.splash_reveal_speed.clamp(1, 10) as u64,
        };
        let fx = Fx {
            animate: p.animations,
            density: p.bg_density,
            stars: p.star_set(),
            twinkle: p.star_twinkle.clamp(1, 10),
            twinklers: p.star_twinklers.min(100),
            drift: p.star_drift,
            drift_speed: p.star_speed.clamp(1, 10),
            brightness: p.star_brightness.clamp(1, 10),
            parallax: p.star_parallax,
            star_color: p.star_color,
            shoot: p.shoot,
            shoot_every_ms: p.shoot_every.max(1) as u64 * 1000,
            shoot_dir: p.shoot_angle,
            shoot_len: p.shoot_length.clamp(1, 40),
            shoot_speed: p.shoot_speed.clamp(1, 10),
            trail: p.trail(),
            shoot_color: p.shoot_color.clone(),
            ramp: p.flow_ramp(),
            flow_zoom: zoom(p.flow_scale),
            flow_speed: per5(p.flow_speed),
            flow_dir: (-0.65 * angle.cos(), 0.65 * angle.sin()),
            flow_octaves: p.flow_octaves.clamp(1, 5) as u8,
            flow_wave: p.flow_wave as f32 * 0.04,
            flow_gain: per5(p.flow_contrast.max(1)),
            flow_cut: (p.flow_cutoff as f32 - 5.0) * 0.03,
            flow_color: p.flow_color,
            ripple_amp: p.ripple_strength as f32 * 0.1,
            ripple_decay: 0.45 / p.ripple_radius.max(1) as f32,
            ripple_speed: per5(p.ripple_speed) * 7.0,
            matrix_glyphs: matrix_glyphs(p.matrix_charset).chars().collect(),
            matrix_speed: p.matrix_speed.clamp(1, 10),
            matrix_density: p.matrix_density.clamp(1, 10),
            snow_density: p.snow_density.clamp(1, 10),
            snow_speed: p.snow_speed.clamp(1, 10),
            snow_wind: p.snow_wind.clamp(-5, 5),
            plasma_ramp: p.plasma_ramp(),
            plasma_zoom: zoom(p.plasma_scale),
            plasma_speed: per5(p.plasma_speed),
            plasma_density: p.plasma_density.clamp(1, 10),
            pulse: p.pulse,
            pulse_ms: 600 * 5 / p.pulse_speed.clamp(1, 10) as u64,
            pulse_from: p.pulse_from.clone(),
            pulse_to: p.pulse_to.clone(),
            burst: p.burst,
            burst_style: p.burst_style,
            burst_ms: p.burst_ms.max(100) as u64,
            burst_flash: p.burst_flash,
            banner,
            banner_w,
            subtitle: p.splash_subtitle.clone(),
            reveal: p.splash_reveal,
            reveal_ms,
        };
        (fx, warning)
    }

    /// Star hash divisor: `base` at density 5, sparser below, denser above.
    fn star_div(&self, base: u32) -> u32 {
        (base * 5 / self.density.clamp(1, 10) as u32).max(2)
    }
    /// Flow cutoff shifted by density (lower = more cells drawn).
    fn cutoff(&self, base: f32) -> f32 {
        (base + (5.0 - self.density.clamp(1, 10) as f32) * 0.04).max(0.0)
    }
}

/// What a background frame needs besides [`Fx`].
#[derive(Clone, Copy, Debug)]
pub struct Scene {
    /// Background clock (ms; frozen at 0 with animations off).
    pub ms: u64,
    /// Mouse cell for the flow ripple.
    pub mouse: Option<(u16, u16)>,
    /// The selected category's hue (category-tinted colours).
    pub tint: Color,
    /// Splash: louder, denser, brighter.
    pub loud: bool,
}

/// Paint a background effect into `area`.
pub fn draw_background(buf: &mut Buffer, area: Rect, bg: Bg, sc: &Scene, fx: &Fx) {
    let area = area.intersection(buf.area);
    if area.is_empty() {
        return;
    }
    match bg {
        Bg::Stars => draw_stars(buf, area, sc, fx),
        Bg::Flow => draw_flow(buf, area, sc, fx),
        Bg::Matrix => draw_matrix(buf, area, sc, fx),
        Bg::Snow => draw_snow(buf, area, sc, fx),
        Bg::Plasma => draw_plasma(buf, area, sc, fx),
    }
}

#[inline]
fn put(buf: &mut Buffer, x: u16, y: u16, ch: char, fg: Color) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        cell.set_char(ch).set_fg(fg);
    }
}

fn rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0x40, 0x50, 0x60),
    }
}
/// Mix two colours, `t` 0 (a) ..= 1 (b).
pub fn blend(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let (ar, ag, ab) = rgb(a);
    let (br, bg, bb) = rgb(b);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
}

/// A theme role by name (falls back to `fallback`).
fn role(name: &str, fallback: Color) -> Color {
    theme().role(name).unwrap_or(fallback)
}

/// A point on the theme's category-hue ring, `pos` wraps at 1.0.
fn ring(cats: &[Color], pos: f32) -> Color {
    if cats.is_empty() {
        return theme().accent;
    }
    let n = cats.len();
    let p = pos.rem_euclid(1.0) * n as f32;
    let i = (p as usize).min(n - 1);
    blend(cats[i], cats[(i + 1) % n], p - i as f32)
}

// ───────────────────────────── starfield ─────────────────────────────

/// One depth layer of the starfield, resolved for this frame.
struct StarLayer {
    seed: u32,
    div: u32,
    ox: i64,
    oy: i64,
    pal: [Color; 4],
}

/// Shift a colour darker (toward the background) or brighter (toward the
/// brightest star colour): `level` 5 = unchanged.
fn brighten(c: Color, level: i32) -> Color {
    let t = theme();
    match level.cmp(&5) {
        std::cmp::Ordering::Less => blend(c, t.bg, (5 - level) as f32 * 0.15),
        std::cmp::Ordering::Greater => blend(c, t.star_hi, (level - 5) as f32 * 0.12),
        std::cmp::Ordering::Equal => c,
    }
}

fn star_palette(fx: &Fx, sc: &Scene, level: i32) -> [Color; 4] {
    let t = theme();
    let base = if sc.loud {
        [t.star_dim, t.star_mid, t.star_hi, t.accent]
    } else {
        [t.star_faint, t.star_faint, t.star_dim, t.star_mid]
    };
    let toward = match fx.star_color {
        StarColor::Theme => None,
        StarColor::Accent => Some(t.accent),
        StarColor::Category => Some(sc.tint),
    };
    let k = if sc.loud { 0.5 } else { 0.35 };
    base.map(|c| brighten(toward.map_or(c, |to| blend(c, to, k)), level))
}

#[inline]
fn star_at(x: u16, y: u16, l: &StarLayer, step: u64, pct: u32, set: Option<&[char]>) -> Option<(char, Color)> {
    let sx = (x as i64 + l.ox) as u16;
    let sy = (y as i64 + l.oy) as u16;
    let h = hash3(sx as u32, sy as u32, l.seed);
    if !h.is_multiple_of(l.div) {
        return None;
    }
    let phase = (h >> 5) & 0x1f;
    let twinkler = (h >> 3) % 100 < pct;
    let step = ((step as u32).wrapping_add(phase)) % 8;
    let [faint, mid, hi, pop] = l.pal;

    // user glyph set: levels dim → bright, the last one is the twinkle peak
    if let Some(g) = set.filter(|g| !g.is_empty()) {
        let n = g.len();
        let at = |k: usize| g[(k * (n - 1) + 2) / 4]; // k in 0..=4 → index
        if !twinkler {
            return Some(if h % 3 == 2 { (at(1), mid) } else { (g[0], faint) });
        }
        return Some(match step {
            0 | 7 => (at(0), faint),
            1 | 6 => (at(1), mid),
            2 | 5 => (at(2), mid),
            3 => (at(3), hi),
            _ => (at(4), pop),
        });
    }
    if !twinkler {
        return Some(match h % 3 {
            0 => ('.', faint),
            1 => ('·', faint),
            _ => ('˙', mid),
        });
    }
    Some(match step {
        0 | 7 => ('·', faint),
        1 | 6 => ('.', mid),
        2 | 5 => ('+', mid),
        3 => ('*', hi),
        _ => (if h & 1 == 0 { '✦' } else { '✧' }, pop),
    })
}

fn draw_stars(buf: &mut Buffer, area: Rect, sc: &Scene, fx: &Fx) {
    let div = fx.star_div(if sc.loud { 7 } else { 42 });
    let step_ms = ((if sc.loud { 200 } else { 450 }) * 5 / fx.twinkle.max(1) as u64).max(1);
    let pct = if sc.loud { (fx.twinklers as u32 * 2).min(100) } else { fx.twinklers as u32 };
    // drift: 1.25 rows/s at speed 5 (columns are half as tall: twice the cells)
    let (dx, dy): (i64, i64) = match (sc.loud, fx.drift) {
        (true, _) | (_, Drift::None) => (0, 0),
        (_, Drift::Up) => (0, 1),
        (_, Drift::Down) => (0, -1),
        (_, Drift::Left) => (2, 0),
        (_, Drift::Right) => (-2, 0),
    };
    let rows_per_s = 0.25 * fx.drift_speed as f64;
    let offset = |mult: f64| (sc.ms as f64 * rows_per_s * mult / 1000.0) as i64;
    let b = fx.brightness as i32;
    let mut layers: [Option<StarLayer>; 3] = [None, None, None];
    if fx.parallax {
        // far (dense, slow, dim) → near (sparse, fast, bright)
        for (i, (seed, dmul, speed, lvl)) in [(0u32, 3u32, 0.45, -2), (101, 6, 1.0, 0), (202, 12, 2.0, 2)].into_iter().enumerate() {
            let o = offset(speed);
            layers[i] = Some(StarLayer {
                seed,
                div: (div * dmul / 2).max(2),
                ox: dx * o,
                oy: dy * o,
                pal: star_palette(fx, sc, b + lvl),
            });
        }
    } else {
        let o = offset(1.0);
        layers[0] = Some(StarLayer { seed: 0, div, ox: dx * o, oy: dy * o, pal: star_palette(fx, sc, b) });
    }
    let step = sc.ms / step_ms;
    let set = fx.stars.as_deref();
    for l in layers.iter().flatten() {
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if let Some((g, c)) = star_at(x, y, l, step, pct, set) {
                    put(buf, x, y, g, c);
                }
            }
        }
    }
}

/// A shooting star streaking across (one every `shoot_every` seconds; the
/// splash: six times as often). `ms`: the animation clock.
pub fn draw_shooting_star(buf: &mut Buffer, area: Rect, ms: u64, fx: &Fx, loud: bool) {
    let area = area.intersection(buf.area);
    if area.width < 10 || area.height < 4 || fx.trail.is_empty() {
        return;
    }
    let period = if loud { (fx.shoot_every_ms / 6).max(800) } else { fx.shoot_every_ms.max(800) };
    let n = ms / period;
    let t = ms % period;
    let seed = hash2(7, (n as u16).wrapping_add(1));
    let dir = match fx.shoot_dir {
        ShootDir::Random => [
            ShootDir::Right,
            ShootDir::Left,
            ShootDir::DownRight,
            ShootDir::DownLeft,
            ShootDir::UpRight,
            ShootDir::UpLeft,
        ][(seed >> 9) as usize % 6],
        d => d,
    };
    // cells are ~2:1 tall, so a diagonal moves one row per two columns
    let (dx, dy): (f32, f32) = match dir {
        ShootDir::Right | ShootDir::Random => (1.0, 0.0),
        ShootDir::Left => (-1.0, 0.0),
        ShootDir::DownRight => (1.0, 0.5),
        ShootDir::DownLeft => (-1.0, 0.5),
        ShootDir::UpRight => (1.0, -0.5),
        ShootDir::UpLeft => (-1.0, -0.5),
        ShootDir::Down => (0.0, 0.5),
    };
    let (w, h) = (area.width as f32, area.height as f32);
    let len = fx.shoot_len as usize;
    // the original: across the screen in 0.55 s at speed 5
    let s = t as f32 * w / 550.0 * fx.shoot_speed as f32 / 5.0;
    let span = if dx == 0.0 { h * 2.0 } else { w };
    if s > span + len as f32 * 2.0 {
        return;
    }
    let r = (seed % 1000) as f32 / 1000.0;
    let (left, top) = (area.left() as f32, area.top() as f32);
    let (x0, y0) = if dx == 0.0 {
        (left + 1.0 + r * (w - 2.0), top)
    } else {
        let x0 = if dx > 0.0 { left } else { area.right() as f32 - 1.0 };
        let y0 = if dy == 0.0 {
            top + 1.0 + (seed % (area.height - 2) as u32) as f32
        } else if dy > 0.0 {
            top + r * h * 0.5
        } else {
            top + h * 0.5 + r * h * 0.5
        };
        (x0, y0)
    };
    let (hx, hy) = (x0 + dx * s, y0 + dy * s);
    let th = theme();
    let auto = fx.shoot_color == "auto";
    let head_c = if auto {
        if loud {
            th.accent
        } else {
            th.star_hi
        }
    } else {
        role(&fx.shoot_color, th.star_hi)
    };
    let n_glyphs = fx.trail.len();
    // tail first, so the head is painted last
    for i in (0..len).rev() {
        let px = (hx - dx * i as f32).round();
        let py = (hy - dy * i as f32).round();
        if px < left || py < top || px >= area.right() as f32 || py >= area.bottom() as f32 {
            continue;
        }
        let g = fx.trail[(i * n_glyphs / len.max(1)).min(n_glyphs - 1)];
        let c = if i == 0 {
            head_c
        } else if auto {
            th.star_dim
        } else {
            blend(head_c, th.star_dim, i as f32 / len as f32)
        };
        put(buf, px as u16, py as u16, g, c);
    }
}

// ───────────────────────── flow field (noise ASCII) ─────────────────────────

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
/// Fractal brownian motion, `octaves` layers, scaled to the range the
/// original 3-octave field had (0..0.875).
fn fbm(x: f32, y: f32, octaves: u8) -> f32 {
    let mut v = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let (mut fx, mut fy) = (x, y);
    for _ in 0..octaves.max(1) {
        v += amp * vnoise(fx, fy);
        norm += amp;
        fx *= 2.0;
        fy *= 2.0;
        amp *= 0.5;
    }
    v * (0.875 / norm)
}

fn draw_flow(buf: &mut Buffer, area: Rect, sc: &Scene, fx: &Fx) {
    let ramp: &[char] = if fx.ramp.len() >= 2 { &fx.ramp } else { &[' ', '.', '*'] };
    let secs = sc.ms as f32 * 0.001;
    let t = secs * fx.flow_speed;
    let tr = secs * fx.ripple_speed;
    let (ox, oy) = (fx.flow_dir.0 * t, fx.flow_dir.1 * t);
    let z = fx.flow_zoom;
    let (peak, gain, cutoff) = if sc.loud { (1.0, 1.8, fx.cutoff(0.05)) } else { (0.5, 1.5, fx.cutoff(0.2)) };
    let gain = gain * fx.flow_gain;
    let cutoff = (cutoff + fx.flow_cut).max(0.0);
    let th = theme();
    let mut colors = if sc.loud {
        [th.star_dim, th.star_mid, th.star_hi, th.accent]
    } else {
        [th.star_faint, th.star_dim, th.star_dim, th.star_mid]
    };
    if fx.flow_color == FlowColor::Category && !sc.loud {
        for (c, k) in colors.iter_mut().zip([0.18, 0.22, 0.30, 0.38]) {
            *c = blend(*c, sc.tint, k);
        }
    }
    let rainbow = (fx.flow_color == FlowColor::Rainbow).then(|| th.cat_colors());
    let rainbow_k = if sc.loud { 0.75 } else { 0.4 };
    // char cells are ~2:1 tall, compensate so the ripple looks round
    let aspect = 0.5f32;
    let mouse = sc.mouse.filter(|_| fx.ripple_amp > 0.0).map(|(x, y)| (x as f32, y as f32));
    let last = ramp.len() - 1;
    for y in area.top()..area.bottom() {
        let fy = (y - area.top()) as f32;
        let wave_y = (fy * 0.18 - t * 0.5).cos() * fx.flow_wave;
        for x in area.left()..area.right() {
            let fxx = (x - area.left()) as f32;
            // scrolling fbm + a slow transverse wave
            let mut v = fbm(fxx * 0.09 * z + ox, fy * 0.16 * z + oy, fx.flow_octaves);
            v += (fxx * 0.10 + t).sin() * wave_y;
            if let Some((mx, my)) = mouse {
                let dx = fxx - mx;
                let dy = (fy - my) / aspect;
                let d = (dx * dx + dy * dy).sqrt();
                v += (d * 0.45 - tr).sin() * (-d * fx.ripple_decay).exp() * fx.ripple_amp;
            }
            let v = ((v - 0.35) * gain).clamp(0.0, 1.0) * peak;
            if v < cutoff {
                continue; // leave empty cells blank
            }
            let glyph = ramp[((v * last as f32) as usize).clamp(1, last)];
            let mut color = colors[((v * 3.999) as usize).min(3)];
            if let Some(cats) = rainbow {
                color = blend(color, ring(cats, fxx * 0.012 + fy * 0.02 + t * 0.06), rainbow_k);
            }
            put(buf, x, y, glyph, color);
        }
    }
}

// ───────────────────────────── matrix rain ─────────────────────────────

fn matrix_glyphs(c: MatrixCharset) -> &'static str {
    match c {
        // half-width katakana (one cell each) + a few digits
        MatrixCharset::Katakana => "ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ012345789",
        MatrixCharset::Ascii => "abcdefghijklmnopqrstuvwxyz0123456789$+-*/=%\"'#&_(),.;:?!\\|{}<>[]^~",
        MatrixCharset::Binary => "01",
        MatrixCharset::Hex => "0123456789abcdef",
    }
}

fn draw_matrix(buf: &mut Buffer, area: Rect, sc: &Scene, fx: &Fx) {
    let g = &fx.matrix_glyphs;
    if g.is_empty() {
        return;
    }
    let th = theme();
    let (head_c, body_c, tail_c) = if sc.loud {
        (th.star_hi, th.ok, th.star_dim)
    } else {
        (blend(th.ok, th.star_mid, 0.5), blend(th.ok, th.star_dim, 0.55), th.star_faint)
    };
    let rows = area.height as u32;
    let share = fx.matrix_density as u32 * 9 + 5; // % of columns that rain
    let speed_k = fx.matrix_speed as f32 / 5.0 * if sc.loud { 1.6 } else { 1.0 };
    for cx in 0..area.width {
        let h = hash3(cx as u32, 0x6d61, 17);
        if h % 100 >= share {
            continue;
        }
        let speed = (3.0 + ((h >> 8) % 8) as f32) * speed_k; // rows per second
        let len = 4 + (h >> 12) % 10;
        let gap = rows / 3 + (h >> 16) % (rows * (11 - fx.matrix_density as u32) / 5 + 1);
        let period = rows + len + gap;
        let head = ((sc.ms as f32 * speed / 1000.0) as u32).wrapping_add(h >> 20) % period;
        let x = area.left() + cx;
        let top = head.saturating_sub(len - 1);
        for row in top..=head.min(rows.saturating_sub(1)) {
            let d = head - row; // 0 = the head
            let flick = hash3(cx as u32, row, 3) % 180;
            let gi = hash3(cx as u32, row, ((sc.ms + flick as u64) / 180) as u32 ^ h) as usize % g.len();
            let c = if d == 0 { head_c } else { blend(body_c, tail_c, d as f32 / len as f32) };
            put(buf, x, area.top() + row as u16, g[gi], c);
        }
    }
}

// ───────────────────────────── snow ─────────────────────────────

fn draw_snow(buf: &mut Buffer, area: Rect, sc: &Scene, fx: &Fx) {
    let th = theme();
    let (far, near) = if sc.loud {
        ([th.star_dim, th.star_mid], [th.star_mid, th.star_hi])
    } else {
        ([th.star_faint, th.star_faint], [th.star_dim, th.star_mid])
    };
    // (seed, base divisor, fall rows/s, wind factor, glyphs, colours)
    let layers = [(11u32, 40u32, 1.1f64, 0.5f64, ['·', '.'], far), (23, 90, 2.4, 1.0, ['*', '•'], near)];
    let fall = fx.snow_speed as f64 / 5.0;
    let wind = fx.snow_wind as f64 * 0.8;
    for (seed, base, speed, wk, glyphs, cols) in layers {
        let mut div = (base * 5 / fx.snow_density as u32).max(3);
        if sc.loud {
            div = (div / 3).max(3);
        }
        let oy = (sc.ms as f64 * speed * fall / 1000.0) as i64;
        let ox = (sc.ms as f64 * wind * wk * 2.0 / 1000.0) as i64;
        for y in area.top()..area.bottom() {
            let sy = y as i64 - oy;
            // each row sways a little, so flakes wobble as they fall
            let sway = ((sc.ms as f32 * 0.0011 + sy as f32 * 0.7 + seed as f32).sin() * 0.9).round() as i64;
            for x in area.left()..area.right() {
                let sx = x as i64 - ox - sway;
                let h = hash3(sx as u32, sy as u32, seed);
                if !h.is_multiple_of(div) {
                    continue;
                }
                let k = ((h >> 7) & 1) as usize;
                put(buf, x, y, glyphs[k], cols[k]);
            }
        }
    }
}

// ───────────────────────────── plasma ─────────────────────────────

fn draw_plasma(buf: &mut Buffer, area: Rect, sc: &Scene, fx: &Fx) {
    let ramp: &[char] = if fx.plasma_ramp.len() >= 2 { &fx.plasma_ramp } else { &[' ', '.', '*'] };
    let last = ramp.len() - 1;
    let t = sc.ms as f32 * 0.001 * fx.plasma_speed;
    let z = fx.plasma_zoom;
    let th = theme();
    // in-app: only the crests show, in the faint half of the ramp
    let (cut, peak, base, k0, k1) =
        if sc.loud { (0.25, 1.0, th.star_mid, 0.4, 0.6) } else { (0.68, 0.5, th.star_faint, 0.12, 0.25) };
    let cut = (cut + (5.0 - fx.plasma_density as f32) * 0.04).clamp(0.0, 0.95);
    let (cx, cy) = (area.width as f32 / 2.0, area.height as f32 / 2.0);
    for y in area.top()..area.bottom() {
        let fy = (y - area.top()) as f32;
        let row = (fy * 0.30 * z + t * 0.7).sin();
        let dy = (fy - cy) * 2.0;
        for x in area.left()..area.right() {
            let fxx = (x - area.left()) as f32;
            let dx = fxx - cx;
            let v = (fxx * 0.15 * z + t).sin()
                + row
                + ((fxx * 0.10 + fy * 0.20) * z + t * 1.3).sin()
                + ((dx * dx + dy * dy).sqrt() * 0.12 * z - t * 1.1).sin();
            let v = (v + 4.0) / 8.0;
            if v < cut {
                continue;
            }
            let lvl = (v - cut) / (1.0 - cut);
            let glyph = ramp[((lvl * peak * last as f32) as usize + 1).min(last)];
            let hue = (((fxx * 0.04 - fy * 0.03) * z + t * 0.4).sin() + 1.0) * 0.5;
            let col = blend(th.accent, th.accent2, hue);
            put(buf, x, y, glyph, blend(base, col, k0 + k1 * lvl));
        }
    }
}

// ───────────────────────────── pulse + burst ─────────────────────────────

/// Pulsing colour for the selected-row marker: ping-pong `from` → `to` →
/// `from` every `period_ms`.
pub fn pulse(ms: u64, period_ms: u64, from: Color, to: Color) -> Color {
    let period = period_ms.max(1);
    let p = (ms % period) as f32 / period as f32;
    blend(from, to, 1.0 - (2.0 * p - 1.0).abs())
}

/// The flag-capture animation, `progress` 0..1.
pub fn draw_burst(buf: &mut Buffer, area: Rect, progress: f32, style: BurstStyle, ms: u64) {
    let area = area.intersection(buf.area);
    if area.is_empty() {
        return;
    }
    let p = progress.clamp(0.0, 1.0);
    let cx = area.left() as f32 + area.width as f32 / 2.0;
    let cy = area.top() as f32 + area.height as f32 / 2.0;
    let maxr = (area.width.max(area.height)) as f32 * 0.7;
    let th = theme();
    let inside = |x: f32, y: f32| {
        x >= area.left() as f32 && y >= area.top() as f32 && x < area.right() as f32 && y < area.bottom() as f32
    };
    match style {
        BurstStyle::Ring => {
            let aspect = 0.5f32;
            let radius = p * maxr;
            let glyphs = ['·', '*', '✦', '✷', '#', '%'];
            let col = blend(th.ok, th.star_hi, 1.0 - p);
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    let dx = x as f32 - cx;
                    let dy = (y as f32 - cy) / aspect;
                    let d = (dx * dx + dy * dy).sqrt();
                    if (d - radius).abs() < 1.8 {
                        put(buf, x, y, glyphs[((d * 0.5) as usize) % glyphs.len()], col);
                    }
                }
            }
        }
        BurstStyle::Sparkle => {
            let glyphs = ['✦', '✧', '*', '+', '·'];
            for k in 0..90u32 {
                let h = hash3(k, 77, 5);
                // a few sparks blink out each frame
                if (ms / 80 + k as u64).is_multiple_of(5) {
                    continue;
                }
                let ang = (h % 6283) as f32 / 1000.0;
                let sp = 0.35 + ((h >> 13) % 650) as f32 / 1000.0;
                let r = p * maxr * sp;
                let (x, y) = (cx + ang.cos() * r, cy + ang.sin() * r * 0.5);
                if !inside(x, y) {
                    continue;
                }
                let g = glyphs[((p * 4.0) as usize + (h >> 20) as usize % 2).min(4)];
                let warm = if (h >> 4) & 1 == 0 { th.ok } else { th.gold };
                put(buf, x as u16, y as u16, g, blend(warm, th.star_dim, p * 0.8));
            }
        }
        BurstStyle::Confetti => {
            let glyphs = ['▪', '•', '◆', '*', '+', '▫'];
            let cats = th.cat_colors();
            let (w, h) = (area.width as f32, area.height as f32);
            for k in 0..110u32 {
                let hh = hash3(k, 91, 6);
                let x0 = area.left() as f32 + (hh % 1000) as f32 / 1000.0 * w;
                let y0 = area.top() as f32 - ((hh >> 10) % 1000) as f32 / 1000.0 * h * 0.6;
                let fall = p * h * 1.4 * (0.7 + ((hh >> 16) % 60) as f32 / 100.0);
                let sway = (p * 9.0 + k as f32).sin() * 2.5;
                let (x, y) = ((x0 + sway).round(), (y0 + fall).round());
                if !inside(x, y) {
                    continue;
                }
                let c = if cats.is_empty() { th.accent } else { cats[(hh >> 3) as usize % cats.len()] };
                put(buf, x as u16, y as u16, glyphs[(hh >> 5) as usize % glyphs.len()], c);
            }
        }
    }
}

// ───────────────────────────── splash banner ─────────────────────────────

/// Big block banner for the splash.
pub const BANNER: &[&str] = &[
    "██████╗ ██╗██╗   ██╗████████╗",
    "██╔══██╗██║██║   ██║╚══██╔══╝",
    "██████╔╝██║██║   ██║   ██║   ",
    "██╔═══╝ ██║╚██╗ ██╔╝   ██║   ",
    "██║     ██║ ╚████╔╝    ██║   ",
    "╚═╝     ╚═╝  ╚═══╝     ╚═╝   ",
];

/// Custom ASCII art is clipped to this many rows / columns when loaded.
pub const ART_MAX_ROWS: usize = 120;
pub const ART_MAX_COLS: usize = 400;
/// Bytes read from a splash art file.
const ART_MAX_BYTES: u64 = 256 * 1024;

/// 5×5 block font: rows concatenated, `#` = lit.
const FONT: &[(char, &str)] = &[
    ('A', ".###.#...#######...##...#"),
    ('B', "####.#...#####.#...#####."),
    ('C', ".#####....#....#.....####"),
    ('D', "####.#...##...##...#####."),
    ('E', "######....####.#....#####"),
    ('F', "######....####.#....#...."),
    ('G', ".#####....#..###...#.####"),
    ('H', "#...##...#######...##...#"),
    ('I', "#####..#....#....#..#####"),
    ('J', "..###...#....#.#..#..##.."),
    ('K', "#...##..#.###..#..#.#...#"),
    ('L', "#....#....#....#....#####"),
    ('M', "#...###.###.#.##...##...#"),
    ('N', "#...###..##.#.##..###...#"),
    ('O', ".###.#...##...##...#.###."),
    ('P', "####.#...#####.#....#...."),
    ('Q', ".###.#...##.#.##..#..##.#"),
    ('R', "####.#...#####.#..#.#...#"),
    ('S', ".#####.....###.....#####."),
    ('T', "#####..#....#....#....#.."),
    ('U', "#...##...##...##...#.###."),
    ('V', "#...##...##...#.#.#...#.."),
    ('W', "#...##...##.#.###.###...#"),
    ('X', "#...#.#.#...#...#.#.#...#"),
    ('Y', "#...#.#.#...#....#....#.."),
    ('Z', "#####...#...#...#...#####"),
    ('0', ".###.#..###.#.###..#.###."),
    ('1', "..#...##....#....#...###."),
    ('2', ".###.#...#..##..#...#####"),
    ('3', "####.....#.###.....#####."),
    ('4', "#..#.#..#.#####...#....#."),
    ('5', "######....####.....#####."),
    ('6', ".###.#....####.#...#.###."),
    ('7', "#####...#...#...#....#..."),
    ('8', ".###.#...#.###.#...#.###."),
    ('9', ".###.#...#.####....#.###."),
    (' ', "........................."),
    ('-', "..........#####.........."),
    ('!', "..#....#....#.........#.."),
    ('?', ".###.#...#..##........#.."),
    ('.', "......................#.."),
    ('_', "....................#####"),
    (':', ".......#.........#......."),
    ('/', "....#...#...#...#...#...."),
    ('+', ".......#...###...#......."),
];

/// `s` in the 5-row block font (unknown characters as `?`).
pub fn block_text(s: &str) -> Vec<String> {
    let mut rows = vec![String::new(); 5];
    for (i, ch) in s.chars().enumerate() {
        let up = ch.to_ascii_uppercase();
        let bits = FONT.iter().find(|(c, _)| *c == up).or_else(|| FONT.iter().find(|(c, _)| *c == '?')).map(|g| g.1);
        let bits: Vec<char> = bits.unwrap_or("").chars().collect();
        for (r, row) in rows.iter_mut().enumerate() {
            if i > 0 {
                row.push(' ');
            }
            for c in 0..5 {
                row.push(if bits.get(r * 5 + c) == Some(&'#') { '█' } else { ' ' });
            }
        }
    }
    rows
}

/// `s` as one letter-spaced row ("pivt" → "P I V T").
pub fn plain_text(s: &str) -> Vec<String> {
    vec![spaced(&s.to_uppercase())]
}

/// Letter-spaced text: "ctf toolkit" → "c t f   t o o l k i t".
pub fn spaced(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

fn expand_tilde(p: &str) -> std::path::PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| p.into()),
        None => p.into(),
    }
}

/// Read an ASCII art file: tabs → 4 spaces, ANSI escapes and other control
/// characters dropped, clipped to [`ART_MAX_ROWS`] × [`ART_MAX_COLS`].
pub fn load_art(path: &str) -> Result<Vec<String>, String> {
    use std::io::Read;
    if path.trim().is_empty() {
        return Err("splash_file isn't set".into());
    }
    let p = expand_tilde(path.trim());
    let f = std::fs::File::open(&p).map_err(|e| format!("splash_file {}: {e}", p.display()))?;
    let mut bytes = Vec::new();
    f.take(ART_MAX_BYTES).read_to_end(&mut bytes).map_err(|e| format!("splash_file {}: {e}", p.display()))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines().take(ART_MAX_ROWS) {
        let mut line = String::new();
        let mut chars = raw.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => {
                    // CSI: ESC [ params final-byte
                    if chars.peek() == Some(&'[') {
                        chars.next();
                        for d in chars.by_ref() {
                            if ('@'..='~').contains(&d) {
                                break;
                            }
                        }
                    }
                }
                '\t' => line.push_str("    "),
                c if c.is_control() => {}
                c => line.push(c),
            }
        }
        lines.push(line.chars().take(ART_MAX_COLS).collect::<String>().trim_end().to_string());
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    while lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    if lines.is_empty() {
        return Err(format!("splash_file {} is empty", p.display()));
    }
    Ok(lines)
}

/// Banner rows as glyphs with their widths (zero-width chars dropped).
fn to_glyphs(lines: &[String]) -> Vec<Vec<Glyph>> {
    lines
        .iter()
        .map(|l| {
            l.chars()
                .filter_map(|c| {
                    let w = prefs::char_width(c);
                    (1..=2).contains(&w).then_some((c, w as u8))
                })
                .collect()
        })
        .collect()
}

/// The splash banner for these prefs, and a warning when the custom one
/// couldn't be used (the built-in banner is used then).
pub fn banner_for(p: &Prefs) -> (Vec<Vec<Glyph>>, Option<String>) {
    let builtin = || to_glyphs(&BANNER.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    match p.splash_banner {
        SplashBanner::Pivt => (builtin(), None),
        SplashBanner::File => match load_art(&p.splash_file) {
            Ok(lines) => (to_glyphs(&lines), None),
            Err(e) => (builtin(), Some(format!("{e} (built-in banner used)"))),
        },
        SplashBanner::Text => {
            let s = p.splash_text.trim();
            if s.is_empty() {
                return (builtin(), Some("splash_text is empty (built-in banner used)".into()));
            }
            let lines = match p.splash_font {
                SplashFont::Block => block_text(s),
                SplashFont::Plain => plain_text(s),
            };
            (to_glyphs(&lines), None)
        }
    }
}

/// Draw the splash banner centred in `area`, `el_ms` into the reveal
/// (pass something large for the finished banner). Clipped to the area.
/// Returns the banner's rect (for the rule / subtitle underneath).
pub fn draw_banner(buf: &mut Buffer, area: Rect, fx: &Fx, el_ms: u64) -> Option<Rect> {
    let area = area.intersection(buf.area);
    if fx.banner.is_empty() || area.width < 4 || area.height < 3 {
        return None;
    }
    let w = fx.banner_w.min(area.width);
    let h = (fx.banner.len() as u16).min(area.height);
    let bx = area.x + (area.width - w) / 2;
    let by = (area.y + area.height / 2).saturating_sub(h / 2 + 1).max(area.y).min(area.bottom() - h);
    let p = if fx.reveal_ms == 0 { 1.0 } else { (el_ms as f32 / fx.reveal_ms as f32).min(1.0) };
    let done = p >= 1.0;
    let th = theme();
    let bold = |c: Color| Style::default().fg(c).add_modifier(Modifier::BOLD);
    let full_w = fx.banner_w as f32;
    let reveal_cols = if done { u16::MAX } else { ((p * full_w) as u16).max(1) };
    let rows_shown = if done { u16::MAX } else { ((p * h as f32).ceil() as u16).max(1) };
    let fade_c = blend(th.star_dim, th.accent, p);
    let mut tmp = [0u8; 4];
    for (r, row) in fx.banner.iter().take(h as usize).enumerate() {
        let r = r as u16;
        let y = by + r;
        let mut col = 0u16;
        for &(ch, cw) in row {
            let cw = cw as u16;
            if col + cw > w {
                break;
            }
            let style = match fx.reveal {
                _ if done => Some(bold(th.accent)),
                Reveal::Instant => Some(bold(th.accent)),
                Reveal::Typewriter => (col < reveal_cols)
                    .then(|| bold(if col + 1 == reveal_cols { th.star_hi } else { th.accent })),
                Reveal::Fade => ((hash2(col, r) % 1000) as f32 / 1000.0 < p).then(|| bold(fade_c)),
                Reveal::Scanline => (r < rows_shown)
                    .then(|| bold(if r + 1 == rows_shown { th.star_hi } else { th.accent })),
            };
            if let Some(st) = style {
                buf.set_stringn(bx + col, y, ch.encode_utf8(&mut tmp), cw as usize, st);
            }
            col += cw;
        }
    }
    Some(Rect::new(bx, by, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(look: &str) -> Prefs {
        let (p, w) = Prefs::load(&toml::Table::new(), &toml::from_str(look).unwrap(), false);
        assert!(w.is_empty(), "{w:?}");
        p
    }

    fn frame(bg: Bg, fx: &Fx, ms: u64, loud: bool, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        let sc = Scene { ms, mouse: Some((w / 2, h / 2)), tint: theme().accent, loud };
        draw_background(&mut buf, area, bg, &sc, fx);
        draw_shooting_star(&mut buf, area, ms, fx, loud);
        buf
    }

    fn lit(b: &Buffer) -> usize {
        b.content().iter().filter(|c| c.symbol() != " ").count()
    }

    #[test]
    fn every_background_renders_at_any_size() {
        let fxs = [
            Fx::default(),
            Fx::from_prefs(&prefs(
                "star_parallax = true\nstar_drift = \"left\"\nstar_color = \"category\"\nflow_color = \"rainbow\"\nflow_octaves = 5\nmatrix_charset = \"binary\"\nsnow_wind = -5\nshoot_angle = \"random\"\nshoot_length = 20",
            ))
            .0,
            Fx::from_prefs(&prefs(
                "bg_density = 10\nmatrix_density = 10\nsnow_density = 10\nplasma_density = 10\nstar_brightness = 10\nflow_scale = 1\nplasma_scale = 1\nshoot_angle = \"down\"",
            ))
            .0,
            Fx::from_prefs(&prefs("bg_density = 1\nmatrix_density = 1\nsnow_density = 1\nstar_brightness = 1\nflow_scale = 10\nflow_speed = 0\nshoot_angle = \"up-left\"")).0,
        ];
        for fx in &fxs {
            for bg in Bg::ALL {
                for &(w, h) in &[(80, 24), (200, 60), (1, 1), (3, 2), (12, 5)] {
                    for ms in [0, 333, 7_777, 123_456_789] {
                        for loud in [false, true] {
                            frame(bg, fx, ms, loud, w, h);
                        }
                    }
                }
                // something actually shows at a normal size
                let n = (0..6).map(|i| lit(&frame(bg, fx, i * 900, true, 80, 24))).max().unwrap();
                assert!(n > 0, "{} drew nothing", bg.name());
            }
        }
    }

    #[test]
    fn backgrounds_stay_subtle_behind_the_ui() {
        // in-app (not loud): a minority of cells, so text stays readable
        let fx = Fx::default();
        for bg in Bg::ALL {
            let n = lit(&frame(bg, &fx, 5_000, false, 200, 60));
            assert!(n < 200 * 60 * 2 / 5, "{}: {n} of 12000 cells", bg.name());
        }
    }

    #[test]
    fn frozen_clock_is_still_and_time_moves_things() {
        let fx = Fx::default();
        for bg in Bg::ALL {
            assert_eq!(frame(bg, &fx, 0, false, 80, 24), frame(bg, &fx, 0, false, 80, 24), "{}", bg.name());
            let moved = (1..20).any(|i| frame(bg, &fx, 0, true, 80, 24) != frame(bg, &fx, i * 700, true, 80, 24));
            assert!(moved, "{} never changes", bg.name());
        }
    }

    #[test]
    fn options_change_the_picture() {
        let base = Fx::default();
        let pairs = [
            ("star_parallax = true", Bg::Stars),
            ("star_drift = \"left\"", Bg::Stars),
            ("star_brightness = 9", Bg::Stars),
            ("flow_scale = 9", Bg::Flow),
            ("flow_angle = 90", Bg::Flow),
            ("flow_octaves = 1", Bg::Flow),
            ("flow_color = \"rainbow\"", Bg::Flow),
            ("matrix_charset = \"binary\"", Bg::Matrix),
            ("snow_wind = -4", Bg::Snow),
            ("plasma_scale = 2", Bg::Plasma),
        ];
        for (look, bg) in pairs {
            let fx = Fx::from_prefs(&prefs(look)).0;
            assert_ne!(frame(bg, &base, 9_000, false, 80, 24), frame(bg, &fx, 9_000, false, 80, 24), "{look}");
        }
    }

    #[test]
    fn shooting_star_follows_its_settings() {
        let area = Rect::new(0, 0, 80, 24);
        let count = |fx: &Fx, ms: u64| {
            let mut b = Buffer::empty(area);
            draw_shooting_star(&mut b, area, ms, fx, false);
            lit(&b)
        };
        let fx = Fx::default();
        // a streak shortly after each period boundary, nothing mid-period
        assert!(count(&fx, 13_000 + 200) > 0);
        assert_eq!(count(&fx, 13_000 + 6_000), 0);
        let long = Fx::from_prefs(&prefs("shoot_length = 12\nshoot_every = 2")).0;
        assert!(count(&long, 2_000 + 300) > count(&fx, 13_000 + 300));
        assert!(count(&long, 4_000 + 300) > 0, "every 2 s");
        // shoot = false is handled by the caller; random headings stay in bounds
        let rnd = Fx::from_prefs(&prefs("shoot_angle = \"random\"")).0;
        for n in 0..40 {
            count(&rnd, n * 13_000 + 250);
        }
    }

    #[test]
    fn pulse_ping_pongs() {
        let (a, b) = (Color::Rgb(0, 0, 0), Color::Rgb(200, 100, 50));
        assert_eq!(pulse(0, 600, a, b), a);
        assert_eq!(pulse(300, 600, a, b), b);
        assert_eq!(pulse(600, 600, a, b), a);
        assert_ne!(pulse(150, 600, a, b), a);
    }

    #[test]
    fn burst_styles_render() {
        for style in [BurstStyle::Ring, BurstStyle::Sparkle, BurstStyle::Confetti] {
            for &(w, h) in &[(80, 24), (200, 60), (1, 1), (5, 3)] {
                for p in [0.0, 0.3, 0.7, 1.0, 2.0] {
                    let area = Rect::new(0, 0, w, h);
                    let mut b = Buffer::empty(area);
                    draw_burst(&mut b, area, p, style, 1234);
                }
            }
            let area = Rect::new(0, 0, 80, 24);
            let mut b = Buffer::empty(area);
            draw_burst(&mut b, area, 0.4, style, 1234);
            assert!(lit(&b) > 0, "{style:?}");
        }
    }

    #[test]
    fn block_font_and_spacing() {
        let rows = block_text("pivt");
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|r| r.chars().count() == 4 * 6 - 1));
        assert!(rows[0].starts_with("████"));
        // every glyph is 5×5
        for (c, bits) in FONT {
            assert_eq!(bits.len(), 25, "{c}");
        }
        assert_eq!(block_text("~")[0], block_text("?")[0], "unknown → ?");
        assert_eq!(spaced("ctf toolkit"), "c t f   t o o l k i t");
        assert_eq!(plain_text("hi"), vec!["H I".to_string()]);
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("pivt-anim-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn splash_art_file_loads_clips_and_falls_back() {
        let d = tmp("art");
        let ok = d.join("art.txt");
        std::fs::write(&ok, "\n  /\\_/\\\n\t( o.o )\x1b[31m red\x1b[0m\n   > ^ <\n\n").unwrap();
        let lines = load_art(ok.to_str().unwrap()).unwrap();
        assert_eq!(lines, ["  /\\_/\\", "    ( o.o ) red", "   > ^ <"], "blank edges trimmed, tabs expanded, ANSI dropped");
        let (fx, warn) = Fx::from_prefs(&prefs(&format!("splash_banner = \"file\"\nsplash_file = \"{}\"", ok.display())));
        assert!(warn.is_none());
        assert_eq!(fx.banner.len(), 3);

        // oversized: clipped when loading and when drawing, never a panic
        let big = d.join("big.txt");
        let row = "#".repeat(500);
        std::fs::write(&big, (0..3000).map(|_| row.as_str()).collect::<Vec<_>>().join("\n")).unwrap();
        let (fx, warn) = Fx::from_prefs(&prefs(&format!("splash_banner = \"file\"\nsplash_file = \"{}\"", big.display())));
        assert!(warn.is_none());
        assert_eq!(fx.banner.len(), ART_MAX_ROWS);
        assert_eq!(fx.banner_w as usize, ART_MAX_COLS);
        for &(w, h) in &[(80, 24), (200, 60), (4, 3), (1, 1)] {
            let area = Rect::new(0, 0, w, h);
            let mut b = Buffer::empty(area);
            for ms in [0, 200, 100_000] {
                let r = draw_banner(&mut b, area, &fx, ms);
                if let Some(r) = r {
                    assert!(area.contains(r.as_position()) && r.right() <= area.right() && r.bottom() <= area.bottom());
                }
            }
        }

        // missing / empty → built-in banner + a warning
        let (fx, warn) = Fx::from_prefs(&prefs("splash_banner = \"file\"\nsplash_file = \"/nonexistent/pivt-art.txt\""));
        assert!(warn.unwrap().contains("built-in"));
        assert_eq!(fx.banner.len(), BANNER.len());
        let empty = d.join("empty.txt");
        std::fs::write(&empty, "\n\n").unwrap();
        let (_, warn) = Fx::from_prefs(&prefs(&format!("splash_banner = \"file\"\nsplash_file = \"{}\"", empty.display())));
        assert!(warn.unwrap().contains("empty"));
        let (_, warn) = Fx::from_prefs(&prefs("splash_banner = \"text\""));
        assert!(warn.is_some(), "empty splash_text falls back");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn readme_animation_samples_are_valid() {
        let readme = include_str!("../README.md");
        let sec = &readme[readme.find("## Animations").expect("Animations section")..];
        // the [look] example parses without a single warning
        let toml_block = &sec[sec.find("```toml\n").unwrap() + 8..];
        let toml_block = &toml_block[..toml_block.find("```").unwrap()];
        let t: toml::Table = toml::from_str(toml_block).unwrap();
        let (p, w) = Prefs::load(&toml::Table::new(), t["look"].as_table().unwrap(), false);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(p.background, Background::Matrix);
        // the sample splash file loads as 5 rows
        let art = &sec[sec.find("```text\n").unwrap() + 8..];
        let art = &art[..art.find("```").unwrap()];
        let d = tmp("readme");
        let f = d.join("splash.txt");
        std::fs::write(&f, art).unwrap();
        assert_eq!(load_art(f.to_str().unwrap()).unwrap().len(), 5);
        std::fs::remove_dir_all(&d).ok();
        // every option is documented
        for o in prefs::OPTIONS {
            assert!(readme.contains(&format!("| `{}` |", o.key)), "README lacks {}", o.key);
        }
    }

    #[test]
    fn every_reveal_style_and_source_draws() {
        let sources = ["", "splash_banner = \"text\"\nsplash_text = \"HTB!\"", "splash_banner = \"text\"\nsplash_text = \"pwn\"\nsplash_font = \"plain\""];
        for src in sources {
            for reveal in Reveal::NAMES {
                let fx = Fx::from_prefs(&prefs(&format!("{src}\nsplash_reveal = \"{reveal}\""))).0;
                for &(w, h) in &[(80, 24), (200, 60), (10, 4)] {
                    let area = Rect::new(0, 0, w, h);
                    let mut first = Buffer::empty(area);
                    let mut last = Buffer::empty(area);
                    draw_banner(&mut first, area, &fx, 1);
                    draw_banner(&mut last, area, &fx, 60_000);
                    if w >= 80 {
                        assert!(lit(&last) > 0, "{src} {reveal}");
                        // (a one-row banner is a single scanline)
                        if *reveal != "instant" && !(*reveal == "scanline" && fx.banner.len() == 1) {
                            assert!(lit(&first) < lit(&last), "{src} {reveal}: reveals over time");
                        }
                    }
                }
            }
        }
    }
}
