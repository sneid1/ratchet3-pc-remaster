"""Image-generation queue shared with the Codex agent: only generation happens there, everything else here.

  python scripts/model-review/imagegen_queue.py plan            # every atlas still needing art -> action and wave
  python scripts/model-review/imagegen_queue.py build --wave 1  # reference sheets + requests for that wave

plan reads the NPC catalogue (npc-catalogue/full/catalogue.json) and decides per atlas stem, from the original
atlases themselves (never names or brightness):
  copy_finished / derive_from_finished  same layout as an already finished atlas: identical pixels (copy its
                                         layers) or the same design in other colours (transfer its detail)
  generate                              a master: needs generated colour + independent height
  copy_master / derive_from_master      same as a generated master: copy, or recolour + shared height
  flat                                  tiny or single-colour swatch: neutral recipe, no generation
Structural equality = edge-map correlation >= 0.8 at 128 px; identical pixels = mean difference <= 3/255 too.
Every copy/derive is re-checked visually in the native views before it counts.

build writes captures/rift-texture-review/authored/imagegen-queue/<id>/reference.png (one or two original
atlases side by side, HD pack colour where it exists, alpha flattened, 512 px per atlas) and appends the
requests to queue.json with the colour, height and retry prompts and the exact output paths.
"""
import argparse
import csv
import hashlib
import json
import re
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "captures/rift-texture-review"
QUEUE = OUT / "authored/imagegen-queue"
DUMPS = Path("C:/Users/<you>/Games/Ratchet and Clank 3/texture_dumps")
EXPORT_TEX = OUT / "npc-catalogue/source-export/tex"
HD = OUT / "profile/texture_replacements/reddyxd-uya"
HERE = Path(__file__).parent

# Wave 1: the named cast and their companions/costumes. Every other master is ranked by plan() into waves 2+.
WAVES = {
    1: ["nefarious", "lawrence", "lawrence_bassist", "courtney", "backup_dancer", "skrunch", "skrunch_qforce", "helga",
        "president", "plumber", "director", "slim", "darla", "big_al_qforce", "skidd", "skidd_robot", "sasha",
        "qwark_hero", "qwark_tribal", "qwark_pajamas", "scorpio", "gary", "helen", "momma_ty", "biobliterator",
        "blackstar", "holostar", "giant_klunk", "clank_standalone", "clank_backpack", "giant_clank", "agent_clank",
        "jeeves"],
}


# Atlases inspected and judged to gain nothing from generated detail.
FLAT_BY_INSPECTION = {"8727e333db00db15": "Slim Cognito's eyes: a smooth 64 px grey gradient; neutral recipe (inspected 17 November)"}
def finished_stems():
    src = (HERE / "imagegen_layers.py").read_text(encoding="utf-8")
    stems = set(re.findall(r"stem='([0-9a-f\-]+)'", src))
    stems |= {c["stem"] for c in json.loads((HERE / "important-npc-materials.json").read_text())}
    # Older passes (jungle Qwark, Ranger, small Tyhrranoid, wasp, Mecha) compiled their atlases through other
    # scripts: every atlas an art-complete fingerprint binds already has finished layers in the candidate pack.
    cat = json.loads((OUT / "npc-catalogue/full/catalogue.json").read_text(encoding="utf-8"))
    stems |= {t for e in cat["entries"] if e["status"] == "art_complete" for t in e["textures"] if not t.startswith("runtime-")}
    return sorted(stems)


def atlas_path(stem):
    for d in (DUMPS, EXPORT_TEX):
        p = d / (stem + ".png")
        if p.exists():
            return p
    raise FileNotFoundError(stem)


def features(stem, size=128):
    a = np.asarray(Image.open(atlas_path(stem)).convert("RGB").resize((size, size), Image.Resampling.BILINEAR), np.float32)
    lum = a.mean(2)
    g = np.abs(np.diff(lum, axis=1))[:-1] + np.abs(np.diff(lum, axis=0))[:, :-1]
    return a, (g - g.mean()) / (g.std() + 1e-6), float(a.reshape(-1, 3).std(0).max())


ENV_MIN_SHARE = 0.002  # the project's world-texture threshold: 0.2% of a level's gameplay picture (docs/surfaces-objects.csv)
WAVE_SIZE = 40         # masters per ranked wave after the named cast
IMPORTANCE = {"enemy": 1.0, "boss": 1.0, "story": 1.0, "companion": 1.0, "environment": 0.8, "museum": 0.5, "ambient": 0.5}
REPLACEMENTS = OUT / "profile/texture_replacements"
RIFT_STEMS = set()


def screen_shares():
    """Per texture: its largest share of a level's gameplay picture and the levels it was measured on."""
    text = (ROOT / "docs/surfaces-objects.csv").read_text(encoding="utf-8").splitlines()
    rows = csv.DictReader(l for l in text if not l.startswith("#"))
    return {r["name"]: dict(share=float(r["share"]), levels=[int(x) for x in r["levels"].split(";") if x], kind=r["kind"],
                            tiling=bool(r["repeat_u"])) for r in rows}


def hd_keys():
    """Texture keys (tex0-clut) every installed HD pack replaces (the Rift Apart candidate excluded)."""
    keys = set()
    for d in REPLACEMENTS.iterdir():
        if d.is_dir() and d.name != "0-riftapart":
            keys |= {"-".join(p.name.split("-")[:2]) for p in d.rglob("*.png")}
    return keys


def quality(stem, size, hd):
    """Current quality, 0 worst .. 1 best: an HD pack replacement, a Rift Apart material, the source resolution."""
    has_hd = "-".join(stem.split("-")[:2]) in hd
    ra = stem in RIFT_STEMS
    return dict(hd=has_hd, rift_apart=ra, resolution=max(size),
                quality=round(0.4 * has_hd + 0.35 * ra + 0.25 * min(1, max(size) / 256), 3))


def plan():
    global RIFT_STEMS
    pack = OUT / "candidate/0-riftapart"
    for t in ("surfaces", "pbr"):
        lines = [l for l in (pack / f"{t}.tsv").read_text().splitlines() if l.strip() and not l.startswith("#")]
        RIFT_STEMS |= {r["stem"] for r in csv.DictReader(lines, delimiter="\t")}
    cat = json.loads((OUT / "npc-catalogue/full/catalogue.json").read_text(encoding="utf-8"))
    done = finished_stems()
    work = [e for e in cat["entries"] if e["status"] == "work_remaining"]
    need = sorted({t for e in work for t in e["textures"] if not t.startswith("runtime-") and t not in done})
    uses = defaultdict(list)
    for e in work:
        for t in e["textures"]:
            if t in need:
                uses[t].append(e)
    feats = {s: features(s) for s in done + need}

    def edge(a, b):
        return float((feats[a][1] * feats[b][1]).mean())

    def diff(a, b):
        return float(np.abs(feats[a][0] - feats[b][0]).mean())

    named = set(WAVES[1])
    actions = {}
    for s in need:
        a = Image.open(atlas_path(s))
        if min(a.size) <= 16 or feats[s][2] < 6:
            actions[s] = dict(action="flat", source=None, reason="tiny or single-colour swatch")
            continue
        if s.split("-")[0] in FLAT_BY_INSPECTION:
            actions[s] = dict(action="flat", source=None, reason=FLAT_BY_INSPECTION[s.split("-")[0]])
            continue
        best = max(done, key=lambda d: edge(s, d))
        e = edge(s, best)
        if e >= 0.8:
            actions[s] = dict(action="copy_finished" if diff(s, best) <= 3 else "derive_from_finished", source=best,
                              edge_correlation=round(e, 3), mean_difference=round(diff(s, best), 2))
    left = [s for s in need if s not in actions]
    # Masters: named cast first, then most-used, so the variant most often seen is the one generated.
    for s in sorted(left, key=lambda s: (not any(e["group"] in named for e in uses[s]), -len(uses[s]), s)):
        if s in actions:
            continue
        actions[s] = dict(action="generate", source=None)
        for t in left:
            if t not in actions and edge(s, t) >= 0.8:
                actions[t] = dict(action="copy_master" if diff(s, t) <= 3 else "derive_from_master", source=s,
                                  edge_correlation=round(edge(s, t), 3), mean_difference=round(diff(s, t), 2))
    for s, a in actions.items():
        a["groups"] = sorted({e["group"] for e in uses[s]})
        a["roles"] = sorted({e["role"] for e in uses[s]})
        a["classes"] = sorted({e["class_id"] for e in uses[s]})
        a["fingerprints"] = sorted({e["asset_id"] for e in uses[s]})
        a["size"] = list(Image.open(atlas_path(s)).size)
        a["hd_source"] = bool(list(HD.glob("-".join(s.split("-")[:2]) + "-*.png")))
        a["named"] = any(g in named for g in a["groups"])

    # Environment textures: every terrain, tie, shrub and object texture above ENV_MIN_SHARE that no character binds.
    shares = screen_shares()
    char_atlases = {t for e in cat["entries"] for t in e["textures"]}
    env = sorted(n for n, r in shares.items() if r["share"] >= ENV_MIN_SHARE and n not in char_atlases and n not in done)
    # Ratchet's own atlases (the review-assets "ratchet" capture) are the player character's: out of this queue.
    player = set(json.loads((HERE / "review-assets.json").read_text(encoding="utf-8"))["ratchet"]["prefixes"].split(","))
    for n in [n for n in env if n.split("-")[0] in player]:
        env.remove(n)
        actions[n] = dict(groups=["player"], roles=["player"], classes=[], fingerprints=[], size=[0, 0], hd_source=False, named=False,
                          action="excluded", source=None, reason="Ratchet's own atlas (player character)")
    # Textures the props pass tagged no surface (skies, which the player deferred to their own panorama method; effects,
    # glows, gradients, decals, flat fills) or water and glass (their own materials) get no texture pass.
    tags = {}
    for line in (ROOT / "textures/riftapart-matches.tsv").read_text(encoding="utf-8").splitlines():
        f = line.split("	")
        if not line.startswith("#") and len(f) > 1 and f[1].split(":")[0] in ("props2 none", "props2 water", "props2 glass"):
            tags[f[0]] = f[1]
    for n in [n for n in env if n in tags]:
        env.remove(n)
        actions[n] = dict(groups=["environment"], roles=["untextured_kind"], classes=[], fingerprints=[], size=[0, 0],
                          hd_source=False, named=False, action="excluded", source=None,
                          reason=f"tagged '{tags[n]}' in textures/riftapart-matches.tsv: no surface for a texture pass")
    for n in [n for n in env if shares[n]["levels"] and min(shares[n]["levels"]) >= 30]:
        env.remove(n)
        actions[n] = dict(groups=["environment"], roles=["vidcomic"], classes=[], fingerprints=[], size=[0, 0], hd_source=False,
                          named=False, action="excluded", source=None,
                          reason="only on vid-comic levels 30-36, which the player dropped (the project’s plan 9.2)")
    for n in [n for n in env if not any((d / (n + ".png")).exists() for d in (DUMPS, EXPORT_TEX))]:
        env.remove(n)
        actions[n] = dict(groups=["environment"], roles=["environment"], classes=[], fingerprints=[], size=[0, 0], hd_source=False,
                          named=False, action="no_source", source=None,
                          reason="on screen but no original dump of this texture exists to give the generator")
    efeat = {}
    for n in env:
        x = np.asarray(Image.open(atlas_path(n)).convert("RGB").resize((64, 64), Image.Resampling.BILINEAR), np.float32)
        lum = x.mean(2)
        g = np.abs(np.diff(lum, axis=1))[:-1] + np.abs(np.diff(lum, axis=0))[:, :-1]
        efeat[n] = (x.ravel(), ((g - g.mean()) / (g.std() + 1e-6)).ravel(), float(x.reshape(-1, 3).std(0).max()))
    order = sorted(env, key=lambda n: -shares[n]["share"])
    G = np.stack([efeat[n][1] for n in order])
    E = (G @ G.T) / G.shape[1]
    for i, n in enumerate(order):
        if n in actions:
            continue
        size = list(Image.open(atlas_path(n)).size)
        base = dict(groups=["environment"], roles=["environment"], classes=[], fingerprints=[], size=size, hd_source=False,
                    named=False, kind=shares[n]["kind"], tiling=shares[n]["tiling"], levels=shares[n]["levels"])
        if min(size) <= 16 or efeat[n][2] < 6:
            actions[n] = dict(base, action="flat", source=None, reason="tiny or single-colour swatch")
            continue
        actions[n] = dict(base, action="generate", source=None)
        for j in range(i + 1, len(order)):
            t = order[j]
            if t not in actions and E[i, j] >= 0.8:
                d = float(np.abs(efeat[n][0] - efeat[t][0]).mean())
                actions[t] = dict(base, kind=shares[t]["kind"], tiling=shares[t]["tiling"], levels=shares[t]["levels"],
                                  size=list(Image.open(atlas_path(t)).size), action="copy_master" if d <= 3 else "derive_from_master",
                                  source=n, edge_correlation=round(float(E[i, j]), 3), mean_difference=round(d, 2))

    # Priority of every master = importance x frequency x (1.25 - current quality). Frequency is ranked within its own
    # category (characters against characters, environment against environment), because a wall fills more of the
    # picture than any enemy: environment by measured on-screen share (the master's and every variant's it covers),
    # characters by that share and by how widely they appear (levels, classes, fingerprints using the atlas).
    hd = hd_keys()
    covered = defaultdict(list)
    for s, a in actions.items():
        covered[a["source"] if a["action"] in ("copy_master", "derive_from_master") else s].append(s)
    masters = [s for s, a in actions.items() if a["action"] == "generate"]
    stats = {}
    for s in masters:
        group = covered[s]
        share = sum(shares.get(t, {}).get("share", 0.0) for t in group)
        levels = {l for t in group for l in shares.get(t, {}).get("levels", [])}
        levels |= {o["level"] for t in group for e in uses.get(t, []) for o in e["occurrences"]}
        spread = len({e["asset_id"] for t in group for e in uses.get(t, [])}) + len({e["class_id"] for t in group for e in uses.get(t, [])})
        stats[s] = dict(share=share, levels=levels, spread=spread)

    def percentile(values):
        order = sorted(values)
        return {v: (sum(1 for x in order if x < v) + 0.5 * sum(1 for x in order if x == v)) / len(order) for v in set(order)}

    for env_side in (True, False):
        group = [s for s in masters if (actions[s]["groups"] == ["environment"]) == env_side]
        if not group:
            continue
        ps = percentile([stats[s]["share"] for s in group])
        pl = percentile([len(stats[s]["levels"]) for s in group])
        pc = percentile([stats[s]["spread"] for s in group])
        for s in group:
            st = stats[s]
            freq = ps[st["share"]] if env_side else (ps[st["share"]] + pl[len(st["levels"])] + pc[st["spread"]]) / 3
            a = actions[s]
            imp = max(IMPORTANCE[r] for r in a["roles"])
            q = quality(s, a["size"], hd)
            a.update(q, screen_share=round(st["share"], 5), levels_seen=sorted(st["levels"]), spread=st["spread"],
                     frequency=round(freq, 3), importance=imp, priority=round(imp * (0.1 + freq) * (1.25 - q["quality"]), 4))
    ranked = sorted((s for s, a in actions.items() if a["action"] == "generate" and not a["named"]),
                    key=lambda s: (-actions[s]["priority"], s))
    for s, a in actions.items():
        if a["action"] == "generate" and a["named"]:
            a["wave"], a["rank"] = 1, 0
    for i, s in enumerate(ranked):
        actions[s]["wave"], actions[s]["rank"] = 2 + i // WAVE_SIZE, i + 1
    for s, a in actions.items():
        if a["action"] in ("copy_master", "derive_from_master"):
            a["wave"] = actions[a["source"]]["wave"]
        elif a["action"] in ("excluded", "no_source"):
            a["wave"] = None
        elif "wave" not in a:
            a["wave"] = 1 if a["named"] else 2
    QUEUE.mkdir(parents=True, exist_ok=True)
    report = dict(date="2026-11-17", atlases=len(actions), finished_atlases=len(done),
                  priority=("wave 1 = named cast; later waves = every other master (enemies, museum, ambient, environment "
                            f"textures above {ENV_MIN_SHARE:.1%} of a level's picture) ranked by importance x frequency x "
                            f"(1.25 - quality), {WAVE_SIZE} masters a wave"),
                  counts=dict(Counter(a["action"] for a in actions.values())),
                  environment_counts=dict(Counter(a["action"] for a in actions.values() if a["groups"] == ["environment"])),
                  generate_by_wave=dict(sorted(Counter(a["wave"] for a in actions.values() if a["action"] == "generate").items())),
                  not_queued={n: a["reason"] for n, a in actions.items() if a["action"] in ("excluded", "no_source")},
                  top_ranked=[dict(stem=s, groups=actions[s]["groups"], kind=actions[s].get("kind"), priority=actions[s]["priority"],
                                   frequency=actions[s]["frequency"], share=actions[s]["screen_share"], quality=actions[s]["quality"])
                              for s in ranked[:25]],
                  actions=actions)
    (QUEUE / "plan.json").write_text(json.dumps(report, indent=1) + "\n")
    print(json.dumps({k: report[k] for k in ("atlases", "counts", "environment_counts", "generate_by_wave")}, indent=1))


def reference_tile(stem, size=(512, 512)):
    """The best available original colour of one atlas, alpha flattened, resized to size."""
    hd = sorted(HD.glob("-".join(stem.split("-")[:2]) + "-*.png"))
    src = hd[0] if len(hd) == 1 else atlas_path(stem)
    im = Image.open(src).convert("RGBA")
    rgb = Image.new("RGB", im.size, (96, 96, 96))
    rgb.paste(im.convert("RGB"), mask=im.getchannel("A").point(lambda v: 255 if v >= 64 else 0))
    resample = Image.Resampling.LANCZOS if src.parent == HD else Image.Resampling.NEAREST
    return rgb.resize(size, resample), src


def sheets(wave, actions, queued):
    """The wave's sheets: a character's master atlases in pairs (one subject per sheet); every environment master on its
    own sheet at its own aspect ratio, because a tiling texture's edges must meet only themselves."""
    out = []
    by_owner = defaultdict(list)
    for s, a in actions.items():
        if a["action"] != "generate" or a["wave"] != wave or s in queued:
            continue
        if a["groups"] == ["environment"]:
            out.append(dict(owner=f"env_{a['kind']}_{s[:16]}", key=s[:16], environment=True, atlases=[s]))
        else:
            by_owner[a["groups"][0] + "_c" + str(a["classes"][0])].append(s)
    for owner, stems in by_owner.items():
        stems = sorted(stems)
        out += [dict(owner=owner, key=owner, environment=False, atlases=stems[k:k + 2], part=k // 2 + 1)
                for k in range(0, len(stems), 2)]
    return sorted(out, key=lambda x: (x["environment"], x["owner"], x.get("part", 1)))


def build(wave):
    plan_ = json.loads((QUEUE / "plan.json").read_text())
    actions = plan_["actions"]
    desc = json.loads((HERE / "imagegen-descriptions.json").read_text(encoding="utf-8"))
    queue_path = QUEUE / "queue.json"
    queue = json.loads(queue_path.read_text()) if queue_path.exists() else dict(requests=[])
    queued = {s for r in queue["requests"] for s in r["atlases"]}
    todo = sheets(wave, actions, queued)
    missing = sorted({(x["owner"], x["key"], tuple(x["atlases"])) for x in todo
                      if x["key"] not in desc["environment" if x["environment"] else "subjects"]})
    if missing:
        raise SystemExit("No description written for these (add them to imagegen-descriptions.json: characters under "
                         "\"subjects\" by owner, environment under \"environment\" by the 16-digit key):\n"
                         + "\n".join(f"  {o}  key {k}  atlases {', '.join(a)}" for o, k, a in missing))
    templates = desc["templates"]
    # A description with "skip" (a smooth gradient, glow or featureless swatch found when it was written) is not sent
    # for generation: there is nothing in it for a generator to restore, only detail to invent.
    skipped = [x["key"] for x in todo if desc["environment" if x["environment"] else "subjects"][x["key"]].get("skip")]
    todo = [x for x in todo if x["key"] not in skipped]
    added = []
    for x in todo:
        pair = x["atlases"]
        if x["environment"]:
            a = actions[pair[0]]
            w, h = a["size"]
            px = (1024, max(256, round(1024 * h / w))) if w >= h else (max(256, round(1024 * w / h)), 1024)
            rid = f"w{wave}-{x['owner']}"
            d = desc["environment"][x["key"]]
            layout = f"single {w}:{h} environment texture" if w != h else "single square environment texture"
            size = f"{px[0]}x{px[1]}"
            fill = dict(subject=d["subject"], layout=layout, size=size, materials=d["materials"], preserve=d["preserve"],
                        height=d["height"], avoid=d.get("avoid", ""),
                        arrangement=templates["env_tiling"] if a["tiling"] else templates["env_atlas"],
                        tiling=templates["env_tiling_height"] if a["tiling"] else "")
            prefix, extra = "env_", dict(classes=[], kind=a["kind"], tiling=a["tiling"], levels=a["levels_seen"],
                                         priority=a["priority"], rank=a["rank"])
            tiles = [reference_tile(pair[0], px)]
        else:
            rid = f"w{wave}-{x['owner']}-{x['part']}"
            d = desc["subjects"][x["owner"]]
            layout = "TWO-SQUARE 2:1 landscape sheet (left atlas | right atlas)" if len(pair) == 2 else "single square atlas"
            size = "2048x1024" if len(pair) == 2 else "1024x1024"
            fill = dict(subject=d["subject"], layout=layout, size=size, materials=d["materials"],
                        preserve=d["preserve"], height=d["height"], avoid=d.get("avoid", ""))
            prefix, extra = "", dict(classes=actions[pair[0]]["classes"])
            tiles = [reference_tile(t) for t in pair]
        folder = QUEUE / rid
        folder.mkdir(parents=True, exist_ok=True)
        sheet = Image.new("RGB", (sum(t.width for t, _ in tiles), tiles[0][0].height))
        for i, (t, _) in enumerate(tiles):
            sheet.paste(t, (512 * i, 0))
        sheet.save(folder / "reference.png")
        req = dict(id=rid, wave=wave, owner=x["owner"], atlases=pair, **extra,
                   reference=str((folder / "reference.png").relative_to(ROOT)).replace("\\", "/"),
                   reference_sources=[str(src) for _, src in tiles],
                   reference_sha256=hashlib.sha256((folder / "reference.png").read_bytes()).hexdigest(),
                   layout=layout, output_size=size,
                   colour_output=str((folder / "colour.png").relative_to(ROOT)).replace("\\", "/"),
                   height_output=str((folder / "height.png").relative_to(ROOT)).replace("\\", "/"),
                   colour_prompt=templates[prefix + "colour"].format(**fill),
                   height_prompt=templates[prefix + "height"].format(**fill),
                   height_retry_prompt=templates[prefix + "height_retry"].format(**fill),
                   status="requested", codex=dict(colour_source=None, height_source=None, notes=None),
                   intake=None)
        queue["requests"].append(req)
        added.append(rid)
    queue_path.write_text(json.dumps(queue, indent=1) + "\n")
    print(json.dumps(dict(wave=wave, added=len(added), skipped=sorted(set(skipped)), requests=len(queue["requests"]))))


def todo(wave):
    """The descriptions a wave still needs, as labelled contact sheets for whoever writes them: descriptions/
    w<N>-todo.json (one entry per description key) and w<N>-sheet-<k>.png (each key's original atlases, labelled)."""
    from PIL import ImageDraw, ImageFont
    plan_ = json.loads((QUEUE / "plan.json").read_text())
    actions = plan_["actions"]
    desc = json.loads((HERE / "imagegen-descriptions.json").read_text(encoding="utf-8"))
    queue_path = QUEUE / "queue.json"
    queued = {s for r in json.loads(queue_path.read_text())["requests"] for s in r["atlases"]} if queue_path.exists() else set()
    titles = {}
    for line in (HERE / "npc-class-identities.psv").read_text(encoding="utf-8").splitlines():
        if line.startswith("@group |"):
            f = [x.strip() for x in line.split("|")]
            titles[f[1]] = f[3]
    keys = {}
    for x in sheets(wave, actions, queued):
        if x["key"] in desc["environment" if x["environment"] else "subjects"]:
            continue
        k = keys.setdefault(x["key"], dict(key=x["key"], environment=x["environment"], atlases=[]))
        k["atlases"] += x["atlases"]
    out = QUEUE / "descriptions"
    out.mkdir(exist_ok=True)
    font = ImageFont.truetype("arial.ttf", 18)
    entries, rows = [], []
    for k in keys.values():
        a = actions[k["atlases"][0]]
        if k["environment"]:
            k.update(kind=a["kind"], tiling=a["tiling"], size=a["size"], levels=a["levels_seen"],
                     title=f"world texture ({a['kind']}{', tiling' if a['tiling'] else ', object atlas'})")
            w, h = a["size"]
            px = (256, max(64, round(256 * h / w))) if w >= h else (max(64, round(256 * w / h)), 256)
        else:
            group = a["groups"][0]
            k.update(group=group, title=titles.get(group, group), classes=sorted({c for s in k["atlases"] for c in actions[s]["classes"]}),
                     roles=a["roles"], levels=sorted({l for s in k["atlases"] for l in actions[s]["levels_seen"]}))
            px = (256, 256)
        tiles = [reference_tile(s, px)[0] for s in k["atlases"]]
        label = f"{k['key']}  -  {k['title']}  -  levels {','.join(map(str, k['levels']))}"
        for i in range(0, len(tiles), 6):
            chunk = tiles[i:i + 6]
            row = Image.new("RGB", (max(1560, sum(t.width + 8 for t in chunk)), max(t.height for t in chunk) + 30), (24, 24, 28))
            ImageDraw.Draw(row).text((4, 4), label + (f"  (atlases {i + 1}-{i + len(chunk)} of {len(tiles)})" if len(tiles) > 6 else ""),
                                     fill=(255, 230, 120), font=font)
            x0 = 0
            for t in chunk:
                row.paste(t, (x0, 30))
                x0 += t.width + 8
            rows.append((k["key"], row))
        entries.append(k)
    sheet_no, page, height = 1, [], 0
    def flush(page, n):
        if not page:
            return
        img = Image.new("RGB", (max(r.width for _, r in page), sum(r.height + 6 for _, r in page)), (40, 40, 44))
        y = 0
        for key, r in page:
            img.paste(r, (0, y))
            y += r.height + 6
        img.save(out / f"w{wave}-sheet-{n}.png")
        for key, _ in page:
            for e in entries:
                if e["key"] == key:
                    e.setdefault("sheets", [])
                    if f"w{wave}-sheet-{n}.png" not in e["sheets"]:
                        e["sheets"].append(f"w{wave}-sheet-{n}.png")
    for key, r in rows:
        if height + r.height > 1700 and page:
            flush(page, sheet_no)
            sheet_no, page, height = sheet_no + 1, [], 0
        page.append((key, r))
        height += r.height + 6
    flush(page, sheet_no)
    (out / f"w{wave}-todo.json").write_text(json.dumps(entries, indent=1) + "\n")
    print(f"wave {wave}: {len(entries)} descriptions to write, {sheet_no if rows else 0} sheets")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("command", choices=["plan", "build", "todo"])
    p.add_argument("--wave", type=int)
    a = p.parse_args()
    dict(plan=lambda: plan(), build=lambda: build(a.wave), todo=lambda: todo(a.wave))[a.command]()
