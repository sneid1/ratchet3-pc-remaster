"""Package the three visually inspected NPC art passes without game installation."""
import argparse
import hashlib
import html
import json
import shutil
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
DEST=OUT/'deliverables/imagegen-layers'
p=argparse.ArgumentParser()
p.add_argument('--reviewed',action='store_true',help='Run only after inspecting final native views')
a=p.parse_args()
if not a.reviewed:raise SystemExit('Final native views must be inspected before recording completion.')

items=[
 ('nefarious','source_c7662','Dr. Nefarious','Refined original mechanical assembly; fine enamel/alloy detail and controlled red optical emission.'),
 ('courtney','source_c7109','Courtney Gears','Refined original contours; rose enamel, white ceramic and mechanical hair detail; eye-only emission.'),
 ('skrunch','source_c7969','Skrunch','Refined original contours, fine directional fur and gentle skin detail; preserved HD facial identity and native fur assignments.'),
]
views=['front','rear','side','close','emission']
assets=json.loads((ROOT/'scripts/model-review/review-assets.json').read_text())
checks={r['model']:r for r in json.loads((OUT/'important-npc-delivery-check.json').read_text())}
recipes=json.loads((ROOT/'scripts/model-review/important-npc-materials.json').read_text())
results=json.loads((OUT/'candidate-viewer-results.json').read_text())
assert len(results)==15 and not any('error' in r for r in results)
source_roster=json.loads((OUT/'character-roster.json').read_text())['assets']
cards=[];records=[]

def copy(src,dst):
    assert src.is_file(),src
    dst.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(src,dst)

def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()

for key,model,title,description in items:
    folder=DEST/key;folder.mkdir(exist_ok=True)
    review=folder/'review';review.mkdir(exist_ok=True)
    for view in views:copy(OUT/f'candidate-{key}-final-{view}.png',review/(view+'.png'))
    blend=DEST/'models'/(model+'.blend')
    copy(blend,folder/(model+'.blend'))
    for suffix in ['r3edit','json']:
        copy(OUT/'blender/edited'/(model+'.'+suffix),folder/(model+'.'+suffix))
    for suffix in ['obj','vertex-lighting.bin','lights.bin','lights.txt']:
        copy(OUT/f'candidate-blender-{model}.{suffix}',folder/'source'/(model+'.'+suffix))
    for suffix in ['colour','height']:
        copy(OUT/'authored/important-npcs'/(key+'-'+suffix+'.png'),folder/'authored'/('source-'+suffix+'.png'))
    copy(OUT/'authored/important-npcs'/(model+'-atlas-reference.png'),folder/'authored'/'atlas-reference.png')
    copy(OUT/'authored/important-npcs/imagegen-prompts.json',folder/'authored/imagegen-prompts.json')
    policy=OUT/(key+'-optical-policy.json')
    if policy.exists():copy(policy,folder/'optical-policy.json')
    selected=[r for r in recipes if r['model']==model]
    for recipe in selected:
        atlas=DEST/recipe['key'];target=folder/'textures'/recipe['key']
        target.mkdir(parents=True,exist_ok=True)
        layers=json.loads((atlas/'layers.json').read_text())
        assert layers['alpha_exact'] and layers['protected_pixels_exact']
        for name in layers['layer_types']:copy(atlas/(name+'.png'),target/(name+'.png'))
        layers['completion']='Reviewed on the final native model; see enclosing asset manifest and review images.'
        (target/'layers.json').write_text(json.dumps(layers,indent=2)+'\n')
        copy(Path(layers['colour_baseline']),target/'identity-colour-baseline.png')
        copy(Path(layers['opacity_baseline']),target/'opacity-baseline.png')
    source=next(r for r in source_roster if r['asset_id']==assets[model]['asset_id'])
    (folder/'source/source-record.json').write_text(json.dumps(source,indent=2)+'\n')
    (folder/'material-recipes.json').write_text(json.dumps(selected,indent=2)+'\n')
    (folder/'workbench-registration.json').write_text(json.dumps({model:assets[model]},indent=2)+'\n')
    limits='Original bind-space art asset. Animation, gameplay export and installation are separate. GLB/Blender do not reproduce native fur shells, glow or vertex-light blending.'
    if key=='skrunch':limits+=' The source bind pose has closed eyelids; no expression/animation approval is implied.'
    record=dict(model=model,name=title,source_class=assets[model]['class_id'],asset_id=assets[model]['asset_id'],
                description=description,triangles=checks[model]['triangles'],art_status='Art pass complete',
                reviewed_views=views,verification=checks[model],limits=limits,
                geometry='Original kit; smoothed normals and adaptive contour refinement, no proxy components',
                texture_atlases=[r['key'] for r in selected],
                generation='Built-in image_gen; original colour and independent technical-height sheets retained',
                reproduce=['source manifest + viewer.py --export-obj','imagegen_layers.py --extra-config important-npc-materials.json with selected --key values',
                           'blender_uya.py --no-bake','finish_important_npc_geometry.py','important_npc_lights.py for mechanical characters, then compile/sync',
                           'viewer.py with final request JSON','verify_important_npc_delivery.py','package_important_npcs.py --reviewed'])
    copy(OUT/(key+'-final-requests.json'),folder/'review/requests.json')
    (folder/'README.md').write_text(f'# {title}\n\n{description}\n\n{limits}\n\n'
        'Editable model: '+model+'.blend. Portable static export: '+model+'.glb. Native preview bindings: '+model+'.r3edit. '
        'The source folder preserves original decoded geometry and lighting records. Textures contains the aligned channels and identity/opacity baselines. '
        'Authored contains original generated sheets and prompts. Review contains unretouched native renders. '
        'See manifest.json for source identity, verification and file hashes. This folder is an art delivery, not an installable game replacement pack.\n',encoding='utf-8')
    record['files']={f.relative_to(folder).as_posix():digest(f) for f in sorted(folder.rglob('*')) if f.is_file() and f.name!='manifest.json'}
    (folder/'manifest.json').write_text(json.dumps(record,indent=2)+'\n');records.append(record)
    cards.append(f'<article><h2>{html.escape(title)}</h2><a href="{key}/review/front.png"><img src="{key}/review/front.png" alt="{html.escape(title)} native render"></a>'
        f'<p>{checks[model]["triangles"]:,} triangles. {html.escape(description)}</p>'
        f'<p><a href="{key}/{model}.blend">Blender</a> · <a href="{key}/{model}.glb">Static GLB</a> · <a href="{key}/manifest.json">All files / evidence</a></p>'
        f'<p>'+ ' · '.join(f'<a href="{key}/review/{v}.png">{v}</a>' for v in views)+f'</p><small>{html.escape(limits)}</small></article>')

(DEST/'important-npcs.html').write_text('<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width">'
 '<title>Three important NPCs</title><style>body{background:#15191d;color:#edf1f5;font:17px/1.5 system-ui;max-width:1500px;margin:32px auto;padding:0 24px}'
 'main{display:grid;grid-template-columns:repeat(auto-fit,minmax(340px,1fr));gap:24px}article{background:#222931;padding:20px;border-radius:10px}'
 'img{width:100%}a{color:#9bcefa}small{color:#bac6d1}</style><h1>Nefarious, Courtney Gears and Skrunch</h1>'
 '<p>Original character assemblies with fitted detailed materials. Isolated native art review; no game installation or animation approval.</p><main>'
 +''.join(cards)+'</main></html>',encoding='utf-8')
ledger_path=OUT/'art-finishing-ledger.json';ledger=json.loads(ledger_path.read_text())
ids={model for _,model,_,_ in items}
ledger['art_complete']=[r for r in ledger['art_complete'] if r['model'] not in ids]
for key,model,title,description in items:
    r=next(r for r in records if r['model']==model)
    ledger['art_complete'].append(dict(model=model,fix=description,triangles=r['triangles'],review=views,
        outstanding_required_art=[],evidence=str((DEST/key/'review').relative_to(ROOT)),limits=r['limits']))
    if model not in ledger['sequence']:ledger['sequence'].append(model)
    ledger['required'][model]=description
ledger['current']=None
ledger['sequence_status']='Nefarious, Courtney Gears and standard Skrunch art passes complete; wider NPC/enemy roster remains separate.'
ledger_path.write_text(json.dumps(ledger,indent=2)+'\n')
print(json.dumps([dict(model=r['model'],triangles=r['triangles'],packaged_files=len(r['files'])) for r in records],indent=2))
