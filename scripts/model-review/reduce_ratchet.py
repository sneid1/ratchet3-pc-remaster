"""Replace blanket subdivision while transplanting accepted eye layers exactly.

Uses the existing original kit mesh, selective refinement and native bindings.
Protected layers retain every accepted triangle, UV, normal and animation record.
"""
import bpy,hashlib,json,math,runpy,shutil,struct,sys
from pathlib import Path
from mathutils.bvhtree import BVHTree
HERE=Path(__file__).resolve().parent;OUT=HERE.parents[1]/'captures/rift-texture-review'
sys.path.insert(0,str(HERE))
from adaptive_refinement import build
archive=OUT/'blender/pre-ratchet-reduction';archive.mkdir(exist_ok=True)
for suffix in ['.blend','.r3edit','.json']:
    source=OUT/'blender/edited'/('ratchet'+suffix)
    if not (archive/source.name).exists():shutil.copy2(source,archive/source.name)
raw=(archive/'ratchet.r3edit').read_bytes()
signature,source_count,material_count,vertex_count=struct.unpack_from('<8sIII',raw)
assert signature==b'R3EDIT02'
offset=20
for _ in range(material_count):
    length=struct.unpack_from('<H',raw,offset)[0];offset+=2+length+4
offset+=source_count*12

bpy.ops.wm.open_mainfile(filepath=str(OUT/'blender/smooth/ratchet.blend'))
obj=bpy.data.objects['UYA_ratchet_captured_pose'];source=obj.data
source.calc_loop_triangles()
rules=json.loads((HERE/'surface-constraints.json').read_text())['ratchet']
protected=set();rule_counts={}
for rule in rules:
    matches=[];x0,y0,x1,y1=rule['uv_bounds']
    for tri in source.loop_triangles:
        if not source.materials[tri.material_index].name.startswith(rule['material_prefix']):continue
        if min(source.vertices[i].co.z for i in tri.vertices)<rule['minimum_source_z']:continue
        coords=[source.uv_layers.active.data[i].uv for i in tri.loops]
        if all(x0<=t.x%1<=x1 and y0<=(1-t.y)%1<=y1 for t in coords):matches.append(min(tri.vertices)//3)
    assert matches,'Missing accepted source layer: '+rule['label']
    protected.update(matches);rule_counts[rule['label']]=len(matches)

preserved=[]
for i in range(vertex_count//3):
    record=raw[offset+i*180:offset+(i+1)*180]
    if struct.unpack_from('<I',record)[0]//3 in protected:preserved.append(record)
assert preserved and len(preserved)<6000
source_faces=sum((source.vertices[t.vertices[1]].co-source.vertices[t.vertices[0]].co).cross(source.vertices[t.vertices[2]].co-source.vertices[t.vertices[0]].co).length_squared>=1e-12 for t in source.loop_triangles)
protected_valid=sum(min(t.vertices)//3 in protected and (source.vertices[t.vertices[1]].co-source.vertices[t.vertices[0]].co).cross(source.vertices[t.vertices[2]].co-source.vertices[t.vertices[0]].co).length_squared>=1e-12 for t in source.loop_triangles)
total_cap=math.floor(source_faces*1.75)
adaptive_cap=total_cap-len(preserved)+protected_valid
assert adaptive_cap>=source_faces
surface=BVHTree.FromPolygons([v.co for v in source.vertices],[tuple(t.vertices) for t in source.loop_triangles],all_triangles=True)
lighting=(OUT/'candidate-blender-ratchet.vertex-lighting.bin').read_bytes()
values,report=build(source,lighting,surface,1.0,1.75,.0008,protected,adaptive_cap)
positions,normals,uvs,bindings,faces,materials=values
kept=[i for i,f in enumerate(faces) if bindings[f[0]][0][0]//3 not in protected]
faces=[faces[i] for i in kept];materials=[materials[i] for i in kept]
# Preserve original corner data from the accepted editable file as well as the
# exact native records. No reprojection or new eye-fitting operation occurs.
source_material_names=[m.get('review_source_stem',m.name.split('.')[0]) for m in source.materials]
original_matrix=obj.matrix_world.copy()
bpy.ops.wm.open_mainfile(filepath=str(archive/'ratchet.blend'))
accepted=bpy.data.objects['UYA_ratchet_captured_pose'];dense=accepted.data;dense.calc_loop_triangles()
assert accepted.matrix_world==original_matrix
grafted=0
for tri in dense.loop_triangles:
    first=tri.vertices[0]
    if dense.attributes['review_src0'].data[first].value//3 not in protected:continue
    face=[]
    for loop_index in tri.loops:
        v=dense.loops[loop_index].vertex_index
        face.append(len(positions));positions.append(tuple(dense.vertices[v].co))
        normals.append(tuple(dense.corner_normals[loop_index].vector))
        uvs.append(tuple(dense.uv_layers.active.data[loop_index].uv))
        bindings.append((tuple(dense.attributes['review_src'+str(k)].data[v].value for k in range(3)),
                         tuple(dense.attributes['review_bary'+str(k)].data[v].value for k in range(3))))
    faces.append(tuple(face))
    mat=dense.materials[tri.material_index]
    materials.append(source_material_names.index(mat.get('review_source_stem',mat.name.split('.')[0])))
    grafted+=1
assert grafted==len(preserved) and len(faces)<=total_cap
mesh=bpy.data.meshes.new('ratchet_budgeted_original_with_accepted_eyes')
mesh.from_pydata(positions,[],faces);mesh.update()
for name in source_material_names:mesh.materials.append(next(m for m in dense.materials if m.get('review_source_stem',m.name.split('.')[0])==name))
layer=mesh.uv_layers.new(name='Original_UYA_UV')
for loop in mesh.loops:layer.data[loop.index].uv=uvs[loop.vertex_index]
for polygon,index in zip(mesh.polygons,materials):polygon.material_index=index;polygon.use_smooth=True
mesh.normals_split_custom_set([normals[l.vertex_index] for l in mesh.loops])
for k in range(3):
    attr=mesh.attributes.new('review_src'+str(k),'INT','POINT');attr.data.foreach_set('value',[b[0][k] for b in bindings])
    attr=mesh.attributes.new('review_bary'+str(k),'FLOAT','POINT');attr.data.foreach_set('value',[b[1][k] for b in bindings])
accepted.data=mesh
accepted['Refinement']='Budgeted original surfaces with exact accepted iris/white-eye geometry graft; no refitting'
target=OUT/'blender/refined/ratchet.blend'
bpy.ops.wm.save_as_mainfile(filepath=str(target))
sys.argv=['blender','--','--model','ratchet','--blend',str(target)]
runpy.run_path(str(HERE/'blender_roundtrip.py'),run_name='__main__')
# Native eye binding records remain byte-exact, avoiding floating-point changes
# from a second export of their local tangent-frame offsets.
export=OUT/'blender/edited/ratchet.r3edit';data=export.read_bytes()
assert len(data[-grafted*180:])==grafted*180
data=data[:-grafted*180]+b''.join(preserved);export.write_bytes(data)
metadata=json.loads(export.with_suffix('.json').read_text())
metadata.update(sha256=hashlib.sha256(data).hexdigest(),accepted_eye_triangles=grafted,
                accepted_eye_records_sha256=hashlib.sha256(b''.join(preserved)).hexdigest())
export.with_suffix('.json').write_text(json.dumps(metadata,indent=2)+'\n')
report.update(model='ratchet',source_triangles=source_count//3,previous_triangles=vertex_count//3,
              refined_triangles=len(faces),accepted_eye_triangles=grafted,eye_records_byte_exact=True,
              accepted_eye_records_sha256=metadata['accepted_eye_records_sha256'],protected_source_triangles=rule_counts,
              total_triangle_cap=total_cap,triangle_reduction=1-len(faces)/(vertex_count/3),
              method='adaptive original surfaces plus exact accepted eye-layer transplant',archive=str(archive))
(OUT/'blender/refined/ratchet.json').write_text(json.dumps(report,indent=2)+'\n')
(OUT/'ratchet-reduction.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
