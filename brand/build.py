# /// script
# requires-python = ">=3.11"
# dependencies = ["fonttools==4.60.1", "uharfbuzz==0.51.1"]
# ///
"""brand/build.py — writes the PractiCal icon and logo (build/assets/icon.svg, build/assets/logo.svg).

`mise run logo`. The lettering is outlined (paths, no font needed to display it) from Inter Display ExtraBold,
Inter 4.1 (SIL OFL 1.1), downloaded once into e2e/.work/brand and checked against its SHA-256.
The design and its reasoning: brand/README.md. Output is byte-identical for the same inputs.
"""

import hashlib
import io
import pathlib
import urllib.request
import zipfile

import uharfbuzz as hb
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont

REPO = pathlib.Path(__file__).resolve().parent.parent
ZIP_URL = "https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip"
ZIP_SHA256 = "9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e"
TTF = "extras/ttf/InterDisplay-ExtraBold.ttf"

GRADIENT = ("#5856d6", "#0a84ff")  # indigo → PractiCal blue, top left → bottom right
GREY = "#7c7c82"  # "Practi": the same contrast on white (4.15:1) and on #1c1c1e (4.10:1)
BLUE = "#0a84ff"  # "Cal": the app's accent colour
WORD, SPLIT = "PractiCal", 6  # glyphs from index 6 on are blue
SIZE, X0, BASELINE = 34.0, 78.0, 44.5  # in the 64-unit icon height; the cap height centres on the icon

ICON = (
    f'<defs><linearGradient id="pc-g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{GRADIENT[0]}"/>'
    f'<stop offset="1" stop-color="{GRADIENT[1]}"/></linearGradient></defs>'
    '<rect width="64" height="64" rx="14" fill="url(#pc-g)"/>'
    '<g fill="none" stroke="#fff" stroke-width="3.5" stroke-linecap="round" stroke-linejoin="round">'
    '<rect x="14" y="17" width="36" height="33" rx="7"/><path d="M14 27h36M24 12v9M40 12v9"/></g>'
    '<circle cx="38" cy="39" r="4" fill="#fff"/>'
)


def font_bytes() -> bytes:
    cache = REPO / "e2e/.work/brand/Inter-4.1.zip"
    if not cache.exists():
        cache.parent.mkdir(parents=True, exist_ok=True)
        with urllib.request.urlopen(ZIP_URL) as r:
            cache.write_bytes(r.read())
    data = cache.read_bytes()
    if hashlib.sha256(data).hexdigest() != ZIP_SHA256:
        raise SystemExit(f"{cache}: SHA-256 mismatch, delete it and run again")
    return zipfile.ZipFile(io.BytesIO(data)).read(TTF)


def num(v: float) -> str:
    return ("%.2f" % v).rstrip("0").rstrip(".")


def lettering(ttf: bytes) -> tuple[float, str, str]:
    """(right edge, grey path, blue path) of WORD, shaped by HarfBuzz (kerning) and outlined by fontTools"""
    font = hb.Font(hb.Face(ttf))
    buf = hb.Buffer()
    buf.add_str(WORD)
    buf.guess_segment_properties()
    hb.shape(font, buf, {"kern": True, "liga": True})
    tt = TTFont(io.BytesIO(ttf))
    glyphs, order, k = tt.getGlyphSet(), tt.getGlyphOrder(), SIZE / tt["head"].unitsPerEm
    x, parts = X0, ([], [])
    for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
        pen = SVGPathPen(glyphs, ntos=num)
        glyphs[order[info.codepoint]].draw(TransformPen(pen, (k, 0, 0, -k, x + pos.x_offset * k, BASELINE - pos.y_offset * k)))
        parts[info.cluster >= SPLIT].append(pen.getCommands())
        x += pos.x_advance * k
    return x, "".join(parts[0]), "".join(parts[1])


def main() -> None:
    right, grey, blue = lettering(font_bytes())
    width = int(right + 2.99)  # a 2-unit margin, whole units
    assets = REPO / "build/assets"
    (assets / "icon.svg").write_text(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">{ICON}</svg>\n')
    (assets / "logo.svg").write_text(
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} 64" role="img" aria-label="PractiCal">'
        f'<title>PractiCal</title>{ICON}<path fill="{GREY}" d="{grey}"/><path fill="{BLUE}" d="{blue}"/></svg>\n'
    )
    print(f"build/assets/icon.svg, build/assets/logo.svg ({width}×64)")


if __name__ == "__main__":
    main()
