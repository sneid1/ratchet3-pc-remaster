"""Export an edited Blender mesh for the native black-room renderer.

Bind edited vertices to triangles on the original captured surface. Each real
recorded pose then deforms that surface binding. This supports topology edits
without inventing animation; a changed source LOD/UV layout is rejected at load.
This is a preview binding, not a substitute for a final game skeleton/weight export.
"""
import argparse
import bpy
import bmesh
import hashlib
import json
from mathutils import Vector
from mathutils.bvhtree import BVHTree
from pathlib import Path
import struct
import sys

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
parser=argparse.ArgumentParser()
parser.add_argument('--model',required=True)
parser.add_argument('--blend',type=Path,default=OUT/'blender/uya-character-workbench.blend')
parser.add_argument('--bevel',type=float,default=0,help='Optional reusable bevel in captured world units; leaves the source blend untouched')
args=parser.parse_args(sys.argv[sys.argv.index('--')+1:])
if not args.model.replace('_','').isalnum():raise ValueError('Model must be an asset identifier')
positions=[];source_uv=[];source_materials={};faces=[];face_materials=[];materials=[];current=None
asset=json.loads(Path(__file__).with_name('review-assets.json').read_text())[args.model]
animated_uv=asset.get('animated_uv_materials',[])
source_lighting=(OUT/f'candidate-blender-{args.model}.vertex-lighting.bin').read_bytes()
for line in (OUT/f'candidate-blender-{args.model}.obj').read_text().splitlines():
    p=line.split()
    if not p:continue
    if p[0]=='v':positions.append(Vector(tuple(map(float,p[1:4]))))
    elif p[0]=='vt':source_uv.append((float(p[1]),1-float(p[2])))
    elif p[0]=='usemtl':
        current=p[1]
        if current not in materials:materials.append(current)
    elif p[0]=='f':
        face=[int(v.split('/')[0])-1 for v in p[1:4]]
        for i in face:source_materials[i]=current
        if (positions[face[1]]-positions[face[0]]).cross(positions[face[2]]-positions[face[0]]).length_squared>1e-12:
            faces.append(face);face_materials.append(current)
bpy.ops.wm.open_mainfile(filepath=str(args.blend.resolve()))
obj=bpy.data.objects.get('UYA_'+args.model+'_captured_pose')
if obj is None or obj.type!='MESH':raise ValueError('Missing editable model '+args.model)
obj.hide_set(False);obj.hide_render=False
bpy.context.view_layer.objects.active=obj;obj.select_set(True)
if args.bevel:
    # Weld the native triangle stream before adding physically connected edges.
    bm=bmesh.new();bm.from_mesh(obj.data);bmesh.ops.remove_doubles(bm,verts=list(bm.verts),dist=.00001);bm.to_mesh(obj.data);bm.free()
    bevel=obj.modifiers.new('Reusable edge bevel','BEVEL');bevel.width=args.bevel;bevel.segments=2;bevel.limit_method='ANGLE';bevel.angle_limit=.6
    bevel.harden_normals=True
depsgraph=bpy.context.evaluated_depsgraph_get();evaluated=obj.evaluated_get(depsgraph)
mesh=evaluated.to_mesh();mesh.calc_loop_triangles()
trees={}
for material in materials:
    indices=[i for i,m in enumerate(face_materials) if m==material]
    trees[material]=(BVHTree.FromPolygons(positions,[faces[i] for i in indices],all_triangles=True),indices)
data=bytearray();max_distance=0;bound=0
uv=mesh.uv_layers.active
if uv is None:raise ValueError('Edited mesh has no UV map')
for tri in mesh.loop_triangles:
    slot_material=mesh.materials[tri.material_index]
    material=slot_material.get('review_source_stem',slot_material.name.removesuffix('.001'))
    if material not in trees:raise ValueError('Author a texture mapping for new material '+material)
    tree,index_map=trees[material]
    for loop_index in tri.loops:
        loop=mesh.loops[loop_index];p=evaluated.matrix_world@mesh.vertices[loop.vertex_index].co
        if 'review_src0' in mesh.attributes:
            ids=[mesh.attributes['review_src'+str(k)].data[loop.vertex_index].value for k in range(3)]
            bary=tuple(mesh.attributes['review_bary'+str(k)].data[loop.vertex_index].value for k in range(3))
            a,b,c=[positions[i] for i in ids];ab=b-a;ac=c-a
            hit=sum((positions[ids[k]]*bary[k] for k in range(3)),Vector())
            distance=(p-hit).length
        else:
            hit,_,nearest,distance=tree.find_nearest(p)
            if hit is None:raise ValueError('Could not bind edited surface')
            ids=faces[index_map[nearest]];a,b,c=[positions[i] for i in ids]
            ab=b-a;ac=c-a;ap=hit-a
            d00=ab.dot(ab);d01=ab.dot(ac);d11=ac.dot(ac);den=d00*d11-d01*d01
            v=(d11*ap.dot(ab)-d01*ap.dot(ac))/den;w=(d00*ap.dot(ac)-d01*ap.dot(ab))/den
            bary=(1-v-w,v,w)
        max_distance=max(max_distance,distance)
        x=ab.normalized();z=ab.cross(ac).normalized();y=z.cross(x)
        offset=p-hit;normal=evaluated.matrix_world.to_3x3()@mesh.corner_normals[loop_index].vector
        word=struct.unpack_from('<I',source_lighting,ids[0]*20+12)[0]
        if obj.get('review_outward_normals',False) and ((word>>16)&3)>=2:normal=-normal
        local_offset=tuple(offset.dot(axis) for axis in (x,y,z));local_normal=tuple(normal.dot(axis) for axis in (x,y,z))
        coordinate=uv.data[loop_index].uv
        data.extend(struct.pack('<3I3f3f3f2fI',*ids,*bary,*local_offset,*local_normal,coordinate.x,1-coordinate.y,materials.index(material)))
        bound+=1
edited=OUT/'blender/edited';edited.mkdir(exist_ok=True)
target=edited/(args.model+'.r3edit')
with target.open('wb') as stream:
    stream.write(b'R3EDIT02'+struct.pack('<3I',len(positions),len(materials),bound))
    for name in materials:
        encoded=name.encode();stream.write(struct.pack('<H',len(encoded))+encoded)
        stream.write(struct.pack('<I',int(name in animated_uv)))
    for i,value in enumerate(source_uv):stream.write(struct.pack('<2fI',*value,materials.index(source_materials[i])))
    stream.write(data)
evaluated.to_mesh_clear()
bpy.ops.wm.save_as_mainfile(filepath=str(edited/(args.model+'.blend')))
report={'model':args.model,'source':str(args.blend),'original_vertices':len(positions),'edited_triangles':bound//3,
        'materials':materials,'animated_uv_materials':animated_uv,'maximum_surface_offset':max_distance,'bevel':args.bevel,
        'animation':'Surface-bound to the actual recorded UYA poses. Source UV/count mismatch is rejected. Final game rig weights remain a separate integration step.'}
if asset.get('source_layer') in ('source','body','shrub','terrain'):
    report['animation']='Bound to the original static source surface for isolated art review. This file does not establish animation or final game rig compatibility.'
report['sha256']=hashlib.sha256(target.read_bytes()).hexdigest()
report['source_obj_sha256']=hashlib.sha256((OUT/f'candidate-blender-{args.model}.obj').read_bytes()).hexdigest()
target.with_suffix('.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report))
