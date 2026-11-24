"""Editable original UYA meshes and short-distance geometry AO bakes in Blender.

The input is the real native decoder's OBJ export, not reconstructed proxy shapes.
Posed meshes are labelled as such; no rig or animation compatibility is implied.
"""
import ast
import argparse
import bpy
import json
import math
from pathlib import Path
import struct
import sys
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
TARGET=OUT/'blender'
PACK=OUT/'candidate/0-riftapart'
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--model',action='append',help='Captured asset key; repeat to assemble a workbench')
parser.add_argument('--output',type=Path,default=TARGET/'uya-character-workbench.blend')
parser.add_argument('--no-bake',action='store_true')
args=parser.parse_args(sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else [])
names=args.model or list(json.loads(Path(__file__).with_name('review-assets.json').read_text()))
for name in names:
    if not name.replace('_','').isalnum():raise ValueError('Expected a captured asset key')


def import_capture(name):
    path=OUT/f'candidate-blender-{name}.obj'
    positions=[];uv=[];normals=[];faces=[];face_materials=[];materials=[];current=0
    for line in path.read_text().splitlines():
        parts=line.split()
        if not parts:continue
        if parts[0]=='v':positions.append(tuple(map(float,parts[1:4])))
        elif parts[0]=='vt':uv.append(tuple(map(float,parts[1:3])))
        elif parts[0]=='vn':normals.append(tuple(map(float,parts[1:4])))
        elif parts[0]=='usemtl':
            if parts[1] not in materials:materials.append(parts[1])
            current=materials.index(parts[1])
        elif parts[0]=='f':
            faces.append([int(p.split('/')[0])-1 for p in parts[1:]])
            face_materials.append(current)
    lighting=(path.with_suffix('.vertex-lighting.bin')).read_bytes()
    # Native PS2 skinned/rigid normals face inward. Blender AO and modelling
    # need outward normals; the round-trip writer restores native convention.
    for i,normal in enumerate(normals):
        word=struct.unpack_from('<I',lighting,i*20+12)[0]
        if ((word>>16)&3)>=2:normals[i]=tuple(-v for v in normal)
    flipped=0
    for face in faces:
        a,b,c=(Vector(positions[i]) for i in face)
        expected=sum((Vector(normals[i]) for i in face),Vector())
        if (b-a).cross(c-a).dot(expected)<0:
            face[1],face[2]=face[2],face[1];flipped+=1
    mesh=bpy.data.meshes.new(name)
    mesh.from_pydata(positions,[],faces);mesh.update()
    obj=bpy.data.objects.new('UYA_'+name+'_captured_pose',mesh)
    bpy.context.collection.objects.link(obj)
    layer=mesh.uv_layers.new(name='Original_UYA_UV')
    # The game repeats atlas UVs; many mobies store a whole atlas at U=-1..0.
    # Put each source triangle in the base tile for Blender's image bake. The
    # translation is an integer repeat, so native texture sampling is unchanged.
    for poly in mesh.polygons:
        coords=[uv[mesh.loops[i].vertex_index] for i in poly.loop_indices]
        tile=[math.floor(sum(c[k] for c in coords)/len(coords)) for k in range(2)]
        for i,c in zip(poly.loop_indices,coords):layer.data[i].uv=(c[0]-tile[0],c[1]-tile[1])
    for poly,index in zip(mesh.polygons,face_materials):poly.material_index=index;poly.use_smooth=True
    mesh.normals_split_custom_set([normals[l.vertex_index] for l in mesh.loops])
    for stem in materials:
        mat=bpy.data.materials.new(stem);mat.use_nodes=True
        nodes=mat.node_tree.nodes;links=mat.node_tree.links
        shader=nodes.get('Principled BSDF')
        image_path=PACK/'replacements'/f'{stem}.png'
        if not image_path.exists():image_path=OUT/'dumps'/f'{stem}.png'
        if not image_path.exists():image_path=Path.home()/'Games/Ratchet and Clank 3/texture_dumps'/f'{stem}.png'
        if image_path.exists():
            tex=nodes.new('ShaderNodeTexImage');tex.image=bpy.data.images.load(str(image_path),check_existing=True)
            links.new(tex.outputs['Color'],shader.inputs['Base Color'])
        mesh.materials.append(mat)
    obj['Source']=path.read_text().splitlines()[0]
    obj['Winding corrected to authored normals']=flipped
    obj['review_outward_normals']=True
    return obj


def bake_ao(obj):
    bpy.ops.object.select_all(action='DESELECT')
    obj.select_set(True);bpy.context.view_layer.objects.active=obj
    images=[];restore=[]
    # 2% of body height: local creases, not a frozen shadow from nearby limbs.
    radius=max(obj.dimensions.z*.02,.015)
    for mat in obj.data.materials:
        image=bpy.data.images.new(mat.name+'_cavity_AO',512,512,alpha=False)
        image.generated_color=(1,1,1,1);image.colorspace_settings.name='Non-Color'
        nodes=mat.node_tree.nodes;links=mat.node_tree.links
        tex=nodes.new('ShaderNodeTexImage');tex.image=image;nodes.active=tex
        ao=nodes.new('ShaderNodeAmbientOcclusion');ao.inputs['Distance'].default_value=radius;ao.only_local=True
        geometry=nodes.new('ShaderNodeNewGeometry')
        links.new(geometry.outputs['True Normal'],ao.inputs['Normal'])
        emission=nodes.new('ShaderNodeEmission');links.new(ao.outputs['AO'],emission.inputs['Color'])
        output=nodes.get('Material Output');old=output.inputs['Surface'].links[0].from_socket
        links.new(emission.outputs[0],output.inputs['Surface'])
        restore.append((mat,output,old,ao,emission,geometry));images.append(image)
    bpy.ops.object.bake(type='EMIT',margin=8,use_clear=False)
    for mat,image in zip(obj.data.materials,images):
        image.filepath_raw=str(TARGET/(mat.name+'_ao.png'));image.file_format='PNG';image.save()
    for mat,output,old,ao,emission,geometry in restore:
        mat.node_tree.links.new(old,output.inputs['Surface'])
        mat.node_tree.nodes.remove(ao);mat.node_tree.nodes.remove(emission)
        mat.node_tree.nodes.remove(geometry)
    return radius


bpy.ops.object.select_all(action='SELECT');bpy.ops.object.delete(use_global=False)
scene=bpy.context.scene
scene.render.engine='CYCLES';scene.cycles.samples=32;scene.cycles.device='CPU'
scene.render.threads_mode='FIXED';scene.render.threads=4
scene.world.color=(0,0,0)
records=[]
for name in names:
    obj=import_capture(name)
    bpy.context.view_layer.update()
    radius=None if args.no_bake else bake_ao(obj)
    records.append({'name':name,'triangles':len(obj.data.polygons),'materials':[m.name for m in obj.data.materials],
                    'ao_radius':radius,'ao_samples':32,'source':f'candidate-blender-{name}.obj'})
    obj.hide_render=True;obj.hide_set(True)
# The stored scene uses the actual captured Ranger light set, with no studio rig.
light_asset='ranger' if 'ranger' in names else names[0]
values=struct.unpack('<256f',(OUT/f'candidate-blender-{light_asset}.lights.bin').read_bytes())
words=ast.literal_eval((OUT/f'candidate-blender-{light_asset}.lights.txt').read_text())
word=max((w for w in words if w),key=lambda w:words[w]);index=word&15
for k in range(2):
    offset=index*16+k*8;colour=values[offset:offset+3];direction=Vector(values[offset+4:offset+7])
    data=bpy.data.lights.new(f'Veldin_set{index}_light{k}','SUN')
    data.color=tuple(max(0,v) for v in colour);data.energy=1
    light=bpy.data.objects.new(data.name,data);scene.collection.objects.link(light)
    ray=direction if ((word>>16)&3)>=2 else -direction
    light.rotation_euler=ray.to_track_quat('-Z','Y').to_euler()
    light['Source']='Captured UYA RAM light set; no synthetic studio light.'
scene['Lighting note']='Captured directional light colours/directions. Native viewer remains authoritative for UYA vertex light terms and final shading.'
ranger=bpy.data.objects['UYA_'+light_asset+'_captured_pose'];ranger.hide_render=False;ranger.hide_set(False)
bpy.ops.wm.save_as_mainfile(filepath=str(args.output.resolve()))
args.output.with_suffix('.bakes.json').write_text(json.dumps(records,indent=2))
print(json.dumps(records))
