"""Generate Comfont's original layout-slot TrueType font (requires fonttools).

No system font outlines are copied. Both private-use characters have the same
advance and metrics. Pairs at E000..E00B have widths of 1, 2, 4, 8, 16, 32 em.
"""
from pathlib import Path
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

builder = FontBuilder(1000, isTTF=True)
widths = [1, 2, 4, 8, 16, 32]
names = [(f"marker{width}", f"blank{width}") for width in widths]
order = [".notdef"] + [name for pair in names for name in pair]
builder.setupGlyphOrder(order)
builder.setupCharacterMap({0xE000 + i: name for i, name in enumerate(order[1:])})
glyphs = {}
metrics = {".notdef": (1000, 0)}
for name in order:
    pen = TTGlyphPen(None)
    width = 1 if name == ".notdef" else int(name.removeprefix("marker").removeprefix("blank"))
    metrics[name] = (width * 1000, 0)
    if name.startswith("marker"):
        pen.moveTo((0, -200))
        pen.lineTo((0, 800))
        pen.lineTo((width * 1000, 800))
        pen.lineTo((width * 1000, -200))
        pen.closePath()
    glyphs[name] = pen.glyph()
builder.setupGlyf(glyphs)
builder.setupHorizontalMetrics(metrics)
builder.setupHorizontalHeader(ascent=800, descent=-200, lineGap=0)
builder.setupNameTable({
    "familyName": "Comfont Emoji Marker v2", "styleName": "Regular",
    "uniqueFontIdentifier": "ComfontEmojiMarker-v2",
    "fullName": "Comfont Emoji Marker v2", "psName": "ComfontEmojiMarker-v2",
    "version": "Version 2.000",
})
builder.setupOS2(sTypoAscender=800, sTypoDescender=-200, sTypoLineGap=0,
                 usWinAscent=800, usWinDescent=200, fsSelection=0x40)
builder.setupPost()
builder.setupMaxp()
builder.font["head"].created = builder.font["head"].modified = 2082844800
destination = Path(__file__).resolve().parent.parent / "resource" / "emoji-marker.ttf"
builder.save(destination)
