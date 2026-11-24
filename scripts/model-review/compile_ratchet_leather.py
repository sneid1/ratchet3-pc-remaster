"""Install generated leather artwork only through the original glove mask.

Original fur, face, iris, teeth, all non-glove colour and GS alpha stay exact.
The generated colour never supplies normal/height/AO or material classification.
"""
import hashlib,json,shutil
from pathlib import Path
import numpy as np
from PIL import Image,ImageDraw,ImageFilter
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart';stem='9a6d927dedfae75e-284e78b48088f99a-00002213'
dest=PACK/'replacements'/(stem+'.png');old=OUT/'authored/pre-depth-pass'/(stem+'.png')
if not old.exists():shutil.copy2(dest,old)
base=Image.open(old).convert('RGBA');size=base.width
art=Image.open(OUT/'authored/ratchet-gloves-depth-source.png').convert('RGB').resize(base.size,Image.Resampling.LANCZOS)
mask=Image.new('L',base.size);d=ImageDraw.Draw(mask)
polygons=[[(117,3),(171,3),(175,9),(172,29),(167,39),(180,45),(191,46),(197,55),(198,62),(184,66),(175,64),(176,97),(171,105),(159,109),(145,110),(129,108),(119,99),(115,74),(116,43)],
          [(110,112),(154,112),(155,122),(151,143),(153,155),(171,158),(181,169),(181,173),(170,176),(161,172),(162,208),(159,218),(146,222),(133,221),(121,218),(109,211),(106,197),(106,172),(109,150)]]
for points in polygons:d.polygon([(x*size/256,y*size/256) for x,y in points],fill=255)
# Original material-assignment mask is semantic. No RGB hue/brightness test.
leather=Image.open(PACK/'regions'/(stem+'.png')).getchannel('R').resize(base.size,Image.Resampling.BILINEAR)
mask=np.minimum(np.asarray(mask),np.asarray(leather))
mask=Image.fromarray(mask).filter(ImageFilter.MinFilter(3)).filter(ImageFilter.GaussianBlur(.65))
# Explicit no-edit fence includes the full iris island and its UV padding.
ImageDraw.Draw(mask).rectangle((625,435,744,548),fill=0)
result=Image.composite(art,base.convert('RGB'),mask);result.putalpha(base.getchannel('A'))
a=np.asarray(base);b=np.asarray(result);m=np.asarray(mask)
assert np.array_equal(a[...,3],b[...,3]) and np.array_equal(a[m==0],b[m==0])
assert np.any(a!=b,axis=2).sum()>1000,'No glove artwork installed; check the semantic material mask'
# The accepted eye region and face remain byte-identical, not regenerated.
assert np.array_equal(a[435:548,625:744],b[435:548,625:744])
result.save(dest);result.save(OUT/'authored/ratchet-body-depth.png')
report=dict(tool='built-in image_gen',source='ratchet-gloves-depth-source.png',
    source_sha256=hashlib.sha256((OUT/'authored/ratchet-gloves-depth-source.png').read_bytes()).hexdigest(),
    installed_sha256=hashlib.sha256(dest.read_bytes()).hexdigest(),changed_pixels=int(np.any(a!=b,axis=2).sum()),
    scope='Only original glove leather mask; face/iris/fur/teeth and source alpha preserved byte-for-byte',
    prompt='Preserve exact UV islands and palette. Edit only two central gloves into dark chocolate leather: fine pores, restrained wear on crests/fingertips, grime inside existing flex creases/cuff seams, tiny edge nicks. Neutral diffuse colour, no painted directional lighting. Keep every other atlas island unchanged. Original semantic masks restrict the installed edit.')
(OUT/'authored/ratchet-body-depth.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
