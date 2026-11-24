#!/usr/bin/env python3
"""Render `PIVT_SNAP_ANSI=1 pivt snap <mode>` output to a PNG.

usage: ansi2png.py in.txt out.png
Cells are drawn on a fixed grid in JetBrains Mono, falling back to Iosevka NF
for glyphs it lacks. Box-drawing / block characters are drawn as vectors so
borders join seamlessly regardless of font metrics.
"""
import re, sys
from PIL import Image, ImageDraw, ImageFont
from fontTools.ttLib import TTFont

JB = "/usr/share/fonts/jetbrains-mono-fonts/JetBrainsMono-{}.otf"
FB = "/usr/share/fonts/IosevkaNerdFont/IosevkaNerdFont-{}.ttf"
SIZE, CW, CH, PAD = 26, 16, 34, 28
fonts = {w: (ImageFont.truetype(JB.format(w), SIZE), ImageFont.truetype(FB.format(w), SIZE)) for w in ("Regular", "Bold")}
JB_CMAP = TTFont(JB.format("Regular")).getBestCmap()
SGR = re.compile(r"\x1b\[([0-9;]*)m")

def parse(line):
    cells, fg, bg, bold, pos = [], (201,209,217), (13,17,23), False, 0
    for m in SGR.finditer(line + "\x1b[0m"):
        for ch in line[pos:m.start()]:
            cells.append((ch, fg, bg, bold))
        pos = m.end()
        p = [int(x) for x in m.group(1).split(";") if x] or [0]
        i = 0; bold = False
        while i < len(p):
            if p[i] == 1: bold = True
            elif p[i] == 38: fg = tuple(p[i+2:i+5]); i += 4
            elif p[i] == 48: bg = tuple(p[i+2:i+5]); i += 4
            i += 1
    return cells

def box(d, ch, x, y, col):
    """Vector-draw light/heavy box drawing + rounded corners + full blocks."""
    cx, cy, t = x + CW // 2, y + CH // 2, 2
    if ch == "█": d.rectangle((x, y, x+CW-1, y+CH-1), fill=col); return True
    if ch == "▎": d.rectangle((x, y, x+CW//4, y+CH-1), fill=col); return True
    arms = {"─":"lr","━":"lr","═":"lr","│":"ud","┃":"ud","║":"ud","╭":"rd","╮":"ld","╰":"ru","╯":"lu",
            "┌":"rd","┐":"ld","└":"ru","┘":"lu","├":"udr","┤":"udl","┬":"lrd","┴":"lru","┼":"lrud",
            "╔":"rd","╗":"ld","╚":"ru","╝":"lu","╠":"udr","╣":"udl"}.get(ch)
    if arms is None: return False
    if ch in "═║╔╗╚╝╠╣":  # double lines
        for o in (-3, 3):
            if "l" in arms: d.line((x, cy+o, cx, cy+o), fill=col, width=t)
            if "r" in arms: d.line((cx, cy+o, x+CW, cy+o), fill=col, width=t)
            if "u" in arms: d.line((cx+o, y, cx+o, cy), fill=col, width=t)
            if "d" in arms: d.line((cx+o, cy, cx+o, y+CH), fill=col, width=t)
        return True
    if ch in "╭╮╰╯":
        r = CW // 2
        if ch == "╭": d.arc((cx, cy, cx+2*r, cy+2*r), 180, 270, fill=col, width=t); d.line((cx, cy+r, cx, y+CH), fill=col, width=t); d.line((cx+r, cy, x+CW, cy), fill=col, width=t)
        if ch == "╮": d.arc((cx-2*r, cy, cx, cy+2*r), 270, 360, fill=col, width=t); d.line((cx, cy+r, cx, y+CH), fill=col, width=t); d.line((x, cy, cx-r, cy), fill=col, width=t)
        if ch == "╰": d.arc((cx, cy-2*r, cx+2*r, cy), 90, 180, fill=col, width=t); d.line((cx, y, cx, cy-r), fill=col, width=t); d.line((cx+r, cy, x+CW, cy), fill=col, width=t)
        if ch == "╯": d.arc((cx-2*r, cy-2*r, cx, cy), 0, 90, fill=col, width=t); d.line((cx, y, cx, cy-r), fill=col, width=t); d.line((x, cy, cx-r, cy), fill=col, width=t)
        return True
    w = 4 if ch in "━┃" else t
    if "l" in arms: d.line((x, cy, cx, cy), fill=col, width=w)
    if "r" in arms: d.line((cx, cy, x+CW, cy), fill=col, width=w)
    if "u" in arms: d.line((cx, y, cx, cy), fill=col, width=w)
    if "d" in arms: d.line((cx, cy, cx, y+CH), fill=col, width=w)
    return True

rows = [parse(l.rstrip("\n")) for l in open(sys.argv[1], encoding="utf-8")]
cols = max(len(r) for r in rows)
im = Image.new("RGB", (cols*CW + 2*PAD, len(rows)*CH + 2*PAD), (13,17,23))
d = ImageDraw.Draw(im)
for ry, row in enumerate(rows):
    for rx, (ch, fg, bg, bold) in enumerate(row):
        x, y = PAD + rx*CW, PAD + ry*CH
        d.rectangle((x, y, x+CW, y+CH), fill=bg)
        if ch == " " or box(d, ch, x, y, fg): continue
        jb, fb = fonts["Bold" if bold else "Regular"]
        f = jb if ord(ch) in JB_CMAP else fb
        d.text((x + CW/2, y + CH/2), ch, font=f, fill=fg, anchor="mm")
im.save(sys.argv[2])
