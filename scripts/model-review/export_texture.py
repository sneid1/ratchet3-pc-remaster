"""Export raw RGBA channels from any catalogued installed texture for inspection.

No normal reconstruction, gamma conversion, or guessed packing is applied.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import numpy as np

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
sys.path.insert(0,str(ROOT/'scripts'))
import riftapart

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--id',required=True);p.add_argument('--slice',type=int,default=0)
    args=p.parse_args();key=f'{int(args.id,16):016x}'
    catalogue=json.loads((OUT/'asset-catalogue.json').read_text())
    entry=next((r for r in catalogue['sources']['textures'] if r['id'].lower().removeprefix('0x')==key),None)
    if entry is None:raise ValueError('Texture is not in the installed catalogue')
    toc=riftapart.Toc();texture=riftapart.Texture(toc,entry['path'])
    if not 0<=args.slice<texture.slices:raise ValueError('Texture slice out of range')
    image=texture.image(args.slice,0).convert('RGBA')
    target=OUT/'library/textures'/key;target.mkdir(parents=True,exist_ok=True)
    path=target/f'slice{args.slice}.png';image.save(path)
    pixels=np.asarray(image)
    report={'id':key,'source':entry['path'],'format':texture.format_name,'slice':args.slice,'slices':texture.slices,
            'size':list(image.size),'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
            'conversion':'Raw decoded channels. No packing assumptions or normal reconstruction.',
            'channels':{c:{'min':int(pixels[...,i].min()),'median':float(np.median(pixels[...,i])),
                           'max':int(pixels[...,i].max())} for i,c in enumerate('RGBA')}}
    path.with_suffix('.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))

if __name__=='__main__':main()
