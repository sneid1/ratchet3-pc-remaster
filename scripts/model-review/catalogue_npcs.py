"""Reconcile the named NPC checklist with source variants and existing art evidence.

Reads existing inventories only. No extraction, render, game launch or asset changes.
Run from any directory: python scripts/model-review/catalogue_npcs.py
"""
import csv
import html
import json
import os
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "captures/rift-texture-review"
DEST = OUT / "npc-catalogue"


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def relative(path):
    return Path(path).resolve().relative_to(ROOT).as_posix()


def link_from(path, base):
    return os.path.relpath(ROOT / path, base).replace("\\", "/")


def tsv(path, fields, rows):
    with path.open("w", encoding="utf-8", newline="") as stream:
        writer = csv.DictWriter(stream, fields, delimiter="\t")
        writer.writeheader()
        writer.writerows(rows)


def main():
    DEST.mkdir(parents=True, exist_ok=True)
    identities = read(Path(__file__).with_name("npc-identities.json"))
    roster = read(OUT / "character-roster.json")
    manifest = read(Path(__file__).with_name("review-assets.json"))
    previews = read(OUT / "roster-review/results.json")
    ledger = read(OUT / "art-finishing-ledger.json")
    completed = {item["model"]: item for item in ledger["art_complete"]}
    with (OUT / "library/uya-mobies/occurrences.tsv").open(encoding="utf-8") as stream:
        occurrences = list(csv.DictReader(stream, delimiter="\t"))
    by_class = defaultdict(list)
    for occurrence in occurrences:
        by_class[int(occurrence["class_id"])].append(occurrence)
    assets = {item["asset_id"]: item for item in roster["assets"]}

    # Completion applies to the documented fingerprint or exact source slot only.
    # Captured assemblies and shared atlas matches do not prove variant completion.
    art = {}
    exact_art = defaultdict(list)
    for key, model in manifest.items():
        if key not in completed and key not in ("source_c7000", "source_c7013"):
            continue
        aid = model.get("asset_id")
        if not aid and model.get("source_layer") == "source":
            matches = [o for o in occurrences if int(o["level"]) == model["level"]
                       and int(o["slot"]) == model["source_id"]]
            assert len(matches) == 1, (key, matches)
            aid = matches[0]["asset_id"]
        item = {
            "key": key, "name": model["name"], "asset_id": aid,
            "status": "art pass complete" if key in completed else "detailed layers already delivered",
            "completion_scope": "documented configuration only; animation/integration not approved",
            "evidence": completed.get(key, {}).get("evidence", "docs/RIFT-TEXTURE-REVIEW.md").replace("\\", "/"),
            "limits": completed.get(key, {}).get("limits", "See existing workbench documentation."),
        }
        art[key] = item
        if aid:
            exact_art[aid].append(item)

    entries, assigned = [], {}
    for original in identities["entries"]:
        entry = dict(original)
        entry["sources"] = []
        for class_id in entry["classes"]:
            assert class_id in by_class, f"Unknown local class {class_id}"
            assert class_id not in assigned, f"Duplicate identity assignment {class_id}"
            assigned[class_id] = entry["id"]
            found = defaultdict(list)
            for occurrence in by_class[class_id]:
                found[occurrence["asset_id"]].append(occurrence)
            preview = previews.get(f"source_c{class_id}", {})
            preview_aid = preview.get("asset", {}).get("asset_id")
            for aid, source_occurrences in sorted(found.items()):
                source = assets[aid]
                image = preview.get("path") if aid == preview_aid else None
                image = relative(image) if image and Path(image).is_file() else None
                existing = exact_art.get(aid, [])
                source_status = "needs individual art / identity review"
                if existing:
                    source_status = existing[0]["status"] + "; verify reuse for this occurrence"
                entry["sources"].append({
                    "class_id": class_id, "asset_id": aid,
                    "occurrences": [{"level": int(o["level"]), "slot": int(o["slot"])}
                                    for o in source_occurrences],
                    "near_triangles": source["near_triangles"], "joints": source["joints"],
                    "atlases": source["atlases"], "unmapped_pbr": source["unmapped_pbr"],
                    "status": source_status,
                    "existing_art": [a["key"] for a in existing],
                    "preview": image,
                    "preview_status": preview.get("state") if image else "No matching fingerprint preview",
                    "identity_scope": "Representative visually matched; other fingerprints require confirmation"
                                      if aid != preview_aid else entry["identity_confidence"],
                })
        entry["existing_art"] = [art[key] for key in entry["existing_art_keys"]]
        entry["status"] = ("Source not identified" if not entry["classes"] else
                           "Existing art + variant audit" if entry["existing_art"] else
                           "Source candidates located; art pending")
        entries.append(entry)

    # Full jointed-source appendix: deliberately not labelled an NPC count.
    # Includes props/skins/enemies, missing previews and zero-mesh placeholders.
    candidates = []
    for class_id, group in sorted(by_class.items()):
        if not any(int(o["near_joints"]) > 0 or int(o["far_joints"]) > 0 for o in group):
            continue
        preview = previews.get(f"source_c{class_id}", {})
        image = preview.get("path")
        candidates.append({
            "class_id": class_id, "catalogue_entry": assigned.get(class_id, "Unclassified"),
            "fingerprints": sorted({o["asset_id"] for o in group}),
            "levels": sorted({int(o["level"]) for o in group}),
            "occurrences": len(group),
            "max_near_triangles": max(int(o["near_triangles"]) for o in group),
            "preview": relative(image) if image and Path(image).is_file() else None,
            "preview_status": preview.get("state", "Not in representative render queue"),
        })

    stats = {
        "catalogue_groups": len(entries),
        "group_statuses": dict(Counter(e["status"] for e in entries)),
        "named_source_classes": len(assigned),
        "catalogued_class_fingerprint_pairs": sum(len(e["sources"]) for e in entries),
        "all_level_fingerprints": len(assets), "all_level_occurrences": len(occurrences),
        "jointed_source_classes_in_appendix": len(candidates),
        "jointed_source_fingerprints": sum(a["joints"] > 0 for a in assets.values()),
        "existing_preview_records": len(previews),
        "unresolved_preview_records": sum("Unresolved" in p["state"] for p in previews.values()),
    }
    report = {
        "date": identities["date"], "scope": identities["scope"], "counts": stats,
        "limitations": [
            "Catalogue groups are not unique characters or finished-model counts.",
            "Visual class/name matches are provisional unless confirmed in the project manifest.",
            "Old source thumbnails have invalid shading; identity reference only, never art approval.",
            "SCENE cutscene-only and multiplayer source gaps remain; complete mesh identification is not claimed.",
            "Shared PBR, shared textures or a completed representative do not finish other fingerprints.",
        ],
        "references": identities["references"], "entries": entries,
        "source_gaps": roster["unresolved_sources"],
    }
    write_json(DEST / "catalogue.json", report)
    write_json(DEST / "jointed-source-index.json", candidates)
    tsv(DEST / "remaining-npcs.tsv", ["id", "name", "category", "status", "classes", "fingerprints", "levels", "identity_confidence", "note"], [
        dict(id=e["id"], name=e["name"], category=e["category"], status=e["status"],
             classes=",".join(map(str, e["classes"])),
             fingerprints=",".join(sorted({s["asset_id"] for s in e["sources"]})),
             levels=",".join(map(str, sorted({o["level"] for s in e["sources"] for o in s["occurrences"]}))),
             identity_confidence=e["identity_confidence"], note=e["note"]) for e in entries])

    lines = ["# Remaining NPC and character catalogue", "", f"Inventory date: {identities['date']}.", "",
             identities["scope"], "",
             "This is a work catalogue, not a claim that all character meshes have been identified. "
             "Each named cast member is tracked even when their local source is unresolved. "
             "Distinct source classes and fingerprints stay separate.", "",
             f"**{len(entries)} work groups; {len(assigned)} candidate source classes.** "
             "These include costumes, bosses, existing-art variant audits and two screen/cameo audit groups, not a count of new NPCs.", "",
             "[Visual catalogue](../captures/rift-texture-review/npc-catalogue/index.html) · "
             "[JSON with every source occurrence](../captures/rift-texture-review/npc-catalogue/catalogue.json) · "
             "[Spreadsheet-ready TSV](../captures/rift-texture-review/npc-catalogue/remaining-npcs.tsv)", "",
             "**Complete catalogue of every jointed and cutscene class, with views, files and statuses: [NPC-ENEMY-CATALOGUE.md](NPC-ENEMY-CATALOGUE.md).**", "",
             "## Status and evidence", "",
             "- **Source candidates located:** a representative silhouette was identified; new proper-name matches remain provisional. Other fingerprints still need confirmation.",
             "- **Source not identified:** the named character/variant is recorded, but no local class ID is asserted. Screen-only roles may have no independent 3D mesh.",
             "- **Existing art + variant audit:** reuse the finished work first. Compare each source variant; do not restart the finished representative or assume that all variants are covered.",
             "- Source thumbnails come from superseded previews with incorrect normals. Use silhouettes for identification only; regenerate corrected views before judging materials.", ""]
    for category in dict.fromkeys(e["category"] for e in entries):
        lines += [f"## {category}", "", "| Character / variant | Candidate classes | Status / remaining work |", "|---|---|---|"]
        for e in entries:
            if e["category"] != category:
                continue
            cls = ", ".join(f"`{c}`" for c in e["classes"]) or "Unresolved"
            note = f"**{e['status']}.** {e['note']}"
            if e["sources"]:
                note += f" {len(e['sources'])} class/fingerprint records; full level/slot list in JSON."
            lines.append(f"| {e['name']} | {cls} | {note.strip()} |")
        lines.append("")
    lines += ["## Existing work to preserve", "",
              "The completion ledger records finished passes for standalone Clank, jungle Qwark, Skidd, Big Al and a captured Ranger assembly. "
              "Hero Qwark and Sasha already have detailed imagegen layers in the workbench documentation. "
              "Their existing deliveries are reuse inputs, not new untouched models. Source equivalence, costumes and additional fingerprints still require checks. "
              "The three standard ships are complete as art passes and excluded from this NPC list.", "",
              "Existing small/armoured/Mecha Tyhrranoid and Florana wasp work is retained separately in the art ledger. "
              "Generic enemies are not silently counted as named NPCs.", "",
              "## Coverage gaps and unidentified candidates", "",
              f"The underlying LEVEL inventory contains {len(assets):,} fingerprints across {len(occurrences):,} occurrences. "
              f"The [jointed-source index](../captures/rift-texture-review/npc-catalogue/jointed-source-index.json) retains {len(candidates)} classes, "
              "including weapons, props, player skins and empty placeholders. It is an identification appendix, **not an NPC roster or remaining-art total**. "
              "Non-jointed sources remain in the full [character roster](../captures/rift-texture-review/character-roster.json).", "",
              "Cutscene-only SCENE sources and multiplayer decoding gaps are copied verbatim into `catalogue.json`. "
              "The 291-class preview queue is only a filtered representative sample; 19 entries have unresolved previews. "
              "Unclassified classes, including possible bosses and cameos, must be reconciled before claiming exhaustive source identification.", "",
              "## How to use this queue", "",
              "1. Confirm each provisional name against its source/reference before authoring. Resolve the character's LEVEL/SCENE source where missing.",
              "2. Inspect the original model kit and existing finished deliveries. Reuse appropriate assemblies and fitted materials.",
              "3. Compare source fingerprints, costumes, runtime materials and assembled parts; identify what actually differs.",
              "4. Make the individual art pass and inspect corrected front/rear/close views. Record completion against the exact configuration, not an entire class family.", "",
              "## References and reproduction", "",
              "External lists establish names/costumes only. Local class matches come from inspected source silhouettes or existing project manifests.", ""]
    for ref in identities["references"]:
        lines.append(f"- [{ref['id']}]({ref['url']}): {ref['use']}")
    lines += ["", "Inputs: `scripts/model-review/npc-identities.json`, `review-assets.json`, the existing source roster, occurrence table, preview results and art-finishing ledger. "
              "Regenerate with `python scripts/model-review/catalogue_npcs.py`. This does not modify assets, the player pack or the executable.", ""]
    (ROOT / "docs/REMAINING-NPCS.md").write_text("\n".join(lines), encoding="utf-8")

    esc = html.escape
    cards = []
    for e in entries:
        pics = []
        for s in e["sources"]:
            if s["preview"]:
                src = link_from(s["preview"], DEST)
                pics.append(f'<figure><img loading="lazy" src="{esc(src)}" alt="Source class {s["class_id"]}">'
                            f'<figcaption>c{s["class_id"]} · {esc(s["asset_id"])}<br>Identity only; invalid shading</figcaption></figure>')
        details = []
        for s in e["sources"]:
            where = ", ".join(f'L{o["level"]:02} slot {o["slot"]}' for o in s["occurrences"])
            details.append(f'<li>c{s["class_id"]} / {esc(s["asset_id"])}: {esc(where)}<br>{esc(s["status"])}</li>')
        existing = " ".join(f'{a["name"]}: {a["status"]}.' for a in e["existing_art"])
        cards.append(f'<article><h2>{esc(e["name"])}</h2><p class="tag">{esc(e["category"])} · {esc(e["status"])}</p>'
                     f'<p>{esc(e["note"])}</p><p>{esc(existing)}</p><p class="muted">{esc(e["identity_confidence"])}</p>'
                     f'<div class="pictures">{"".join(pics)}</div><details><summary>Source variants ({len(e["sources"])})</summary>'
                     f'<ul>{"".join(details)}</ul></details></article>')
    page = '''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Remaining NPC catalogue</title><style>
body{background:#171b20;color:#edf0f2;font:16px system-ui;margin:32px auto;max-width:1400px;padding:0 24px}a{color:#94d7f7}
h1{font-size:36px}h2{font-size:22px}p{line-height:1.5}.notice{background:#3b3325;padding:16px;border-left:4px solid #e3ba6a}
input{padding:12px;font:inherit;width:min(650px,90%);margin:18px 0}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(360px,1fr));gap:18px}
article{background:#242a32;padding:20px;border-radius:8px;align-self:start}.tag{color:#aadbbf}.muted,figcaption{color:#b6bdc8;font-size:13px}
.pictures{display:flex;flex-wrap:wrap;gap:8px}figure{margin:0;max-width:210px}img{width:200px;height:160px;object-fit:contain;background:black}
details{margin-top:12px}li{overflow-wrap:anywhere;margin-bottom:8px}[hidden]{display:none!important}</style>
<h1>Remaining NPC catalogue</h1><p>CATALOGUE_DATE · GROUP_COUNT work groups. Characters, costumes, bosses and source gaps; not a count of new NPCs.</p>
<p class="notice">The thumbnails are old source identity references with incorrect shading. They are not current finished-art previews.
Names matched visually are provisional. Missing cutscene sources and unidentified variants remain open.</p>
<p><a href="catalogue.json">Full JSON / source slots</a> · <a href="remaining-npcs.tsv">TSV</a> · <a href="jointed-source-index.json">Unclassified source appendix</a></p>
<label for="filter">Find character, class or status</label><br><input id="filter" type="search" placeholder="e.g. Qwark, source not identified, 7013"><p id="count"></p><main>'''
    page = page.replace("CATALOGUE_DATE", identities["date"]).replace("GROUP_COUNT", str(len(entries)))
    page += "".join(cards)
    page += '''</main><script>const input=document.querySelector('#filter'),cards=[...document.querySelectorAll('article')];
function filter(){let n=0;for(const card of cards){card.hidden=!card.textContent.toLowerCase().includes(input.value.toLowerCase());if(!card.hidden)n++}document.querySelector('#count').textContent=n+' groups shown';}input.addEventListener('input',filter);filter();</script></html>'''
    (DEST / "index.html").write_text(page, encoding="utf-8")
    print(json.dumps(stats, indent=2))


if __name__ == "__main__":
    main()
