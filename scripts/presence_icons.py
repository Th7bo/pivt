#!/usr/bin/env python3
"""Render Discord presence badges: python3 scripts/presence_icons.py src/theme.rs assets/presence

One 256px badge per category in theme::category_style (glyph + colour), plus
default.png. JetBrains Mono Bold; glyphs it lacks fall back to Iosevka NF."""
import re, sys, os
from PIL import Image, ImageDraw, ImageFont, ImageFilter
src = open(sys.argv[1]).read()
out = sys.argv[2]; os.makedirs(out, exist_ok=True)
cats = re.findall(r'"(\w+)" => \(\'(.)\', Color::Rgb\(0x(..), 0x(..), 0x(..)\)\)', src)
cats.append(("default", "◆", "2d", "d4", "bf"))
JB = "/usr/share/fonts/jetbrains-mono-fonts/JetBrainsMono-Bold.otf"
FALLBACK = "/usr/share/fonts/IosevkaNerdFont/IosevkaNerdFont-Bold.ttf"
from fontTools.ttLib import TTFont
JB_CMAP = TTFont(JB).getBestCmap()
used = {}
BG = (0x0d,0x11,0x17); PANEL=(0x12,0x18,0x21)
N = 1024  # supersample, downsample to 256
for name, g, r, gg, b in cats:
    col = (int(r,16), int(gg,16), int(b,16))
    im = Image.new("RGB", (N,N), BG)
    # soft glow of the category color
    glow = Image.new("RGB", (N,N), BG); gd = ImageDraw.Draw(glow)
    gd.ellipse((N*0.2,N*0.2,N*0.8,N*0.8), fill=tuple(int(c*0.35+BG[i]*0.65) for i,c in enumerate(col)))
    im = glow.filter(ImageFilter.GaussianBlur(N*0.12))
    d = ImageDraw.Draw(im)
    # ring (rounded border, like the TUI panels)
    m = N*0.07; w = int(N*0.035)
    d.ellipse((m,m,N-m,N-m), outline=col, width=w)
    # glyph centered on its ink box
    path = JB if ord(g) in JB_CMAP else FALLBACK
    used[name] = "JetBrains Mono" if path == JB else "Iosevka (fallback)"
    font = ImageFont.truetype(path, int(N*0.50))
    x0,y0,x1,y1 = d.textbbox((0,0), g, font=font)
    # glyph glow
    gl = Image.new("L", (N,N), 0); ImageDraw.Draw(gl).text(((N-(x1-x0))/2-x0, (N-(y1-y0))/2-y0), g, font=font, fill=255)
    halo = gl.filter(ImageFilter.GaussianBlur(N*0.03))
    im = Image.composite(Image.new("RGB",(N,N),col), im, halo.point(lambda v: int(v*0.45)))
    im = Image.composite(Image.new("RGB",(N,N),col), im, gl)
    im.resize((256,256), Image.LANCZOS).save(f"{out}/{name}.png")
    print(name, g, used[name])
