"""Compile authored, restrained edge wear while preserving every emitter/alpha."""
import json,hashlib,shutil
from pathlib import Path
import numpy as np
from PIL import Image
from surface_layers import mask_for
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart';stem='efa796ee9a43f16c-23492c0e19b3712f-00001dd3'
dest=PACK/'replacements'/(stem+'.png');old=OUT/'authored/pre-depth-pass'/(stem+'.png')
if not old.exists():shutil.copy2(dest,old)
base=Image.open(old).convert('RGBA');size=base.width
source=OUT/'authored/ranger-wear-depth-source.png'
art=Image.open(source).convert('RGB').resize(base.size,Image.Resampling.LANCZOS)
weights=Image.open(PACK/'regions'/(stem+'.png')).convert('RGBA')
mask=np.maximum.reduce([np.asarray(c.resize(base.size,Image.Resampling.BILINEAR),dtype=np.float32)/255 for c in weights.split()])*.65
target=next(t for t in json.loads(Path(__file__).with_name('material-recipes.json').read_text())['targets'] if t['model']=='ranger')
for region in target['emissive_regions']:
    r=dict(region,bevel=0);mask[mask_for(r,size)>.001]=0
mask_image=Image.fromarray(np.uint8(mask*255+.5))
result=Image.composite(art,base.convert('RGB'),mask_image);result.putalpha(base.getchannel('A'))
a=np.asarray(base);b=np.asarray(result)
assert np.array_equal(a[...,3],b[...,3]) and np.array_equal(a[mask==0],b[mask==0])
result.save(dest);result.save(OUT/'authored/ranger-depth-colour.png')
report=dict(tool='built-in image_gen',source=source.name,source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),
    changed_pixels=int(np.any(a!=b,axis=2).sum()),blend=.65,source_alpha_exact=True,emitters_unchanged=True,
    prompt='Preserve exact UV layout and colours. Add fine enamel/coating finish variation, sparse edge abrasion, grime at existing seams/socket rims/vents, and rubbed alloy around fasteners. Keep broad interiors calm. No new construction lines, lighting, rust or dirt splatter. Orange emitters and negative space unchanged. Neutral albedo only.')
(OUT/'authored/ranger-depth-colour.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
