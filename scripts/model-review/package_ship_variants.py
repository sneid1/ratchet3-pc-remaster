"""Package the two inspected additional ship variants and all-three review page."""
import hashlib,html,json,shutil
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
DEST=OUT/'deliverables/imagegen-layers';shared=DEST/'ratchet-ship'
ledger_path=OUT/'art-finishing-ledger.json';ledger=json.loads(ledger_path.read_text())
checks={r['model']:r for r in json.loads((OUT/'next-assets-delivery-check.json').read_text())}
layers=json.loads((shared/'layers.json').read_text());cards=[]
views=['front','rear','underside','cockpit','emission']
for number in (1,2,3):
    model='ratchet_ship' if number==1 else f'ratchet_ship_{number}'
    key='ratchet-ship' if number==1 else f'ratchet-ship-{number}'
    folder=DEST/key;review=folder/'review';review.mkdir(parents=True,exist_ok=True)
    triangles=json.loads((OUT/'blender/edited'/(model+'.json')).read_text())['edited_triangles']
    if number!=1:
        assert checks[model]['triangles']==triangles and checks[model]['packed_textures']>=4
        for layer in layers['layer_types']:
            shutil.copy2(shared/(layer+'.png'),folder/(layer+'.png'))
        for name in ('source-colour.png','source-height.png'):
            shutil.copy2(shared/name,folder/name)
        for suffix in ('r3edit','json','emission.bin','emission.json'):
            source=OUT/'blender/edited'/(model+'.'+suffix);shutil.copy2(source,folder/source.name)
        assembly=OUT/f'candidate-blender-{model}.assembly.json'
        shutil.copy2(assembly,folder/'assembly.json')
        shutil.copy2(OUT/'ship-kit-scales.json',folder/'source-scales.json')
        for view in views:
            shutil.copy2(OUT/f'candidate-ship-{number}-final-{view}.png',review/(view+'.png'))
        texture_report=dict(layers,model=model,key=key,shared_atlas='ratchet-ship',
                            geometry='Refined original hull and matching original wing variant; source UVs retained.',
                            completion='Native front/rear, underside, cockpit and emission views inspected')
        (folder/'layers.json').write_text(json.dumps(texture_report,indent=2)+'\n')
        description=f'Original hull variant {number-1}, matching mirrored class {4307+number} wings, twin engines and paired weapons. Refined contours, fitted shared ship materials and engine-only emission.'
        record=dict(model=model,triangles=triangles,description=description,review_views=views,
                    art_status='Art pass complete; isolated native views inspected',
                    limits='Original bind-space art asset; no animation approval or player-pack installation.',
                    model_file='../models/'+model+'.blend',
                    model_sha256=hashlib.sha256((DEST/'models'/(model+'.blend')).read_bytes()).hexdigest(),
                    files={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.iterdir() if p.is_file() and p.name!='art-review.json'})
        (folder/'art-review.json').write_text(json.dumps(record,indent=2)+'\n')
        ledger['art_complete']=[r for r in ledger['art_complete'] if r['model']!=model]
        ledger['art_complete'].append(dict(model=model,triangles=triangles,fix=description,review=views,outstanding_required_art=[],limits=record['limits']))
        if model not in ledger['sequence']:ledger['sequence'].append(model)
        ledger['required'][model]='Complete matching original assembly, refined geometry, fitted shared materials and engine-only emission'
    links=' · '.join(f'<a href="{key}/review/{v}.png">{v.title()}</a>' for v in views)
    cards.append(f'<article><h2>Standard ship {number}</h2><a href="{key}/review/front.png"><img src="{key}/review/front.png" alt="Standard ship {number}"></a><p>{triangles:,} triangles · original hull and matching wings</p><p>{links}</p><p><a href="models/{model}.blend">Editable Blender asset</a> · <a href="{key}/layers.json">Texture layers</a> · <a href="{key}/art-review.json">Review record</a></p></article>')
(DEST/'standard-ships.html').write_text('''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Three standard ships</title><style>body{margin:32px auto;padding:0 24px;max-width:1700px;background:#111519;color:#e8edf0;font:17px/1.5 system-ui}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(360px,1fr));gap:24px}article{background:#1b2227;padding:22px;border-radius:12px}img{width:100%;background:black}a{color:#9bcbff}h1,h2{line-height:1.2}small{color:#bdc8d0}</style><h1>All three standard ships</h1><p>Finished art passes: original assemblies, refined curves and fitted paint, alloy, seat and vent materials.</p><p><small>Unretouched isolated native renders using saved level lighting. Engine-only emission. Animation and playable integration are separate.</small></p><main>'''+''.join(cards)+'</main>',encoding='utf-8')
ledger['current']=None;ledger['sequence_status']='All three requested standard ship art passes complete; broader roster remains separate.'
ledger_path.write_text(json.dumps(ledger,indent=2)+'\n')
print('Packaged standard ships:',DEST/'standard-ships.html')
