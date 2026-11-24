"""Transfer the original shirt palette onto its actual new Blender UV triangles."""
import json
from pathlib import Path
import numpy as np
from PIL import Image
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
data=json.loads((OUT/'authored/big-al-shirt-uv.json').read_text());size=1024
source=np.asarray(Image.open(OUT/'authored/roster'/(data['stem']+'-baseline.png')).convert('RGBA'),dtype=np.float32)
h,w=source.shape[:2];result=np.zeros((size,size,4),np.float32);filled=np.zeros((size,size),bool)
for tri in data['triangles']:
    p=np.array(tri['new'])*size;a,b,c=p
    lo=np.maximum(np.floor(p.min(axis=0)).astype(int),0);hi=np.minimum(np.ceil(p.max(axis=0)).astype(int),size)
    x,y=np.meshgrid(np.arange(lo[0],hi[0])+.5,np.arange(lo[1],hi[1])+.5)
    den=(b[1]-c[1])*(a[0]-c[0])+(c[0]-b[0])*(a[1]-c[1])
    s=((b[1]-c[1])*(x-c[0])+(c[0]-b[0])*(y-c[1]))/den
    t=((c[1]-a[1])*(x-c[0])+(a[0]-c[0])*(y-c[1]))/den
    inside=(s>=-1e-5)&(t>=-1e-5)&(s+t<=1.00001)
    uv=s[...,None]*tri['old'][0]+t[...,None]*tri['old'][1]+(1-s-t)[...,None]*tri['old'][2]
    px=uv[...,0]*w-.5;py=uv[...,1]*h-.5;ix=np.floor(px).astype(int);iy=np.floor(py).astype(int)
    fx=(px-ix)[...,None];fy=(py-iy)[...,None]
    colour=(source[iy%h,ix%w]*(1-fx)+source[iy%h,(ix+1)%w]*fx)*(1-fy)+(source[(iy+1)%h,ix%w]*(1-fx)+source[(iy+1)%h,(ix+1)%w]*fx)*fy
    tile=result[lo[1]:hi[1],lo[0]:hi[0]];tile[inside]=colour[inside]
    filled[lo[1]:hi[1],lo[0]:hi[0]]|=inside
# Pad island edges for bilinear/mip filtering without changing occupied texels.
for _ in range(12):
    old=filled.copy()
    for axis,step in [(0,1),(0,-1),(1,1),(1,-1)]:
        valid=np.roll(old,step,axis)
        if axis==0:valid[0 if step==1 else -1,:]=False
        if axis==1:valid[:,0 if step==1 else -1]=False
        take=~filled&valid;result[take]=np.roll(result,step,axis)[take];filled|=take
result[...,3]=128
dest=OUT/'authored/big-al-shirt-rebaked.png';Image.fromarray(np.uint8(np.clip(result,0,255)+.5)).save(dest)
print('Transferred original palette to',len(data['triangles']),'shirt UV triangles:',dest)
