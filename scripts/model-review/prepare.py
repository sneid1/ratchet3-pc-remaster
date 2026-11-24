"""Snapshot the real renderer into a separately named, disposable review executable.

No edits to port/, the active uya.exe, or the player profile. The small hooks below
are checked before insertion, so renderer changes fail visibly rather than drifting.
"""
from pathlib import Path
import hashlib
import json
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
BUILD = ROOT / "captures/rift-texture-review/viewer-build"


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise RuntimeError(f"Renderer hook changed ({text.count(old)} matches): {old[:80]}")
    return text.replace(old, new, 1)


def main():
    src = BUILD / "src"
    src.mkdir(parents=True, exist_ok=True)
    provenance = {}
    for path in (ROOT / "port/uya/src").rglob("*"):
        if path.is_file():
            rel = path.relative_to(ROOT / "port/uya/src")
            dest = src / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, dest)
            provenance[str(path.relative_to(ROOT))] = hashlib.sha256(path.read_bytes()).hexdigest()
    cargo = (ROOT / "port/uya/Cargo.toml").read_text()
    cargo = replace_once(cargo, 'name = "uya"', 'name = "uya-model-review"\nautobins = false')
    cargo = re.sub(r'path = "([^"]+)"', lambda m: 'path = "' + (ROOT / "port/uya" / m[1]).resolve().as_posix() + '"', cargo)
    cargo += '\n[workspace]\nresolver = "3"\n\n[[bin]]\nname = "uya-model-review"\npath = "src/review_main.rs"\n'
    cargo += '\n[patch.crates-io]\nwgpu-hal = { path = "' + (ROOT / 'port/vendor/wgpu-hal').as_posix() + '" }\n'
    cargo += '\n[profile.release]\nopt-level = 3\ndebug = 1\n[profile.release.package."*"]\ndebug = 0\n'
    # These are workspace members in port/, so retain their normal line tables and
    # dependency fingerprints. Otherwise every translated EE crate recompiles.
    for member in ("uyafmt", "uyart", "uycpu"):
        cargo += f'\n[profile.release.package.{member}]\ndebug = 1\n'
    (BUILD / "Cargo.toml").write_text(cargo)
    if not (BUILD / "Cargo.lock").exists():
        shutil.copy2(ROOT / "port/Cargo.lock", BUILD / "Cargo.lock")
    lib = src / "lib.rs"
    lib.write_text(lib.read_text() + '\npub mod material_review;\nmod mesh_edit;\n')
    # Experimental PBR remains in this black-room snapshot until its visual
    # review is accepted. Applying with context rejects overlapping changes.
    patch=HERE/'renderer-pbr.patch'
    command=['git','apply','--directory='+src.relative_to(ROOT).as_posix(),str(patch)]
    subprocess.run(command[:2]+['--check']+command[2:],cwd=ROOT,check=True)
    subprocess.run(command,cwd=ROOT,check=True)
    native = src / "native.rs"
    text = native.read_text()
    anchor = '        self.queue.write_buffer(&pic.view, SETS_AT, sets);'
    hook = '''
        // Selected-model mode is enabled only by the review protocol. The same
        // isolated library can exercise the ordinary game path for validation.
        if crate::material_review::enabled() {
        let mo = self.level.as_mut().expect("loaded above").1.mobies.as_mut().ok_or("review: no mobies")?;
        crate::material_review::draw(&self.device, &self.queue, &self.mat_layout,
            &self.boot, mo, ram, jobs, &g.mobies, &cam, &fog, &self.objects,
            &pic.tv, &pic.dv, &pic.depth, &self.ao, &pic.view, &pic.view_bg, self.size)?;
        return Ok(pic.tex.clone());
        }
'''
    text = replace_once(text, anchor, anchor + hook)
    native.write_text(text)
    mobies = src / "mobies.rs"
    text = mobies.read_text()
    # The gameplay camera's CPU back-face rejection must not remove the model's rear.
    text = replace_once(text, 'if run.variant_a && (a.cull == 0 || a.cull == 2) {',
                        'if false && run.variant_a && (a.cull == 0 || a.cull == 2) {')
    # Preserve each run's true object origin, allowing exact instance isolation rather
    # than guessing which triangles belong together by their screen positions.
    text = replace_once(text, '        for (r, run) in runs.iter().enumerate() {\n            while',
                        '        for (r, run) in runs.iter().enumerate() {\n            let review_start = out.len();\n            while')
    text = replace_once(text, "            // The run's places, for the next frame's.",
                        '            crate::material_review::record_run(review_start, out.len(), world([0, 0, 0]));\n' + "            // The run's places, for the next frame's.")
    mobies.write_text(text)
    for name in ("material_review.rs", "review_main.rs", "mesh_edit.rs"):
        shutil.copy2(HERE / name, src / name)
    # Use the exact dependency artifacts of the built game library. Cargo's
    # workspace-relative package identities would otherwise rebuild translated code.
    fingerprints = ROOT / "port/target/release/.fingerprint"
    built = list(fingerprints.glob("uya-*/lib-uya.json"))
    if not built:
        raise RuntimeError("Build the normal game first: its dependency artifacts are required")
    latest = max(built, key=lambda p: p.stat().st_mtime)
    deps = json.loads(latest.read_text())["deps"]
    externs = []
    for _, name, _, digest in deps:
        matches = []
        for record in fingerprints.glob(f"*/lib-{name}"):
            if int.from_bytes(bytes.fromhex(record.read_text().strip()), "little") == digest:
                suffix = record.parent.name.rsplit("-", 1)[1]
                artifact = ROOT / f"port/target/release/deps/lib{name}-{suffix}.rlib"
                if artifact.exists():
                    matches.append(artifact)
        if len(matches) != 1:
            raise RuntimeError(f"Expected one cached {name} artifact, found {matches}")
        externs += ["--extern", f"{name}={matches[0]}"]
    base = ["--edition=2024", "-C", "opt-level=2", "-C", "debuginfo=1", "-C", "linker=rust-lld.exe",
            "-L", f"dependency={ROOT / 'port/target/release/deps'}"]
    native_paths = set()
    for output in (ROOT / "port/target/release/build").glob("*/output"):
        for line in output.read_text(errors="replace").splitlines():
            if line.startswith("cargo:rustc-link-search="):
                native_paths.add(line.split("=", 1)[1])
    for native in sorted(native_paths):
        if Path(native.removeprefix("native=")).is_dir():
            base += ["-L", native]
    library = BUILD / "libuya_model_review.rlib"
    lib_args = base + ["--crate-name", "uya_model_review", "--crate-type", "lib", str(src / "lib.rs"), "-o", str(library)] + externs
    bin_args = base + ["--crate-name", "uya_model_review_bin", str(src / "review_main.rs"), "--extern", f"uya_model_review={library}", "-o", str(BUILD / "uya-model-review.exe")]
    # rustc response files take one argument per line, including paths with spaces.
    (BUILD / "library.rsp").write_text("\n".join(lib_args) + "\n")
    (BUILD / "binary.rsp").write_text("\n".join(bin_args) + "\n")
    game_args=base+["--crate-name","uya_art_review",str(src/"main.rs"),"--extern",f"uya={library}","-o",str(BUILD/"uya-art-review.exe")]+externs
    (BUILD / "game.rsp").write_text("\n".join(game_args)+"\n")
    (BUILD / "renderer-source-sha256.json").write_text(json.dumps(provenance, indent=2) + "\n")
    from channels import apply
    apply(src)
    from source_lod import apply as apply_lod
    apply_lod()
    from family_shader import apply as apply_families
    apply_families()
    from probe_shader import apply as apply_probes
    apply_probes()
    from emissive_shader import apply as apply_emission
    apply_emission()
    from pbr_mixed_atlas import apply as apply_mixed_atlas
    apply_mixed_atlas()
    print(BUILD)


if __name__ == "__main__":
    main()
