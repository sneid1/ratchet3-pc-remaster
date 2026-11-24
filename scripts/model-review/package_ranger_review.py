"""Package the inspected Ranger revision and its unchanged native bindings."""
import csv
import hashlib
import json
import shutil
import struct
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart'
DEST=OUT/'deliverables/ranger-v4'
DEST.mkdir(parents=True,exist_ok=True)
STEM='efa796ee9a43f16c-23492c0e19b3712f-00001dd3'
copies={
    PACK/'replacements'/(STEM+'.png'):'colour.png',
    PACK/'materials'/('pbr_'+STEM+'_n.png'):'normal.png',
    PACK/'materials'/('pbr_'+STEM+'_orm.png'):'orm.png',
    OUT/'blender/edited/ranger.r3edit':'ranger.r3edit',
    OUT/'authored/ranger-albedo-v4.json':'colour-provenance.json',
}
for source,name in copies.items():shutil.copy2(source,DEST/name)
for view in ('close','rear','side'):
    shutil.copy2(OUT/f'candidate-ranger-completion-after-{view}.png',DEST/f'{view}.png')
recipes=json.loads((ROOT/'scripts/model-review/material-recipes.json').read_text())
target=next(t for t in recipes['targets'] if t['model']=='ranger')
target['state']='Colour/material revision implemented and inspected at matched close, rear and side sandbox views; no user approval inferred'
meta_path=OUT/'authored/ranger-albedo-v4.json'
meta=json.loads(meta_path.read_text());meta['status']=target['state'];meta_path.write_text(json.dumps(meta,indent=2)+'\n')
shutil.copy2(meta_path,DEST/'colour-provenance.json')
(ROOT/'scripts/model-review/material-recipes.json').write_text(json.dumps(recipes,indent=2)+'\n')
(DEST/'material-recipe.json').write_text(json.dumps(dict(target=target,finishes={k:v for k,v in recipes['finishes'].items() if k in ['enamel','powder_coat','brushed_steel','cast_alloy','rubber']}),indent=2)+'\n')
font=lambda n:ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf',n)
def comparison(sources,labels,name):
    # Identical crop for each matched camera. Original renders stay unchanged.
    panels=[]
    for source in sources:
        image=Image.open(source).convert('RGB').crop((350,260,1200,900))
        image=image.resize((850,640),Image.Resampling.LANCZOS)
        panels.append(image)
    result=Image.new('RGB',(850*len(panels),715),'#111517')
    draw=ImageDraw.Draw(result)
    for i,(p,label) in enumerate(zip(panels,labels)):
        draw.text((i*850+22,16),label,fill='#e5eddf',font=font(27))
        result.paste(p,(i*850,65))
    result.save(DEST/name)
comparison([OUT/'candidate-ranger-completion-before-close.png',DEST/'close.png'],
           ['Previous revision / 2,167 triangles','New revision / 2,167 triangles'],'previous-v4.png')
comparison([OUT/'before-ranger-v4-source-close.png',DEST/'close.png'],
           ['Starting pack / source mesh','New revision / 2,167 triangles'],'starting-pack-v4.png')
original_alpha=Image.open(OUT/'authored/pre-ranger-v4'/(STEM+'.png')).getchannel('A').tobytes()
new_alpha=Image.open(DEST/'colour.png').getchannel('A').tobytes()
mesh=(DEST/'ranger.r3edit').read_bytes()
triangle_count=struct.unpack_from('<I',mesh,16)[0]//3
assert triangle_count==2167
assert original_alpha==new_alpha
rows=list(csv.DictReader((PACK/'pbr.tsv').read_text().splitlines(),delimiter='\t'))
assert len(rows)==10 and len({r['stem'] for r in rows})==10
report=dict(scope='Ranger colour/material revision in the native sandbox; no game port or full-game completion claim',
    triangles=triangle_count,geometry_changed=False,source_alpha_exact=True,pbr_sets_preserved=len(rows),
    changed=['Refined amber optic/diffuser artwork and clean enamel/alloy base colour',
             'Actual per-model finish overrides, previously bypassed by named finishes',
             'Finer normal scale and restrained roughness variation on paint and metals',
             'Separate protective trim coating and smaller fastener relief',
             'Synced Blender file, embedded-texture delivery, unchanged native mesh bindings'],
    inspection=['Matched close/front, rear and side with captured Veldin lighting',
                'Starting texture pack/source mesh compared at the same close camera',
                'Cleaner material response and detailed optics; original low-count silhouette retained'],
    limits=['Original source optics remain emissive/unlit; no glass refraction added',
            'This does not complete the remaining characters, enemies, NPCs or environment assets',
            'Production integration and complete animation approval are still outstanding'],
    files={p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in DEST.iterdir() if p.is_file()})
(DEST/'report.json').write_text(json.dumps(report,indent=2)+'\n')
(DEST/'README.md').write_text('''# Galactic Ranger — v4 material delivery

This is the actual inspected low-count Ranger revision, not a generated model illustration.

- `ranger.blend`: editable captured-pose model with embedded colour, normal and ORM images, and captured directional lights.
- `ranger.r3edit`: unchanged 2,167-triangle native animation bindings for the sandbox.
- `colour.png`: 1,024-square sRGB base colour, retaining the source game's alpha convention (128 = opaque).
- `normal.png`: linear tangent-space normal map for the existing UV atlas.
- `orm.png`: linear R = AO, G = roughness, B = metalness.
- `close.png`, `rear.png`, `side.png`: native captures using actual captured Veldin lighting.
- `previous-v4.png`: the preceding revision and this revision at the same camera and triangle count.
- `starting-pack-v4.png`: the starting pack/source mesh against this revision; it is not a claim of an untouched PS2-renderer comparison.
- `material-recipe.json`, `colour-provenance.json`, `report.json`: authored inputs, imagegen prompt and checks.

The candidate is already updated. In http://127.0.0.1:8768 select Galactic Ranger, Candidate, Blender edit, then Reload textures.

Native rendering is authoritative for per-vertex light blending, AO, reflection probe and emissive overlays. This package is not deployed into the playable game. The full all-moby/environment request remains incomplete.
''',encoding='utf-8')
manifest_path=OUT/'candidate/manifest.json'
manifest=json.loads(manifest_path.read_text())
manifest['pbr_recipes']={'path':str(ROOT/'scripts/model-review/material-recipes.json'),'sha256':hashlib.sha256((ROOT/'scripts/model-review/material-recipes.json').read_bytes()).hexdigest(),'recipes':recipes}
manifest['ranger_v4']='deliverables/ranger-v4/report.json'
manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
print(json.dumps({k:report[k] for k in ['triangles','geometry_changed','source_alpha_exact','pbr_sets_preserved']}))
