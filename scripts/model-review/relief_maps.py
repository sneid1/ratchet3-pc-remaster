"""Relief maps from Codex's generated heights, for every texture installed colour only.

Each request Codex returned carries a height map beside its colour. Character atlases compiled through imagegen_layers.py take
their height into their fitted normal map; every other installed request (the old queue's world textures, the level priority
queue, the gear queue) took the colour only. Their heights become relief maps here: the game tilts the surface's normal by them
at the texture's own coordinates, over its Rift Apart material when it has one (texnames.rs `relief.tsv`, lighting.rs
`material_relief`).

  python scripts/model-review/relief_maps.py

Writes relief/<stem>.png (RGBA: the normal's x and y in red and green, green up against v, z in blue) and relief.tsv (stem, map,
strength) into the installed pack and the main candidate pack, for each installed colour-only request's atlases and, on the gear
queue, each alias id of the same image. Run again after more requests are installed: it rewrites both whole.
"""
import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import imagegen_wire as w

# The map's size a side (the texture's 1024 colour is seen at this relief or finer on screen) and the slope a unit of height
# gives per texel at the height's own resolution: imagegen_layers.py's relief for painted metal and leather (3).
SIZE = 512
SLOPE = 3.0
# The longest tilt a texel takes, as imagegen_layers.py clamps it.
MAX_TILT = 0.65


def relief(height_png):
    """The relief normal of a generated height: its broad gradients removed (the mesh owns the shape: a blur of 0.8 texel less
    one of 12, as imagegen_layers.py), its slope by wrapped central differences, at SIZE."""
    h = np.asarray(Image.open(height_png).convert("L"), np.float32) / 255
    surface = (w.wrap_blur(h[..., None], 0.8) - w.wrap_blur(h[..., None], 12))[..., 0]
    dx = (np.roll(surface, -1, axis=1) - np.roll(surface, 1, axis=1)) / 2
    dy = (np.roll(surface, -1, axis=0) - np.roll(surface, 1, axis=0)) / 2
    xy = np.stack((-dx, dy), axis=2) * SLOPE
    # To SIZE by area, then the tilt clamped and z made.
    xy = np.stack([np.asarray(Image.fromarray(xy[..., k]).resize((SIZE, SIZE), Image.Resampling.BOX)) for k in range(2)], axis=2)
    xy *= np.minimum(1, MAX_TILT / np.maximum(np.linalg.norm(xy, axis=2), 1e-6))[..., None]
    z = np.sqrt(np.maximum(0, 1 - (xy * xy).sum(axis=2)))
    rgba = np.dstack((xy * 0.5 + 0.5, z * 0.5 + 0.5, np.ones_like(z)))
    return Image.fromarray(np.uint8(np.round(np.clip(rgba, 0, 1) * 255)), "RGBA")


def installed_colour_only():
    """Every installed request drawn colour only, with its atlases and aliases. A texture two queues installed (a gear atlas
    the level queue also had) takes its relief from the later install, whose colour the game draws."""
    rids = [r["id"] for r in json.loads((w.QUEUE / "queue.json").read_text())["requests"]]
    rids += list(w.priority_requests()) + list(w.gear_requests())
    latest = {}
    for rid in rids:
        r = w.request(rid)
        if (r["dir"] / "wired.json").exists() and w.colour_only(r):
            date = json.loads((r["dir"] / "wired.json").read_text())["date"]
            for stem in r["atlases"]:
                if stem not in latest or date > latest[stem][0]:
                    latest[stem] = (date, r)
    out = {}
    for stem, (_, r) in latest.items():
        out.setdefault(r["id"], (r, []))[1].append(stem)
    return [dict(r, atlases=stems) for r, stems in out.values()]


def main():
    rows = []
    requests = installed_colour_only()
    for pack in (w.INSTALLED, w.MAIN_CAND):
        (pack / "relief").mkdir(exist_ok=True)
    for i, r in enumerate(requests):
        for stem in r["atlases"]:
            height = w.ART / f"{w.atlas_key(r, stem)}-height.png"
            if not height.exists():
                raise SystemExit(f"{r['id']}: no height {height}")
            img = relief(height)
            for pack in (w.INSTALLED, w.MAIN_CAND):
                img.save(pack / "relief" / f"{stem}.png")
            rows.append((stem, f"{stem}.png"))
            for alias in r.get("aliases", []):
                rows.append((alias, f"{stem}.png"))
        if i % 50 == 0:
            print(f"{i} of {len(requests)} requests", flush=True)
    text = "# Relief maps from Codex's generated heights (scripts/model-review/relief_maps.py): stem, map in relief/, strength.\n"
    text += "stem\tmap\tstrength\n" + "".join(f"{stem}\t{m}\t1\n" for stem, m in rows)
    for pack in (w.INSTALLED, w.MAIN_CAND):
        (pack / "relief.tsv").write_text(text, encoding="utf-8")
    print(f"relief maps for {len(rows)} texture names ({len(requests)} requests) in {w.INSTALLED} and {w.MAIN_CAND}")


if __name__ == "__main__":
    main()
