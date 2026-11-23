use ratatui::style::Color;

// Sleek dark palette (truecolor). Tuned for kitty / modern terminals.
pub const BG: Color = Color::Rgb(0x0d, 0x11, 0x17);
pub const PANEL: Color = Color::Rgb(0x12, 0x18, 0x21);
pub const FG: Color = Color::Rgb(0xc9, 0xd1, 0xd9);
pub const DIM: Color = Color::Rgb(0x56, 0x62, 0x70);
pub const ACCENT: Color = Color::Rgb(0x2d, 0xd4, 0xbf); // teal
pub const ACCENT2: Color = Color::Rgb(0xa7, 0x8b, 0xfa); // violet
pub const WARN: Color = Color::Rgb(0xf8, 0x72, 0x72); // red/pink for unset vars
pub const OK: Color = Color::Rgb(0x7e, 0xe7, 0x87); // green
pub const BORDER: Color = Color::Rgb(0x23, 0x2b, 0x36);
pub const BORDER_HI: Color = ACCENT;
pub const SEL_BG: Color = Color::Rgb(0x1b, 0x2a, 0x33);

// starfield ramp (dim -> bright)
pub const STAR_FAINT: Color = Color::Rgb(0x1e, 0x26, 0x30);
pub const STAR_DIM: Color = Color::Rgb(0x30, 0x3b, 0x49);
pub const STAR_MID: Color = Color::Rgb(0x4b, 0x5a, 0x6b);
pub const STAR_HI: Color = Color::Rgb(0x8f, 0xa6, 0xbd);

// syntax coloring in the command preview
pub const SYN_FLAG: Color = Color::Rgb(0x8f, 0xb8, 0xff); // -flags
pub const SYN_STR: Color = Color::Rgb(0xe5, 0xc0, 0x7b); // 'quoted'
pub const SYN_NUM: Color = Color::Rgb(0xf0, 0xb4, 0x57); // 123 / ip / :port
pub const SYN_OP: Color = Color::Rgb(0x6d, 0x7a, 0x8a); // | ; && > <

/// Per-category glyph + accent color. Unknown categories fall back to a diamond.
pub fn category_style(name: &str) -> (char, Color) {
    match name {
        "nmap" => ('⌖', Color::Rgb(0x2d, 0xd4, 0xbf)),
        "web" => ('◉', Color::Rgb(0x60, 0xa5, 0xfa)),
        "smb" => ('▣', Color::Rgb(0xa7, 0x8b, 0xfa)),
        "shells" => ('⌁', Color::Rgb(0x7e, 0xe7, 0x87)),
        "upgrade" => ('▲', Color::Rgb(0xf0, 0xb4, 0x57)),
        "transfer" => ('⇅', Color::Rgb(0x22, 0xd3, 0xee)),
        "ad" => ('✦', Color::Rgb(0xf8, 0x72, 0x72)),
        "crack" => ('⬢', Color::Rgb(0xfb, 0x92, 0x3c)),
        "pivot" => ('⇄', Color::Rgb(0xf4, 0x72, 0xb6)),
        "misc" => ('●', Color::Rgb(0x94, 0xa3, 0xb8)),
        "encode" => ('⧉', Color::Rgb(0x34, 0xd3, 0x99)),
        "dns" => ('◈', Color::Rgb(0x81, 0x8c, 0xf8)),
        "snmp" => ('▦', Color::Rgb(0x2d, 0xd4, 0xbf)),
        "rpc" => ('⊞', Color::Rgb(0xc0, 0x8b, 0xfa)),
        "db" => ('◰', Color::Rgb(0xf4, 0x72, 0xb6)),
        "tunnel" => ('≈', Color::Rgb(0x22, 0xd3, 0xee)),
        "forensics" => ('⌕', Color::Rgb(0xf0, 0xb4, 0x57)),
        "pwn" => ('⚑', Color::Rgb(0xf8, 0x72, 0x72)),
        "win" => ('❖', Color::Rgb(0x60, 0xa5, 0xfa)),
        "favorites" => ('★', Color::Rgb(0xf0, 0xb4, 0x57)),
        "recent" => ('⏱', Color::Rgb(0x8f, 0xa6, 0xbd)),
        "suggested" => ('✸', Color::Rgb(0x2d, 0xd4, 0xbf)),
        _ => ('◆', ACCENT),
    }
}
