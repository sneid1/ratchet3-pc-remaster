"""Complete NPC/enemy catalogue: every jointed LEVEL class and every SCENE (cutscene) class, with identity, evidence,
existing work, associated files and status. Reads existing inventories, the offline source export and the native
identity views; renders nothing and changes no asset.

Inputs (all must exist; a class of the source export missing from npc-class-identities.psv is an error):
  scripts/model-review/npc-class-identities.psv              authored identities, groups and evidence
  captures/rift-texture-review/npc-catalogue/source-export   catalogue_export inventory, problems, OBJ/MTL/PNG, mesh signatures
  captures/rift-texture-review/npc-catalogue/native          catalogue_native.py front/rear views (normal decode revision 2)
  review-assets.json, important-npc-assets.json, art-finishing-ledger.json, character-roster.json, deliverables/
Outputs:
  captures/rift-texture-review/npc-catalogue/full/catalogue.json, catalogue.tsv, entries/<group>.json, index.html, thumbs/
  docs/NPC-ENEMY-CATALOGUE.md

python scripts/model-review/catalogue_full.py
"""
import csv
import html
import json
import os
from collections import Counter, defaultdict
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "captures/rift-texture-review"
CAT = OUT / "npc-catalogue"
EXPORT = CAT / "source-export"
NATIVE = CAT / "native"
DEST = CAT / "full"
HERE = Path(__file__).parent

STATUS = {
    "art_complete": "Art complete",
    "reusable": "Existing art verified reusable",
    "work_remaining": "Work remaining",
    "unresolved": "Source/identity unresolved",
    "out_of_scope": "Out of scope",
}
OUT_ROLES = {
    "vidcomic": "Vid-comic levels 30-36 were dropped by the player (PLAN.md 9.2; docs/HANDOVER.md, Rift Apart remaster).",
    "summon": "Created by the player's weapons; a weapon effect, not an NPC or enemy.",
    "player": "Player gear, weapon or armour model; not an NPC or enemy.",
    "vehicle": "Vehicle without a character mesh (enemy craft are catalogued as enemies).",
    "prop": "Jointed prop or world object, not a character.",
}
# Captured assemblies whose atlases other classes also bind: the Ranger capture is the attached upper/lower pair
# (review-assets.json selection_evidence); class 6593 binds the legs atlas too but is a separate small robot.
CAPTURED_PAIRS = {"ranger": {(5542, "a898d292f748418b"), (6886, "30ceb696a79d979a")}}
# Deliverable folders that do not name their model in a manifest.
FOLDER_MODEL = {"florana_creature": "florana_creature", "mecha-body": "source_c6306", "mecha-gear": "source_c6306",
                "phoenix_crew": "phoenix_crew", "ranger": "ranger", "small_ty_near": "small_ty_near"}


def rel(p):
    return Path(p).resolve().relative_to(ROOT).as_posix()


def table(path):
    with path.open(encoding="utf-8") as f:
        return list(csv.DictReader(f, delimiter="\t"))


def read_identities():
    groups, classes = {}, {}
    for line in (HERE / "npc-class-identities.psv").read_text(encoding="utf-8").splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        cells = [c.strip() for c in line.split("|")]
        if cells[0] == "@group":
            _, gid, section, title, note = cells
            assert gid not in groups, gid
            groups[gid] = dict(id=gid, section=section, title=title, note=note)
        else:
            cid, gid, role, confidence, identity, note = cells
            assert int(cid) not in classes, cid
            classes[int(cid)] = dict(group=gid, role=role, confidence=confidence, identity=identity, note=note)
    for c, v in classes.items():
        assert v["group"] in groups, (c, v["group"])
        assert v["confidence"] in ("manifest", "visual+level", "visual", "signature", "unviewed"), (c, v)
    return groups, classes


def existing_work():
    """Map exact (class, fingerprint) pairs to the existing reviewed asset keys, completion and files."""
    manifest = json.loads((HERE / "review-assets.json").read_text(encoding="utf-8"))
    manifest.update(json.loads((OUT / "important-npc-assets.json").read_text(encoding="utf-8")))
    ledger = json.loads((OUT / "art-finishing-ledger.json").read_text(encoding="utf-8"))
    complete = {a["model"]: a for a in ledger["art_complete"]}
    occ = table(OUT / "library/uya-mobies/occurrences.tsv")
    work = defaultdict(list)
    for key, m in manifest.items():
        if m.get("source_layer") in ("terrain", "shrub", "body"):
            continue
        pairs = set()
        if m.get("class_id") is not None and m.get("asset_id"):
            pairs.add((m["class_id"], m["asset_id"]))
        elif m.get("class_id") is not None:
            pairs |= {(int(o["class_id"]), o["asset_id"]) for o in occ if int(o["class_id"]) == m["class_id"]
                      and int(o["level"]) == m["level"] and int(o["slot"]) == m["source_id"]}
        elif key in CAPTURED_PAIRS:
            pairs |= CAPTURED_PAIRS[key]
        else:
            prefixes = m["prefixes"].split(",")
            for o in occ:
                if int(o["level"]) != m["level"]:
                    continue
                stems = [s.split("-")[0] for s in o["textures"].split(",") if s]
                if any(p in stems for p in prefixes):
                    pairs.add((int(o["class_id"]), o["asset_id"]))
        files = []
        for d in sorted((OUT / "deliverables/imagegen-layers").iterdir()):
            if not d.is_dir():
                continue
            models = set()
            for j in d.glob("*.json"):
                try:
                    data = json.loads(j.read_text(encoding="utf-8"))
                except ValueError:
                    continue
                for x in data if isinstance(data, list) else [data]:
                    if isinstance(x, dict) and "model" in x:
                        models.add(str(x["model"]))
            if key in models or FOLDER_MODEL.get(d.name) == key:
                files.append(rel(d))
        for f in [OUT / f"deliverables/imagegen-layers/models/{key}.blend"] + sorted((OUT / "blender/edited").glob(key + ".*")):
            if f.is_file() and not f.name.endswith(".blend1"):
                files.append(rel(f))
        state = "art pass complete" if key in complete else ("detailed layers delivered; no ledger completion"
                                                             if files else "reviewed in the workbench only")
        for pair in pairs:
            work[pair].append(dict(key=key, name=m["name"], state=state, complete=key in complete,
                                   evidence=complete.get(key, {}).get("evidence", "").replace("\\", "/"),
                                   limits=complete.get(key, {}).get("limits", ""), files=files))
    return work


def thumb(src, dst, height=230):
    if dst.exists() and dst.stat().st_mtime >= src.stat().st_mtime:
        return
    im = Image.open(src).convert("RGB")
    box = im.point(lambda v: 255 if v > 10 else 0).getbbox()
    if box:
        im = im.crop(box)
    im.thumbnail((int(height * 1.4), height))
    dst.parent.mkdir(parents=True, exist_ok=True)
    im.save(dst, "JPEG", quality=86)


def main():
    groups, ids = read_identities()
    inv = table(EXPORT / "inventory.tsv")
    problems = table(EXPORT / "problems.tsv")
    signatures = json.loads((EXPORT / "geometry-signatures.json").read_text())
    assets = json.loads((NATIVE / "assets.json").read_text())
    results = json.loads((NATIVE / "results.json").read_text())
    unrenderable = json.loads((NATIVE / "not-renderable.json").read_text())
    roster = {a["asset_id"]: a for a in json.loads((OUT / "character-roster.json").read_text())["assets"]}
    work = existing_work()

    views, costume_views = {}, {}
    for name, a in assets.items():
        rec = {v: results.get(f"{name}-{v}", {}) for v in ("front", "rear")}
        if a.get("costume"):
            # Scene class 10's further WAD 5 costumes: extra views beside costume 0's.
            costume_views.setdefault((a["class_id"], a["asset_id"]), []).append((a["costume"], rec))
            continue
        views[(a["class_id"], a["asset_id"])] = rec

    # One record per (class, fingerprint).
    recs = {}
    for r in inv:
        cid = int(r["class_id"])
        joints = max(int(r["near_joints"]), int(r["far_joints"]))
        if not ((joints > 0 and int(r["near_triangles"]) > 0) or r["source"] == "SCENE"):
            continue
        if cid not in ids:
            raise SystemExit(f"class {cid} ({r['source']} level {r['level']}) is not in npc-class-identities.psv")
        key = (cid, r["asset_id"])
        rec = recs.setdefault(key, dict(class_id=cid, asset_id=r["asset_id"], sources=set(), occurrences=[],
                                        triangles=dict(near=int(r["near_triangles"]), far=int(r["far_triangles"]),
                                                       third=int(r["third_triangles"])),
                                        meshes=int(r["meshes"]), skinned_meshes=int(r["skinned_meshes"]),
                                        rigid_meshes=int(r["rigid_meshes"]),
                                        joints=dict(near=int(r["near_joints"]), far=int(r["far_joints"])),
                                        textures=[t for t in r["textures"].split(",") if t], export=r["exported"],
                                        bbox=[r["bbox_min"], r["bbox_max"]]))
        rec["sources"].add(r["source"])
        if r["source"] == "LEVEL":
            rec["occurrences"].append(dict(container="LEVEL core", level=int(r["level"]), slot=int(r["slot"])))
        else:
            rec["occurrences"].append(dict(container="SCENE block", level=int(r["level"]), scene_record=int(r["record"]),
                                           entry=int(r["slot"])))
    unused = set(ids) - {c for c, _ in recs}
    if unused:
        raise SystemExit(f"identities for classes absent from the source export: {sorted(unused)}")

    completed = {k: w for k, ws in work.items() for w in ws if w["complete"]}
    complete_sig = {signatures[fp]["mesh_and_materials"]: (c, fp) for (c, fp) in completed if fp in signatures}
    mesh_peers = defaultdict(set)
    for (c, fp) in recs:
        if fp in signatures:
            mesh_peers[signatures[fp]["mesh"]].add((c, fp))
    fp_classes = defaultdict(set)
    for (c, fp) in recs:
        fp_classes[fp].add(c)

    DEST.mkdir(parents=True, exist_ok=True)
    entries = []
    for (cid, fp), rec in sorted(recs.items()):
        ident = ids[cid]
        role = ident["role"]
        sig = signatures.get(fp)
        files = {}
        if rec["export"]:
            obj = EXPORT / rec["export"]
            files["source_obj"] = rel(obj)
            files["source_mtl"] = rel(obj.with_suffix(".mtl"))
            if obj.with_suffix(".far.obj").exists():
                files["source_far_lod_obj"] = rel(obj.with_suffix(".far.obj"))
            files["source_textures"] = [rel(EXPORT / "tex" / (line.split("/")[-1].strip()))
                                        for line in obj.with_suffix(".mtl").read_text().splitlines() if line.startswith("map_Kd")]
        v = views.get((cid, fp))
        view_note = None
        for n, rec_c in costume_views.get((cid, fp), []):
            for x in ("front", "rear"):
                if "path" in rec_c[x]:
                    files[f"native_{x}_costume{n}"] = rel(OUT / rec_c[x]["path"])
        if v and all("path" in v[x] for x in ("front", "rear")):
            for x in ("front", "rear"):
                src = OUT / v[x]["path"]
                files[f"native_{x}"] = rel(src)
                t = DEST / "thumbs" / f"c{cid}_{fp}_{x}.jpg"
                thumb(src, t)
                files[f"thumb_{x}"] = rel(t)
        elif v:
            view_note = "Review renderer drew nothing: " + v["front"].get("error", "no record")[:160]
        elif "SCENE" in rec["sources"] and "LEVEL" not in rec["sources"]:
            view_note = "Cutscene SCENE class: the review renderer has no SCENE source path, so there is no native view."
        else:
            levels = sorted({o["level"] for o in rec["occurrences"]})
            view_note = (f"Only on level(s) {levels}, which have no saved gameplay capture (ram/jobs); the review renderer "
                         "needs one for light sets and game launches are excluded.")
        old = OUT / f"roster-review/source_c{cid}.png"
        if old.exists():
            files["superseded_preview"] = rel(old)
        existing = work.get((cid, fp), [])
        for w in existing:
            files.setdefault("existing_work", []).extend(w["files"])
        peers = sorted(f"c{c}/{p}" for c, p in mesh_peers.get(sig["mesh"], ()) if (c, p) != (cid, fp)) if sig else []
        # Status.
        if role in OUT_ROLES:
            status, remaining = "out_of_scope", OUT_ROLES[role]
        elif (cid, fp) in completed:
            w = completed[(cid, fp)]
            status, remaining = "art_complete", f"None for this documented configuration ({w['key']}). Animation and game integration are separate."
        elif any(fp == p for (_, p) in completed):
            src = next(f"c{c}" for (c, p) in completed if p == fp)
            status, remaining = "reusable", f"Identical source fingerprint to completed {src}: reuse its delivery unchanged; record completion for this class after a view check."
        elif sig and sig["mesh_and_materials"] in complete_sig:
            c2, p2 = complete_sig[sig["mesh_and_materials"]]
            status, remaining = "reusable", (f"Exported mesh, UVs and texture bindings identical to completed c{c2}/{p2} "
                                             "(fingerprint differs only in rig/packet data): reuse its layers; check the rig before integration.")
        elif ident["confidence"] == "unviewed" or role == "unknown":
            status, remaining = "unresolved", "Identity not settled. Blocker: " + (view_note or ident["note"] or "no native view")
        else:
            status = "work_remaining"
            same_mesh_done = [f"c{c}/{p}" for (c, p) in completed if signatures.get(p, {}).get("mesh") == (sig or {}).get("mesh")]
            if same_mesh_done:
                remaining = (f"Mesh identical to completed {', '.join(same_mesh_done)}; materials differ: adapt the completed "
                             "layers to this class's atlases and review front/rear.")
            elif existing and any(w["files"] for w in existing):
                remaining = ("Existing detailed work (" + ", ".join(w["key"] for w in existing) + ") without a ledger "
                             "completion: finish the art review against the documented configuration.")
            else:
                remaining = ("Individual art pass: fitted detailed colour and independent height, material masks/finishes, "
                             "source-face emission policy where shared UVs need it, editable .blend, front/rear/close views.")
        pbr = roster.get(fp)
        entries.append(dict(
            group=ident["group"], class_id=cid, asset_id=fp, identity=ident["identity"], role=role,
            identity_confidence=ident["confidence"], identity_note=ident["note"],
            status=status, status_label=STATUS[status], remaining=remaining,
            source_containers=sorted(rec["sources"]), occurrences=rec["occurrences"],
            levels=sorted({o["level"] for o in rec["occurrences"]}),
            triangles=rec["triangles"], joints=rec["joints"], meshes=rec["meshes"],
            skinned_meshes=rec["skinned_meshes"], rigid_meshes=rec["rigid_meshes"],
            lods=dict(near_mesh_records="near run", far_triangles=rec["triangles"]["far"], third_triangles=rec["triangles"]["third"]),
            textures=rec["textures"], runtime_materials=[t for t in rec["textures"] if t.startswith("runtime-")],
            pbr_mapped_atlases=pbr["pbr_atlases"] if pbr else None, pbr_unmapped_atlases=pbr["unmapped_pbr"] if pbr else None,
            bind_bbox=rec["bbox"], mesh_signature=sig, identical_mesh_peers=peers,
            other_classes_with_same_fingerprint=sorted(fp_classes[fp] - {cid}),
            existing_work=[{k: w[k] for k in ("key", "name", "state", "evidence", "limits")} for w in existing],
            view_blocker=view_note, files=files))

    # Group roll-up, including roster names that have no local class.
    by_group = defaultdict(list)
    for e in entries:
        by_group[e["group"]].append(e)
    group_rows = []
    order = list(groups)
    for gid in order:
        g = groups[gid]
        es = by_group.get(gid, [])
        counts = Counter(e["status"] for e in es)
        if not es:
            gstatus = "unresolved"
        elif set(counts) == {"out_of_scope"}:
            gstatus = "out_of_scope"
        elif counts.get("work_remaining"):
            gstatus = "work_remaining"
        elif counts.get("unresolved"):
            gstatus = "unresolved"
        elif counts.get("art_complete"):
            gstatus = "art_complete"  # every in-scope fingerprint complete or verified reusable
        else:
            gstatus = "reusable"
        group_rows.append(dict(g, status=gstatus, status_label=STATUS[gstatus], fingerprint_statuses=dict(counts),
                               classes=sorted({e["class_id"] for e in es}), fingerprints=len(es),
                               entries=[f"c{e['class_id']}/{e['asset_id']}" for e in es]))

    no_data = defaultdict(set)
    for p in problems:
        if p["problem"] == "Class has no decoded mesh data":
            no_data[int(p["class_id"])].add(int(p["level"]))
    mp = sorted(int(p["level"]) for p in problems if "multiplayer" in p["problem"])
    gaps = dict(
        class_records_without_core_data=dict(
            classes=len(no_data), slots=sum(len(v) for v in no_data.values()),
            note=("Class records whose core offset is 0: the LEVEL core holds no mesh for them. The hero body, armour, wrench "
                  "and held gadget are loaded from WADs (uyafmt levelcode/scenecode), and others may be unused records. "
                  "Not identified as characters; no mesh to view."),
            by_class={str(c): sorted(v) for c, v in sorted(no_data.items())}),
        multiplayer_levels=dict(levels=mp, note="uyafmt walk::level refuses multiplayer levels; their classes and skins are not decoded. Not counted as gameplay characters."),
        scene_blocks=dict(rows=sum(1 for r in inv if r["source"] == "SCENE"),
                          fingerprints=len({r["asset_id"] for r in inv if r["source"] == "SCENE"}),
                          note="All story levels' SCENE records decoded offline (uyafmt scene::load); views are the remaining gap."),
        unrenderable_native=unrenderable)

    counts = Counter(e["status"] for e in entries)
    in_scope = [e for e in entries if e["status"] != "out_of_scope"]
    summary = dict(
        date="2026-11-17",
        classes=len({e["class_id"] for e in entries}), fingerprints=len(entries),
        fingerprint_status=dict(counts), groups=len(group_rows),
        group_status=dict(Counter(g["status"] for g in group_rows)),
        in_scope_classes=len({e["class_id"] for e in in_scope}),
        native_views=sum(1 for e in entries if "native_front" in e["files"]),
        level_fingerprints_total=len({r["asset_id"] for r in inv if r["source"] == "LEVEL"}),
        level_occurrences_total=sum(1 for r in inv if r["source"] == "LEVEL"))
    report = dict(summary=summary, statuses=STATUS, out_of_scope_reasons=OUT_ROLES, groups=group_rows,
                  entries=entries, gaps=gaps,
                  method=("Identity from native review-renderer views (catalogue_native.py; original bind-space near mesh, "
                          "normal decode revision 2), level context, texture-prefix matches to existing reviewed assets, exact "
                          "fingerprints and exported-mesh signatures (catalogue_export, whose LEVEL fingerprints reproduce "
                          "occurrences.tsv exactly). Completion is per documented (class, fingerprint) configuration."),
                  reproduce=["cargo build --release --offline (scripts/model-review/catalogue_export) and run it on the disc",
                             "python scripts/model-review/catalogue_native.py --plan; --render --level N per captured level; --retry-runtime-skins",
                             "python scripts/model-review/catalogue_full.py"])
    (DEST / "catalogue.json").write_text(json.dumps(report, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    (DEST / "entries").mkdir(exist_ok=True)
    for g in group_rows:
        (DEST / "entries" / f"{g['id']}.json").write_text(json.dumps(dict(group=g, entries=by_group.get(g["id"], [])),
                                                                   indent=1, ensure_ascii=False) + "\n", encoding="utf-8")
    with (DEST / "catalogue.tsv").open("w", encoding="utf-8", newline="") as f:
        w = csv.writer(f, delimiter="\t")
        w.writerow(["group", "section", "class_id", "asset_id", "identity", "role", "confidence", "status", "levels",
                    "containers", "near_triangles", "joints", "native_front", "source_obj", "existing_work", "remaining"])
        for e in entries:
            g = groups[e["group"]]
            w.writerow([e["group"], g["section"], e["class_id"], e["asset_id"], e["identity"], e["role"],
                        e["identity_confidence"], e["status_label"], ",".join(map(str, e["levels"])),
                        ",".join(e["source_containers"]), e["triangles"]["near"], e["joints"]["near"],
                        e["files"].get("native_front", ""), e["files"].get("source_obj", ""),
                        ",".join(x["key"] for x in e["existing_work"]), e["remaining"]])
    write_html(group_rows, by_group, summary, gaps)
    write_markdown(group_rows, by_group, summary, gaps)
    print(json.dumps(summary, indent=1))


def link(path, base):
    return os.path.relpath(ROOT / path, base).replace("\\", "/")


def write_html(group_rows, by_group, summary, gaps):
    esc = html.escape
    sections = []
    for g in group_rows:
        cards = []
        for e in by_group.get(g["id"], []):
            f = e["files"]
            pics = "".join(f'<a href="{esc(link(f["native_" + x], DEST))}"><img loading="lazy" src="{esc(link(f["thumb_" + x], DEST))}" alt="{x}"></a>'
                           for x in ("front", "rear") if "thumb_" + x in f) or f'<p class="blocker">{esc(e["view_blocker"] or "")}</p>'
            links = " · ".join(f'<a href="{esc(link(f[k], DEST))}">{k.replace("_", " ")}</a>'
                               for k in ("source_obj", "source_far_lod_obj", "source_mtl", "superseded_preview") if k in f)
            work = "".join(f'<li><a href="{esc(link(p, DEST))}">{esc(Path(p).name)}</a></li>' for p in f.get("existing_work", []))
            where = ", ".join(f'L{o["level"]:02} ' + (f'slot {o["slot"]}' if "slot" in o else f'scene {o["scene_record"]}#{o["entry"]}')
                              for o in e["occurrences"][:12]) + (" …" if len(e["occurrences"]) > 12 else "")
            cards.append(
                f'<div class="card {e["status"]}"><div class="pics">{pics}</div>'
                f'<p><b>c{e["class_id"]}</b> · {esc(e["asset_id"])} · <span class="chip {e["status"]}">{esc(e["status_label"])}</span></p>'
                f'<p>{esc(e["identity"])} <span class="muted">({esc(e["identity_confidence"])})</span></p>'
                f'<p class="muted">{e["triangles"]["near"]:,} near / {e["triangles"]["far"]:,} far triangles · {e["joints"]["near"]} joints · '
                f'{e["skinned_meshes"]} skinned + {e["rigid_meshes"]} rigid meshes · {esc(", ".join(e["source_containers"]))}</p>'
                f'<p class="muted">{esc(where)}</p><p>{esc(e["remaining"])}</p><p class="links">{links}</p>'
                + (f'<details><summary>Existing work files</summary><ul>{work}</ul></details>' if work else "") + '</div>')
        body = "".join(cards) or '<p class="blocker">No local source class found.</p>'
        sections.append(f'<section class="group" data-status="{g["status"]}"><h2>{esc(g["title"])}</h2>'
                        f'<p class="tag">{esc(g["section"])} · <span class="chip {g["status"]}">{esc(g["status_label"])}</span> · '
                        f'{len(g["classes"])} classes / {g["fingerprints"]} fingerprints</p><p>{esc(g["note"])}</p>'
                        f'<div class="cards">{body}</div></section>')
    filters = "".join(f'<label><input type="checkbox" value="{k}" checked> {v}</label>' for k, v in STATUS.items())
    stats = " · ".join(f'{STATUS[k]}: {n}' for k, n in summary["fingerprint_status"].items())
    page = f'''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>NPC and enemy catalogue</title><style>
:root{{--bg:#15181d;--panel:#20252d;--text:#e9edf1;--muted:#a9b2bd;--link:#8fd0f5}}
body{{background:var(--bg);color:var(--text);font:15px system-ui;margin:24px auto;max-width:1500px;padding:0 16px}}a{{color:var(--link)}}
h1{{font-size:30px}}h2{{font-size:21px;margin:0 0 4px}}.group{{background:var(--panel);border-radius:8px;padding:16px;margin:14px 0}}
.cards{{display:grid;grid-template-columns:repeat(auto-fill,minmax(330px,1fr));gap:12px}}.card{{background:#191d23;border-radius:6px;padding:10px}}
.pics{{display:flex;gap:4px;min-height:120px;background:#000;border-radius:4px;justify-content:center;align-items:center}}.pics img{{max-height:200px;max-width:160px}}
p{{margin:6px 0;line-height:1.4}}.muted,.tag{{color:var(--muted);font-size:13px}}.blocker{{color:#e8b86b;font-size:13px;padding:8px}}
.chip{{padding:1px 7px;border-radius:9px;font-size:12px;color:#111}}.art_complete{{background:#7fd69a}}.reusable{{background:#9fd0ff}}
.work_remaining{{background:#f2c76b}}.unresolved{{background:#f08a8a}}.out_of_scope{{background:#aab}}.card.art_complete,.card.reusable,.card.work_remaining,.card.unresolved,.card.out_of_scope{{background:#191d23}}
.controls label{{margin-right:14px;white-space:nowrap}}input[type=search]{{padding:8px;width:min(520px,90%);font:inherit}}.links{{font-size:13px}}[hidden]{{display:none!important}}
</style><h1>NPC and enemy catalogue</h1>
<p>{summary["classes"]} classes, {summary["fingerprints"]} source fingerprints (LEVEL cores and cutscene SCENE blocks), {summary["groups"]} groups. {esc(stats)}.</p>
<p class="muted">Views are the native review renderer's bind-space front/rear (normal decode revision 2) with the saved level's lights: identity evidence, not material approval. Completion is per documented configuration.
<a href="catalogue.json">catalogue.json</a> · <a href="catalogue.tsv">catalogue.tsv</a> · <a href="entries/">per-group manifests</a> · <a href="{esc(link("docs/NPC-ENEMY-CATALOGUE.md", DEST))}">readable catalogue</a></p>
<div class="controls">{filters}</div><p><input id="q" type="search" placeholder="Search name, class, fingerprint, level, status"></p><p id="n" class="muted"></p>
{"".join(sections)}
<script>
const boxes=[...document.querySelectorAll('.controls input')],q=document.querySelector('#q'),cards=[...document.querySelectorAll('.card')],groups=[...document.querySelectorAll('.group')];
function f(){{const on=new Set(boxes.filter(b=>b.checked).map(b=>b.value)),t=q.value.toLowerCase();let n=0;
for(const c of cards){{const s=[...c.classList].find(k=>k in {json.dumps(STATUS)});c.hidden=!(on.has(s)&&(c.textContent.toLowerCase().includes(t)||c.closest('.group').querySelector('h2').textContent.toLowerCase().includes(t)));if(!c.hidden)n++}}
for(const g of groups){{const cs=g.querySelectorAll('.card');g.hidden=cs.length?[...cs].every(c=>c.hidden):!(on.has(g.dataset.status)&&g.textContent.toLowerCase().includes(t))}}
document.querySelector('#n').textContent=n+' fingerprint cards shown'}}
boxes.forEach(b=>b.addEventListener('change',f));q.addEventListener('input',f);f();
</script></html>'''
    (DEST / "index.html").write_text(page, encoding="utf-8")


def write_markdown(group_rows, by_group, summary, gaps):
    L = ["# NPC and enemy catalogue", "",
         f"Inventory date: {summary['date']}. Generated by `python scripts/model-review/catalogue_full.py`; do not edit by hand.", "",
         f"**{summary['classes']} classes, {summary['fingerprints']} source fingerprints, {summary['groups']} groups.** "
         "Every jointed LEVEL class of the 34 story levels and every class of the cutscene SCENE blocks is listed, "
         "including props, gear and vehicles, which are out of scope with a stated reason.", "",
         "[Visual index](../captures/rift-texture-review/npc-catalogue/full/index.html) · "
         "[catalogue.json](../captures/rift-texture-review/npc-catalogue/full/catalogue.json) · "
         "[catalogue.tsv](../captures/rift-texture-review/npc-catalogue/full/catalogue.tsv) · "
         "[per-group manifests](../captures/rift-texture-review/npc-catalogue/full/entries/) · "
         "[named-cast checklist](REMAINING-NPCS.md)", "",
         "## Status of every fingerprint", "", "| Status | Fingerprints | Groups |", "|---|---:|---:|"]
    for k, v in STATUS.items():
        L.append(f"| {v} | {summary['fingerprint_status'].get(k, 0)} | {summary['group_status'].get(k, 0)} |")
    L += ["", "A group's status is its least-finished in-scope fingerprint. Completion belongs to the documented "
          "(class, fingerprint) configuration; other variants are listed separately.", ""]
    for section in dict.fromkeys(g["section"] for g in group_rows):
        L += [f"## {section}", "", "| Group | Classes | Status | Notes |", "|---|---|---|---|"]
        for g in group_rows:
            if g["section"] != section:
                continue
            cls = ", ".join(f"`{c}`" for c in g["classes"]) or "none found"
            fs = ", ".join(f"{STATUS[k]} {n}" for k, n in g["fingerprint_statuses"].items())
            L.append(f"| {g['title']} | {cls} | **{g['status_label']}**{' (' + fs + ')' if fs else ''} | {g['note']} |")
        L.append("")
    L += ["## Work remaining, per fingerprint", "", "| Class / fingerprint | Identity | Levels | Remaining work |", "|---|---|---|---|"]
    for g in group_rows:
        for e in by_group.get(g["id"], []):
            if e["status"] == "work_remaining":
                L.append(f"| `{e['class_id']}` / `{e['asset_id']}` | {e['identity']} | {','.join(map(str, e['levels']))} | {e['remaining']} |")
    L += ["", "## Unresolved, per fingerprint", "", "| Class / fingerprint | What is known | Blocker |", "|---|---|---|"]
    for g in group_rows:
        for e in by_group.get(g["id"], []):
            if e["status"] == "unresolved":
                L.append(f"| `{e['class_id']}` / `{e['asset_id']}` | {e['identity']} | {e['view_blocker'] or e['remaining']} |")
    L += ["", "## Source gaps", "",
          f"- **Cutscene SCENE blocks:** decoded for the first time: {gaps['scene_blocks']['rows']} class rows, "
          f"{gaps['scene_blocks']['fingerprints']} fingerprints. The review renderer has no SCENE path, so cutscene-only classes have no native view.",
          f"- **Class records without core data:** {gaps['class_records_without_core_data']['classes']} class IDs in "
          f"{gaps['class_records_without_core_data']['slots']} level slots. {gaps['class_records_without_core_data']['note']} Listed in catalogue.json.",
          f"- **Multiplayer levels {gaps['multiplayer_levels']['levels'][0]}-{gaps['multiplayer_levels']['levels'][-1]}:** {gaps['multiplayer_levels']['note']}",
          "- **Levels 12 and 20** have no saved gameplay capture; their only-there classes have source exports but no native view (game launches are excluded).", "",
          "## Evidence and files", "",
          "Each fingerprint entry in `catalogue.json` lists its level/slot or SCENE record occurrences, source container, near/far/third-run "
          "triangles, joints, skinned/rigid meshes, atlases and runtime materials, PBR coverage, bind bounds, exported-mesh signature and "
          "identical-mesh peers, existing work keys and every associated file: native front/rear views and thumbnails, the source OBJ/MTL "
          "(and far LOD), original textures, the superseded roster preview, and existing deliverable folders and `.blend` files.", "",
          "- Source export: `captures/rift-texture-review/npc-catalogue/source-export` (`catalogue_export`: offline disc read; its LEVEL "
          "fingerprints, triangle counts and texture lists reproduce `library/uya-mobies/occurrences.tsv` for all 5,270 occurrences).",
          "- Native views: `captures/rift-texture-review/npc-catalogue/native` (`catalogue_native.py`, the existing isolated review renderer; "
          "runtime-skin classes rendered with the skins the renderer itself reported).",
          "- Old roster-review thumbnails have invalid normals and are linked only as superseded previews.", "",
          "## Reproduction", "",
          "```", "cd scripts/model-review/catalogue_export && cargo build --release --offline",
          "catalogue_export.exe disc/SCUS_973.53.iso captures/rift-texture-review/npc-catalogue/source-export captures/rift-texture-review/source-skin-trig.bin",
          "python scripts/model-review/catalogue_native.py --plan", "python scripts/model-review/catalogue_native.py --render --level N   # each captured level, under scripts/watch-run.ps1",
          "python scripts/model-review/catalogue_native.py --retry-runtime-skins   # then --render the listed levels again",
          "python scripts/model-review/catalogue_full.py", "```", ""]
    (ROOT / "docs/NPC-ENEMY-CATALOGUE.md").write_text("\n".join(L), encoding="utf-8")


if __name__ == "__main__":
    main()
