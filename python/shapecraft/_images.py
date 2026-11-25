"""Pictures of a sheet and of an original, cut without hand work.

    crop("sheet.pdf", (x, y, w, h), "out.png")      # a sheet's millimetres, from its lower left corner
    crop("original.png", (x, y, w, h), "out.png")   # pixels, from the upper left corner
    tile("original.png", (3, 2), "tiles/")          # overlapping tiles at 2x, to read small text once
    view_crops(sheet_pdf, inventory, folder)        # one image per view (what a build writes)

From the command line: `python -m shapecraft.tools crop|tile ...` (see `help images`).
Needs poppler's pdftoppm/pdfinfo for PDFs (not a Python package) and Pillow for PNGs
(`pip install 'shapecraft-cad[images]'`); each says what is missing when it is.
"""

from __future__ import annotations

import os
import re
import subprocess

MM = 25.4 / 72.0  # millimetres per point


def poppler_available() -> bool:
    """Whether poppler's `pdftoppm` is on the PATH (it renders PDF sheets to pictures)."""
    import shutil
    return shutil.which("pdftoppm") is not None


def _poppler(command: list) -> "subprocess.CompletedProcess":
    """Runs one of poppler's tools; says what to install when it is missing."""
    try:
        return subprocess.run(command, capture_output=True, text=True)
    except FileNotFoundError:
        raise RuntimeError(
            f"{command[0]} not found: reading PDF sheets needs poppler-utils "
            "(apt install poppler-utils, brew install poppler, or poppler for Windows on the PATH)") from None


def _pillow():
    """Pillow's Image module; says how to get it when it is missing."""
    try:
        from PIL import Image
    except ImportError:
        raise RuntimeError("cropping and tiling pictures needs Pillow: pip install 'shapecraft-cad[images]'") from None
    return Image


def _page_points(pdf: str) -> tuple[float, float]:
    out = _poppler(["pdfinfo", pdf]).stdout
    m = re.search(r"Page size:\s+([\d.]+) x ([\d.]+) pts", out)
    if not m:
        raise RuntimeError(f"cannot read the page size of {pdf}")
    return float(m[1]), float(m[2])


def crop(path: str, box, out: str, long_side: int = 1200, margin: float = 0.0) -> str:
    """Writes the part of a sheet or an image in `box` = (x, y, w, h) to `out` (PNG) and returns
    `out`. A PDF's box is in sheet millimetres from its lower left corner (what `sheet.txt` says
    of views: `at`, `box`), rendered so its long side is about `long_side` pixels. A PNG/JPG's box is in
    pixels from its upper left corner (a smaller box is enlarged to about `long_side`).
    `margin` widens the box on every side (millimetres, or pixels)."""
    x, y, w, h = (float(v) for v in box)
    x, y, w, h = x - margin, y - margin, w + 2 * margin, h + 2 * margin
    os.makedirs(os.path.dirname(os.path.abspath(out)) or ".", exist_ok=True)
    if path.lower().endswith(".pdf"):
        pw, ph = _page_points(path)
        sheet_w, sheet_h = pw * MM, ph * MM
        x0, y0 = max(x, 0.0), max(y, 0.0)
        x1, y1 = min(x + w, sheet_w), min(y + h, sheet_h)
        if x1 <= x0 or y1 <= y0:
            raise ValueError(f"the box {box} is off the sheet ({sheet_w:.0f} x {sheet_h:.0f} mm)")
        dpi = min(max(long_side / (max(x1 - x0, y1 - y0) / 25.4), 50.0), 800.0)
        px = dpi / 25.4  # pixels per millimetre
        left, top = int(round(x0 * px)), int(round((sheet_h - y1) * px))
        width, height = max(int(round((x1 - x0) * px)), 1), max(int(round((y1 - y0) * px)), 1)
        done = _poppler(
            ["pdftoppm", "-png", "-singlefile", "-r", f"{dpi:.1f}", "-x", str(left), "-y", str(top),
             "-W", str(width), "-H", str(height), path, out[:-4] if out.endswith(".png") else out])
        if done.returncode != 0:
            raise RuntimeError(done.stderr.strip())
        return out if out.endswith(".png") else out + ".png"
    Image = _pillow()
    with Image.open(path) as im:
        x0, y0 = max(int(x), 0), max(int(y), 0)
        x1, y1 = min(int(x + w), im.width), min(int(y + h), im.height)
        if x1 <= x0 or y1 <= y0:
            raise ValueError(f"the box {box} is off the image ({im.width} x {im.height} px)")
        part = im.crop((x0, y0, x1, y1)).convert("RGB")
        scale = long_side / max(part.size)
        if scale > 1.0:
            part = part.resize((int(part.width * scale), int(part.height * scale)), Image.LANCZOS)
        part.save(out)
    return out


def tile(path: str, grid=(3, 2), out_dir: str = "tiles", scale: float = 2.0, overlap: float = 0.12) -> list:
    """Cuts an image into `grid` = (columns, rows) overlapping tiles (each `overlap` wider on every
    side), enlarged `scale` times, as `<out_dir>/tile-r<row>c<col>.png` (counted from 1, the
    upper left first). Returns [(file, (x, y, w, h) in the original's pixels)] — to say where a
    tile is — and writes the same list to `<out_dir>/tiles.txt`."""
    Image = _pillow()
    os.makedirs(out_dir, exist_ok=True)
    cols, rows = int(grid[0]), int(grid[1])
    made = []
    with Image.open(path) as im:
        im = im.convert("RGB")
        cw, ch = im.width / cols, im.height / rows
        for r in range(rows):
            for c in range(cols):
                x0, y0 = max(int(c * cw - overlap * cw), 0), max(int(r * ch - overlap * ch), 0)
                x1, y1 = min(int((c + 1) * cw + overlap * cw), im.width), min(int((r + 1) * ch + overlap * ch), im.height)
                part = im.crop((x0, y0, x1, y1)).resize((int((x1 - x0) * scale), int((y1 - y0) * scale)), Image.LANCZOS)
                file = os.path.join(out_dir, f"tile-r{r + 1}c{c + 1}.png")
                part.save(file)
                made.append((file, (x0, y0, x1 - x0, y1 - y0)))
    with open(os.path.join(out_dir, "tiles.txt"), "w") as f:
        for file, (x, y, w, h) in made:
            f.write(f"{os.path.basename(file)}  original pixels x={x} y={y} w={w} h={h}  (shown at {scale:g}x)\n")
    return made


def view_crops(pdf: str, inventory: dict, folder: str, long_side: int = 1200, margin: float = 3.0) -> dict:
    """One image per view of a sheet: the view's box (its outline and its dimensions) from the PDF,
    as `<folder>/view-<id>.png`. Returns {view id: file}. The images for a sheet with several
    drawings go on one folder: ids are the sheet's own."""
    from concurrent.futures import ThreadPoolExecutor

    def one(v):
        x0, y0, x1, y1 = v["box"]
        file = os.path.join(folder, f"view-{v['id']}.png")
        try:
            return v["id"], crop(pdf, (x0, y0, x1 - x0, y1 - y0), file, long_side, margin)
        except (ValueError, RuntimeError):
            return v["id"], None

    with ThreadPoolExecutor(max_workers=4) as pool:
        return {i: f for i, f in pool.map(one, inventory["views"]) if f}
