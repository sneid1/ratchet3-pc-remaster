"""Package the inspected standalone Clank and standard ship art passes."""
import hashlib,html,json,shutil
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
DEST=OUT/'deliverables/imagegen-layers'
items=[('clank-standalone','clank_standalone','Standalone Clank',6746,
        'Full-body class 87, refined original contours, satin alloy and fitted mechanical relief. Eye and antenna emission; torso and limbs excluded.',
        ['front','rear','emission']),
       ('ratchet-ship','ratchet_ship','Ratchet’s ship',4018,
        'Complete standard ship: original hull, mirrored wings, twin engines and paired weapons. Fitted paint, exposed alloy, seat and vent materials; only the two engine cores emit.',
        ['front','rear','underside','cockpit','emission'])]
cards=[];records=[]
for key,model,title,count,description,views in items:
    folder=DEST/key;review=folder/'review';review.mkdir(exist_ok=True)
    for view in views:
        prefix='clank' if model=='clank_standalone' else 'ship'
        shutil.copy2(OUT/f'candidate-{prefix}-final-{view}.png',review/(view+'.png'))
    for suffix in ['r3edit','json','emission.bin','emission.json']:
        source=OUT/'blender/edited'/(model+'.'+suffix)
        if source.exists():shutil.copy2(source,folder/source.name)
    for suffix in ['colour','height']:
        shutil.copy2(OUT/'authored/roster'/(key+'-'+suffix+'.png'),folder/('source-'+suffix+'.png'))
    metadata=folder/(model+'.json');data=json.loads(metadata.read_text())
    data['animation']='Static source art review only; animation and playable game integration are separate.'
    metadata.write_text(json.dumps(data,indent=2)+'\n')
    record=dict(model=model,title=title,triangles=count,description=description,
                geometry='Refined original source mesh and UVs; no primitive proxy parts.',
                review_views=views,art_status='Art pass complete; native views inspected',
                limits='Standard source configuration and bind pose. No animation approval or player-pack installation.',
                model_file='../models/'+model+'.blend',
                model_sha256=hashlib.sha256((DEST/'models'/(model+'.blend')).read_bytes()).hexdigest(),
                files={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.iterdir() if p.is_file() and p.name!='art-review.json'})
    (folder/'art-review.json').write_text(json.dumps(record,indent=2)+'\n');records.append(record)
    links=' · '.join(f'<a href="{key}/review/{v}.png">{v.title()}</a>' for v in views)
    cards.append(f'<article><h2>{html.escape(title)}</h2><a href="{key}/review/front.png"><img src="{key}/review/front.png" alt="{html.escape(title)} native model review"></a><p>{count:,} triangles. {html.escape(description)}</p><p>{links}</p><p><a href="models/{model}.blend">Editable Blender file</a> · <a href="{key}/layers.json">Texture layers</a> · <a href="{key}/art-review.json">Review record</a></p></article>')
for name in ['ship-assembly.json','ship-material-policy.json','next-assets-delivery-check.json']:
    source=OUT/name
    if source.exists():shutil.copy2(source,DEST/'ratchet-ship'/name)
(DEST/'next-assets.html').write_text('''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Standalone Clank and Ratchet’s ship</title><style>body{margin:32px auto;padding:0 24px;max-width:1500px;background:#111519;color:#e8edf0;font:17px/1.5 system-ui}main{display:grid;grid-template-columns:repeat(auto-fit,minmax(350px,1fr));gap:28px}article{background:#1b2227;padding:24px;border-radius:12px}img{width:100%;background:black}a{color:#9bcbff}h1,h2{line-height:1.2}small{color:#bdc8d0}</style><h1>Standalone Clank and Ratchet’s ship</h1><p>Completed art passes using the original game assemblies, fitted generated colour/height and inspected material finishes.</p><p><small>Unretouched isolated native renders with saved level lighting. Bind-space art review; no game launch, animation approval or player-pack installation.</small></p><main>'''+''.join(cards)+'</main></html>',encoding='utf-8')
ledger_path=OUT/'art-finishing-ledger.json';ledger=json.loads(ledger_path.read_text())
ledger['art_complete']=[r for r in ledger['art_complete'] if r['model'] not in {x[1] for x in items}]
for record in records:
    ledger['art_complete'].append(dict(model=record['model'],fix=record['description'],triangles=record['triangles'],review=record['review_views'],outstanding_required_art=[],limits=record['limits']))
ledger['current']=None;ledger['sequence_status']='Requested standalone Clank and standard Ratchet ship art passes complete; wider roster remains separate.'
ledger['required'].update(clank_standalone='Full-body Clank with fitted detailed colour/height, mechanical materials and controlled lights',ratchet_ship='Complete standard original assembly with fitted detailed paint/alloy/seat/recess materials and engine-only light')
ledger_path.write_text(json.dumps(ledger,indent=2)+'\n')
print('Packaged both inspected art passes:',DEST/'next-assets.html')
