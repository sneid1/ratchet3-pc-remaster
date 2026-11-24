"""Check the actual saved NPC Blender deliveries and export portable static GLBs."""
import bpy
import json
import math
import struct
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
DEST=OUT/'deliverables/imagegen-layers'
report=[]
for key,model in [('nefarious','source_c7662'),('courtney','source_c7109'),('skrunch','source_c7969')]:
    path=DEST/'models'/(model+'.blend')
    bpy.ops.wm.open_mainfile(filepath=str(path))
    obj=bpy.data.objects['UYA_'+model+'_captured_pose']
    mesh=obj.data;mesh.calc_loop_triangles()
    metadata=json.loads((OUT/'blender/edited'/(model+'.json')).read_text())
    assert len(mesh.loop_triangles)==metadata['edited_triangles']
    assert mesh.uv_layers.active and len(mesh.uv_layers.active.data)==len(mesh.loops)
    assert all(math.isfinite(c) for v in mesh.vertices for c in v.co)
    assert all(math.isfinite(c) for uv in mesh.uv_layers.active.data for c in uv.uv)
    images={n.image.name:n.image for mat in mesh.materials for n in mat.node_tree.nodes if n.type=='TEX_IMAGE' and n.image}
    assert images and all(im.packed_file for im in images.values())
    assert all(im.size[0]>0 and im.size[1]>0 for im in images.values())
    for other in bpy.context.scene.objects:other.select_set(False)
    obj.hide_set(False);obj.hide_viewport=False;obj.select_set(True)
    bpy.context.view_layer.objects.active=obj
    folder=DEST/key;folder.mkdir(exist_ok=True)
    glb=folder/(model+'.glb')
    bpy.ops.export_scene.gltf(filepath=str(glb),export_format='GLB',use_selection=True,export_materials='EXPORT')
    raw=glb.read_bytes()
    magic,version,length=struct.unpack_from('<4sII',raw)
    assert magic==b'glTF' and version==2 and length==len(raw)
    size,kind=struct.unpack_from('<II',raw,12);assert kind==0x4e4f534a
    data=json.loads(raw[20:20+size])
    triangles=0
    for part in [q for m in data['meshes'] for q in m['primitives']]:
        assert part.get('mode',4)==4
        assert all(k in part['attributes'] for k in ['POSITION','NORMAL','TEXCOORD_0'])
        triangles+=data['accessors'][part['indices']]['count']//3
    assert triangles==metadata['edited_triangles']
    assert len(data['materials'])==len(mesh.materials)
    assert data['images'] and all('bufferView' in image for image in data['images'])
    report.append(dict(model=model,triangles=len(mesh.loop_triangles),materials=len(mesh.materials),
                       packed_images=len(images),uv_layer=mesh.uv_layers.active.name,
                       blend=str(path.relative_to(ROOT)),glb=str(glb.relative_to(ROOT)),
                       checks=['saved triangle count','finite positions and UVs','UV loop coverage','packed textures','GLB header',
                               'GLB triangle count matches Blender','GLB positions/normals/UV attributes','GLB embedded images and material count'],
                       glb_images=len(data['images']),
                       limits='Static bind-space delivery; no animation approval. GLB/Blender do not reproduce native fur shells, glow or vertex-light blending.'))
(OUT/'important-npc-delivery-check.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report,indent=2))
