"""Sync editable original meshes to the native candidate's material contract.

No rebake or render. Original colour and alpha are retained; data maps use
Non-Color. Each saved file gets its own captured level's directional lights.
Native captures remain authoritative for per-vertex light blending and AO.
"""
import argparse,ast,csv,json,struct,sys
from pathlib import Path
import bpy
from mathutils import Vector
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review';PACK=OUT/'candidate/0-riftapart'
p=argparse.ArgumentParser();p.add_argument('--model',action='append',required=True)
p.add_argument('--delivery',type=Path,help='Also save self-contained material review copies here')
p.add_argument('--assets',type=Path,default=Path(__file__).with_name('review-assets.json'))
a=p.parse_args(sys.argv[sys.argv.index('--')+1:])
def table(name):
    return {r['stem']:r for r in csv.DictReader((s for s in (PACK/(name+'.tsv')).read_text().splitlines() if s and not s.startswith('#')),delimiter='\t')}
pbr=table('pbr');regions=table('regions');surfaces=table('surfaces')
assets=json.loads(a.assets.read_text())
report_path=OUT/'blender/material-sync.json'
report=[r for r in json.loads(report_path.read_text()) if r['model'] not in a.model] if report_path.exists() else []
for model in a.model:
    source=OUT/'blender/edited'/(model+'.blend')
    if not source.exists():source=OUT/'blender/refined'/(model+'.blend')
    bpy.ops.wm.open_mainfile(filepath=str(source))
    obj=bpy.data.objects['UYA_'+model+'_captured_pose']
    count=0
    for mat in obj.data.materials:
        stem=mat.get('review_source_stem',mat.name.split('.')[0]);row=pbr.get(stem)
        if not row:continue
        nodes=mat.node_tree.nodes;links=mat.node_tree.links
        for node in list(nodes):
            if node.get('review_pbr'):nodes.remove(node)
        shader=nodes.get('Principled BSDF')
        def image_node(path,label,data=False):
            node=nodes.new('ShaderNodeTexImage');node['review_pbr']=True;node.label=label
            node.image=bpy.data.images.load(str(path),check_existing=True)
            # The same atlas filename may have been revised since this blend
            # was saved. Refresh its actual pixels rather than retain a stale
            # embedded/cached version while the native viewer uses newer maps.
            if node.image.packed_file:node.image.unpack(method='REMOVE')
            node.image.reload()
            node.image.colorspace_settings.name='Non-Color' if data else 'sRGB'
            node.image.alpha_mode='CHANNEL_PACKED' if data else 'STRAIGHT'
            return node
        colour=next((q for q in [PACK/'replacements'/(stem+'.png'),OUT/'dumps'/(stem+'.png'),Path.home()/'Games/Ratchet and Clank 3/texture_dumps'/(stem+'.png')] if q.exists()),None)
        if colour:
            tex=image_node(colour,'Original atlas / authored UV-compatible colour')
            links.new(tex.outputs['Color'],shader.inputs['Base Color'])
        tex=image_node(PACK/'materials'/(row['normal_map']+'.png'),'Linear tangent-space normal',True)
        normal=nodes.new('ShaderNodeNormalMap');normal['review_pbr']=True
        links.new(tex.outputs['Color'],normal.inputs['Color']);links.new(normal.outputs['Normal'],shader.inputs['Normal'])
        tex=image_node(PACK/'materials'/(row['orm_map']+'.png'),'R AO / G roughness / B metalness',True)
        split=nodes.new('ShaderNodeSeparateColor');split['review_pbr']=True
        links.new(tex.outputs['Color'],split.inputs['Color'])
        links.new(split.outputs['Green'],shader.inputs['Roughness']);links.new(split.outputs['Blue'],shader.inputs['Metallic'])
        if row.get('emissive_map'):
            tex=image_node(PACK/'materials'/(row['emissive_map']+'.png'),'Linear emission / 4; explicit light regions',True)
            links.new(tex.outputs['Color'],shader.inputs['Emission Color'])
            shader.inputs['Emission Strength'].default_value=4
        # Cycles geometry handles occlusion. Do not multiply cavity AO into
        # base colour or direct illumination; native uses it on indirect only.
        count+=1
    policy=assets[model].get('emission_policy');policy_counts={}
    if policy:
        data=(OUT/policy).read_bytes();edit=OUT/'blender/edited'/(model+'.r3edit')
        mesh=edit.read_bytes() if edit.exists() else None
        if mesh:
            signature,source_count,material_count,vertex_count=struct.unpack_from('<8sIII',mesh)
            assert signature==b'R3EDIT02' and struct.unpack_from('<I',data,8)[0]==source_count
            offset=20
            for _ in range(material_count):
                length=struct.unpack_from('<H',mesh,offset)[0];offset+=2+length+4
            offset+=source_count*12
            assert len(obj.data.polygons)*3==vertex_count and all(len(p.vertices)==3 for p in obj.data.polygons)
        else:
            assert assets[model].get('source_layer')=='source'
            source_faces=[[int(v.split('/')[0])-1 for v in line.split()[1:4]] for line in
                          (OUT/f'candidate-blender-{model}.obj').read_text().splitlines() if line.startswith('f ')]
            assert len(source_faces)==len(obj.data.polygons)
            assert struct.unpack_from('<I',data,8)[0]==len(obj.data.vertices)
        variants={}
        for face in obj.data.polygons:
            source_id=struct.unpack_from('<I',mesh,offset+face.index*3*60)[0]//3 if mesh else min(source_faces[face.index])//3
            mode=data[12+source_id]
            policy_counts[str(mode)]=policy_counts.get(str(mode),0)+1
            if mode==1:continue
            mat=obj.data.materials[face.material_index]
            stem=mat.get('review_source_stem',mat.name.split('.')[0])
            key=(stem,mode)
            if key not in variants:
                existing=next((i for i,m in enumerate(obj.data.materials) if m.get('review_source_stem')==stem and m.get('review_emission_mode')==mode),None)
                if existing is None:
                    replacement=mat.copy();replacement.name=stem+('.unlit_off' if mode==0 else '.cannon_light')
                    replacement['review_source_stem']=stem;replacement['review_emission_mode']=mode
                    obj.data.materials.append(replacement);existing=len(obj.data.materials)-1
                replacement=obj.data.materials[existing];shader=replacement.node_tree.nodes.get('Principled BSDF')
                for link in list(shader.inputs['Emission Color'].links):replacement.node_tree.links.remove(link)
                shader.inputs['Emission Strength'].default_value=0 if mode==0 else 1.8
                if mode==2:
                    base=shader.inputs['Base Color']
                    if base.is_linked:replacement.node_tree.links.new(base.links[0].from_socket,shader.inputs['Emission Color'])
                    else:shader.inputs['Emission Color'].default_value=base.default_value
                variants[key]=existing
            face.material_index=variants[key]
    for light in list(bpy.data.objects):
        if light.type=='LIGHT':bpy.data.objects.remove(light,do_unlink=True)
    values=struct.unpack('<256f',(OUT/f'candidate-blender-{model}.lights.bin').read_bytes())
    words=ast.literal_eval((OUT/f'candidate-blender-{model}.lights.txt').read_text())
    word=max((w for w in words if w&0x80000000),key=lambda w:words[w])
    ia=(word&15)*16;ib=((word>>4)&15)*16;blend=((word>>8)&255)/256
    for k in range(2):
        o=8*k
        colour=[values[ia+o+j]*(1-blend)+values[ib+o+j]*blend for j in range(3)]
        ray=Vector([values[ia+o+4+j]*(1-blend)+values[ib+o+4+j]*blend for j in range(3)])
        if ((word>>16)&3)<2:ray=-ray
        data=bpy.data.lights.new(f'Captured_L{assets[model]["level"]:02}_{k}','SUN')
        data.color=tuple(max(0,v) for v in colour);data.energy=.25 if ((word>>16)&3)==3 else 1
        light=bpy.data.objects.new(data.name,data);bpy.context.scene.collection.objects.link(light)
        light.rotation_euler=ray.to_track_quat('-Z','Y').to_euler()
        light['Source']='Actual captured light set, including its blend; native renderer retains all per-vertex terms.'
    bpy.context.scene['Lighting note']='Captured directional rays. Native viewer is authoritative for vertex ambient, light-set blending and final material response.'
    for other in bpy.data.objects:
        if other.type=='MESH':other.hide_render=other!=obj;other.hide_set(other!=obj)
    bpy.context.scene.world.color=(0,0,0)
    bpy.ops.wm.save_as_mainfile(filepath=str(source))
    delivery=None
    if a.delivery:
        a.delivery.mkdir(parents=True,exist_ok=True)
        for mat in obj.data.materials:
            for node in mat.node_tree.nodes:
                if node.type=='TEX_IMAGE' and node.image:node.image.pack()
        delivery=a.delivery/(model+'.blend')
        bpy.ops.wm.save_as_mainfile(filepath=str(delivery.resolve()))
    report.append({'model':model,'file':str(source),'pbr_materials':count,'captured_level':assets[model]['level'],'light_word':word,
                   'emission_triangles':policy_counts,'delivery':str(delivery) if delivery else None})
report_path.write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
