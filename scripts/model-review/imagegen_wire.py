"""Wire Codex's returned wave sheets into the candidate pack and the game, one request at a time.

Codex writes <id>/colour.png, <id>/height.png and <id>/result.json into the queue folder; everything after that is here:

  python scripts/model-review/imagegen_wire.py status
  python scripts/model-review/imagegen_wire.py intake ID     # checks, split into per-atlas art, draft recipe, intake sheet
  python scripts/model-review/imagegen_wire.py compile ID    # zones for unmapped atlases, family baseline, imagegen_layers
  python scripts/model-review/imagegen_wire.py views ID      # native front/rear/close views of every class using the atlases
  python scripts/model-review/imagegen_wire.py install ID    # the reviewed files and rows into the game's installed pack

The level texture priority queue (authored/texture-priority-queue.json, one texture per request, results in
authored/vidcomic/<texture id>/ or authored/noncomic/<texture id>/) is wired the same way, by texture id: each of its
requests is a world texture, colour only, drawn where the level puts it (terrain, tie, moby or shrub). The gear queue
(authored/gear-texture-priority-queue.json, results in authored/gear/<texture id>/) is the player's weapons, gadgets and
armour: each request one atlas, colour only too (its fitted normal and ORM maps keep the same UVs), written also under
each alias id the queue lists (an alternate id of the same image).

Recipes live in npc-texture-materials.json (one entry per atlas, authored after looking at the intake sheet: protected
boxes, relief, colour blend; `zones` for atlases the pack has no material map for). queue.json is never written here,
because Codex reads it while a wave runs; this script's state is <id>/intake.json, <id>/wired.json and the views.
"""
import argparse
import csv
import glob
import hashlib
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "captures/rift-texture-review"
QUEUE = OUT / "authored/imagegen-queue"
ART = OUT / "authored/roster"
# The candidate pack. A worker lane (worker --lane N) compiles and draws in its own copy, candidate-laneN/0-riftapart,
# so several workers can run at once without one's compile showing in another's before|after pair; intake records
# baselines against the main pack, so baseline checks compare with MAIN_CAND.
MAIN_CAND = OUT / "candidate/0-riftapart"
CAND = MAIN_CAND
HD = OUT / "profile/texture_replacements/reddyxd-uya"
GAPFILL = OUT / "profile/texture_replacements/texmaster-pal-gapfill"
DUMPS = Path.home() / "Games/Ratchet and Clank 3/texture_dumps"
SOURCE_TEX = OUT / "npc-catalogue/source-export/tex"
INSTALLED = Path.home() / "Games/Ratchet and Clank 3/texture_replacements/0-riftapart"
BACKUPS = Path.home() / "Games/Ratchet and Clank 3/backups"
LOG = OUT / "integration"
RECIPES = Path(__file__).with_name("npc-texture-materials.json")
CATALOGUE_ASSETS = OUT / "npc-catalogue/native/assets.json"

# Materials family_pbr.py classifies (its families table plus material-recipes.json profiles), with the relief
# strength a generated height gets on each (the mesh owns the shape; skin and lenses take little or none).
RELIEF = {"paint": 3.0, "metal": 3.0, "cloth": 4.0, "leather": 3.0, "skin": 1.5, "lens": 0.0, "fur": 1.0, "-": 0.0}
MATERIAL_CLASS = {
    "steel_painted_panels_002": "paint", "mol_arch_painted_metal_04": "paint", "paint_001": "paint", "paint_003": "paint",
    "review_painted_metal": "paint", "plastic_004": "paint", "rust_tile_001": "metal",
    "aged_steel_001": "metal", "steel_carbon_001": "metal", "steel_parkerized_001": "metal", "brushed_steel_001": "metal",
    "steel_001": "metal", "anodized_metal_01": "metal", "bronze_001": "metal", "brass_001": "metal", "gold_001": "metal",
    "chromium_001": "metal", "review_dark_alloy": "metal", "review_satin_alloy": "metal",
    "detail_heavyclothwrinkles": "cloth", "cotton_001": "cloth", "ard_cloth_tiling": "cloth", "denim_001": "cloth",
    "detail_leather_02": "leather", "review_soft_leather": "leather",
    "detail_skinreptile_02": "skin", "detail_skinhippo": "skin", "detail_skinrhino_02": "skin",
    "detail_skincrocodile": "skin", "skin_micro_details": "skin", "review_ty_skin": "skin",
    "review_wasp_cuticle": "skin", "review_wasp_membrane": "skin", "review_lens": "lens", "fur": "fur", "-": "-",
}


def tsv(name):
    lines = [s for s in (CAND / name).read_text(encoding="utf-8").splitlines() if s and not s.startswith("#")]
    reader = csv.DictReader(lines, delimiter="\t")
    return reader.fieldnames, list(reader)


PRIORITY_QUEUE = OUT / "authored/texture-priority-queue.json"
_priority = {}


def priority_requests():
    if not _priority:
        _priority.update((x["id"], x) for x in json.loads(PRIORITY_QUEUE.read_text())["requests"])
    return _priority


def request(rid):
    """A request in the shape the wiring uses, with `dir`, its folder. A priority queue request is one world texture:
    its owner names its kind and full texture id (two ids can share their first 16 digits: palette variants of one image),
    its wave is the priority queue's (P1, P2, ...)."""
    for r in json.loads((QUEUE / "queue.json").read_text())["requests"]:
        if r["id"] == rid:
            return dict(r, dir=QUEUE / rid)
    if rid in gear_requests():
        return gear_request(gear_requests()[rid])
    if rid not in priority_requests():
        raise SystemExit(f"no request {rid}")
    x = priority_requests()[rid]
    return dict(id=rid, wave=f"P{x['wave']}", owner=f"env_{x['kind']}_{rid}", atlases=[rid], classes=[], kind=x["kind"],
                tiling=x["tiling"], levels=x["levels"], reference=x["reference"], output_size=x["output_size"],
                dir=ROOT / Path(x["result_output"]).parent)


GEAR_QUEUE = OUT / "authored/gear-texture-priority-queue.json"
_gear = {}


def gear_requests():
    if not _gear:
        _gear.update((x["id"], x) for x in json.loads(GEAR_QUEUE.read_text())["requests"])
    return _gear


def gear_request(x):
    """A gear queue request in the wiring's shape: square references compile at 1024 a side, others keep their aspect."""
    w, h = x["reference_size"]
    size = f"1024x{round(1024 * h / w)}" if w >= h else f"{round(1024 * w / h)}x1024"
    return dict(id=x["id"], wave=f"G{x['wave']}", owner=f"gear_{x['id']}", atlases=[x["id"]], classes=[], kind="gear",
                tiling=False, levels=[], reference=x["reference"], output_size=size, aliases=x["aliases"],
                dir=ROOT / Path(x["result_output"]).parent)


def rdir(rid):
    return request(rid)["dir"]


def atlas_key(r, stem):
    return r["owner"].replace("_", "-") + "-" + stem[:6]


def is_env(r):
    """A world texture (terrain, ties) or a gear atlas: compiled colour only over its current pixels (`compile_env`), its
    Rift Apart surface row or fitted maps unchanged."""
    return r["owner"].startswith(("env_", "gear_"))


def colour_only(r):
    """Compiled, reset and installed like a world texture: a world texture, or a character request whose recipes all say
    `env` (an atlas whose surfaces.tsv material is one family_pbr.py has no class for, such as sand on a robot's base, keeps
    that Rift Apart row and takes the generated colour only). Its views stay the character's."""
    if is_env(r):
        return True
    entries = {e["key"]: e for e in recipes()}
    return all(entries[atlas_key(r, s)].get("env") for s in r["atlases"])


def wrap_blur(a, radius):
    """Gaussian blur of an H x W x C float image that wraps at its edges (a tiling texture's neighbours are its own far edges)."""
    h, w = a.shape[:2]
    # A Gaussian of standard deviation `radius` px applied in frequency space, which is periodic by construction.
    g = np.exp(-2 * np.pi ** 2 * radius ** 2 * (np.fft.fftfreq(h)[:, None] ** 2 + np.fft.fftfreq(w)[None, :] ** 2))
    return np.real(np.fft.ifft2(np.fft.fft2(a, axes=(0, 1)) * g[..., None], axes=(0, 1))).astype(np.float32)


def env_base(r, rec):
    """What the game draws for a world texture today, in the game's pack order (texnames.rs reads the folders sorted): the pack's
    own replacement if it has one (0-riftapart), else the HD pack's (reddyxd-uya), else the gap-fill pack's (texmaster-pal-gapfill),
    else the PS2 texel. The gap-fill pack's upscale carries the smooth cut-out outline the game draws; the PS2 texel's would put
    each texel's step on every leaf (level 17's plant, 4ce5754d)."""
    if Path(rec["baseline"]) == MAIN_CAND / "replacements" / (rec["stem"] + ".png"):
        return ART / f"{rec['stem']}-baseline.png"
    if rec["hd_identity_source"]:
        return Path(rec["hd_identity_source"])
    gap = glob.glob(str(GAPFILL / ("-".join(rec["stem"].split("-")[:2]) + "-*.png")))
    return Path(gap[0]) if len(gap) == 1 else ART / f"{rec['stem']}-baseline.png"


def compile_env(r):
    """The generated colour over the texture's current pixels at the request's size, under its unchanged surfaces.tsv row (the
    Rift Apart material keeps its grain, normal and gloss); the generated height is used only by intake's checks."""
    entries = {e["key"]: e for e in recipes()}
    intake_rec = {a["stem"]: a for a in json.loads((r["dir"] / "intake.json").read_text())["atlases"]}
    ow, oh = map(int, r["output_size"].split("x"))
    for stem in r["atlases"]:
        e, rec = entries[atlas_key(r, stem)], intake_rec[stem]
        if "DRAFT" in e["notes"]:
            raise SystemExit(f"{e['key']}: recipe is still a draft")
        base_img = Image.open(env_base(r, rec)).convert("RGBA").resize((ow, oh), Image.Resampling.LANCZOS)
        base = np.asarray(base_img, np.float32) / 255
        art = np.asarray(Image.open(ART / f"{e['key']}-colour.png").convert("RGB").resize((ow, oh), Image.Resampling.LANCZOS), np.float32) / 255
        cutout = base[..., 3].min() < 0.5
        if cutout:
            # A cut-out (leaves, grilles): the sheet paints one flat colour where the texture is transparent (the old queue's
            # references were drawn over grey 96; on the priority queue's dumps Codex paints white), and the original alpha
            # reaches a little past the generated outline, so that colour would show as a fringe. The colour is measured on
            # the sheet inside the texture's transparent areas, and near them it is filled from the nearest drawn colour
            # (blurs of growing size, normalised by coverage) before blending; a light highlight well inside the drawn
            # shape is the art's own and stays.
            clear = (base[..., 3] == 0).astype(np.float32)[..., None]
            core = wrap_blur(clear, 6)[..., 0] > 0.99
            bg = np.zeros(core.shape, bool)
            if core.any():
                flat = np.median(art[core], axis=0)
                bg = (np.abs(art - flat).max(axis=2) < 20 / 255) & (wrap_blur(clear, 12)[..., 0] > 0.01)
            fg = (~bg).astype(np.float32)[..., None]
            filled = art.copy()
            todo = bg.copy()
            for radius in (2, 4, 8, 16, 32, 64):
                num, den = wrap_blur(art * fg, radius), wrap_blur(fg, radius)
                ok = todo & (den[..., 0] > 1e-3)
                filled[ok] = num[ok] / den[ok]
                todo &= ~ok
            art = filled
        if e.get("palette_lock") and cutout:
            # On a cut-out both averages are weighted by the texture's alpha: the transparent texels' colour (dark, never
            # drawn) would otherwise pull every drawn texel within the lock's reach toward it, a dark rim along each outline
            # and along an edge that wraps onto a transparent one (level 16's moss dome, 8ff378e7).
            rad, w = e["palette_lock"], base[..., 3:4]
            den = wrap_blur(w, rad)
            shift = (wrap_blur(base[..., :3] * w, rad) - wrap_blur(art * w, rad)) / np.maximum(den, 1e-4)
            art = np.clip(art + np.where(den > 1e-3, shift, 0), 0, 1)
        elif e.get("palette_lock"):
            art = np.clip(art + wrap_blur(base[..., :3], e["palette_lock"]) - wrap_blur(art, e["palette_lock"]), 0, 1)
        b = e["colour_blend"]
        out = base.copy()
        out[..., :3] = base[..., :3] * (1 - b) + art * b
        Image.fromarray(np.uint8(np.round(np.clip(out, 0, 1) * 255)), "RGBA").save(CAND / "replacements" / (stem + ".png"))
        print("env colour", stem, f"{ow}x{oh}", "blend", b, "palette lock", e.get("palette_lock", 0), "base", env_base(r, rec).name)


def reset_env(r):
    intake_rec = {a["stem"]: a for a in json.loads((r["dir"] / "intake.json").read_text())["atlases"]}
    for stem in r["atlases"]:
        rep = CAND / "replacements" / (stem + ".png")
        if Path(intake_rec[stem]["baseline"]) == MAIN_CAND / "replacements" / (stem + ".png"):
            shutil.copy2(ART / f"{stem}-baseline.png", rep)
        else:
            rep.unlink(missing_ok=True)


TERRAIN_INDEX = OUT / "npc-catalogue/terrain-index"
# A world texture's views: the terrain chunks that carry most of it, from above, low across it and close.
ENV_VIEWS = (("high", 35, 35, 1.0, "game"), ("low", 215, 12, 1.0, "game"), ("close", 35, 35, 2.2, "game"))


TIE_INDEX = OUT / "npc-catalogue/tie-index"
MOBY_INDEX = OUT / "library/uya-mobies/occurrences.tsv"
SHRUB_INDEX = OUT / "npc-catalogue/shrub-index"


def env_candidates(r):
    """Where a world texture is drawn, best first: terrain chunks by their triangles of it (`@terrain:N`), then per tie class
    carrying it its instances (`@tie:N`; only an instance the level capture lit can be drawn, so `env_models` tries them in turn)."""
    stem = r["atlases"][0]
    terrain, ties = [], {}
    for level in r["levels"]:
        for index in (TERRAIN_INDEX / f"L{level:02d}.tsv", TIE_INDEX / f"L{level:02d}.tsv"):
            if not index.exists():
                raise SystemExit(f"{r['id']}: no index {index}")
        for row in csv.DictReader((TERRAIN_INDEX / f"L{level:02d}.tsv").open(encoding="utf-8"), delimiter="\t"):
            if row["layer"] == "terrain" and row["stem"] == stem:
                terrain.append((int(row["stem_triangles"]), level, int(row["id"])))
        for row in csv.DictReader((TIE_INDEX / f"L{level:02d}.tsv").open(encoding="utf-8"), delimiter="\t"):
            if stem in row["stems"].split(","):
                ties.setdefault((level, row["class_id"]), []).append((int(row["triangles"]), int(row["instance"])))
    out = []
    for n, level, chunk in sorted(terrain, reverse=True):
        out.append([dict(key=f"terrain_l{level:02d}_c{chunk}", name=f"Level {level} terrain chunk {chunk} ({n} triangles of it)",
                         level=level, source_layer="terrain", source_id=chunk)])
    for (level, cls), inst in sorted(ties.items(), key=lambda kv: -kv[1][0][0]):
        out.append([dict(key=f"tie_l{level:02d}_{i}", name=f"Level {level} tie class {cls} instance {i} ({n} triangles)",
                         level=level, source_layer="tie", source_id=i) for n, i in inst])
    # Level objects drawn as mobies (a world texture on a prop class): the moby inventory's core slot, `@source:SLOT`.
    mobies = []
    for row in csv.DictReader(MOBY_INDEX.open(encoding="utf-8"), delimiter="\t"):
        if int(row["level"]) in r["levels"] and stem in row["textures"].split(","):
            mobies.append((int(row["near_triangles"]), int(row["level"]), int(row["slot"]), row["class_id"]))
    for n, level, slot, cls in sorted(mobies, reverse=True):
        out.append([dict(key=f"moby_l{level:02d}_{slot}", name=f"Level {level} moby class {cls} ({n} triangles)",
                         level=level, source_layer="source", source_id=slot)])
    # Shrubs (foliage and small props, `@shrub:INSTANCE`): one instance of each class carrying it (the --shrub-index lists one per class).
    shrubs = []
    for level in r["levels"]:
        index = SHRUB_INDEX / f"L{level:02d}" / "instances.tsv"
        if not index.exists():
            raise SystemExit(f"{r['id']}: no shrub index {index}")
        for row in csv.DictReader(index.open(encoding="utf-8"), delimiter="\t"):
            if stem in row["textures"].split(","):
                shrubs.append((int(row["triangles"]), level, int(row["instance"]), row["class_id"]))
    for n, level, inst, cls in sorted(shrubs, reverse=True):
        out.append([dict(key=f"shrub_l{level:02d}_{inst}", name=f"Level {level} shrub class {cls} instance {inst} ({n} triangles)",
                         level=level, source_layer="shrub", source_id=inst)])
    if not out:
        raise SystemExit(f"{r['id']}: the texture is on no terrain chunk, tie, moby or shrub of levels {r['levels']}")
    return out


def env_models(r, renderer, dest):
    """Two isolated pieces of the world carrying the texture (the first terrain chunk and the first tie class, else the first
    two of either), each the first of its instances the level capture can draw; chosen at the 'before' stage and kept in
    views/models.json, so 'after' draws the same ones."""
    saved = r["dir"] / "views" / "models.json"
    if saved.exists():
        return json.loads(saved.read_text())
    stem = r["atlases"][0]
    groups = env_candidates(r)
    kinds = {}
    for g in groups:
        kinds.setdefault(g[0]["source_layer"], g)
    order = list(kinds.values()) + [g for g in groups if g not in kinds.values()]
    models, tried = {}, []
    for group in order:
        if len(models) == 2:
            break
        # Once one piece is drawable, a second is a nicety: a few instances of each further class are tried, not all of
        # them (a level 9 rock's tie class has 457 instances, nearly all unlit in the capture, about half an hour of probes).
        for c in (group if not models else group[:12]):
            m = dict(name=c["name"], prefixes=stem[:16], instance=0, level=c["level"],
                     poses=str(ROOT / f"captures/gameplay/L{c['level']:02d}"), whole=True, source_layer=c["source_layer"], source_id=c["source_id"])
            import viewer
            viewer.MODELS = {c["key"]: m}
            try:
                renderer.render(dict(model=c["key"], yaw=35, pitch=35, zoom=1.0, channel="game", pack="candidate", reload=True), dest / "probe.png")
            except RuntimeError as e:
                # A piece the capture cannot draw: an unlit tie, a far LOD, or a moby inventory slot the level's
                # capture has no model for ("no matching model"); the next candidate is tried, and none failing all.
                if "colour buffer" in str(e) or "LOD" in str(e) or "no matching model" in str(e):
                    tried.append(f"{c['key']}: {e}")
                    continue
                raise
            models[c["key"]] = m
            break
    (dest / "probe.png").unlink(missing_ok=True)
    if not models:
        raise SystemExit(f"{r['id']}: no drawable piece; " + "; ".join(tried[:6]))
    saved.write_text(json.dumps(models, indent=1) + "\n")
    return models


def recipes():
    return json.loads(RECIPES.read_text()) if RECIPES.exists() else []


def save_recipes(entries):
    RECIPES.write_text(json.dumps(entries, indent=1) + "\n")


def tiles(img, n):
    """The sheet's atlases left to right (a 2:1 sheet holds two squares)."""
    w, h = img.size
    return [img.crop((i * w // n, 0, (i + 1) * w // n, h)) for i in range(n)]


def grey(img, size=256):
    return np.asarray(img.convert("L").resize((size, size), Image.Resampling.BILINEAR), np.float32) / 255


def edge_map(a):
    a = np.asarray(Image.fromarray(np.uint8(a * 255)).filter(ImageFilter.GaussianBlur(1)), np.float32) / 255
    dy, dx = np.gradient(a)
    return np.hypot(dx, dy)


def corr(a, b):
    a, b = a.ravel() - a.mean(), b.ravel() - b.mean()
    return float((a * b).sum() / max(np.sqrt((a * a).sum() * (b * b).sum()), 1e-9))


def baseline(stem, decoded=None):
    """The atlas's own pixels the compiler starts from: candidate replacement, PS2 dump, else the disc export, else (a gear
    request) the queue's own decode of the disc's image."""
    for p in (CAND / "replacements" / (stem + ".png"), DUMPS / (stem + ".png"), SOURCE_TEX / (stem + ".png")) + ((decoded,) if decoded else ()):
        if p.exists():
            return p
    raise SystemExit(f"no source pixels for {stem}")


def state(d):
    return ("installed" if (d / "wired.json").exists() else "viewed" if (d / "views" / "compare.jpg").exists() else
            "intake" if (d / "intake.json").exists() else "returned" if (d / "result.json").exists() else "waiting")


def status():
    for r in json.loads((QUEUE / "queue.json").read_text())["requests"]:
        print(f"{state(QUEUE / r['id']):9} {r['id']}")
    # The priority queue's requests Codex has returned (its thousands of waiting ones are counted, not listed).
    waiting = 0
    for x in priority_requests().values():
        st = state(ROOT / Path(x["result_output"]).parent)
        if st == "waiting":
            waiting += 1
        else:
            print(f"{st:9} {x['id']}  P{x['wave']} {x['scope']} {x['kind']} L{','.join(map(str, x['levels']))}")
    print(f"waiting   {waiting} priority queue requests")
    waiting = 0
    for x in gear_requests().values():
        st = state(ROOT / Path(x["result_output"]).parent)
        if st == "waiting":
            waiting += 1
        else:
            print(f"{st:9} {x['id']}  G{x['wave']} gear tier {x['priority_tier']}")
    print(f"waiting   {waiting} gear queue requests")


def intake(rid):
    r = request(rid)
    d = r["dir"]
    result = json.loads((d / "result.json").read_text())
    ref, col, hgt = (Image.open(p).convert("RGBA") for p in (ROOT / r["reference"], d / "colour.png", d / "height.png"))
    ow, oh = map(int, r["output_size"].split("x"))
    checks = dict(result=result, aspect={})
    for name, img in (("colour", col), ("height", hgt)):
        ratio = img.width / img.height
        checks["aspect"][name] = dict(size=list(img.size), ratio_error=round(abs(ratio / (ow / oh) - 1), 4))
        if abs(ratio / (ow / oh) - 1) > 0.02:
            raise SystemExit(f"{rid}: {name} aspect {img.size} does not match {r['output_size']}")
    n = len(r["atlases"])
    atlases = []
    entries = recipes()
    known = {e["key"] for e in entries}
    _, region_rows = tsv("regions.tsv")
    _, surface_rows = tsv("surfaces.tsv")
    regions = {x["stem"]: x for x in region_rows}
    surfaces = {x["stem"]: x for x in surface_rows}
    for stem, rt, ct, ht in zip(r["atlases"], tiles(ref, n), tiles(col, n), tiles(hgt, n)):
        key = atlas_key(r, stem)
        g_ref, g_col, g_hgt = grey(rt), grey(ct), grey(ht)
        alpha = np.asarray(rt.getchannel("A").resize((256, 256)), np.float32) / 255
        rec = dict(stem=stem, key=key,
                   edge_correlation=round(corr(edge_map(g_ref), edge_map(g_col)), 3),
                   colour_mean_difference=round(float(np.abs(np.asarray(rt.convert("RGB").resize((32, 32)), np.float32) -
                                                             np.asarray(ct.convert("RGB").resize((32, 32)), np.float32)).mean()), 1),
                   height_vs_reference_luminance=round(corr(g_hgt, g_ref), 3),
                   height_near_mid=round(float((np.abs(g_hgt - 0.5) < 24 / 255).mean()), 3),
                   reference_opaque=round(float((alpha > 0.5).mean()), 3))
        ART.mkdir(parents=True, exist_ok=True)
        ct.convert("RGB").save(ART / f"{key}-colour.png")
        ht.convert("L").convert("RGB").save(ART / f"{key}-height.png")
        source = baseline(stem, ROOT / r["reference"] if r.get("kind") == "gear" else None)
        archive = ART / f"{stem}-baseline.png"
        if not archive.exists():
            shutil.copy2(source, archive)
        rec["baseline"] = str(source)
        hd = glob.glob(str(HD / ("-".join(stem.split("-")[:2]) + "-*.png")))
        rec["hd_identity_source"] = hd[0] if len(hd) == 1 else None
        if is_env(r):
            # A tiling world texture must meet itself at its edges: each edge's step against the texture's own steps.
            for name, img in (("colour", ct), ("height", ht)):
                a = np.asarray(img.convert("RGB"), np.float32)
                inner = (np.abs(np.diff(a, axis=1)).mean() + np.abs(np.diff(a, axis=0)).mean()) / 2
                rec[f"{name}_seam_ratio"] = round(float((np.abs(a[:, 0] - a[:, -1]).mean() + np.abs(a[0] - a[-1]).mean()) / 2 / max(inner, 1e-6)), 2)
            if key not in known:
                entries.append(dict(key=key, stem=stem, env=True, colour_blend=0.85 if rec["hd_identity_source"] else 0.9,
                                    palette_lock=0, notes=f"Wave {r['wave']} request {rid}; DRAFT: colour blend not yet judged"))
                known.add(key)
            rec["recipe_materials"] = "surface " + (surfaces[stem]["material"] if stem in surfaces else "none")
            atlases.append(rec)
            continue
        if key not in known:
            entry = dict(key=key, stem=stem, model=r["owner"], relief=[0, 0, 0, 0], protect=[], colour_blend=0.65,
                         skip_geometry_ao=True, preserve_metalness=True, finishes=[None, None, None, None],
                         notes=f"Wave {r['wave']} request {rid}; DRAFT: protected boxes and relief not yet authored")
            if stem in regions:
                mats = [regions[stem]["material" + (str(i) if i else "")] for i in range(4)]
            else:
                mats = [surfaces[stem]["material"] if stem in surfaces else "", "-", "-", "-"]
                entry["zones"] = dict(materials=mats, fill=0, boxes=[], polygons=[], surface=surfaces.get(stem))
            entry["relief"] = [RELIEF.get(MATERIAL_CLASS.get(m, ""), 0.0) for m in mats]
            entries.append(entry)
            known.add(key)
        rec["recipe_materials"] = [regions[stem]["material" + (str(i) if i else "")] for i in range(4)] if stem in regions else "zones"
        atlases.append(rec)
    save_recipes(entries)
    checks["atlases"] = atlases
    (d / "intake.json").write_text(json.dumps(checks, indent=1) + "\n")
    sheet(r, ref, col, hgt, atlases, d / "intake-sheet.jpg")
    print(json.dumps(atlases, indent=1))
    print("sheet", d / "intake-sheet.jpg")


def sheet(r, ref, col, hgt, atlases, path):
    n = len(atlases)
    cell = 512
    font = ImageFont.truetype("arial.ttf", 18)
    s = Image.new("RGB", (cell * 3 + 20, (cell + 70) * n), (24, 24, 28))
    dr = ImageDraw.Draw(s)
    for i, (a, rt, ct, ht) in enumerate(zip(atlases, tiles(ref, n), tiles(col, n), tiles(hgt, n))):
        y = i * (cell + 70)
        grid = Image.new("RGB", (cell, cell))
        for k, img in enumerate((rt, ct, ht)):
            im = img.convert("RGB").resize((cell, cell), Image.Resampling.LANCZOS)
            if k == 0:
                grid = im
            s.paste(im, (k * (cell + 10), y + 60))
        # 128-unit grid on the reference, the coordinate space of the recipe's protected boxes and zones
        g = ImageDraw.Draw(s)
        for v in range(0, 129, 16):
            p = v * cell // 128
            g.line([(p, y + 60), (p, y + 60 + cell)], fill=(255, 0, 255), width=1)
            g.line([(0, y + 60 + p), (cell, y + 60 + p)], fill=(255, 0, 255), width=1)
            g.text((p + 2, y + 62), str(v), fill=(255, 255, 0), font=font)
        dr.text((4, y + 4), f"{a['key']}  {a['stem']}", fill=(255, 230, 120), font=font)
        dr.text((4, y + 30), f"edges {a['edge_correlation']}  colour diff {a['colour_mean_difference']}  "
                             f"height~lum {a['height_vs_reference_luminance']}  height mid {a['height_near_mid']}  "
                             f"materials {a['recipe_materials']}", fill=(200, 200, 200), font=font)
    s.save(path, quality=90)


def remove_row(name, stem, pack):
    path = pack / name
    lines = path.read_text(encoding="utf-8").splitlines()
    kept = [l for l in lines if l.split("\t")[0] != stem]
    if len(kept) != len(lines):
        path.write_text("\n".join(kept) + "\n", encoding="utf-8")
        return True
    return False


def write_zones(entry):
    """A material map for an atlas the pack has none for, from the recipe's authored zones (128-unit coordinates)."""
    z = entry["zones"]
    for m in z["materials"]:
        if m not in MATERIAL_CLASS:
            raise SystemExit(f"{entry['key']}: zone material {m!r} is not one family_pbr.py classifies")
    size = 1024
    labels = Image.new("L", (size, size), z["fill"])
    dr = ImageDraw.Draw(labels)
    for ch, *box in z["boxes"]:
        dr.rectangle(tuple(round(v * size / 128) for v in box), fill=ch)
    for ch, points in z["polygons"]:
        dr.polygon([(round(x * size / 128), round(y * size / 128)) for x, y in points], fill=ch)
    lab = np.asarray(labels)
    rgba = np.stack([np.uint8(lab == i) * 255 for i in range(4)], axis=2)
    stem = entry["stem"]
    Image.fromarray(rgba, "RGBA").save(CAND / "regions" / (stem + ".png"))
    fields, rows = tsv("regions.tsv")
    _, surface_rows = tsv("surfaces.tsv")
    # Each material's parameters as the pack already calibrates it: its first regions row, else its surfaces row.
    params = {}
    for row in rows:
        for i in range(4):
            sfx = str(i) if i else ""
            params.setdefault(row["material" + sfx], {k: row[k + sfx] for k in
                              ("tiles_u", "tiles_v", "reflectance", "grain", "specular", "fur", "catchlight", "metal")})
    surface_keys = ("tiles_u", "tiles_v", "reflectance", "grain", "specular", "metal")
    if z.get("surface"):
        # The atlas's own single-material row, kept in the recipe at intake (an atlas is a surface or a region, never
        # both): its calibration moves into the region row, and the surface row goes.
        own = z["surface"]
        params[own["material"]] = {**{k: own[k] for k in surface_keys}, "fur": "0", "catchlight": "0"}
        remove_row("surfaces.tsv", stem, CAND)
    for row in surface_rows:
        params.setdefault(row["material"], {**{k: row[k] for k in surface_keys}, "fur": "0", "catchlight": "0"})
    new = {"stem": stem, "map": stem + ".png", "normal": "1"}
    for i, m in enumerate(z["materials"]):
        sfx = str(i) if i else ""
        new["material" + sfx] = m
        p = params[m] if m != "-" else dict(tiles_u="0", tiles_v="0", reflectance="0", grain="0", specular="0", fur="0", catchlight="0", metal="0")
        for k, v in p.items():
            new[k + sfx] = v
    rows = [x for x in rows if x["stem"] != stem] + [new]
    text = (CAND / "regions.tsv").read_text(encoding="utf-8").splitlines()
    head = [l for l in text if l.startswith("#")]
    with (CAND / "regions.tsv").open("w", newline="", encoding="utf-8") as f:
        for l in head:
            f.write(l + "\n")
        w = csv.DictWriter(f, fields, delimiter="\t", lineterminator="\n")
        w.writeheader()
        w.writerows(rows)


def compile_(rid):
    r = request(rid)
    if colour_only(r):
        return compile_env(r)
    keys = [atlas_key(r, s) for s in r["atlases"]]
    entries = {e["key"]: e for e in recipes()}
    for k in keys:
        if "DRAFT" in entries[k]["notes"]:
            raise SystemExit(f"{k}: recipe is still a draft; author it from the intake sheet first")
    zoned = [entries[k] for k in keys if "zones" in entries[k]]
    for e in zoned:
        write_zones(e)
        # The material baseline of a zoned atlas comes from its zones: drop the last compile's pbr row (family_pbr.py
        # leaves non-family rows alone) and imagegen_layers' frozen family inputs, so both are rebuilt from these zones.
        remove_row("pbr.tsv", e["stem"], CAND)
        for label in ("normal", "orm"):
            (ART / f"{e['key']}-family-{label}.png").unlink(missing_ok=True)
    py = sys.executable
    if zoned:
        subprocess.run([py, str(Path(__file__).with_name("family_pbr.py"))], check=True, cwd=ROOT)
    cmd = [py, str(Path(__file__).with_name("imagegen_layers.py")), "--extra-config", str(RECIPES)]
    for k in keys:
        cmd += ["--key", k]
    subprocess.run(cmd, check=True, cwd=ROOT)


# Lit with the game's own light pass and every PBR map (review channel "game"), not the review tool's separate PBR
# response, which veils fitted atlases with its captured environment reflection where the game draws no such veil.
VIEWS = (("front", 35, 8, 1.15, "game"), ("rear", 215, 8, 1.15, "game"), ("side", 125, 8, 1.15, "game"),
         ("close", 35, 8, 1.7, "game"), ("emission", 35, 8, 1.15, "emission"))


def reset(rid):
    """The 'before' state of a request, drawn with full PBR like the 'after': the atlas's original pixels on the same
    material zones and family PBR maps the compile starts from, so a before|after pair differs by the texture work only
    (an atlas the pack had no material map for gets its authored zones here too, not the plain unlit-material path)."""
    r = request(rid)
    if colour_only(r):
        return reset_env(r)
    entries = {e["key"]: e for e in recipes()}
    intake_rec = {a["stem"]: a for a in json.loads((r["dir"] / "intake.json").read_text())["atlases"]}
    for stem in r["atlases"]:
        e = entries[atlas_key(r, stem)]
        if "DRAFT" in e["notes"]:
            raise SystemExit(f"{e['key']}: recipe is still a draft")
        rep = CAND / "replacements" / (stem + ".png")
        if Path(intake_rec[stem]["baseline"]) == rep:
            shutil.copy2(ART / f"{stem}-baseline.png", rep)
        else:
            rep.unlink(missing_ok=True)
        remove_row("pbr.tsv", stem, CAND)
        if "zones" in e:
            write_zones(e)
            for label in ("normal", "orm"):
                (ART / f"{e['key']}-family-{label}.png").unlink(missing_ok=True)
    subprocess.run([sys.executable, str(Path(__file__).with_name("family_pbr.py"))], check=True, cwd=ROOT)


def views(rid, stage):
    """Codex's final-view set (front, rear, side, close, emission) through the isolated review renderer, full PBR,
    into views/<stage>/; with both stages present, a before|after sheet cropped to the character."""
    r = request(rid)
    sys.path.insert(0, str(Path(__file__).parent))
    import viewer
    # The emission view only where an atlas has an emission map (imagegen_layers keeps the pre-compile one, so the
    # answer is the same for both stages).
    _, rows = tsv("pbr.tsv")
    glow = any(x["emissive_map"] for x in rows if x["stem"] in r["atlases"])
    dest = r["dir"] / "views" / stage
    dest.mkdir(parents=True, exist_ok=True)
    args = argparse.Namespace(height=1100, export_obj=False, limit_mb=3200, batch=True, requests=True, poses=None)
    viewer.CANDIDATE = CAND
    renderer = viewer.Renderer(args)
    try:
        if is_env(r):
            chosen = env_models(r, renderer, dest)
            view_set = ENV_VIEWS
        else:
            assets = json.loads(CATALOGUE_ASSETS.read_text())
            prefixes = {s.split("-")[0] for s in r["atlases"]}
            chosen = {k: a for k, a in assets.items() if a.get("class_id") in r["classes"] and prefixes & set(a["prefixes"].split(","))}
            view_set = VIEWS
        if not chosen:
            raise SystemExit(f"{rid}: no catalogue asset of classes {r['classes']} binds these atlases")
        viewer.MODELS = chosen
        for name in sorted(chosen, key=lambda k: chosen[k]["level"]):
            for view, yaw, pitch, zoom, channel in view_set:
                if channel == "emission" and not glow:
                    continue
                renderer.render(dict(model=name, yaw=yaw, pitch=pitch, zoom=zoom, channel=channel, pack="candidate", reload=True),
                                dest / f"{name}-{view}.png")
                print(stage, name, view, flush=True)
    finally:
        renderer.close()
    if stage == "after" and (dest.parent / "before").is_dir():
        compare(r["dir"] / "views", chosen, view_set)


def compare(d, chosen, view_set):
    font = ImageFont.truetype("arial.ttf", 22)
    rows = []
    for name in chosen:
        for view, *_ in view_set:
            if not (d / "before" / f"{name}-{view}.png").exists():
                continue
            b = Image.open(d / "before" / f"{name}-{view}.png").convert("RGB")
            a = Image.open(d / "after" / f"{name}-{view}.png").convert("RGB")
            both = np.maximum(np.asarray(b).max(2), np.asarray(a).max(2)) > 12
            if not both.any():
                continue
            ys, xs = np.nonzero(both)
            pad = 24
            box = (max(xs.min() - pad, 0), max(ys.min() - pad, 0), min(xs.max() + pad, a.width), min(ys.max() + pad, a.height))
            b, a = b.crop(box), a.crop(box)
            h = 640
            w = int(a.width * h / a.height)
            if w > 900:  # wide models (a gun pod) keep the sheet under the 8000 px a sent image may be
                w, h = 900, int(a.height * 900 / a.width)
            row = Image.new("RGB", (2 * w + 12, h + 34), (24, 24, 28))
            row.paste(b.resize((w, h), Image.Resampling.LANCZOS), (0, 34))
            row.paste(a.resize((w, h), Image.Resampling.LANCZOS), (w + 12, 34))
            ImageDraw.Draw(row).text((6, 6), f"{chosen[name]['name']}  {view}    before | after", fill=(255, 230, 120), font=font)
            rows.append(row)
    W = max(x.width for x in rows)
    s = Image.new("RGB", (W, sum(x.height for x in rows)), (24, 24, 28))
    y = 0
    for x in rows:
        s.paste(x, ((W - x.width) // 2, y))
        y += x.height
    s.save(d / "compare.jpg", quality=90)
    print("compare", d / "compare.jpg")


def install(rid):
    r = request(rid)
    keys = [atlas_key(r, s) for s in r["atlases"]]
    # The before|after views are not needed to install (the player, 18 November 2026): the recipe, authored from the
    # intake sheet, is the review.
    if colour_only(r):
        # Colour only: the installed surfaces.tsv row (its Rift Apart material) is left as it is. A request a worker
        # lane drew (views/lane) installs that lane's compile, which also goes into the main candidate pack; any other
        # is compiled here, so what is installed is always this recipe's compile.
        lane = r["dir"] / "views" / "lane"
        if not lane.exists():
            compile_env(r)
        source = OUT / f"candidate-lane{lane.read_text().strip()}/0-riftapart" if lane.exists() else MAIN_CAND
        files = ["replacements/" + stem + ".png" for stem in r["atlases"]]
        stamp = time.strftime("%Y%m%d-%H%M%S")
        backup = BACKUPS / f"0-riftapart-wire-{rid}-{stamp}"
        record = dict(request=rid, date=stamp, backup=str(backup), files=[], tables={})
        for rel in files:
            src, dst = source / rel, INSTALLED / rel
            if dst.exists():
                (backup / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(dst, backup / rel)
            shutil.copy2(src, dst)
            if source != MAIN_CAND:
                shutil.copy2(src, MAIN_CAND / rel)
            record["files"].append(dict(file=rel, sha256=hashlib.sha256(dst.read_bytes()).hexdigest(), replaced=(backup / rel).exists()))
        for alias in r.get("aliases", []):
            # The same image under its alternate id: the game names it either way.
            rel = "replacements/" + alias + ".png"
            dst = INSTALLED / rel
            if dst.exists():
                (backup / rel).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(dst, backup / rel)
            shutil.copy2(source / ("replacements/" + r["atlases"][0] + ".png"), dst)
            shutil.copy2(dst, MAIN_CAND / rel)
            record["files"].append(dict(file=rel, sha256=hashlib.sha256(dst.read_bytes()).hexdigest(), replaced=(backup / rel).exists(), alias_of=r["atlases"][0]))
        (r["dir"] / "wired.json").write_text(json.dumps(record, indent=1) + "\n")
        LOG.mkdir(exist_ok=True)
        (LOG / f"wire-{rid}-{stamp}.json").write_text(json.dumps(record, indent=1) + "\n")
        print(f"installed {len(record['files'])} files (colour only); backup {backup}")
        return
    _, cand_pbr = tsv("pbr.tsv")
    _, cand_regions = tsv("regions.tsv")
    cpbr = {x["stem"]: x for x in cand_pbr}
    creg = {x["stem"]: x for x in cand_regions}
    files, pbr_rows, region_rows = [], [], []
    for stem, key in zip(r["atlases"], keys):
        row = cpbr[stem]
        if not row["normal_map"].startswith("authored_"):
            raise SystemExit(f"{stem}: candidate pbr row is not this compile's ({row['normal_map']})")
        pbr_rows.append(row)
        files += ["replacements/" + stem + ".png", "materials/" + row["normal_map"] + ".png", "materials/" + row["orm_map"] + ".png"]
        if row["emissive_map"]:
            files.append("materials/" + row["emissive_map"] + ".png")
        if stem in creg:
            region_rows.append(creg[stem])
            files.append("regions/" + creg[stem]["map"])
            for i in range(4):
                m = creg[stem]["material" + (str(i) if i else "")]
                if m not in ("-", "fur") and not list(INSTALLED.glob("materials/" + m + "*")):
                    raise SystemExit(f"{stem}: material {m} has no textures in the installed pack")
    stamp = time.strftime("%Y%m%d-%H%M%S")
    backup = BACKUPS / f"0-riftapart-wire-{rid}-{stamp}"
    record = dict(request=rid, date=stamp, backup=str(backup), files=[], tables={})
    for rel in files:
        src, dst = CAND / rel, INSTALLED / rel
        if dst.exists():
            (backup / rel).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(dst, backup / rel)
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)
        record["files"].append(dict(file=rel, sha256=hashlib.sha256(dst.read_bytes()).hexdigest(), replaced=(backup / rel).exists()))
    for x in region_rows:
        if any(l.split("\t")[0] == x["stem"] for l in (INSTALLED / "surfaces.tsv").read_text(encoding="utf-8").splitlines()):
            backup.mkdir(parents=True, exist_ok=True)
            if not (backup / "surfaces.tsv").exists():
                shutil.copy2(INSTALLED / "surfaces.tsv", backup / "surfaces.tsv")
            remove_row("surfaces.tsv", x["stem"], INSTALLED)
            record["tables"].setdefault("surfaces.tsv removed", []).append(x["stem"])
    for name, new in (("pbr.tsv", pbr_rows), ("regions.tsv", region_rows)):
        if not new:
            continue
        path = INSTALLED / name
        backup.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, backup / name)
        lines = path.read_text(encoding="utf-8").splitlines()
        head = [l for l in lines if l.startswith("#")]
        body = [l for l in lines if l and not l.startswith("#")]
        fields = body[0].split("\t")
        stems = {x["stem"] for x in new}
        kept = [l for l in body[1:] if l.split("\t")[0] not in stems]
        out = head + [body[0]] + kept + ["\t".join(x.get(f, "") for f in fields) for x in new]
        path.write_text("\n".join(out) + "\n", encoding="utf-8")
        record["tables"][name] = sorted(stems)
    (r["dir"] / "wired.json").write_text(json.dumps(record, indent=1) + "\n")
    LOG.mkdir(exist_ok=True)
    (LOG / f"wire-{rid}-{stamp}.json").write_text(json.dumps(record, indent=1) + "\n")
    print(f"installed {len(record['files'])} files, rows {record['tables']}; backup {backup}")


PENDING = QUEUE / "wire-pending.txt"


def worker(lane=None):
    """Work through wire-pending.txt one request at a time (the candidate pack has one writer): exact before views,
    compile, after views. Prints 'ready ID' or 'failed ID: why'; stops after 40 idle minutes. With a lane N it works
    through wire-pending-N.txt in its own candidate-laneN copy of the pack (world textures only: a character compile
    writes shared material files)."""
    global CAND, PENDING
    if lane is not None:
        CAND = OUT / f"candidate-lane{lane}/0-riftapart"
        PENDING = QUEUE / f"wire-pending-{lane}.txt"
        if not CAND.is_dir():
            raise SystemExit(f"lane {lane}: copy the candidate pack to {CAND} first")
    idle = 0.0
    while idle < 2400:
        ids = [l.strip() for l in PENDING.read_text().splitlines() if l.strip()] if PENDING.exists() else []
        if not ids:
            time.sleep(15)
            idle += 15
            continue
        idle = 0.0
        rid = ids[0]
        try:
            r = request(rid)
            if lane is not None and not colour_only(r):
                raise SystemExit("a character request runs in the main worker, not a lane")
            if not (r["dir"] / "views" / "before" / "done").exists():
                reset(rid)
                views(rid, "before")
                (r["dir"] / "views" / "before" / "done").write_text("full PBR before\n")
            compile_(rid)
            views(rid, "after")
            if lane is not None:
                (r["dir"] / "views" / "lane").write_text(f"{lane}\n")
            print("ready", rid, flush=True)
        except (Exception, SystemExit) as e:
            print("failed", rid + ":", str(e)[:300], flush=True)
        ids = [l.strip() for l in PENDING.read_text().splitlines() if l.strip()]
        PENDING.write_text("".join(i + "\n" for i in ids if i != rid))


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("command", choices=["status", "intake", "reset", "compile", "views", "install", "worker"])
    p.add_argument("id", nargs="?")
    p.add_argument("--stage", choices=["before", "after"], default="after")
    p.add_argument("--lane", type=int, help="worker only: run in candidate-laneN with wire-pending-N.txt")
    a = p.parse_args()
    if a.command == "status":
        return status()
    if a.command == "worker":
        return worker(a.lane)
    if not a.id:
        raise SystemExit("an request id is required")
    lane = rdir(a.id) / "views" / "lane"
    if a.command in ("reset", "compile", "views") and lane.exists():
        # A request a worker lane drew is reset, compiled and drawn in that lane's copy of the pack.
        global CAND
        CAND = OUT / f"candidate-lane{lane.read_text().strip()}/0-riftapart"
    if a.command == "views":
        return views(a.id, a.stage)
    dict(intake=intake, reset=reset, compile=compile_, install=install)[a.command](a.id)


if __name__ == "__main__":
    main()
