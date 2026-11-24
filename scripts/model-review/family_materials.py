"""Apply explicit physical material families across the existing mapped library."""
import csv
import hashlib
import json
from pathlib import Path
import numpy as np
from PIL import Image, ImageFilter
from material_finishes import load as load_finish, provenance as finish_provenance, spread

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart'
BASE=OUT/'profile/texture_replacements/0-riftapart'
recipe=json.loads(Path(__file__).with_name('family-materials.json').read_text())
material_recipes=json.loads(Path(__file__).with_name('material-recipes.json').read_text())
profiles={m:f for f in recipe['families'] for m in f['materials']}
def table(path):
    reader=csv.DictReader((s for s in path.read_text().splitlines() if s and not s.startswith('#')),delimiter='\t')
    return reader.fieldnames,list(reader)
changed=[];used=set()
overrides=json.loads(Path(__file__).with_name('environment-overrides.json').read_text())
reviewed=json.loads(Path(__file__).with_name('reviewed-surfaces.json').read_text())
for file in ('surfaces.tsv','regions.tsv'):
    fields,rows=table(PACK/file)
    if file=='surfaces.tsv':
        by_stem={r['stem']:r for r in rows}
        for item in overrides['surfaces']+reviewed['surfaces']:
            row=by_stem.get(item['stem'])
            if row is None:
                row={'stem':item['stem'],'material':item['material'],'tiles_u':'1','tiles_v':'1','reflectance':'0.2','grain':'0','normal':'1','specular':'1','metal':'0'}
                rows.append(row)
            for key in fields:
                if key in item:row[key]=str(item[key])
    for row in rows:
        for suffix in ('','1','2','3') if file=='regions.tsv' else ('',):
            name=row['material'+suffix]
            if file=='regions.tsv':row['catchlight'+suffix]='0'
            family=profiles.get(name)
            if family is None:continue
            used.add(name)
            old=row['metal'+suffix];row['metal'+suffix]=str(family['metalness'])
            # Artificial eye lights are removed; captured level lights supply
            # the highlight. Their fitted dielectric material remains glossy.
            if file=='regions.tsv':row['catchlight'+suffix]='0'
            changed.append({'stem':row['stem'],'slot':suffix,'family':family['name'],'material':name,'metal_before':old,'metal_after':row['metal'+suffix]})
    with (PACK/file).open('w',newline='') as stream:
        writer=csv.DictWriter(stream,fields,delimiter='\t',lineterminator='\n');writer.writeheader();writer.writerows(rows)
maps=[]
for name in sorted(used):
    if profiles[name].get('builtin'):
        maps.append({'material':name,'family':profiles[name]['name'],'source':'Native procedural fur; no bitmap source','response':'Preserved strand shader; physical metalness 0; no artificial catchlight'})
        continue
    source=BASE/'materials'/(name+'.png')
    im=Image.open(source).convert('RGBA');data=np.array(im)
    family=profiles[name];low,high=family['roughness']
    if 'normal_scale' in family:
        data[...,:2]=np.uint8(np.clip(127.5+(data[...,:2].astype(np.float32)-127.5)*family['normal_scale'],0,255)+.5)
    raw_rough=1-data[...,2].astype(np.float32)/255
    rough=(raw_rough).clip(low,high)
    if family.get('remap_roughness'):
        # Clamping almost-white source gloss made whole masonry/soil families
        # flat. Remap the existing encoded gloss, retaining each source pattern.
        rough=np.clip((low+high)*.5+spread(raw_rough)*(high-low)*.8,low,high)
    data[...,2]=np.uint8((1-rough)*255+.5)
    # The old A channel multiplied another material's photographed albedo and
    # normal-derived "cavities" over the existing colour. That made every panel
    # dirty and painted foreign seams across intact UYA surface designs.
    name_of_family=family['name']
    if name_of_family=='painted surfaces':
        clean=Image.open(PACK/'materials/review_painted_metal.png').convert('RGBA')
        for channel in (0,1):
            data[...,channel]=np.asarray(clean.getchannel(channel).resize(im.size,Image.Resampling.BILINEAR))
        # Fine coating variation, with no unrelated panel layout or scratches.
        detail=np.asarray(clean.getchannel(0).resize(im.size,Image.Resampling.BILINEAR),dtype=np.float32)/255-.5
        rough=np.clip(.47+detail*.12,.43,.52)
        data[...,2]=np.uint8((1-rough)*255+.5)
    finish_name=recipe.get('finish_by_material',{}).get(name)
    if finish_name:
        spec=material_recipes['finishes'][finish_name]
        detail,finish_rough=load_finish(json.dumps(spec,sort_keys=True),512)
        # The finish is fine scale only. Existing UV assignments provide its
        # repetition; no foreign panel seams or colour are copied from the kit.
        for channel in range(2):
            encoded=Image.fromarray(np.uint8(np.clip(detail[...,channel]*.5+.5,0,1)*255+.5))
            data[...,channel]=np.asarray(encoded.resize(im.size,Image.Resampling.BILINEAR))
        gloss=Image.fromarray(np.uint8((1-finish_rough)*255+.5))
        data[...,2]=np.asarray(gloss.resize(im.size,Image.Resampling.BILINEAR))
        rough=1-data[...,2].astype(np.float32)/255
    if name_of_family in ('stone','soil and sand','bark and timber','masonry','mud','snow'):
        # Retain only subdued fine variation, removing broad photographic
        # light/shadow bands that cannot follow the level's illumination.
        grain=np.asarray(im.getchannel('A'),dtype=np.float32)/102
        blur=np.asarray(im.getchannel('A').filter(ImageFilter.GaussianBlur(max(im.size)/48)),dtype=np.float32)/102
        grain=np.clip(1+(grain-blur)*.18,.92,1.08)
        data[...,3]=np.uint8(grain*102+.5)
    else:data[...,3]=102
    target=PACK/'materials'/(name+'.png');Image.fromarray(data).save(target)
    maps.append({'material':name,'family':family['name'],'roughness_bounds':[float(rough.min()),float(rough.max())],'source':str(source),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'sha256':hashlib.sha256(target.read_bytes()).hexdigest(),'colour_grain':'Subdued high-frequency variation on natural surfaces; neutral on manufactured surfaces, skin, cloth and foliage.'})
    if finish_name:maps[-1]['authored_finish']=finish_provenance(finish_name,material_recipes['finishes'][finish_name])
explicit=set(json.loads(Path(__file__).with_name('material-recipes.json').read_text())['material_profiles'])
uncovered=set()
for file in ('surfaces.tsv','regions.tsv'):
    _,rows=table(PACK/file)
    for row in rows:
        for suffix in ('','1','2','3') if file=='regions.tsv' else ('',):
            name=row['material'+suffix]
            if name!='-' and name not in profiles and name not in explicit:uncovered.add(name)
if uncovered:raise ValueError('Mapped materials have no explicit physical family: '+', '.join(sorted(uncovered)))
report={'recipes':recipe,'environment_overrides':overrides,'reviewed_surface_assignments':reviewed,'changed_slots':changed,'maps':maps,'unclassified_mapped_materials':sorted(uncovered),'ao':'Existing geometry/depth AO; no cavity inferred from colour or packed grain alpha.','colour':'Existing authored replacements and cutout alpha retained byte-for-byte by this material-family bake.'}
(OUT/'candidate/family-materials.json').write_text(json.dumps(report,indent=2))
print(f'Updated {len(maps)} shared material maps across {len({r["stem"] for r in changed})} mapped atlases/surfaces. Individual visual approval is separate.')
