"""Native identity views for the NPC/enemy catalogue through the existing review renderer.

Uses viewer.Renderer exactly as character_roster.py --render does (isolated review executable, original bind-space
near mesh, normal decode revision 2, saved level light sets), but writes into npc-catalogue/native/ with its own
results file, so other sessions' viewer result files are never touched. Resumable; no game launch.

python scripts/model-review/catalogue_native.py --plan                 # write native/assets.json + native/queue.json
python scripts/model-review/catalogue_native.py --render --level 3     # render one level's queue (run under watch-run.ps1)
"""
import argparse
import csv
import json
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "captures/rift-texture-review"
DEST = OUT / "npc-catalogue/native"
VIEWS = (("front", 35), ("rear", 215))


def assets_template(r, folder, key, atlases):
    return dict(prefixes=",".join(s.split("-")[0] for s in atlases) or "00000", instance=0, level=int(r["level"]),
                poses=str(folder.resolve()), whole=True, source_layer="scene", asset_id=key, class_id=int(r["class_id"]),
                scene_record=int(r["record"]), scene_entry=int(r["slot"]),
                lighting_note="Saved level directional sets; nearest-terrain ambient; not actor-specific light.",
                pose_note="Original bind-space cutscene mesh; identity views only.")


def plan():
    rows = list(csv.DictReader((OUT / "library/uya-mobies/occurrences.tsv").open(encoding="utf-8"), delimiter="\t"))
    by_class = defaultdict(list)
    for r in rows:
        if (int(r["near_joints"]) > 0 or int(r["far_joints"]) > 0) and int(r["near_triangles"]) > 0:
            by_class[int(r["class_id"])].append(r)
    assets, queue, skipped = {}, [], {}
    for class_id, group in sorted(by_class.items()):
        captured = []
        for r in group:
            folder = ROOT / f"captures/gameplay/L{int(r['level']):02}"
            if not list(folder.glob("ram*.bin")):
                # Levels 12 and 20 have no gameplay capture; their saved tour8 frames (fields 18900, 27900) are copied here.
                folder = DEST / f"poses-L{int(r['level'])}"
            if list(folder.glob("ram*.bin")) and list(folder.glob("jobs*.bin")):
                captured.append((r, folder))
        # Every distinct fingerprint of the class, each at its highest-detail captured occurrence.
        best = {}
        for r, folder in captured:
            key = r["asset_id"]
            if key not in best or int(r["near_triangles"]) > int(best[key][0]["near_triangles"]):
                best[key] = (r, folder)
        missing = sorted({r["asset_id"] for r in group} - set(best))
        if missing:
            skipped[str(class_id)] = {"fingerprints": missing, "levels": sorted({int(r["level"]) for r in group if r["asset_id"] in missing}),
                                      "reason": "No saved gameplay capture (ram/jobs) for these levels; the native renderer needs one for light sets"}
        for key, (r, folder) in best.items():
            atlases = [s for s in r["textures"].split(",") if s and not s.startswith("runtime-")]
            name = f"cat_c{class_id}_{key}"
            assets[name] = dict(name=f"Source class {class_id} / {key}", prefixes=",".join(s.split("-")[0] for s in atlases) or "00000",
                                instance=0, level=int(r["level"]), poses=str(folder.resolve()), whole=True, source_layer="source",
                                source_id=int(r["slot"]), asset_id=key, class_id=class_id,
                                lighting_note="Saved level directional sets; nearest-terrain ambient; not actor-specific light.",
                                pose_note="Original bind-space near mesh; identity views only.")
            for view, yaw in VIEWS:
                queue.append(dict(model=name, name=f"{name}-{view}", yaw=yaw, pitch=8, zoom=1.15))
    # Cutscene SCENE classes (catalogue_export inventory): drawn by the review renderer's "@scene:" path,
    # source_id = scene record * 1000 + entry, at the first occurrence on a level with a saved capture.
    scene_rows = [r for r in csv.DictReader((OUT / "npc-catalogue/source-export/inventory.tsv").open(encoding="utf-8"), delimiter="\t")
                  if r["source"] == "SCENE" and int(r["near_triangles"]) > 0]
    seen = set()
    for r in sorted(scene_rows, key=lambda r: (int(r["level"]), int(r["record"]), int(r["slot"]))):
        key = r["asset_id"]
        if key in seen:
            continue
        folder = ROOT / f"captures/gameplay/L{int(r['level']):02}"
        if not list(folder.glob("ram*.bin")):
            folder = DEST / f"poses-L{int(r['level'])}"
        if not (list(folder.glob("ram*.bin")) and list(folder.glob("jobs*.bin"))):
            continue
        seen.add(key)
        atlases = [s for s in r["textures"].split(",") if s and not s.startswith("runtime-")]
        name = f"cat_s{int(r['class_id'])}_{key}"
        assets[name] = dict(name=f"Cutscene class {r['class_id']} / {key}", prefixes=",".join(s.split("-")[0] for s in atlases) or "00000",
                            instance=0, level=int(r["level"]), poses=str(folder.resolve()), whole=True, source_layer="scene",
                            source_id=int(r["record"]) * 1000 + int(r["slot"]), asset_id=key, class_id=int(r["class_id"]),
                            scene_record=int(r["record"]), scene_entry=int(r["slot"]),
                            lighting_note="Saved level directional sets; nearest-terrain ambient; not actor-specific light.",
                            pose_note="Original bind-space cutscene mesh; identity views only.")
        for view, yaw in VIEWS:
            queue.append(dict(model=name, name=f"{name}-{view}", yaw=yaw, pitch=8, zoom=1.15))
        if int(r["class_id"]) == 10:
            # Scene class 10 is drawn with WAD 5 image `class10_texture` (a run-time costume number): one view set per
            # costume number. WAD 5 holds two (costume 0 = Clank's standard atlas 407dd834, 1 = atlas 5e837b1a); numbers 2-11 failed with "WAD 5 has no pair 114".."123" (17 November).
            del assets[name]
            queue[:] = [q for q in queue if q["model"] != name]
            for n in range(2):
                cname = f"{name}_k{n}"
                assets[cname] = dict(assets_template(r, folder, key, atlases), name=f"Cutscene class 10 / {key}, WAD 5 costume {n}",
                                     source_id=n * 100000 + int(r["record"]) * 1000 + int(r["slot"]), costume=n)
                for view, yaw in VIEWS:
                    queue.append(dict(model=cname, name=f"{cname}-{view}", yaw=yaw, pitch=8, zoom=1.15))
    queue.sort(key=lambda q: (assets[q["model"]]["level"], q["model"], q["name"]))
    DEST.mkdir(parents=True, exist_ok=True)
    (DEST / "assets.json").write_text(json.dumps(assets, indent=1) + "\n")
    (DEST / "queue.json").write_text(json.dumps(queue, indent=1) + "\n")
    (DEST / "not-renderable.json").write_text(json.dumps(skipped, indent=1) + "\n")
    levels = defaultdict(int)
    for q in queue:
        levels[assets[q["model"]]["level"]] += 1
    print(json.dumps(dict(classes=len(by_class), fingerprints=len(assets), renders=len(queue), per_level=dict(sorted(levels.items())),
                          classes_with_unrenderable_fingerprints=len(skipped))))


def render(level):
    import sys
    sys.path.insert(0, str(Path(__file__).parent))
    import viewer
    viewer.MODELS = json.loads((DEST / "assets.json").read_text())
    queue = [q for q in json.loads((DEST / "queue.json").read_text()) if viewer.MODELS[q["model"]]["level"] == level]
    args = argparse.Namespace(height=640, export_obj=False, limit_mb=3200, batch=True, requests=True, poses=None)
    renderer = viewer.Renderer(args)
    report = DEST / "results.json"
    records = json.loads(report.read_text()) if report.exists() else {}
    try:
        for q in queue:
            path = DEST / (q["name"] + ".png")
            if q["name"] in records and ("path" in records[q["name"]] or "error" in records[q["name"]]):
                continue
            try:
                renderer.render({**q, "pack": "candidate"}, path)
                records[q["name"]] = dict(path=str(path.relative_to(OUT)), model=q["model"], normal_decode_revision=2)
                print(q["name"] + ": rendered", flush=True)
            except Exception as e:
                records[q["name"]] = dict(model=q["model"], error=str(e))
                print(q["name"] + ": " + str(e), flush=True)
            report.write_text(json.dumps(records, indent=1) + "\n")
    finally:
        renderer.close()


def retry_runtime_skins():
    """Classes drawn only with a runtime skin (ship mode 1, Clank mode 2): the renderer's own 'no matching model'
    answer lists the draws it made; use their texture stems as the selection prefixes and render again."""
    import re
    assets = json.loads((DEST / "assets.json").read_text())
    report = DEST / "results.json"
    records = json.loads(report.read_text())
    levels = set()
    for name, rec in list(records.items()):
        error = rec.get("error", "")
        m = re.search(r"available (\{.*?\}); skipped", error)
        if not m:
            continue
        stems = re.findall(r"\"([0-9a-f-]+)\"", m.group(1))
        if not stems:
            continue
        model = rec["model"]
        assets[model]["prefixes"] = ",".join(sorted({s.split("-")[0] for s in stems}))
        assets[model]["selection_note"] = "Runtime skin only; prefixes are the draws the review renderer reported for this class"
        del records[name]
        levels.add(assets[model]["level"])
    (DEST / "assets.json").write_text(json.dumps(assets, indent=1) + "\n")
    report.write_text(json.dumps(records, indent=1) + "\n")
    print(json.dumps(sorted(levels)))


def retry_untextured():
    """Classes whose only setups are the special -1 set: the renderer reported no draws ("available {}"); select all."""
    assets = json.loads((DEST / "assets.json").read_text())
    report = DEST / "results.json"
    records = json.loads(report.read_text())
    levels = set()
    for name, rec in list(records.items()):
        if "available {}" in rec.get("error", ""):
            model = rec["model"]
            assets[model]["prefixes"] = "*"
            assets[model]["selection_note"] = "Only special setup -1 (no texture name): every draw selected"
            del records[name]
            levels.add(assets[model]["level"])
    (DEST / "assets.json").write_text(json.dumps(assets, indent=1) + "\n")
    report.write_text(json.dumps(records, indent=1) + "\n")
    print(json.dumps(sorted(levels)))


if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("--plan", action="store_true")
    p.add_argument("--render", action="store_true")
    p.add_argument("--retry-runtime-skins", action="store_true")
    p.add_argument("--retry-untextured", action="store_true")
    p.add_argument("--level", type=int)
    a = p.parse_args()
    if a.retry_runtime_skins:
        retry_runtime_skins()
    if a.retry_untextured:
        retry_untextured()
    if a.plan:
        plan()
    if a.render:
        render(a.level)
