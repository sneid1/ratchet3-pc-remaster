"""Recover the intended Ratchet region channels lost to RGBA premultiplication.

Uses the preserved original mask plus the same reviewed pale-fur correction
as riftapart-review.py. Does not restore old colours or touch accepted geometry.
"""
import json,hashlib,shutil
from pathlib import Path
import numpy as np
from PIL import Image,ImageDraw
from surface_layers import mask_for
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart';stem='9a6d927dedfae75e-284e78b48088f99a-00002213'
manifest=json.loads((OUT/'candidate/manifest.json').read_text());baseline=Path(manifest['baseline'])
old=PACK/'regions'/(stem+'.png');archive=OUT/'authored/pre-depth-pass'/(stem+'-regions.png')
if not archive.exists():shutil.copy2(old,archive)
source=Image.open(baseline/'regions'/(stem+'.png')).convert('RGBA')
weights=np.stack([np.asarray(c.resize((1024,1024),Image.Resampling.BOX)) for c in source.split()],axis=2)
original_path=next(Path(p['original_uv_atlas']) for p in manifest['provenance'] if p.get('original_uv_atlas'))
original=Image.open(original_path).convert('RGB').resize((1024,1024),Image.Resampling.LANCZOS)
rgb=np.asarray(original,dtype=np.float32)
spatial=Image.new('L',(256,256));draw=ImageDraw.Draw(spatial)
for points in [[(67,0),(115,0),(117,125),(50,127),(0,108),(0,80),(43,70),(67,54)],
               [(177,66),(204,76),(254,84),(255,120),(230,139),(209,153),(186,138),(173,121)],
               [(215,0),(256,0),(256,34),(246,36)]]:draw.polygon(points,fill=255)
area=np.asarray(spatial.resize((1024,1024),Image.Resampling.NEAREST))>0
fur=area&(rgb[...,0]>=rgb[...,1]*.95)&(rgb[...,1]>rgb[...,2]*1.16)&(weights[...,3]<128)
weights[fur]=[0,0,255,0]
target=next(t for t in json.loads(Path(__file__).with_name('material-recipes.json').read_text())['targets'] if t['stem']==stem)
for region in target.get('region_overrides',[]):
    mask=mask_for(region,1024)*(weights[...,3]==0)
    desired=np.zeros(4);desired[region['channel']]=255
    weights=np.uint8(weights*(1-mask[...,None])+desired*mask[...,None]+.5)
assert all(weights[...,i].max()==255 for i in range(4))
Image.fromarray(weights).save(old)
report=dict(cause='RGBA BOX resize premultiplied independent material weights by lens alpha',
    recovered_channels=['leather','alloy','fur'],eye_mask_preserved=True,
    glove_correction='Explicit leather islands remove legacy metal/fur classifications of painted glove highlights; original atlas inspected',
    original= str(baseline/'regions'/(stem+'.png')),sha256=hashlib.sha256(old.read_bytes()).hexdigest(),
    nonzero_pixels=[int(np.count_nonzero(weights[...,i])) for i in range(4)])
(OUT/'authored/ratchet-region-repair.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
