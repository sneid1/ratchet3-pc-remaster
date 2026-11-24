"""Repair split shading normals on the inspected original character meshes.

Only coincident corners whose authored normals differ by less than the selected
crease angle are joined. Geometry, UVs, materials and separate parts stay intact.
The saved source is consumed by the existing contour refinement/export step.
"""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import sys
import bpy
from mathutils import Vector

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--model', required=True)
p.add_argument('--blend', type=Path, required=True)
p.add_argument('--angle', type=float, default=45)
a = p.parse_args(sys.argv[sys.argv.index('--')+1:])
if not 0 < a.angle < 90: raise ValueError('Crease angle must preserve hard edges')
bpy.ops.wm.open_mainfile(filepath=str(a.blend.resolve()))
obj = bpy.data.objects['UYA_'+a.model+'_captured_pose']
mesh = obj.data
original = [n.vector.copy().normalized() for n in mesh.corner_normals]
groups = defaultdict(list)
for polygon in mesh.polygons:
    for loop_index in polygon.loop_indices:
        position = mesh.vertices[mesh.loops[loop_index].vertex_index].co
        key = (polygon.material_index, tuple(round(v, 5) for v in position))
        groups[key].append(loop_index)
threshold = math.cos(math.radians(a.angle))
normals = list(original)
changed = 0
for indices in groups.values():
    # Deduplicate the triangle stream so triangle count cannot bias the normal.
    unique = {tuple(round(v, 6) for v in original[i]): original[i] for i in indices}
    for i in indices:
        neighbours = [n for n in unique.values() if n.dot(original[i]) >= threshold]
        normal = sum(neighbours, Vector()).normalized()
        if (normal-original[i]).length > .0001: changed += 1
        normals[i] = normal
mesh.normals_split_custom_set(normals)
out = Path(__file__).resolve().parents[2]/'captures/rift-texture-review/blender/smooth'
out.mkdir(exist_ok=True)
bpy.ops.wm.save_as_mainfile(filepath=str(out/(a.model+'.blend')))
(out/(a.model+'.json')).write_text(json.dumps({
    'model':a.model, 'source':str(a.blend), 'crease_degrees':a.angle,
    'changed_corners':changed, 'geometry_and_uvs':'unchanged',
    'method':'Coincident corners of the same material; authored crease threshold.'
}, indent=2)+'\n')
print(f'{a.model}: repaired {changed} split shading corners; preserved all source positions and UVs')
