"""Name the original ship assemblies and check the two self-contained deliveries.

This is an art-file check, not an animation or playable game export test.
"""
import bpy,json,argparse,sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
folder=OUT/'deliverables/imagegen-layers/models'
parser=argparse.ArgumentParser();parser.add_argument('--model',action='append',choices=['ratchet_ship','ratchet_ship_2','ratchet_ship_3','clank_standalone'])
args=parser.parse_args(sys.argv[sys.argv.index('--')+1:] if '--' in sys.argv else [])
models=args.model or ['ratchet_ship','clank_standalone']
report_path=OUT/'next-assets-delivery-check.json'
reports=[r for r in json.loads(report_path.read_text()) if r['model'] not in models] if report_path.exists() else []
for model in models:
    expected=json.loads((OUT/'blender/edited'/(model+'.json')).read_text())['edited_triangles']
    path=folder/(model+'.blend');bpy.ops.wm.open_mainfile(filepath=str(path))
    obj=bpy.data.objects['UYA_'+model+'_captured_pose'];mesh=obj.data
    mesh.calc_loop_triangles();assert len(mesh.loop_triangles)==expected
    groups={}
    if model.startswith('ratchet_ship'):
        parts=[('Hull and cockpit',0,1656),('Port wing',1656,1782),('Starboard wing',1782,1908),
               ('Twin engine assembly',1908,2360),('Port weapon',2360,2438),('Starboard weapon',2438,2516)]
        variant=0
        if model!='ratchet_ship':
            assembly=json.loads((OUT/f'candidate-blender-{model}.assembly.json').read_text())
            variant=assembly['variant'];parts=[(p['name'],p['first'],p['end']) for p in assembly['parts']]
        # Creating vertex groups can reallocate Blender's custom-data storage.
        # Copy the existing point attributes before adding any groups.
        source=[v.value//3 for v in mesh.attributes['review_src0'].data]
        for name,first,end in parts:
            group=obj.vertex_groups.get(name) or obj.vertex_groups.new(name=name)
            ids=[i for i,triangle in enumerate(source) if first<=triangle<end]
            assert ids;group.add(ids,1.0,'REPLACE');groups[name]=len(ids)
        obj['Assembly']=f'Original hull {variant}, mirrored {4308+variant} wings, 4312 engines, mirrored 4314 weapons; class-header scales verified from source data.'
        obj['Art status']='Reviewed front, rear, cockpit, underside and engine-only emission. Standard configuration; animation and game integration separate.'
        bpy.ops.wm.save_as_mainfile(filepath=str(path))
    images={node.image.name:node.image for mat in mesh.materials for node in mat.node_tree.nodes if node.type=='TEX_IMAGE' and node.image}
    assert images and all(image.packed_file for image in images.values())
    assert mesh.uv_layers.active
    reports.append(dict(model=model,triangles=len(mesh.loop_triangles),packed_textures=len(images),
                        uv_layer=mesh.uv_layers.active.name,part_groups=groups,file=str(path.relative_to(ROOT))))
(OUT/'next-assets-delivery-check.json').write_text(json.dumps(reports,indent=2)+'\n')
print(json.dumps(reports))
