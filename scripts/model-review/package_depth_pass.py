"""Package the inspected material revision, with matched native comparisons."""
import csv,hashlib,json,shutil,struct
from pathlib import Path
from PIL import Image,ImageDraw,ImageFont
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart';DEST=OUT/'deliverables/material-depth'
DEST.mkdir(parents=True,exist_ok=True)
models=['ranger','ratchet','clank','tyhrranoid']
recipes=json.loads(Path(__file__).with_name('material-recipes.json').read_text())
targets=[t for t in recipes['targets'] if t['model'] in models]
stems={t['stem'] for t in targets}
rows=[r for r in csv.DictReader((PACK/'pbr.tsv').read_text().splitlines(),delimiter='\t') if r['stem'] in stems]
geometries={}
for model in models:
    mesh=OUT/'blender/edited'/(model+'.r3edit');metadata=json.loads(mesh.with_suffix('.json').read_text())
    digest=hashlib.sha256(mesh.read_bytes()).hexdigest()
    assert digest==metadata['sha256'],'Material pass unexpectedly changed geometry: '+model
    assert (DEST/(model+'.blend')).is_file(),'Missing embedded Blender delivery'
    geometries[model]=dict(triangles=struct.unpack_from('<I',mesh.read_bytes(),16)[0]//3,sha256=digest,geometry_unchanged=model!='ratchet')
    shutil.copy2(mesh,DEST/mesh.name)
    for p in (OUT/'blender/edited').glob(model+'.emission.*'):shutil.copy2(p,DEST/p.name)
for row in rows:
    folder=DEST/'textures';folder.mkdir(exist_ok=True)
    for key in ['normal_map','orm_map','emissive_map']:
        if row.get(key):shutil.copy2(PACK/'materials'/(row[key]+'.png'),folder/(row[key]+'.png'))
    colour=PACK/'replacements'/(row['stem']+'.png')
    if colour.exists():shutil.copy2(colour,folder/colour.name)
(DEST/'material-recipes.json').write_text(json.dumps(dict(targets=targets,finishes=recipes['finishes'],channel_contract=recipes['channel_contract']),indent=2)+'\n')
for name in ['ratchet-body-depth','ratchet-region-repair','ranger-depth-colour']:
    shutil.copy2(OUT/'authored'/(name+'.json'),DEST/(name+'.json'))
captures={
    'ranger-close':'candidate-depth-after-ranger-close.png',
    'ranger-side':'candidate-depth-final-ranger-side.png',
    'ranger-rear':'candidate-depth-final-ranger-rear.png',
    'ratchet-angle-a':'candidate-reduction-after-front.png',
    'ratchet-angle-b':'candidate-reduction-after-pose.png',
    'clank':'candidate-emission-fixed-clank.png',
    'mecha':'candidate-emission-fixed-mecha.png',
    'ranger-emission':'candidate-depth-after-ranger-mask.png',
    'clank-emission':'candidate-emission-fixed-clank-mask.png',
    'mecha-emission':'candidate-emission-fixed-mecha-mask.png'}
for label,src in captures.items():shutil.copy2(OUT/src,DEST/(label+'.png'))
font=ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf',24)
def comparison(before,after,box,name,labels=('Before this depth pass','Current materials')):
    # Identical crop only: no relighting, retouching, selective sharpening or
    # regenerated screenshots. Labels describe this material pass, not PS2.
    panels=[Image.open(p).convert('RGB').crop(box) for p in [before,after]]
    w,h=panels[0].size;im=Image.new('RGB',(w*2,h+48),'#111517');d=ImageDraw.Draw(im)
    for i,(p,label) in enumerate(zip(panels,labels)):
        im.paste(p,(i*w,48));d.text((i*w+14,10),label,font=font,fill='#e8eee4')
    im.save(DEST/name)
comparison(OUT/'candidate-emission-fixed-ranger.png',DEST/'ranger-close.png',(410,285,1190,900),'ranger-comparison.png')
comparison(OUT/'candidate-reduction-before-front.png',DEST/'ratchet-angle-a.png',(590,30,1100,895),'ratchet-comparison.png',('154,368 triangles','16,884 triangles'))
shutil.copy2(OUT/'ratchet-reduction.json',DEST/'ratchet-reduction.json')
report=dict(scope='Emission plus fitted material depth revision; isolated sandbox only',geometry=geometries,
    pbr_targets=len(targets),whole_candidate_pbr_sets=len(list(csv.DictReader((PACK/'pbr.tsv').read_text().splitlines(),delimiter='\t'))),
    emission=dict(ranger='Authored amber optic/panels; all 36 antenna triangles explicitly excluded',
                  clank='Green eye island and red antenna',mecha='Only hand-cannon fronts/cells: 54 source triangles; remaining 1,978 source triangles excluded'),
    material_changes=dict(ranger='Stronger fitted bevels/vents/recess AO, edge roughness/wear and controlled colour grime',
        ratchet='Recovered erased leather/metal/fur weights; new main-atlas normal/ORM; fitted glove folds, harness/boot relief, glove-only authored colour; original fur tiling retained'),
    preserved=['Source bindings','Ratchet accepted eye records (byte-exact)','Ratchet colour outside the glove mask','Source colour alpha','Captured Veldin lights'],
    inspected=['Ranger close, side and rear','Ratchet two matched angles','Clank/Mecha lit and emission-only views'],
    assessment='Ratchet has the clearer change: glove folds/grain, metallic harness and functioning fur. Ranger has deeper construction edges/vents and restrained wear, but broad front panels still depend on the original simple shape.',
    limits=['This is not a finished all-moby/environment pass','Ratchet reduction is 16,884 triangles; full animation approval remains pending',
            'Mecha remains the captured distant mesh','Native renderer is authoritative; Blender copies do not reproduce native fur shells, per-vertex light blending, AO postpass or glow exactly',
            'Veldin reflection capture is LDR; no surrounding shadow casters in this blackroom','No game integration or full animation approval'],
    no_game_review=True,no_timing_runs=True)
(DEST/'README.md').write_text('''# Current material-depth revision

The four `.blend` files are editable captured-pose models with embedded material
textures and captured directional lights. The `.r3edit` files retain the existing
native animation bindings. Ratchet was reduced from 154,368 to 16,884 triangles;
its accepted eye-layer native records remain byte-identical. Other geometry is
unchanged. The Ratchet comparison now shows matched views before/after reduction.

`textures/` contains colour, linear normal and ORM, and explicit emission maps.
Normal alpha stores the original fur mask; ORM alpha stores the lens mask.
The source-face `.emission.bin` policies prevent shared UV swatches from lighting
the Ranger antenna or Mecha feet/body. Use these with the current full sandbox
candidate; these bindings alone are not a deployable game mod.

The PNGs are actual native sandbox captures under captured Veldin lighting.
Comparisons use identical crops of matched cameras. No screenshot was retouched.
`report.json` distinguishes the implemented revision from the unfinished wider
asset request. Colour provenance JSON records the built-in image-generation
prompts and constrained compilation. Native PBR maps use explicit UV features
and kit finish data, never colour brightness as height or AO.

The gallery is http://127.0.0.1:8771/ and live sandbox http://127.0.0.1:8768/.
This directory supersedes `ranger-v4` for the current materials. No game review,
timing run, game deployment or claim of full-game art completion is included.
''',encoding='utf-8')
report['files']={p.relative_to(DEST).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in DEST.rglob('*') if p.is_file() and p.name!='report.json'}
(DEST/'report.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:report[k] for k in ['geometry','whole_candidate_pbr_sets','inspected']}))
