"""Roughness-filter six real level captures in linear colour for reflection.

GGX importance sampling, N=V approximation; this is an LDR captured environment,
not an HDR reconstruction. Capture orientation follows review_probe.rs.
"""
import argparse,hashlib,json,struct
from pathlib import Path
import numpy as np
from PIL import Image
p=argparse.ArgumentParser();p.add_argument('folder',type=Path);a=p.parse_args()
images=[np.asarray(Image.open(a.folder/f'face-{i}.png').convert('RGB'),dtype=np.float32)[::-1]/255 for i in range(6)]
src=np.stack([np.where(im<=.04045,im/12.92,((im+.055)/1.055)**2.4) for im in images])
def dirs(size):
    u,v=np.meshgrid((np.arange(size)+.5)/size*2-1,(np.arange(size)+.5)/size*2-1)
    one=np.ones_like(u)
    faces=[(one,-v,-u),(-one,-v,u),(u,one,v),(u,-one,-v),(u,-v,one),(-u,-v,-one)]
    n=np.stack([np.stack(f,-1) for f in faces]).reshape(-1,3)
    return n/np.linalg.norm(n,axis=-1,keepdims=True)
def sample(n):
    shape=n.shape[:-1];n=n.reshape(-1,3);axis=np.argmax(np.abs(n),axis=1);major=np.take_along_axis(n,axis[:,None],axis=1)[:,0]
    face=axis*2+(major<0);x,y,z=n.T;div=np.abs(major)
    sc=np.choose(face,[-z,z,x,x,x,-x])/div;tc=np.choose(face,[-y,-y,z,-z,-y,-y])/div
    size=src.shape[1];u=np.clip((sc+1)*size*.5-.5,0,size-1);v=np.clip((tc+1)*size*.5-.5,0,size-1)
    i=u.astype(int);j=v.astype(int);k=np.minimum(i+1,size-1);l=np.minimum(j+1,size-1)
    fu=(u-i)[:,None];fv=(v-j)[:,None]
    return ((src[face,j,i]*(1-fu)+src[face,j,k]*fu)*(1-fv)+(src[face,l,i]*(1-fu)+src[face,l,k]*fu)*fv).reshape(*shape,3)
def radical(i):
    value=0.;weight=.5
    while i:value+=(i&1)*weight;i>>=1;weight*=.5
    return value
size=128;mips=8;samples=64;u=np.arange(samples)/samples;v=np.array([radical(i) for i in range(samples)])
out=bytearray(b'R3PRB01\0'+struct.pack('<II',size,mips))
for mip in range(mips):
    edge=size>>mip;n=dirs(edge);colour=np.zeros_like(n)
    if mip==0:colour=sample(n)
    else:
        rough=mip/(mips-1);alpha=rough**2
        cosine=np.sqrt((1-v)/(1+(alpha*alpha-1)*v));sine=np.sqrt(1-cosine*cosine);phi=2*np.pi*u
        for start in range(0,len(n),1024):
            ns=n[start:start+1024]
            up=np.tile([0.,0.,1.],(len(ns),1));up[np.abs(ns[:,2])>.99]=[1,0,0]
            tangent=np.cross(up,ns);tangent/=np.linalg.norm(tangent,axis=-1,keepdims=True);bitangent=np.cross(ns,tangent)
            h=tangent[:,None,:]*(sine*np.cos(phi))[None,:,None]+bitangent[:,None,:]*(sine*np.sin(phi))[None,:,None]+ns[:,None,:]*cosine[None,:,None]
            light=2*cosine[None,:,None]*h-ns[:,None,:]
            weight=np.maximum(np.sum(ns[:,None,:]*light,axis=-1),0)
            colour[start:start+len(ns)]=(sample(light)*weight[...,None]).sum(axis=1)/weight.sum(axis=1)[:,None]
    rgba=np.concatenate([colour,np.ones((len(colour),1))],axis=1).astype('<f2')
    if not np.isfinite(rgba).all():raise ValueError('Non-finite probe radiance')
    out.extend(rgba.tobytes())
(a.folder/'probe.bin').write_bytes(out)
(a.folder/'probe.json').write_text(json.dumps({'source':(a.folder/'source.txt').read_text(),'width':size,'mips':mips,'samples':samples,'encoding':'linear RGBA16F, GGX filtered; LDR source','faces':[{'path':f'face-{i}.png','sha256':hashlib.sha256((a.folder/f'face-{i}.png').read_bytes()).hexdigest()} for i in range(6)],'sha256':hashlib.sha256(out).hexdigest()},indent=2)+'\n')
print('Captured level probe filtered; 6 source faces, 8 roughness levels')
