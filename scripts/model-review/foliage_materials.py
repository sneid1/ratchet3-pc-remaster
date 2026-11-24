"""Atlas-fitted leaf relief; original colour/opacity and geometry stay independent."""
import csv,json,hashlib
from pathlib import Path
import numpy as np
from PIL import Image
from material_finishes import load,provenance
from surface_layers import apply

ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review';PACK=OUT/'candidate/0-riftapart'
recipes=json.loads(Path(__file__).with_name('foliage-recipes.json').read_text())
finishes=json.loads(Path(__file__).with_name('material-recipes.json').read_text())['finishes']
targets={r['stem']:r for r in recipes['targets']};size=recipes['size']
rows=[r for r in csv.DictReader((PACK/'pbr.tsv').open(),delimiter='\t') if r['stem'] not in targets]
records=[]
for stem,target in targets.items():
    normal,rough=load(json.dumps(finishes[target['finish']],sort_keys=True),size)
    normal=normal.copy();rough=rough.copy()
    ao=np.ones((size,size),np.float32);metal=np.zeros_like(ao)
    report=apply(target['features'],normal,rough,metal,ao,finishes)
    z=np.sqrt(np.maximum(1-(normal*normal).sum(axis=2),0))
    n=np.dstack((normal*.5+.5,z*.5+.5));orm=np.dstack((ao,rough,metal))
    prefix='pbr_'+stem
    for suffix,data in [('_n',n),('_orm',orm)]:
        Image.fromarray(np.uint8(np.clip(data,0,1)*255+.5)).save(PACK/'materials'/(prefix+suffix+'.png'))
    rows.append(dict(stem=stem,normal_map=prefix+'_n',orm_map=prefix+'_orm'))
    records.append(dict(target=stem,**report,source=provenance(target['finish'],finishes[target['finish']]),
        ao='Neutral texture AO; actual sandbox depth AO supplies leaf overlap. No darkening from leaf colour.',
        original_source_sha256=hashlib.sha256(Path(target['source']).read_bytes()).hexdigest()))
with (PACK/'pbr.tsv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,['stem','normal_map','orm_map','emissive_map'],delimiter='\t',lineterminator='\n');writer.writeheader();writer.writerows(rows)
(OUT/'candidate/foliage-materials.json').write_text(json.dumps(records,indent=2)+'\n')
print(f'Authored {len(records)} fitted leaf normal/ORM atlases; source colour/alpha retained')
