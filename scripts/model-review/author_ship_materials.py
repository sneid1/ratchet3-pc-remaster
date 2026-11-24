"""Fit paint, alloy, seat leather and dark recess finishes to the original ship kit.

The inspected original palette separates orange/copper coating from grey metal;
the seat is an explicit UV island. This is a local authoring mask, not a runtime
colour classifier. Shape relief is supplied separately by the imagegen height.
"""
import csv,json,shutil
from pathlib import Path
import numpy as np
from PIL import Image,ImageDraw

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review';PACK=OUT/'candidate/0-riftapart'
ART=OUT/'authored/roster';STEM='8b45ad96956dc5cb-2f12be7762d79200-00002213';SIZE=1024
source=OUT/'authored/ship-source'/(STEM+'.png')
baseline=ART/(STEM+'-baseline.png')
if not baseline.exists():shutil.copy2(source,baseline)
rgba=np.asarray(Image.open(baseline).convert('RGBA').resize((SIZE,SIZE),Image.Resampling.NEAREST),np.float32)/255
rgb=rgba[...,:3];r,g,b=rgb.transpose(2,0,1)
paint=(r>g*1.22)&(r>b*1.4)&(r>.12)
dark=(rgb.max(2)<.25)&~paint
label=np.ones((SIZE,SIZE),np.uint8);label[paint]=0;label[dark]=3
seat=Image.new('L',(SIZE,SIZE));d=ImageDraw.Draw(seat)
d.polygon([(round(x*4),round(y*4)) for x,y in [(64,0),(128,0),(128,43),(82,43),(82,30),(70,20)]],fill=255)
label[np.asarray(seat)>0]=2
weights=np.stack([label==i for i in range(4)],axis=2).astype(np.uint8)*255
Image.fromarray(weights).save(PACK/'regions'/(STEM+'.png'))
normal=np.zeros((SIZE,SIZE,4),np.uint8);normal[:]=[128,128,255,255]
orm=np.zeros_like(normal);orm[...,0]=255;orm[...,3]=255
rough=[.37,.33,.72,.62];metal=[0,.88,0,.15]
for i in range(4):orm[label==i,1]=round(rough[i]*255);orm[label==i,2]=round(metal[i]*255)
for kind,array in [('normal',normal),('orm',orm)]:
    path=ART/('ratchet-ship-family-'+kind+'.png')
    Image.fromarray(array).save(path)
    Image.fromarray(array).save(PACK/'materials'/('ship_source_'+kind+'.png'))
def read(name):return list(csv.DictReader((PACK/(name+'.tsv')).read_text().splitlines(),delimiter='\t'))
def write(name,rows):
    with (PACK/(name+'.tsv')).open('w',newline='') as f:
        w=csv.DictWriter(f,list(rows[0]),delimiter='\t',lineterminator='\n');w.writeheader();w.writerows(rows)
rows=read('regions');template=dict(next(r for r in rows if r['stem'].startswith('407dd834')))
template.update(stem=STEM,map=STEM+'.png',normal='1')
for i,material in enumerate(['review_painted_metal','review_satin_alloy','review_soft_leather','review_dark_alloy']):
    suffix=str(i) if i else ''
    for k,value in dict(material=material,tiles_u='1',tiles_v='1',reflectance='.2',grain='1',specular='.25',fur='0',catchlight='0',metal=str(metal[i])).items():template[k+suffix]=value
rows=[r for r in rows if r['stem']!=STEM]+[template];write('regions',rows)
# A semantic atlas replaces the former single tiled finish, rather than
# declaring two different materials for the same source texture.
write('surfaces',[r for r in read('surfaces') if r['stem']!=STEM])
rows=read('pbr')
if not any(r['stem']==STEM for r in rows):
    rows.append(dict(stem=STEM,normal_map='ship_source_normal',orm_map='ship_source_orm',emissive_map=''));write('pbr',rows)
(OUT/'ship-material-policy.json').write_text(json.dumps(dict(source=str(source),regions=['Inspected orange/copper paint','Exposed neutral alloy','Explicit cockpit seat UV island','Dark grilles and recesses'],height='Independent imagegen mechanical height; never derived from albedo luminance',emission='Engine-only emission is assigned separately by author_next_asset_lights.py; this material seeding step does not infer emitters from colour.'),indent=2)+'\n')
print('Authored ship kit material masks and original-colour baseline')
