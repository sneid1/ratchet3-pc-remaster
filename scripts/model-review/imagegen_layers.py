"""Compile reviewed imagegen colour/height art into editable, aligned PBR layers.

Height is independently authored technical art, never albedo luminance. Existing
semantic masks determine physical materials; colour alpha and protected eyes
remain exact. All published channels have the same dimensions/UV coordinates.
"""
import argparse,csv,hashlib,json,shutil
from pathlib import Path
import numpy as np
from PIL import Image,ImageFilter,ImageDraw
from material_finishes import load
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review';PACK=OUT/'candidate/0-riftapart'
ART=OUT/'authored/roster';DEST=OUT/'deliverables/imagegen-layers';SIZE=1024
parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--key',action='append',help='Rebuild only a selected atlas, preserving the other reviewed sets')
parser.add_argument('--extra-config',type=Path,help='Additional individually authored atlas recipes')
args=parser.parse_args()
CONFIG=[
 dict(key='ratchet-ship',stem='8b45ad96956dc5cb-2f12be7762d79200-00002213',model='ratchet_ship',
      relief=[4,5,1,3],protect=[(0,27,5,38),(6,7,12,14),(127/2,27/2,149/2,49/2),(0,235/2,34/2,128)],colour_blend=.85,
      preserve_candidate_colour=True,skip_geometry_ao=True,preserve_metalness=True,
      finishes=['enamel','polished_alloy','leather','rubber'],
      finish_overrides={'enamel':dict(normal_gain=.015,repeats=24,roughness=.38,variation=.018,roughness_range=[.35,.42]),
                        'polished_alloy':dict(normal_gain=.025,repeats=16,tool_marks=.002),
                        'leather':dict(normal_gain=.1,repeats=24)},
      notes='Original orange/copper ship skin 0. Fitted coating/alloy/leather/recess materials; shared by all three hull configurations and original kit parts.'),
 dict(key='clank-standalone',stem='407dd834b00a78e5-ead08c0b13f84a33-00002213',model='clank_standalone',
      relief=[3,0,0,0],protect=[(0,102,18,128),(76,16,84,23)],colour_blend=.45,
      preserve_candidate_colour=True,skip_geometry_ao=True,
      finishes=['polished_alloy',None,None,None],preserve_metalness=True,
      finish_overrides={'polished_alloy':dict(normal_gain=.04,repeats=12,tool_marks=.003,roughness=.36,variation=.03,roughness_range=[.32,.42])},
      notes='Standalone class 87 mesh. Existing satin alloy, polymer/lens masks and exact green-eye/red-antenna emission retained. Shared atlas also reviewed on backpack Clank.'),
 dict(key='skidd',stem='8ab322449dd1c179-b3400fef9136c093-00001dd3',model='source_c7012',
      relief=[1.2,1.6,6.0,0],protect=[(112,29,128,55)],protect_falloff=3,colour_blend=.72,
      finishes=['leather',None,None,None],finish_overrides={'leather':dict(normal_gain=.12,repeats=32,normal_highpass=.006)},
      notes='Green soft skin, vest leather, brass trim. Eye retained from the original atlas. Not scaly reptile hide.'),
 dict(key='large-ty',stem='ca66c534af98574e-4d65c55bfb974eed-00001dd3',model='source_c7074',
      relief=[8.0,8.0,9.0,4.0],protect=[(0,27,10,55),(47,53,61,76),(118,21,128,34)],colour_blend=1.0,
      colour_source='large-ty-colour-clean.png',
      notes='Armoured three-eyed Tyhrranoid. UV lamp swatches protected; emission remains off pending part-specific assignment.'),
 dict(key='skidd-uniform',stem='dc2917b9a7a93d45-5e43e2764fc037a-00001dd3',model='source_c7012',
      relief=[4,4,5,0],protect=[(67,14,119,63)],colour_blend=.38,
      finish_overrides={'leather':dict(normal_gain=.12,repeats=32,normal_highpass=.006)},
      finishes=['woven_cloth','leather','enamel',None],notes='Fan uniform: woven orange torso, charcoal reinforcement and leather gloves; original chest emblem protected.'),
 dict(key='qwark-uniform',stem='bb473cf961787faf-93f11c38cd19f1cf-00001dd3',model='source_c7000',
      relief=[4,4,3,0],protect=[],surrounding_cloth=1,colour_blend=.65,
      protect_polygon=[(38,52),(59,45),(61,41),(72,45),(86,41),(89,44),(88,51),
                       (95,59),(95,72),(89,78),(93,84),(86,86),(80,81),(70,85),
                       (65,83),(59,86),(55,84),(55,78),(50,71),(49,63),(38,58)],
      finishes=['woven_cloth','woven_cloth','enamel',None],notes='Hero uniform: woven stretch fabric, leather and printed emblem. Original emblem protected; dielectric coatings.'),
 dict(key='sasha-uniform',stem='ae98086fc46a628c-2d18c4c3c5dea1d7-00001dd3',model='source_c7013',
      relief=[4,4,0,0],protect=[(88,111,108,121)],colour_blend=.68,
      finishes=['leather','woven_cloth',None,None],notes='Officer uniform: boot/glove leather and woven purple tailoring. Removes the former skin finish on cloth.'),
 dict(key='big-al',stem='80903324af4b6f64-801f57cebe514413-00001dd3',model='source_c6501',
      relief=[3.5,0,2,0],protect=[(83,7,97,21),(87,25,99,34),(93,38,99,46),(105,39,121,53),(105,53,118,68)],source_skin=1,colour_blend=.62,
      finishes=['woven_cloth','soft_skin',None,None],notes='Cotton lab coat, soft skin and fine directional hair. Eyes, nose, mouth and lenses protected.'),
 dict(key='big-al-phoenix',stem='567fb40a2adebf5c-991af79c599fdb99-00001dd3',model='big_al_qforce_c7121',
      relief=[0,0,0,0],protect=[],source_only=True,colour_blend=0,skip_geometry_ao=True,
      finishes=[None,None,'soft_skin',None],finish_overrides={'soft_skin':dict(normal_gain=.06)},
      notes='Phoenix Q-Force face uses its own colour atlas and a fine skin finish in its fitted skin channel.'),
 dict(key='qwark-gear',stem='3e891aafab7d6426-c4510488afc57f2c-00001dd3',model='source_c7000',
      relief=[4,3,0,5],protect=[(42,56,89,86),(49,89,74,101)],colour_blend=.62,
      dielectric_uv=[dict(label='Eye white',roughness=.38,points=[(50.4,50.2),(54.7,47.6),(59.5,55.8),(56.3,57.2),(52.1,55.8)])],
      finishes=['rubber','woven_cloth','soft_skin',None],notes='Remaining hero suit and equipment. Blue sleeve/torso UV islands are cloth, black bellows rubber, green suit fabric; white hardware remains alloy. Tan face patches retain original identity.'),
 dict(key='sasha-head',stem='7b7e72086a72f20c-b23d415d010e4ae-00001dd3',model='source_c7013',
      relief=[1.2,0,0,0],protect=[(7,27,37,55),(71,30,105,53),(113,45,128,68),(112,70,128,93)],protect_falloff=4,colour_blend=.58,
      finishes=['soft_skin',None,None,None],notes='Fine short facial fur and directional hair. Eye/lid, nose, mouth and gemstone UVs protected.'),
 dict(key='big-al-shirt',stem='46c953e3735925da-4998c4df7ee95149-00001993',model='source_c6501',
      relief=[0,0,0,0],protect=[],colour_blend=0,flat=True,
      uv_bake='captures/rift-texture-review/authored/big-al-shirt-rebaked.png',
      finishes=['woven_cloth',None,None,None],notes='Original shirt palette transferred to nondegenerate Blender UVs; fine woven normals use the new UVs. Requires the edited Big Al mesh; topology unchanged.'),
]
def table(name):return list(csv.DictReader((s for s in (PACK/(name+'.tsv')).read_text().splitlines() if s and not s.startswith('#')),delimiter='\t'))
def image(path):
    return np.stack([np.asarray(c.resize((SIZE,SIZE),Image.Resampling.BILINEAR),np.float32)/255 for c in Image.open(path).convert('RGBA').split()],axis=2)
def save(path,a):
    path.parent.mkdir(parents=True,exist_ok=True);Image.fromarray(np.uint8(np.clip(a,0,1)*255+.5)).save(path)
def blur(a,r):return np.asarray(Image.fromarray(np.uint8(np.clip(a,0,1)*255+.5)).filter(ImageFilter.GaussianBlur(r)),np.float32)/255

def unique_uv(model,stem):
    """Exclude stacked/mirrored UVs from geometry AO, matching the hero pipeline."""
    size=512;uv=[];counts=np.zeros((size,size),np.uint16);current=None
    for line in (OUT/f'candidate-blender-{model}.obj').read_text().splitlines():
        p=line.split()
        if not p:continue
        if p[0]=='vt':uv.append((float(p[1])*size,(1-float(p[2]))*size))
        elif p[0]=='usemtl':current=p[1]
        elif p[0]=='f' and current==stem:
            a,b,c=[np.array(uv[int(v.split('/')[1])-1]) for v in p[1:4]]
            tile=np.floor((a+b+c)/(3*size))*size;a,b,c=a-tile,b-tile,c-tile
            lo=np.maximum(np.floor(np.minimum(np.minimum(a,b),c)).astype(int),0)
            hi=np.minimum(np.ceil(np.maximum(np.maximum(a,b),c)).astype(int),size)
            if np.any(hi<=lo):continue
            x,y=np.meshgrid(np.arange(lo[0],hi[0])+.5,np.arange(lo[1],hi[1])+.5)
            den=(b[1]-c[1])*(a[0]-c[0])+(c[0]-b[0])*(a[1]-c[1])
            if abs(den)<1e-8:continue
            s=((b[1]-c[1])*(x-c[0])+(c[0]-b[0])*(y-c[1]))/den
            t=((c[1]-a[1])*(x-c[0])+(a[0]-c[0])*(y-c[1]))/den
            counts[lo[1]:hi[1],lo[0]:hi[0]]+=((s>0)&(t>0)&(s+t<1)).astype(np.uint16)
    safe=Image.fromarray(np.uint8(counts==1)*255).filter(ImageFilter.MinFilter(3))
    return np.asarray(safe.resize((SIZE,SIZE),Image.Resampling.BILINEAR),np.float32)/255
if args.extra_config:
    CONFIG.extend(json.loads(args.extra_config.read_text()))
    assert len({c['key'] for c in CONFIG})==len(CONFIG),'Duplicate atlas recipe'
regions={r['stem']:r for r in table('regions')};pbr={r['stem']:r for r in table('pbr')}
recipes=json.loads(Path(__file__).with_name('material-recipes.json').read_text())
prior={r['key']:r for r in json.loads((DEST/'report.json').read_text())} if (DEST/'report.json').exists() else {}
reports=[r for key,r in prior.items() if key not in {c['key'] for c in CONFIG}]
if args.key and set(args.key)-{c['key'] for c in CONFIG}:raise ValueError('Unknown atlas key')
for c in CONFIG:
    if args.key and c['key'] not in args.key:
        if c['key'] in prior:reports.append(prior[c['key']])
        continue
    stem,key=c['stem'],c['key'];folder=DEST/key;folder.mkdir(parents=True,exist_ok=True)
    original=Path('C:/Users/<you>/Games/Ratchet and Clank 3/texture_dumps')/(stem+'.png')
    # Deterministic rebuilds always use this baseline, never the last output.
    archive=ART/(stem+'-baseline.png')
    if not archive.exists():shutil.copy2(PACK/'replacements'/(stem+'.png') if (PACK/'replacements'/(stem+'.png')).exists() else original,archive)
    base=image(archive)
    # The installed HD pack uses a different format suffix with the same two
    # content hashes. Protect its existing sharp identity detail, rather than
    # replacing eyes/emblems with a resampled 128-square PS2 dump. Keep source
    # opacity independently: alpha is not an extra colour/material channel.
    hd=list((OUT/'profile/texture_replacements/reddyxd-uya').glob('-'.join(stem.split('-')[:2])+'-*.png'))
    identity_source=archive
    if len(hd)==1 and not c.get('preserve_candidate_colour'):
        identity_source=hd[0];base[...,:3]=image(identity_source)[...,:3]
    elif len(hd)>1 and not c.get('preserve_candidate_colour'):raise ValueError('Ambiguous source colour variants: '+stem)
    if c.get('uv_bake'):
        identity_source=ROOT/c['uv_bake'];base=image(identity_source)
    use_source=c.get('flat') or c.get('source_only')
    art=base.copy() if use_source else image(ART/c.get('colour_source',key+'-colour.png'))
    if c.get('palette_lock'):
        # The original palette, where the generated sheet drifted its hues: broad colour (Gaussian radius in px at
        # SIZE) from the identity source, finer detail from the generated art.
        r=c['palette_lock'];art[...,:3]=np.clip(art[...,:3]+blur(base[...,:3],r)-blur(art[...,:3],r),0,1)
    height=np.full((SIZE,SIZE),.5,np.float32) if use_source else image(ART/(key+'-height.png'))[...,0]
    if stem in regions:weights=image(PACK/'regions'/regions[stem]['map'])
    elif key=='skidd-uniform':
        # Explicit UV zones for Skidd's previously unmapped fan-uniform atlas.
        weights=np.zeros((SIZE,SIZE,4),np.float32);weights[...,0]=1
        labels=Image.new('L',(SIZE,SIZE));d=ImageDraw.Draw(labels)
        for box in [(0,26,58,63),(58,78,128,120),(22,80,54,126)]:d.rectangle(tuple(round(v*SIZE/128) for v in box),fill=1)
        for box in [(60,78,77,90),(63,121,116,128)]:d.rectangle(tuple(round(v*SIZE/128) for v in box),fill=2)
        labels=np.asarray(labels)
        for i in range(3):weights[...,i]=(labels==i)
    else:
        assert c.get('flat'),'Missing semantic material zones'
        weights=np.zeros((SIZE,SIZE,4),np.float32);weights[...,0]=1
    if key=='qwark-gear':
        # The prior kit mask called blue cloth and black bellows aged steel.
        # Inspected source UV polygons isolate both blue cloth islands; the
        # remaining former steel channel now receives rubber, not metal.
        zones=Image.new('L',(SIZE,SIZE));d=ImageDraw.Draw(zones)
        for points in [[(54,26),(81,24),(114,26),(115,61),(62,63)],[(0,93),(12,87),(36,87),(49,91),(59,116),(58,128),(0,128)]]:
            d.polygon([(round(x*SIZE/128),round(y*SIZE/128)) for x,y in points],fill=255)
        cloth=np.asarray(zones)>0;weights[cloth]=[0,1,0,0]
        # The family map classified the painted highlights inside the two chin
        # islands as polished metal. Cover the full skin UVs, including their
        # filter margins, while leaving the adjacent black vent and silver kit.
        skin=Image.new('L',(SIZE,SIZE)); d=ImageDraw.Draw(skin)
        for points in [[(56,71),(64,66),(76,61),(87,61),(86,66),(87,72),
                        (91,80),(92,84),(87,86),(56,86)],
                       [(49,89),(72,89),(74,92),(72,97),(65,100),(53,99),(49,96)]]:
            d.polygon([(round(x*SIZE/128),round(y*SIZE/128)) for x,y in points],fill=255)
        weights[np.asarray(skin)>0]=[0,0,1,0]
    weights/=np.maximum(weights.sum(axis=2),1)[...,None]
    mask=np.clip(weights.sum(axis=2),0,1)
    fence=Image.new('L',(SIZE,SIZE));d=ImageDraw.Draw(fence)
    for box in c['protect']:d.rectangle(tuple(round(v*SIZE/128) for v in box),fill=255)
    if 'protect_polygon' in c:
        d.polygon([(round(x*SIZE/128),round(y*SIZE/128)) for x,y in c['protect_polygon']],fill=255)
    protection=np.asarray(fence,np.float32)/255
    if key=='skidd-uniform':
        # This badge is blue, yellow, white and ink on burnt-orange fabric.
        # Keep its printed edge and let the fabric weave continue through
        # the orange gaps between the badge and its orbit strokes.
        red,green,blue=base[...,:3].transpose(2,0,1)
        cloth=np.clip((np.minimum(red-green,red-blue)-.12)*8,0,1)
        protection*=1-cloth
    if c.get('protect_falloff'):
        # Keep every protected texel exact; taper the transition outside the
        # box so a straight boundary cannot cut across facial fur or skin.
        y,x=np.ogrid[:SIZE,:SIZE]
        for box in c['protect']:
            left,top,right,bottom=[round(v*SIZE/128) for v in box]
            dx=np.maximum(np.maximum(left-x,x-right),0)
            dy=np.maximum(np.maximum(top-y,y-bottom),0)
            t=np.minimum(np.hypot(dx,dy)/(c['protect_falloff']*SIZE/128),1)
            protection=np.maximum(protection,1-t*t*(3-2*t)).astype(np.float32)
    if 'surrounding_cloth' in c:
        # Follow the emblem silhouette in the fitted atlas. The green cloth inside its
        # bounding box needs the same colour and relief as the cloth just outside.
        protection*=1-weights[...,c['surrounding_cloth']]
    if 'source_skin' in c:
        protection=np.maximum(protection,weights[...,c['source_skin']])
    keep=protection>=1
    mask*=1-protection
    # Erode the eligibility mask at atlas boundaries so generated edge drift
    # cannot overwrite neighbouring islands or the original empty space.
    mask=np.asarray(Image.fromarray(np.uint8(mask*255)).filter(ImageFilter.MinFilter(5)),np.float32)/255
    colour=base.copy();colour[...,:3]=base[...,:3]*(1-mask[...,None]*c['colour_blend'])+art[...,:3]*mask[...,None]*c['colour_blend']
    colour[...,3]=base[...,3]
    if key=='large-ty':
        # Visually identified green coating on this robot atlas. Its source
        # palette isolates that already-identified paint, not a game-wide hue
        # classifier. The replacement artwork already removes organic spots.
        green=(base[...,1]>base[...,0]*1.25)&(base[...,1]>base[...,2]*1.12)&(mask>.2)
        # The corrected imagegen atlas has clean enamel. Do not blend the old
        # displaced vents/panel borders back into this single coherent drawing.
    previous=pbr.get(stem,{'emissive_map':''})
    if previous.get('normal_map'):
        normal=image(PACK/'materials'/(previous['normal_map']+'.png'));orm=image(PACK/'materials'/(previous['orm_map']+'.png'))
    else:
        normal=np.zeros_like(base);normal[...,:2]=.5;normal[...,2]=1
        orm=np.zeros_like(base);orm[...,0]=1;orm[...,1]=.75
    # Family inputs must remain the pre-authoring input on repeated compiles.
    for label,array in [('normal',normal),('orm',orm)]:
        f=ART/(key+'-family-'+label+'.png')
        if not f.exists():save(f,array)
    normal=image(ART/(key+'-family-normal.png'));orm=image(ART/(key+'-family-orm.png'))
    if c.get('skip_geometry_ao'):orm[...,0]=1
    for i,finish in enumerate(c.get('finishes',[])):
        if not finish:continue
        spec=dict(recipes['finishes'][finish])
        if finish=='leather':spec.update(normal_gain=.28,repeats=8,normal_highpass=.006)
        if finish=='woven_cloth':spec.update(normal_gain=.16,repeats=64)
        spec.update(c.get('finish_overrides',{}).get(finish,{}))
        n,r=load(json.dumps(spec,sort_keys=True),SIZE);w=weights[...,i]
        if c.get('flat') and not c.get('uv_bake'):n*=0;r[:]=spec['roughness']
        normal[...,:2]=normal[...,:2]*(1-w[...,None])+(n*.5+.5)*w[...,None]
        orm[...,1]=orm[...,1]*(1-w)+r*w
        if not c.get('preserve_metalness'):orm[...,2]*=1-w
    if key=='large-ty':orm[green,2]=0;orm[green,1]=.43
    if key=='skidd':
        n,r=load(json.dumps(recipes['finishes']['soft_skin'],sort_keys=True),SIZE)
        w=weights[...,1];normal[...,:2]=normal[...,:2]*(1-w[...,None])+(n*.5+.5)*w[...,None]
        orm[...,1]=orm[...,1]*(1-w)+r*w;orm[...,2]*=1-w
    # Suppress broad generated bas-relief gradients: the mesh owns the shape.
    surface=blur(height,.8)-blur(height,12)
    strength=sum(weights[...,i]*v for i,v in enumerate(c['relief']))*mask
    dy,dx=np.gradient(surface);xy=normal[...,:2]*2-1+np.stack((-dx,dy),axis=2)*strength[...,None]
    xy*=np.minimum(1,.65/np.maximum(np.linalg.norm(xy,axis=2),1e-6))[...,None]
    normal[...,:2]=xy*.5+.5;normal[...,2]=np.sqrt(np.maximum(0,1-(xy*xy).sum(axis=2)))*.5+.5
    cavity=np.maximum(blur(height,4)-blur(height,1),0)*mask
    crests=np.maximum(blur(height,1)-blur(height,4),0)*mask
    ao=np.clip(1-cavity*1.3,.72,1)
    ao_path=OUT/'blender'/(stem+'_ao.png');geometry_coverage=0.0
    ao_source='Local height cavity plus native depth contact AO; geometry UV bake unavailable'
    if ao_path.exists() and not c.get('uv_bake') and not c.get('skip_geometry_ao'):
        safe=unique_uv(c['model'],stem);geometry_coverage=float(safe.mean())
        ao*=1-(1-np.clip(image(ao_path)[...,0],.55,1))*safe
        ao_source='Blender local geometry AO on unique UVs + authored height cavity; native depth contact for stacked UVs'
    wear=np.clip(crests*2,0,1);grime=np.clip(cavity*2.5,0,1)
    if key=='skidd':wear*=1-weights[...,1];grime*=1-weights[...,1]
    orm[...,0]*=ao;orm[...,1]=np.clip(orm[...,1]+grime*.12-wear*.10,.12,.94)
    for region in c.get('dielectric_uv',[]):
        # Eye whites and teeth can share an atlas with silver hardware.
        # Correct only the UV islands sampled by the original face mesh.
        zone=Image.new('L',(SIZE,SIZE))
        ImageDraw.Draw(zone).polygon([(round(x*SIZE/128),round(y*SIZE/128))
                                     for x,y in region['points']],fill=255)
        inside=np.asarray(zone)>0
        orm[...,1][inside]=region['roughness']
        orm[...,2][inside]=0
    # No new emissive region is inferred from hue or generated brightness.
    emission=image(PACK/'materials'/(previous['emissive_map']+'.png')) if previous.get('emissive_map') else np.zeros_like(normal)
    opacity=base[...,3]
    detail=height-blur(height,2);dy,dx=np.gradient(detail);dn=np.stack((-dx,dy),axis=2)*strength[...,None]*.4
    dn*=np.minimum(1,.35/np.maximum(np.linalg.norm(dn,axis=2),1e-6))[...,None]
    dn=np.dstack((dn*.5+.5,np.sqrt(np.maximum(0,1-(dn*dn).sum(axis=2)))*.5+.5))
    layers={'base-colour':colour,'normal':normal[...,:3],'roughness':orm[...,1],'metalness':orm[...,2],
        'ao':orm[...,0],'height':np.clip(.5+surface*mask,0,1),'detail-normal':dn,
        'emission':emission[...,:3],'opacity':opacity,'wear-mask':wear,'grime-mask':grime,'material-masks':weights,'orm-packed':orm[...,:3]}
    for name,array in layers.items():save(folder/(name+'.png'),array)
    assert np.array_equal(np.uint8(colour[...,3]*255+.5),np.uint8(base[...,3]*255+.5))
    assert np.array_equal(colour[keep],base[keep])
    save(PACK/'replacements'/(stem+'.png'),colour)
    prefix='authored_'+key.replace('-','_')
    save(PACK/'materials'/(prefix+'_n.png'),normal);save(PACK/'materials'/(prefix+'_orm.png'),orm)
    pbr[stem]={'stem':stem,'normal_map':prefix+'_n','orm_map':prefix+'_orm','emissive_map':previous.get('emissive_map','')}
    record=dict(**c,size=SIZE,layer_types=list(layers),alpha_exact=True,protected_pixels_exact=True,
        height_source=('Neutral: original colour and fitted finish only' if c.get('source_only') else 'Neutral: source is a constant palette swatch' if c.get('flat') else 'Independently imagegen-authored technical height, broad gradients removed; not albedo luminance'),
        ao_source=ao_source,geometry_ao_unique_uv_fraction=geometry_coverage,
        colour_baseline=str(identity_source),opacity_baseline=str(archive),
        geometry=('Original topology; shirt UVs remapped in edited mesh' if c.get('uv_bake') else 'Original source mesh unchanged'),completion='Candidate; native multi-angle inspection required',
        files={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in folder.glob('*.png')},
        art_sources={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in ART.glob(key+'-*.png')})
    (folder/'layers.json').write_text(json.dumps(record,indent=2)+'\n');reports.append(record)
with (PACK/'pbr.tsv').open('w',newline='') as f:
    w=csv.DictWriter(f,['stem','normal_map','orm_map','emissive_map'],delimiter='\t',lineterminator='\n');w.writeheader();w.writerows(pbr.values())
(DEST/'report.json').write_text(json.dumps(reports,indent=2)+'\n')
print(json.dumps([dict(model=r['model'],layers=len(r['layer_types']),alpha_exact=r['alpha_exact']) for r in reports]))
