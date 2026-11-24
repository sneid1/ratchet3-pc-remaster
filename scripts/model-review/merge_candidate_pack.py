"""Merge the workbench candidate pack (Codex's finished texture work) into the game's installed Rift Apart pack.

Three-way, by file: base = the snapshot the candidate started from (captures/rift-texture-review/profile/...),
ours = the installed pack the game draws, theirs = the candidate. Only the candidate's changes since the base are
applied; a file changed on both sides stops the merge. The installed pack is copied to a backup outside
texture_replacements first (the game would load a second folder there as another pack).

Left out, with reasons:
  Big Al's shirt atlas (46c953e3...-00001993): its replacement and maps are laid out for the edited shirt UVs
  (review-assets.json source_c6501 "candidate_geometry": "edited"); on the game's original mesh they scramble the shirt.

python scripts/model-review/merge_candidate_pack.py --dry-run
python scripts/model-review/merge_candidate_pack.py
"""
import argparse
import hashlib
import json
import os
import shutil
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BASE = ROOT / "captures/rift-texture-review/profile/texture_replacements/0-riftapart"
CAND = ROOT / "captures/rift-texture-review/candidate/0-riftapart"
INSTALLED = Path.home() / "Games/Ratchet and Clank 3/texture_replacements/0-riftapart"
BACKUPS = Path.home() / "Games/Ratchet and Clank 3/backups"
LOG = ROOT / "captures/rift-texture-review/integration"
EXCLUDE_STEMS = {"46c953e3735925da-4998c4df7ee95149-00001993"}  # Big Al shirt: needs the edited mesh's UVs
EXCLUDE_MATERIAL_PREFIXES = ("authored_big_al_shirt",)
TABLES = ("surfaces.tsv", "regions.tsv", "pbr.tsv", "lights.tsv")


def files(root):
    return {p.relative_to(root).as_posix(): p for p in root.rglob("*") if p.is_file()}


def digest(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def excluded(rel):
    name = rel.split("/")[-1]
    return any(s in name for s in EXCLUDE_STEMS) or (rel.startswith("materials/") and name.startswith(EXCLUDE_MATERIAL_PREFIXES))


def table_without_excluded(path):
    lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
    kept = [l for l in lines if not any(l.startswith(s + "\t") for s in EXCLUDE_STEMS)]
    return "".join(kept), len(lines) - len(kept)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--dry-run", action="store_true")
    a = p.parse_args()
    base, cand, inst = files(BASE), files(CAND), files(INSTALLED)
    plan, conflicts, skipped = [], [], []
    for rel, src in sorted(cand.items()):
        if rel in base and digest(src) == digest(base[rel]):
            continue  # unchanged by the candidate
        if excluded(rel):
            skipped.append(rel)
            continue
        if rel in inst:
            ours_changed = rel not in base or digest(inst[rel]) != digest(base[rel])
            if ours_changed and digest(inst[rel]) != digest(src):
                conflicts.append(rel)
                continue
        plan.append(rel)
    if conflicts:
        raise SystemExit(f"Changed on both sides, not merged: {conflicts[:20]}")
    summary = dict(date=time.strftime("%Y-%m-%d %H:%M"), base=str(BASE), candidate=str(CAND), installed=str(INSTALLED),
                   to_apply=len(plan), skipped_for_edited_geometry=skipped,
                   by_folder={k: sum(1 for r in plan if r.split("/")[0] == k) for k in sorted({r.split("/")[0] for r in plan})})
    print(json.dumps({k: v for k, v in summary.items() if k != "skipped_for_edited_geometry"}, indent=1), "skipped", len(skipped))
    if a.dry_run:
        return
    backup = BACKUPS / f"0-riftapart-before-codex-merge-{time.strftime('%Y%m%d-%H%M')}"
    shutil.copytree(INSTALLED, backup)
    summary["backup"] = str(backup)
    applied = []
    for rel in plan:
        src, dst = CAND / rel, INSTALLED / rel
        dst.parent.mkdir(parents=True, exist_ok=True)
        if rel in TABLES:
            text, dropped = table_without_excluded(src)
            dst.write_text(text, encoding="utf-8")
            applied.append(dict(file=rel, sha256=digest(dst), rows_left_out=dropped))
        else:
            shutil.copy2(src, dst)
            applied.append(dict(file=rel, sha256=digest(dst)))
    summary["applied"] = applied
    LOG.mkdir(parents=True, exist_ok=True)
    out = LOG / f"pack-merge-{time.strftime('%Y%m%d-%H%M')}.json"
    out.write_text(json.dumps(summary, indent=1) + "\n")
    print("merged", len(applied), "files; backup", backup, "; log", out)


if __name__ == "__main__":
    main()
