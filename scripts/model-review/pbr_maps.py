"""Author native PBR character atlases from inspected material regions and bakes.

AO comes from Blender geometry. Roughness/normal variation comes from the local
Rift Apart sources, while the original game's colour atlas remains authoritative.
"""
import csv
import argparse
import hashlib
import json
from pathlib import Path
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart'
EXPORT=Path('C:/riftapart-export')
parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--recipes',type=Path,default=Path(__file__).with_name('material-recipes.json'))
parser.add_argument('--model',action='append',help='Rebuild only these models, preserving other atlas maps and registrations')
args=parser.parse_args()
recipes=json.loads(args.recipes.read_text())
if recipes['schema']!=1:raise ValueError('Unsupported recipe schema')
SIZE=int(recipes['size'])
if not 64<=SIZE<=4096:raise ValueError('Material size out of range')
TARGETS={r['stem']:r for r in recipes['targets']}
if len(TARGETS)!=len(recipes['targets']):raise ValueError('Duplicate recipe target')
if args.model:
    if set(args.model)-{t['model'] for t in TARGETS.values()}:raise ValueError('Unknown model filter')
    TARGETS={s:t for s,t in TARGETS.items() if t['model'] in args.model}
from surface_layers import apply as apply_surface_layers, mask_for
from material_finishes import load as load_finish, provenance as finish_provenance

def read(path):
    # These are four independent data channels. RGBA resize premultiplies RGB
    # by A and destroys a mask whose fourth material has zero weight.
    channels=Image.open(path).convert('RGBA').split()
    return np.stack([np.asarray(c.resize((SIZE,SIZE),Image.Resampling.BILINEAR),dtype=np.float32)/255 for c in channels],axis=2)
def tiled(array,tiles_u,tiles_v):
    # Fractional and unequal repeats are common on environment surfaces.
    h,w=array.shape[:2]
    x=((np.arange(SIZE)+.5)/SIZE*tiles_u*w-.5)%w
    y=((np.arange(SIZE)+.5)/SIZE*tiles_v*h-.5)%h
    x0=np.floor(x).astype(int);y0=np.floor(y).astype(int)
    fx=(x-x0)[None,:,None];fy=(y-y0)[:,None,None]
    top=array[y0[:,None],x0[None,:]]*(1-fx)+array[y0[:,None],(x0+1)[None,:]%w]*fx
    bottom=array[(y0+1)[:,None]%h,x0[None,:]]*(1-fx)+array[(y0+1)[:,None]%h,(x0+1)[None,:]%w]*fx
    return top*(1-fy)+bottom*fy
def save(path,array):Image.fromarray(np.uint8(np.clip(array,0,1)*255+.5)).save(path)
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def polygon(points):
    im=Image.new('L',(256,256));ImageDraw.Draw(im).polygon(points,fill=255)
    return np.asarray(im.resize((SIZE,SIZE),Image.Resampling.BILINEAR),dtype=np.float32)/255

def unique_uv_coverage(model,stem):
    # Mirrored/stacked UVs cannot store different cavity lighting for each face.
    # Keep those texels neutral and let the native depth AO handle their contact.
    uv=[];counts=np.zeros((SIZE,SIZE),np.uint16);current=None
    for line in (OUT/f'candidate-blender-{model}.obj').read_text().splitlines():
        p=line.split()
        if not p:continue
        if p[0]=='vt':uv.append((float(p[1])*SIZE,(1-float(p[2]))*SIZE))
        elif p[0]=='usemtl':current=p[1]
        elif p[0]=='f' and current==stem:
            a,b,c=[np.array(uv[int(v.split('/')[1])-1]) for v in p[1:4]]
            tile=np.floor((a+b+c)/(3*SIZE))*SIZE
            a,b,c=a-tile,b-tile,c-tile
            lo=np.maximum(np.floor(np.minimum(np.minimum(a,b),c)).astype(int),0)
            hi=np.minimum(np.ceil(np.maximum(np.maximum(a,b),c)).astype(int),SIZE)
            if np.any(hi<=lo):continue
            x,y=np.meshgrid(np.arange(lo[0],hi[0])+.5,np.arange(lo[1],hi[1])+.5)
            den=(b[1]-c[1])*(a[0]-c[0])+(c[0]-b[0])*(a[1]-c[1])
            if abs(den)<1e-8:continue
            s=((b[1]-c[1])*(x-c[0])+(c[0]-b[0])*(y-c[1]))/den
            t=((c[1]-a[1])*(x-c[0])+(a[0]-c[0])*(y-c[1]))/den
            counts[lo[1]:hi[1],lo[0]:hi[0]]+=((s>0)&(t>0)&(s+t<1)).astype(np.uint16)
    ambiguous=Image.fromarray(np.uint8(counts!=1)*255).filter(ImageFilter.MaxFilter(3))
    return 1-np.asarray(ambiguous,dtype=np.float32)/255

rows=list(csv.DictReader((l for l in (PACK/'regions.tsv').read_text().splitlines() if not l.startswith('#')),delimiter='\t'))
manifest_path=OUT/'candidate/manifest.json'
manifest=json.loads(manifest_path.read_text())
source_records=[r for r in manifest.get('pbr',[]) if r.get('target') not in TARGETS]
pbr_rows=[r for r in csv.DictReader((PACK/'pbr.tsv').read_text().splitlines(),delimiter='\t') if r['stem'] not in TARGETS]
for row in rows:
    stem=row['stem']
    if stem not in TARGETS:continue
    target=TARGETS[stem];kind=target['model']
    weights=read(PACK/'regions'/row['map'])
    if target.get('imagegen_colour'):
        from authored_height import colour as compile_colour
        compile_colour(target['imagegen_colour'],weights,ROOT,PACK/'replacements'/(stem+'.png'))
    normal=np.zeros((SIZE,SIZE,2),np.float32)
    rough=np.ones((SIZE,SIZE),np.float32)*.7
    metal=np.zeros((SIZE,SIZE),np.float32)
    used=weights.sum(axis=2).clip(0,1);rough*=1-used
    fur_mask=np.zeros_like(rough);catch_mask=np.zeros_like(rough)
    max_catch=max(float(row.get('catchlight'+(str(i) if i else ''),0)) for i in range(4))
    for index in range(4):
        suffix=str(index) if index else '';name=row['material'+suffix]
        name=target.get('material_overrides',{}).get(name,name)
        if name=='-':continue
        w=weights[...,index]
        catch_mask+=w*float(row.get('catchlight'+suffix,0))/max(max_catch,1e-6)
        if name=='fur':
            # Preserve the existing shell mask and length when the rest of this
            # mixed atlas receives fitted normal/ORM detail. No colour-derived fur.
            fur_mask+=w
            rough+=w*.72
            continue
        source=PACK/'materials'/f'{name}.png'
        tiles=(float(row['tiles_u'+suffix]),float(row['tiles_v'+suffix]))
        mat=tiled(read(source),*tiles)
        w=weights[...,index]
        # Explicit recipes make source packing and physical classes reviewable.
        # A new material never silently inherits a guess from its filename.
        profile=target.get('profile_overrides',{}).get(name,recipes['material_profiles'][name])
        finish=profile.get('finish')
        if finish:
            finish_spec={**recipes['finishes'][finish],**profile.get('finish_overrides',{})}
            detail,finish_rough=load_finish(json.dumps(finish_spec,sort_keys=True),SIZE)
            normal+=detail*w[...,None]
            source_records.append({'target':stem,**finish_provenance(finish,finish_spec)})
        else:normal+=(mat[...,:2]*2-1)*w[...,None]*float(row.get('normal'+suffix,row['normal']))*profile.get('normal_strength',1)
        metallic=float(profile['metalness']);spec=profile['roughness']
        if metallic not in (0,1):raise ValueError('Use physical metal classes; blend at mask edges')
        mode=spec['mode']
        if finish:r=finish_rough
        elif mode=='source_variation':
            gloss_source=Path(spec['source'])
            g=Image.open(gloss_source).convert('RGBA')
            if 'crop' in spec:g=g.crop(tuple(spec['crop']))
            g=np.asarray(g.resize((SIZE,SIZE)),dtype=np.float32)/255
            variation=tiled(g,*tiles)[...,int(spec['channel'])]
            r=spec['base']+(variation.mean()-variation)*spec['strength']
            source_records.append({'target':stem,'source':str(gloss_source),'sha256':sha(gloss_source),'encoding':spec.get('encoding_evidence')})
        elif mode=='normal_slope':
            slope=np.linalg.norm(mat[...,:2]*2-1,axis=2)
            r=spec['base']+slope*spec['strength']
        elif mode=='constant':r=np.full_like(rough,spec['value'])
        elif mode=='inverse_channel':r=1-mat[...,int(spec['channel'])]
        else:raise ValueError('Unknown roughness recipe '+mode)
        r=finish_rough if finish else r.clip(*spec.get('range',[0,1]))
        rough+=w*r;metal+=w*metallic
        source_records.append({'target':stem,'material':name,'source':str(source),'sha256':sha(source)})
    for override in target.get('overrides',[]):
        mask=np.maximum.reduce([polygon(points) for points in override['polygons']])
        if override.get('scope')=='unassigned':mask*=1-used
        if 'metalness' in override:metal=metal*(1-mask)+override['metalness']*mask
        if 'roughness' in override:rough=rough*(1-mask)+override['roughness']*mask
        if 'normal_strength' in override:normal*=1-mask[...,None]*(1-override['normal_strength'])
    ao_path=OUT/'blender'/f'{stem}_ao.png'
    if not ao_path.exists():raise FileNotFoundError(f'Geometry AO must be baked first: {ao_path}')
    ao=read(ao_path)[...,0]
    valid_ao=unique_uv_coverage(kind,stem)
    ao=1-(1-ao)*valid_ao
    detail_report=apply_surface_layers(target.get('surface_layers',[]),normal,rough,metal,ao,recipes.get('finishes',{}),weights)
    authored=None
    if target.get('imagegen_height'):
        from authored_height import apply as apply_height, publish as publish_height
        authored=apply_height(target['imagegen_height'],target,weights,normal,rough,ao,ROOT)
    emission_name=''
    emission=np.zeros((SIZE,SIZE,4),np.float32)
    if target.get('emissive_regions'):
        # Authored light regions, never inferred from a yellow/orange hue.
        colour_source=next(p for p in [PACK/'replacements'/(stem+'.png'),OUT/'dumps'/(stem+'.png')] if p.exists())
        colour=read(colour_source)[...,:3]
        linear=np.where(colour<=.04045,colour/12.92,((colour+.055)/1.055)**2.4)
        emission=np.zeros((SIZE,SIZE,4),np.float32)
        for region in target['emissive_regions']:
            mask=mask_for(region,SIZE)
            emission[...,:3]=np.maximum(emission[...,:3],linear*np.array(region.get('tint',[1,1,1]),np.float32)*mask[...,None]*region.get('strength',1.5)/4)
            emission[...,3]=np.maximum(emission[...,3],mask*region.get('glow',.7))
            metal*=1-mask
            rough=rough*(1-mask)+region.get('roughness',.22)*mask
            ao=ao*(1-mask)+mask
        emission_name='pbr_'+stem+'_e'
        save(PACK/'materials'/(emission_name+'.png'),emission)
        source_records.append({'target':stem,'emissive_regions':target['emissive_regions'],
                               'emission_encoding':'Linear RGB / 4; A bloom mask. Painted armour excluded by explicit UV masks.'})
    nmap=np.ones((SIZE,SIZE,4),np.float32)
    nmap[...,:2]=normal*.5+.5
    nmap[...,2]=np.sqrt(np.maximum(1-(normal*normal).sum(axis=2),0))*.5+.5
    nmap[...,3]=fur_mask
    orm=np.stack((ao,rough,metal,catch_mask),axis=2)
    prefix='pbr_'+stem
    save(PACK/'materials'/f'{prefix}_n.png',nmap)
    save(PACK/'materials'/f'{prefix}_orm.png',orm)
    if authored is not None:
        colour_source=PACK/'replacements'/(stem+'.png')
        publish_height(authored,OUT/'deliverables/imagegen-layers'/target.get('delivery_key',kind),
                       read(colour_source),nmap,orm,emission,weights)
        source_records.append({'target':stem,'imagegen_height':authored['source'],
                               'sha256':authored['sha256']})
    pbr_rows.append({'stem':stem,'normal_map':prefix+'_n','orm_map':prefix+'_orm','emissive_map':emission_name})
    source_records.append({'target':stem,'ao_source':str(ao_path),'sha256':sha(ao_path),'ao':'Blender 32-sample local geometry bake, 2% body height ray distance',
                           'colour':'Original UYA atlas/installed fitted replacement, unchanged by this stage',
                           'authored_cavity':'Explicit UV recesses attenuate indirect AO in addition to the geometry bake; never inferred from colour brightness',
                           'metalness_classes':'paint/skin/glass/rubber 0; exposed alloy 1',
                           'unique_uv_ao_fraction':float(valid_ao.mean()),'shared_uv_ao':'neutral; native depth AO supplies contact',
                           'roughness_range':[float(rough.min()),float(rough.max())],
                           'authored_surface_features':detail_report})
with (PACK/'pbr.tsv').open('w',newline='') as stream:
    writer=csv.DictWriter(stream,['stem','normal_map','orm_map','emissive_map'],delimiter='\t',lineterminator='\n')
    writer.writeheader();writer.writerows(pbr_rows)
if not set(TARGETS)<={r['stem'] for r in pbr_rows}:raise ValueError('Recipe target has no matching region mapping')
manifest['pbr']=source_records
manifest['pbr_recipes']={'path':str(args.recipes.resolve()),'sha256':sha(args.recipes),'recipes':recipes}
baseline=Path(manifest['baseline']);manifest['files']=[]
for path in sorted(p for p in PACK.rglob('*') if p.is_file()):
    rel=path.relative_to(PACK).as_posix();old=baseline/rel
    digest=sha(path);old_digest=sha(old) if old.exists() else None
    manifest['files'].append({'path':rel,'bytes':path.stat().st_size,'sha256':digest,'baseline_sha256':old_digest,'changed':digest!=old_digest})
manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
print('Authored PBR atlases:',', '.join(t['model'] for t in TARGETS.values()))
