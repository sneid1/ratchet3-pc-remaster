"""Measure the authored iris against the actual white eyeball triangles."""
import bpy,json,sys
from pathlib import Path
from mathutils import Vector
from mathutils.bvhtree import BVHTree
OUT=Path(__file__).resolve().parents[2]/'captures/rift-texture-review'
edited='--edited' in sys.argv
bpy.ops.wm.open_mainfile(filepath=str(OUT/('blender/edited/ratchet.blend' if edited else 'blender/smooth/ratchet.blend')))
mesh=bpy.data.objects['UYA_ratchet_captured_pose'].data;mesh.calc_loop_triangles()
rules=json.loads(Path(__file__).with_name('surface-constraints.json').read_text())['ratchet']
def matching(rule,tri):
    x0,y0,x1,y1=rule['uv_bounds']
    uv=[mesh.uv_layers.active.data[i].uv for i in tri.loops]
    return mesh.materials[tri.material_index].name.startswith(rule['material_prefix']) and min(mesh.vertices[i].co.z for i in tri.vertices)>=rule['minimum_source_z'] and all(x0<=v.x%1<=x1 and y0<=(1-v.y)%1<=y1 for v in uv)
iris=[t for t in mesh.loop_triangles if matching(rules[0],t)]
white=[t for t in mesh.loop_triangles if matching(rules[1],t)]
tree=BVHTree.FromPolygons([v.co for v in mesh.vertices],[tuple(t.vertices) for t in white],all_triangles=True)
hits=[];back=[];normals=[]
for tri in iris:
    centre=sum((mesh.vertices[i].co for i in tri.vertices),Vector())/3
    n=sum((mesh.corner_normals[i].vector for i in tri.loops),Vector()).normalized()
    normals.append(tuple(n))
    for sign,dest in [(1,hits),(-1,back)]:
        q,_,_,d=tree.ray_cast(centre+n*sign*.00001,n*sign,.05)
        if q is not None:dest.append(d)
report={'iris_triangles':len(iris),'white_triangles':len(white),'white_in_front':len(hits),'front_depth_min_max':[min(hits),max(hits)] if hits else [],'white_behind':len(back),'back_depth_min_max':[min(back),max(back)] if back else [],'mean_normal':tuple(sum((Vector(n) for n in normals),Vector())/len(normals))}
(OUT/'blender'/('eye-layer-measurement-edited.json' if edited else 'eye-layer-measurement.json')).write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
