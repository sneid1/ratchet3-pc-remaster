"""Wire the authored maps into the editable Blender file and render inspection views.

Only captured Veldin directional lights are used. These are Blender comparison
views; final approval uses the native renderer's complete lighting calculation.
"""
import ast
import bpy
import csv
import json
import math
from mathutils import Vector
from pathlib import Path
import struct
import sys

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
TARGET=OUT/'blender'
PACK=OUT/'candidate/0-riftapart'

def captured_ambient():
    values=struct.unpack('<256f',(OUT/'candidate-blender-ranger.lights.bin').read_bytes())
    data=(OUT/'candidate-blender-ranger.vertex-lighting.bin').read_bytes()
    total=Vector();count=0
    for off in range(0,len(data),20):
        nx,ny,nz,share,s0,s1,word,r,g,b,a=struct.unpack_from('<4b2fI4B',data,off)
        if not word&0x80000000:continue
        ia=(word&15)*16;ib=((word>>4)&15)*16;w=((word>>8)&255)/256
        c0=Vector(tuple(values[ia+k]*(1-w)+values[ib+k]*w for k in range(3)))
        c1=Vector(tuple(values[ia+8+k]*(1-w)+values[ib+8+k]*w for k in range(3)))
        scale=.25 if ((word>>16)&3)==3 else 1
        base=Vector((r/128,g/128,b/128))-scale*(c0*s0+c1*s1)
        total+=Vector(tuple(max(0,x) for x in base));count+=1
    if not count:raise ValueError('No captured vertex ambient lighting')
    return tuple(total/count)

def camera_fit(objects,yaw=45):
    scene=bpy.context.scene
    camera=bpy.data.objects.get('ReviewCamera')
    if not camera:
        camera=bpy.data.objects.new('ReviewCamera',bpy.data.cameras.new('ReviewCamera'));scene.collection.objects.link(camera)
    points=[o.matrix_world@Vector(p) for o in objects for p in o.bound_box]
    lo=Vector(tuple(min(p[k] for p in points) for k in range(3)));hi=Vector(tuple(max(p[k] for p in points) for k in range(3)))
    center=(lo+hi)*.5;radius=(hi-lo).length*.5
    angle=math.radians(yaw);direction=Vector((math.cos(angle),math.sin(angle),.12)).normalized()
    camera.location=center+direction*radius*3.8
    camera.rotation_euler=(center-camera.location).to_track_quat('-Z','Y').to_euler()
    camera.data.lens=55;camera.data.clip_end=1000;scene.camera=camera
    return {'min':list(lo),'max':list(hi)}

def render(path):
    scene=bpy.context.scene;scene.render.engine='CYCLES';scene.cycles.samples=24
    scene.render.threads_mode='FIXED';scene.render.threads=4
    scene.render.resolution_x=1200;scene.render.resolution_y=1000;scene.render.resolution_percentage=100
    scene.world.use_nodes=True
    nodes=scene.world.node_tree.nodes;links=scene.world.node_tree.links
    ambient=nodes.get('Background');ambient.inputs['Color'].default_value=(*captured_ambient(),1)
    black=nodes.new('ShaderNodeBackground');black.inputs['Color'].default_value=(0,0,0,1)
    lightpath=nodes.new('ShaderNodeLightPath');mix=nodes.new('ShaderNodeMixShader')
    links.new(lightpath.outputs['Is Camera Ray'],mix.inputs[0]);links.new(ambient.outputs[0],mix.inputs[1]);links.new(black.outputs[0],mix.inputs[2])
    links.new(mix.outputs[0],nodes.get('World Output').inputs[0])
    for light in (o.data for o in scene.objects if o.type=='LIGHT'):light.energy=math.pi
    scene.view_settings.view_transform='Standard';scene.render.image_settings.file_format='PNG';scene.render.filepath=str(path)
    bpy.ops.render.render(write_still=True)

bpy.ops.wm.open_mainfile(filepath=str(TARGET/'uya-character-workbench.blend'))
rows=list(csv.DictReader((PACK/'pbr.tsv').open(),delimiter='\t'))
by_stem={row['stem']:row for row in rows}
for mat in list(bpy.data.materials):
    row=by_stem.get(mat.name.split('.')[0])
    if row is None:continue
    nodes=mat.node_tree.nodes;links=mat.node_tree.links;shader=nodes.get('Principled BSDF')
    for channel,key in [('Normal','normal_map'),('ORM','orm_map')]:
        tex=nodes.new('ShaderNodeTexImage');tex.image=bpy.data.images.load(str(PACK/'materials'/(row[key]+'.png')),check_existing=True)
        tex.image.colorspace_settings.name='Non-Color';tex.label=channel
        if channel=='Normal':
            normal=nodes.new('ShaderNodeNormalMap');links.new(tex.outputs['Color'],normal.inputs['Color']);links.new(normal.outputs['Normal'],shader.inputs['Normal'])
        else:
            split=nodes.new('ShaderNodeSeparateColor');links.new(tex.outputs['Color'],split.inputs['Color'])
            links.new(split.outputs['Green'],shader.inputs['Roughness']);links.new(split.outputs['Blue'],shader.inputs['Metallic'])
            split.label='R: geometry AO (indirect only in native renderer) / G: roughness / B: metalness'
    # Recover source colour for an atlas absent from the candidate replacement folder.
    if not shader.inputs['Base Color'].is_linked:
        path=Path.home()/'Games/Ratchet and Clank 3/texture_dumps'/(row['stem']+'.png')
        tex=nodes.new('ShaderNodeTexImage');tex.image=bpy.data.images.load(str(path),check_existing=True)
        links.new(tex.outputs['Color'],shader.inputs['Base Color'])
ranger=bpy.data.objects['UYA_ranger_captured_pose']
camera_fit([ranger],45)
render(TARGET/'uya-ranger-blender.png')
bpy.ops.wm.save_as_mainfile(filepath=str(TARGET/'uya-character-workbench.blend'))
if '--uya-only' in sys.argv:
    raise SystemExit(0)

# Inspect the actual RA Clank import with these same captured directional lights.
bpy.ops.wm.open_mainfile(filepath=str(TARGET/'rift-model-inspection.blend'))
scene=bpy.context.scene
collection=bpy.data.collections['hero_clank_ps4'];collection.hide_viewport=False;collection.hide_render=False
meshes=[]
for obj in collection.objects:
    if obj.type=='MESH' and '-subset' in obj.name:meshes.append(obj)
    elif obj.type=='MESH':obj.hide_render=True # glTF importer's bone display shape
for mat in bpy.data.materials:
    if 'clank' not in mat.name.lower():continue
    mat.use_nodes=True;nodes=mat.node_tree.nodes;links=mat.node_tree.links;shader=nodes.get('Principled BSDF')
    family=next((k for k in ('helipack','insides','head','limbs','body','eyeinner') if k in mat.name.lower()),None)
    if family:
        source=Path('C:/riftapart-export/more/characters/hero/hero_clank/textures')/f'hero_clank_{family}_c.png'
        if source.exists():
            tex=nodes.new('ShaderNodeTexImage');tex.image=bpy.data.images.load(str(source),check_existing=True)
            links.new(tex.outputs['Color'],shader.inputs['Base Color'])
        # This character _g is specular RGB + gloss alpha, unlike the tiling
        # library's packed R=gloss/G=reflectance maps. Channel inspection is
        # recorded in source-channels.log; never infer packing from _g alone.
        spec=source.with_name(f'hero_clank_{family}_g.png')
        if spec.exists() and family in ('head','body','limbs','helipack'):
            tex=nodes.new('ShaderNodeTexImage');tex.image=bpy.data.images.load(str(spec),check_existing=True);tex.image.colorspace_settings.name='sRGB'
            links.new(tex.outputs['Color'],shader.inputs['Base Color'])
            inverse=nodes.new('ShaderNodeMath');inverse.operation='SUBTRACT';inverse.inputs[0].default_value=1;links.new(tex.outputs['Alpha'],inverse.inputs[1]);links.new(inverse.outputs[0],shader.inputs['Roughness'])
    shader.inputs['Metallic'].default_value=1 if family in ('head','body','limbs','helipack') else 0
    shader.inputs['Roughness'].default_value=.4
values=struct.unpack('<256f',(OUT/'candidate-blender-ranger.lights.bin').read_bytes())
words=ast.literal_eval((OUT/'candidate-blender-ranger.lights.txt').read_text());index=max((w for w in words if w),key=lambda w:words[w])&15
for k in range(2):
    offset=index*16+k*8;data=bpy.data.lights.new(f'Veldin_light{k}','SUN');data.color=tuple(max(0,v) for v in values[offset:offset+3]);data.energy=1
    light=bpy.data.objects.new(data.name,data);scene.collection.objects.link(light)
    light.rotation_euler=(-Vector(values[offset+4:offset+7])).to_track_quat('-Z','Y').to_euler()
bpy.context.view_layer.update()
bounds=camera_fit(meshes,-90)
render(TARGET/'rift-clank-import.png')
bpy.ops.wm.save_as_mainfile(filepath=str(TARGET/'rift-clank-inspection.blend'))
(TARGET/'rift-clank-bounds.json').write_text(json.dumps(bounds))
