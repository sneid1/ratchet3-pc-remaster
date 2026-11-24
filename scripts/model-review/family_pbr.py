"""Compile calibrated finishes for every existing reviewed four-material atlas.

This is a shared material pass, not per-model art approval. Explicit fitted PBR
recipes take priority. No colour-derived normals/AO, new emission, or geometry.
"""
import csv, hashlib, json
from pathlib import Path
import numpy as np
from PIL import Image
from material_finishes import load, provenance

ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart';SIZE=512
recipes=json.loads(Path(__file__).with_name('material-recipes.json').read_text())
def rows(name):
    return list(csv.DictReader((s for s in (PACK/(name+'.tsv')).read_text().splitlines() if s and not s.startswith('#')),delimiter='\t'))

# Explicit physical classes of the previously inspected kit, never inferred
# from the colour of the character. Paint and corrosion are dielectrics.
families={
    'steel_painted_panels_002':('enamel',0), 'mol_arch_painted_metal_04':('enamel',0),
    'paint_001':('enamel',0), 'paint_003':('powder_coat',0),
    'aged_steel_001':('cast_alloy',1), 'steel_carbon_001':('cast_alloy',1),
    'steel_parkerized_001':('cast_alloy',1), 'brushed_steel_001':('brushed_steel',1),
    'steel_001':('brushed_steel',1), 'anodized_metal_01':('brushed_steel',1),
    'bronze_001':('polished_alloy',1), 'brass_001':('polished_alloy',1),
    'gold_001':('polished_alloy',1), 'chromium_001':('polished_alloy',1),
    'rust_tile_001':('cast_alloy',0),
    'detail_heavyclothwrinkles':('kit_detail_heavyclothwrinkles',0),
    'cotton_001':('kit_cotton_001',0), 'ard_cloth_tiling':('kit_ard_cloth_tiling',0),
    'detail_skinreptile_02':('kit_detail_skinreptile_02',0),
    'detail_skinhippo':('kit_detail_skinhippo',0),
    'detail_skinrhino_02':('kit_detail_skinrhino_02',0),
    'detail_skincrocodile':('kit_detail_skincrocodile',0),
}
profiles=dict(recipes['material_profiles'])
for material,(finish,metal) in families.items():profiles[material]={'finish':finish,'metalness':metal}
explicit={t['stem'] for t in recipes['targets']}
old=rows('pbr');result=[r for r in old if not r['normal_map'].startswith('family_')]
protected={r['stem'] for r in result}|explicit
records=[];cache={};sources={}
def save(name,a): Image.fromarray(np.uint8(np.clip(a,0,1)*255+.5)).save(PACK/'materials'/(name+'.png'))
for row in rows('regions'):
    if row['stem'] in protected:continue
    for i in range(4):
        name=row['material'+(str(i) if i else '')]
        if name not in ('-','fur') and name not in profiles:raise ValueError('Unclassified material '+name)
    mask_path=PACK/'regions'/row['map']
    spec={k:v for k,v in row.items() if k not in ('stem','map')}
    key=hashlib.sha256(mask_path.read_bytes()+json.dumps(spec,sort_keys=True).encode()).hexdigest()[:20]
    if key not in cache:
        masks=np.stack([np.asarray(c.resize((SIZE,SIZE),Image.Resampling.BILINEAR),dtype=np.float32)/255 for c in Image.open(mask_path).convert('RGBA').split()],axis=2)
        total=masks.sum(axis=2);masks/=np.maximum(total,1)[...,None]
        normal=np.zeros((SIZE,SIZE,2),np.float32);rough=np.full((SIZE,SIZE),.7,np.float32)
        metal=np.zeros_like(rough);fur=np.zeros_like(rough);catch=np.zeros_like(rough)
        max_catch=max(float(row.get('catchlight'+(str(i) if i else ''),0)) for i in range(4))
        for i in range(4):
            suffix=str(i) if i else '';name=row['material'+suffix];w=masks[...,i]
            if name=='-':continue
            catch+=w*float(row.get('catchlight'+suffix,0))/max(max_catch,1e-6)
            if name=='fur':fur+=w;continue
            profile=profiles[name]
            if profile.get('finish'):
                finish=profile['finish'];spec=dict(recipes['finishes'][finish])
                # Fine leather grain must not emboss whole glove-sized cells.
                if finish=='leather':spec.update(normal_gain=.32,normal_highpass=.006,repeats=8)
                if name=='rust_tile_001':spec.update(roughness=.76,roughness_range=[.66,.86],normal_gain=.25)
                n,r=load(json.dumps(spec,sort_keys=True),SIZE)
                sources[name]=provenance(finish,spec)
            else:
                n=np.zeros_like(normal);r=profile['roughness']['value']
            normal+=n*w[...,None];rough+=(r-.7)*w;metal+=profile['metalness']*w
        length=np.linalg.norm(normal,axis=2);normal*=np.minimum(1,.65/np.maximum(length,1e-6))[...,None]
        n=np.stack((normal[...,0]*.5+.5,normal[...,1]*.5+.5,np.sqrt(np.maximum(1-(normal*normal).sum(axis=2),0))*.5+.5,fur),axis=2)
        orm=np.stack((np.ones_like(rough),rough,metal,catch),axis=2)
        prefix='family_'+key
        save(prefix+'_n',n);save(prefix+'_orm',orm)
        cache[key]=(prefix+'_n',prefix+'_orm')
    n,orm=cache[key]
    result.append(dict(stem=row['stem'],normal_map=n,orm_map=orm,emissive_map=''))
    records.append(dict(stem=row['stem'],mask=row['map'],normal=n,orm=orm,
        materials=[row['material'+(str(i) if i else '')] for i in range(4)],
        state='Shared calibrated finish; fitted structural detail and individual model review pending'))
with (PACK/'pbr.tsv').open('w',newline='') as f:
    writer=csv.DictWriter(f,['stem','normal_map','orm_map','emissive_map'],delimiter='\t',lineterminator='\n');writer.writeheader();writer.writerows(result)
report=dict(scope='Every existing reviewed four-material atlas; all level occurrences automatically share it',
    atlas_count=len(records),unique_map_pairs=len(cache),explicit_pbr_preserved=len(protected),size=SIZE,
    colour='Original colour and alpha unchanged',ao='Texture AO neutral until a valid unique-UV geometry bake; native depth AO supplies contact',
    emission='Existing explicit emissions preserved; no hue-based emission inferred',
    geometry='Unchanged',completion='Shared material pass only; not all-enemy/NPC completion',sources=sources,assets=records)
(OUT/'family-pbr.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:report[k] for k in ['atlas_count','unique_map_pairs','explicit_pbr_preserved','completion']}))
