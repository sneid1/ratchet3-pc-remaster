"""Extract local installed RA models for Blender evaluation, never deployment.

Uses this project's archive reader and the separately downloaded RCRA Forge
Python model parser. No bundled Forge DLLs or UI are loaded. Sources and parser
hashes are recorded alongside the local-only assets.
"""
import hashlib
import argparse
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "captures/rift-texture-review"
FORGE = OUT / "tools/RCRA-Forge-main"
sys.path[:0] = [str(ROOT / "scripts"), str(FORGE)]
import riftapart
from core.mesh import ModelParser, mesh_to_numpy
from exporters.gltf_exporter import GltfExporter


class ReviewExporter(GltfExporter):
    def _build_primitive(self, mesh):
        primitive=super()._build_primitive(mesh)
        if primitive is not None:
            _,_,uv,_=mesh_to_numpy(self.model,mesh)
            # Both the cooked texture and glTF use top-left image UVs. Forge's
            # OBJ-style V flip must not also be applied to glTF coordinates.
            if uv is not None:primitive['attributes']['TEXCOORD_0']=self._add_accessor(uv,'VEC2',5126,34962)
        return primitive

    def _build(self, bin_uri=None):
        doc = super()._build(bin_uri)
        # glTF joint nodes must belong to the scene. Keep the actual hierarchy.
        roots = [i for i, joint in enumerate(self.model.joints) if joint.parent == -1]
        doc["scenes"][0]["nodes"] += roots
        return doc


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--id',help='Installed model asset ID from the complete catalogue')
    parser.add_argument('--model',action='append',help='Exact installed model path; may be repeated')
    parser.add_argument('--lod',type=int,default=0)
    args=parser.parse_args()
    if args.lod<0:raise ValueError('LOD must be nonnegative')
    target = OUT / "blender"
    target.mkdir(exist_ok=True)
    toc = riftapart.Toc()
    names = riftapart.Dag().names
    available = [n for n in names if n.endswith(".model") and riftapart.asset_id(n) in toc.by_id]
    (target / "installed-model-paths.txt").write_text("\n".join(available))
    candidates = [
        r"characters\hero\hero_clank_ps4\hero_clank_ps4.model",
        r"characters\hero\hero_ratchet\hero_ratchet.model",
        r"characters\hero\hero_ratchet\bangle_set\bangle_galacticranger_torso\hero_ratchet_bangle_galacticranger_torso.model",
    ]
    if args.model:candidates=args.model
    if args.id:
        if args.model:raise ValueError('Choose either --id or --model')
        identifier=int(args.id,16)
        candidates=[n for n in available if riftapart.asset_id(n)==identifier]
        if len(candidates)!=1:raise ValueError('Asset ID does not identify one installed model')
        target=OUT/'library/models'/f'{identifier:016x}'
        target.mkdir(parents=True,exist_ok=True)
    report = {"game": str(toc.game), "parser_source": "https://github.com/ROStudios25/RCRA-Forge",
              "parser_hashes": {str(p.relative_to(FORGE)): hashlib.sha256(p.read_bytes()).hexdigest()
                                for p in [FORGE / "core/mesh.py", FORGE / "core/archive.py", FORGE / "exporters/gltf_exporter.py"]},
              "enemy_name_matches": [n for n in names if n.endswith(".model") and any(k in n.lower() for k in ("tyhrr", "tyran", "ranger"))], "models": []}
    for name in candidates:
        if riftapart.asset_id(name) not in toc.by_id:
            raise ValueError('Model is not available in the installed archives: '+name)
        asset = next(a for a in toc.by_id[riftapart.asset_id(name)] if a.span == 0)
        raw = toc.read(asset)
        model = ModelParser(raw).parse()
        stem = name.split("\\")[-1].removesuffix(".model")
        (target / (stem + ".model")).write_bytes(raw)
        subsets = [m for m in model.meshes if m.look_index == 0 and m.lod_level == args.lod]
        if not subsets:raise ValueError(f'No look 0 / LOD {args.lod}: {name}')
        for subset in subsets:
            pos, normal, uv, idx = mesh_to_numpy(model, subset)
            if idx is None or pos is None or int(idx.max()) >= len(pos):
                raise ValueError(f"Invalid indices: {name}, subset {subset.mesh_id}")
        ReviewExporter(model, stem, lod=args.lod).export_glb(str(target / (stem + ".glb")))
        record = {"source": name, "sha256": hashlib.sha256(raw).hexdigest(),
                  "selected_lod":args.lod,"triangles": sum(m.index_count // 3 for m in subsets), "vertices": sum(m.vertex_count for m in subsets),
                  "lods": model.lod_count, "subsets": len(subsets), "bones": len(model.joints),
                  "materials": model.material_names, "bone_names": [j.name for j in model.joints],
                  "joint_scales_nonunit": sum(any(abs(v-1) > .001 for v in s) for s in model.joint_scales)}
        report["models"].append(record)
        print(stem, record["triangles"], "triangles", len(model.joints), "bones", flush=True)
    (target / "source-inspection.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
