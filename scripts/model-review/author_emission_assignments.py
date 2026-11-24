"""Explicit source-component light decisions; no palette-wide emission rule."""
import json,struct,hashlib
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
assets_path=Path(__file__).with_name('review-assets.json')
assets=json.loads(assets_path.read_text())
def write_policy(model,mask,description):
    raw=(OUT/'blender/edited'/(model+'.r3edit')).read_bytes()
    source_count=struct.unpack_from('<I',raw,8)[0]
    assert len(mask)*3==source_count
    # OBJ groups are emitted in draw/material order, whereas bindings index
    # the exported vertex stream. Multi-material Mecha uses different orders.
    faces=[[int(v.split('/')[0])-1 for v in line.split()[1:4]] for line in (OUT/f'candidate-blender-{model}.obj').read_text().splitlines() if line.startswith('f ')]
    ordered=[0]*len(mask)
    seen=set()
    for face,mode in zip(faces,mask):
        index=min(face)//3
        assert {v//3 for v in face}=={index} and index not in seen
        ordered[index]=mode;seen.add(index)
    assert len(seen)==len(mask)
    mask=ordered
    p=OUT/'blender/edited'/(model+'.emission.bin')
    p.write_bytes(b'R3EMIT01'+struct.pack('<I',source_count)+bytes(mask))
    p.with_suffix('.json').write_text(json.dumps(dict(description=description,source_vertices=source_count,
        face_policy={str(v):[i for i,m in enumerate(mask) if m==v] for v in sorted(set(mask))},
        source_obj_sha256=hashlib.sha256((OUT/f'candidate-blender-{model}.obj').read_bytes()).hexdigest()),indent=2)+'\n')
    assets[model]['emission_policy']=p.relative_to(OUT).as_posix()
    print(model,{v:mask.count(v) for v in set(mask)})

components=json.loads((OUT/'ranger-emission-components.json').read_text())
# Inspected original antenna cap, two crossbars and paired mast strips. The
# shared orange swatch remains available to the cannon/indicator regions.
antenna_components={650,1211,1219,1227,1193}
blocked=[i for c in components if c['first'] in antenna_components for i in c['triangles']]
assert len(blocked)==36
ranger=[1]*1251
for i in blocked:ranger[i]=0
write_policy('ranger',ranger,'Ranger antenna explicitly non-emissive, as directed by the user; shared-UV lights elsewhere retain their authored mask.')

# These original faces are the two hand-cannon energy cells and muzzle faces.
# The visible front lenses are the four-face fans 248..251 and 417..420.
# Native triangle-ID inspection identified these; the constant-UV overlays
# alone sit behind the outer front surfaces in this captured pose.
# Feet (1964..1969,1986..1991), head and all other armour are explicitly off.
mecha=[0]*2032
for i in list(range(248,252))+list(range(417,421))+list(range(1970,1986))+list(range(1992,2022)):mecha[i]=2
write_policy('tyhrranoid',mecha,'Only the hand cannons emit. No Mecha eyes, antenna, feet, body or other armour emission.')
assets_path.write_text(json.dumps(assets,indent=2)+'\n')
recipe_path=Path(__file__).with_name('material-recipes.json');recipes=json.loads(recipe_path.read_text())
clank=next(t for t in recipes['targets'] if t['model']=='clank')
clank['emissive_regions']=[
    dict(label='Clank green eyes: original shared eye island',shape='polygon',points=[[0,211],[8,207],[24,207],[31,213],[35,228],[28,253],[5,255],[0,247]],bevel=.3,strength=1.8,glow=.65,tint=[.65,1,.2]),
    dict(label='Clank red antenna indicator',shape='ellipse',bounds=[154,33,167,47],bevel=.2,strength=2.1,glow=.65)]
recipe_path.write_text(json.dumps(recipes,indent=2)+'\n')
