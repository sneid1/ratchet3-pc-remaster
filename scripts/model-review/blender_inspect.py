"""Import extracted RA geometry into an editable Blender inspection file.

No model replacement is installed. This checks the actual glTF importer, bounds,
UV layers, skeleton and weights rather than treating successful extraction as proof.
"""
import bpy
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "captures/rift-texture-review/blender"
bpy.ops.object.select_all(action='SELECT')
bpy.ops.object.delete(use_global=False)
report = []
for name in ("hero_clank_ps4", "hero_ratchet", "hero_ratchet_bangle_galacticranger_torso"):
    previous = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(OUT / (name + '.glb')))
    objects = [o for o in bpy.data.objects if o not in previous]
    collection = bpy.data.collections.new(name)
    bpy.context.scene.collection.children.link(collection)
    for obj in objects:
        for old in list(obj.users_collection):
            old.objects.unlink(obj)
        collection.objects.link(obj)
    meshes = [o for o in objects if o.type == 'MESH']
    rigs = [o for o in objects if o.type == 'ARMATURE']
    report.append({"model":name,"meshes":len(meshes),"vertices":sum(len(o.data.vertices) for o in meshes),
                   "triangles":sum(len(o.data.polygons) for o in meshes),"armatures":len(rigs),
                   "bones":sum(len(o.data.bones) for o in rigs),"uv_layers":[len(o.data.uv_layers) for o in meshes],
                   "unweighted_vertices":sum(sum(not v.groups for v in o.data.vertices) for o in meshes),
                   "bounds":[[list(c) for c in o.bound_box] for o in meshes],
                   "material_slots":[[m.name if m else None for m in o.data.materials] for o in meshes]})
    collection.hide_render = name != 'hero_clank_ps4'
    collection.hide_viewport = name != 'hero_clank_ps4'
    for rig in rigs:
        rig.show_in_front = True
bpy.context.scene['Purpose'] = 'Local source inspection. UYA game lighting and animation have not been transferred to these RA rigs.'
bpy.ops.wm.save_as_mainfile(filepath=str(OUT / 'rift-model-inspection.blend'))
(OUT / 'blender-inspection.json').write_text(json.dumps(report, indent=2))
print(json.dumps([{k:v for k,v in r.items() if k not in ('bounds','material_slots','uv_layers')} for r in report]))
