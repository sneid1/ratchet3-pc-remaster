"""Compile approved imagegen artwork into the source UV/GS-alpha contract."""
import hashlib,json,shutil
from pathlib import Path
import numpy as np
from PIL import Image

ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
recipes=json.loads(Path(__file__).with_name('foliage-recipes.json').read_text())
records=[]
for target in recipes['targets']:
    art=target.get('colour_artwork')
    if not art:continue
    source=Path(target['source']);generated=Path(art)
    dest=OUT/'authored'/('foliage-'+target['stem']+'.png')
    if generated.resolve()!=dest.resolve():shutil.copy2(generated,dest)
    original=Image.open(source).convert('RGBA')
    authored=Image.open(dest).convert('RGBA').resize((1024,1024),Image.Resampling.LANCZOS)
    rgb=np.asarray(authored.convert('RGB'),dtype=np.float32).copy()
    valid=np.asarray(authored.getchannel('A'))>=128
    alpha=np.asarray(original.getchannel('A').resize(authored.size,Image.Resampling.BILINEAR))
    # Colour dilation for texture filtering, not a new silhouette or an albedo
    # painting operation. Retain the original game's 0..128 cutout coverage.
    for _ in range(96):
        if np.all(valid[alpha>0]):break
        total=np.zeros_like(rgb);count=np.zeros(valid.shape,np.float32)
        for axis,shift in ((0,1),(0,-1),(1,1),(1,-1)):
            mask=np.roll(valid,shift,axis);values=np.roll(rgb,shift,axis)
            edge=[slice(None),slice(None)];edge[axis]=0 if shift==1 else -1;mask[tuple(edge)]=False
            total+=values*mask[...,None];count+=mask
        new=(~valid)&(count>0);rgb[new]=total[new]/count[new,None];valid|=new
    if not np.all(valid[alpha>0]):raise ValueError('Artwork has an unfilled source UV region: '+target['stem'])
    packed=np.dstack((np.uint8(np.clip(rgb,0,255)+.5),alpha))
    output=OUT/'candidate/0-riftapart/replacements'/(target['stem']+'.png')
    Image.fromarray(packed).save(output)
    records.append(dict(stem=target['stem'],artwork=str(dest),source=str(source),output=str(output),
        source_sha256=hashlib.sha256(source.read_bytes()).hexdigest(),artwork_sha256=hashlib.sha256(dest.read_bytes()).hexdigest(),
        alpha='Original source alpha resampled to 1024; native GS 128 opaque; verified byte-for-byte against resampled source',
        alpha_verified=bool(np.array_equal(np.asarray(Image.open(output))[...,3],alpha)),
        provenance='Built-in imagegen edit of the exact original leaf atlas; source silhouette/UV and separate PBR channels retained.'))
(OUT/'candidate/foliage-colour.json').write_text(json.dumps(records,indent=2)+'\n')
print('Compiled',len(records),'authored colour atlases with original cutout coverage')
