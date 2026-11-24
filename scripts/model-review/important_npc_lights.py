"""Explicit optical UV regions, independent of generated colour brightness."""
import argparse
import csv
import json
from pathlib import Path
import numpy as np
from PIL import Image, ImageDraw

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart'
p=argparse.ArgumentParser();p.add_argument('--character',choices=['nefarious','courtney'],required=True)
a=p.parse_args()
regions={
 'nefarious':[
 ('d2edceea6b4eb822-b5523062a676b27-00001dd3', [(0,25,8,42)]),
 ('d4c012b63b610012-22acb3a5e506e70d-00001dd3', [(0,104,128,128)])],
 'courtney':[('622ca52469c1ef2a-3df6c592c455684a-00001dd3', [(82,34,94,45)])],
}
rows=list(csv.DictReader((PACK/'pbr.tsv').read_text().splitlines(),delimiter='\t'))
report=[]
for i,(stem,boxes) in enumerate(regions[a.character]):
    baseline=OUT/'authored/important-npcs'/(stem+'-baseline.png')
    rgb=np.asarray(Image.open(baseline).convert('RGB').resize((1024,1024)),np.float32)/255
    mask=Image.new('L',(1024,1024));d=ImageDraw.Draw(mask)
    for box in boxes:d.rectangle(tuple(v*8 for v in box),fill=255)
    weight=np.asarray(mask,np.float32)/255
    # Red lenses only within manually inspected optical islands. Black surrounds
    # stay dark; this is an offline per-asset decision, not a runtime hue rule.
    if a.character=='nefarious':weight*=np.clip((rgb[...,0]-rgb[...,1]*1.5)*3,0,1)
    else:weight*=np.clip((np.minimum(rgb[...,0],rgb[...,1])-rgb[...,2]*1.3)*3,0,1)
    linear=np.where(rgb<=.04045,rgb/12.92,((rgb+.055)/1.055)**2.4)
    emission=np.dstack((linear*weight[...,None]*1.4/4,weight*.25))
    name=f'important_{a.character}_{i}_emit'
    Image.fromarray(np.uint8(np.clip(emission,0,1)*255+.5)).save(PACK/'materials'/(name+'.png'))
    next(r for r in rows if r['stem']==stem)['emissive_map']=name
    report.append(dict(stem=stem,boxes_128=boxes,strength=1.4,glow=.25,description='Inspected optical islands only; no new world lights.'))
with (PACK/'pbr.tsv').open('w',newline='') as f:
    w=csv.DictWriter(f,list(rows[0]),delimiter='\t',lineterminator='\n');w.writeheader();w.writerows(rows)
(OUT/(a.character+'-optical-policy.json')).write_text(json.dumps(report,indent=2)+'\n')
