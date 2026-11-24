"""Fitted source-face emission for standalone Clank and the assembled ship.

Uses the inspected original geometry/UVs. Shared colour swatches do not make
Clank's torso or the ship's orange paint emit. No new world light is introduced.
"""
import csv,hashlib,json,struct
from pathlib import Path
import numpy as np
from PIL import Image,ImageDraw,ImageFilter

ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart';manifest=Path(__file__).with_name('review-assets.json')
assets=json.loads(manifest.read_text())
def source(model):
    path=OUT/f'candidate-blender-{model}.obj';vertices=[];uv=[]
    for line in path.read_text().splitlines():
        p=line.split()
        if p and p[0]=='v':vertices.append(list(map(float,p[1:4])))
        elif p and p[0]=='vt':uv.append([float(p[1])%1,(1-float(p[2]))%1])
    return path,np.array(vertices).reshape(-1,3,3),np.array(uv).reshape(-1,3,2)
def policy(model,path,flags,description):
    target=OUT/'blender/edited'/(model+'.emission.bin')
    target.write_bytes(b'R3EMIT01'+struct.pack('<I',len(flags)*3)+bytes(flags))
    report=dict(model=model,description=description,source_obj_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                source_vertices=len(flags)*3,allowed_source_faces=np.flatnonzero(flags).tolist())
    target.with_suffix('.json').write_text(json.dumps(report,indent=2)+'\n')
    assets[model]['emission_policy']=target.relative_to(OUT).as_posix()
    print(model,int(np.count_nonzero(flags)),'allowed source faces')

path,v,uv=source('clank_standalone');z=v[...,2]
flags=(z.min(1)>z.min()+.57*(z.max()-z.min())).astype(np.uint8)
assert len(flags)==4502 and flags.sum()==2023
policy('clank_standalone',path,flags,'Only head/antenna faces may use the existing eye/antenna texel mask; torso and limbs never emit.')

path,v,uv=source('ratchet_ship');assert len(v)==2516
# The inspected engine fan interiors are twelve original triangles. Their UVs
# occupy the dedicated round hot-core island, not the adjacent silver rim.
flags=np.zeros(len(v),np.uint8);flags[2300:2312]=1
assert np.all((uv[2300:2312,:,0]*256>=129)&(uv[2300:2312,:,0]*256<=147))
assert np.all((uv[2300:2312,:,1]*256>=29)&(uv[2300:2312,:,1]*256<=47))
assert np.ptp(v[2300:2312,:,0])<.001
policy('ratchet_ship',path,flags,'Only the two engine-core fans emit: six original triangles each. Hull, wings, weapons, cockpit and engine casings are off.')
stem='8b45ad96956dc5cb-2f12be7762d79200-00002213';size=1024
colour=np.asarray(Image.open(OUT/'authored/roster'/(stem+'-baseline.png')).convert('RGB').resize((size,size),Image.Resampling.BILINEAR),np.float32)/255
linear=np.where(colour<=.04045,colour/12.92,((colour+.055)/1.055)**2.4)
mask=Image.new('L',(size,size));d=ImageDraw.Draw(mask)
d.ellipse((129*4,29*4,147*4,47*4),fill=255)
weight=np.asarray(mask.filter(ImageFilter.GaussianBlur(1)),np.float32)/255
emission=np.dstack((linear*weight[...,None]*1.5/4,weight*.45))
Image.fromarray(np.uint8(np.clip(emission,0,1)*255+.5)).save(PACK/'materials/ship_engine_emit.png')
table=PACK/'pbr.tsv'
rows=list(csv.DictReader(table.read_text().splitlines(),delimiter='\t'))
next(r for r in rows if r['stem']==stem)['emissive_map']='ship_engine_emit'
with table.open('w',newline='') as f:
    writer=csv.DictWriter(f,list(rows[0]),delimiter='\t',lineterminator='\n');writer.writeheader();writer.writerows(rows)
manifest.write_text(json.dumps(assets,indent=2)+'\n')
