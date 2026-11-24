"""Give the existing palette-mapped shirt nondegenerate UVs; retain all geometry."""
import bpy,json,math,shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
model='source_c6501';stem='46c953e3735925da-4998c4df7ee95149-00001993'
path=OUT/'blender/edited'/f'{model}.blend'
archive=OUT/'blender/pre-shirt-uv';archive.mkdir(exist_ok=True)
if not (archive/path.name).exists():shutil.copy2(path,archive/path.name)
bpy.ops.wm.open_mainfile(filepath=str(path))
obj=bpy.data.objects['UYA_'+model+'_captured_pose'];mesh=obj.data
bpy.ops.object.select_all(action='DESELECT');obj.hide_set(False);obj.select_set(True);bpy.context.view_layer.objects.active=obj
new=mesh.uv_layers.get('Shirt_Detail_UV') or mesh.uv_layers.new(name='Shirt_Detail_UV')
old=mesh.uv_layers['Original_UYA_UV']
for i in range(len(mesh.loops)):new.data[i].uv=old.data[i].uv
mesh.uv_layers.active=new;new.active_render=True
selected=[]
bpy.context.tool_settings.mesh_select_mode=(False,False,True)
for v in mesh.vertices:v.select=False
for edge in mesh.edges:edge.select=False
for face in mesh.polygons:
    material=mesh.materials[face.material_index]
    face.select=material.get('review_source_stem',material.name.split('.')[0])==stem
    if face.select:selected.append(face.index)
assert selected,'Shirt material absent'
# This shirt is the forward-facing chest inset. Project across the body (Y)
# and vertically (Z), keeping neighbouring triangles continuous despite the
# source mesh's split vertex stream. A common scale keeps the weave square.
points=[mesh.vertices[mesh.loops[i].vertex_index].co for f in mesh.polygons if f.index in selected for i in f.loop_indices]
low_y=min(p.y for p in points);high_y=max(p.y for p in points)
low_z=min(p.z for p in points);high_z=max(p.z for p in points)
scale=.90/max(high_y-low_y,high_z-low_z)
for face in mesh.polygons:
    if face.index in selected:
        for i in face.loop_indices:
            p=mesh.vertices[mesh.loops[i].vertex_index].co
            new.data[i].uv=(.5+(p.y-(low_y+high_y)*.5)*scale,.5+(p.z-(low_z+high_z)*.5)*scale)
old=mesh.uv_layers['Original_UYA_UV'];new=mesh.uv_layers['Shirt_Detail_UV']
for face in mesh.polygons:
    if face.index not in selected:
        for i in face.loop_indices:new.data[i].uv=old.data[i].uv
mesh.calc_loop_triangles();triangles=[]
for tri in mesh.loop_triangles:
    if tri.polygon_index not in selected:continue
    a=[list(new.data[i].uv) for i in tri.loops]
    area=abs((a[1][0]-a[0][0])*(a[2][1]-a[0][1])-(a[1][1]-a[0][1])*(a[2][0]-a[0][0]))
    assert area>1e-10,'Degenerate shirt UV after unwrap'
    triangles.append(dict(old=[[old.data[i].uv.x,1-old.data[i].uv.y] for i in tri.loops],new=[[new.data[i].uv.x,1-new.data[i].uv.y] for i in tri.loops]))
(OUT/'authored/big-al-shirt-uv.json').write_text(json.dumps(dict(model=model,stem=stem,triangles=triangles,total_triangles=len(mesh.loop_triangles),geometry='Unchanged; shirt UVs only'),indent=2)+'\n')
bpy.ops.wm.save_as_mainfile(filepath=str(path))
print('Unwrapped',len(triangles),'shirt triangles; geometry unchanged')
