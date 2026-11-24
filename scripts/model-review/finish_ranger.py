"""Compile the Ranger's fitted colour artwork and calibrated material recipe.

Keeps the existing 2,167-triangle export, source opacity and UV bindings intact.
Colour comes from the imagegen-authored atlas; this only restores game packing.
"""
import hashlib
import json
import shutil
from pathlib import Path
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'captures/rift-texture-review'
PACK = OUT / 'candidate/0-riftapart'
STEM = 'efa796ee9a43f16c-23492c0e19b3712f-00001dd3'
ART = OUT / 'authored/ranger-albedo-v4-source.png'
ARCHIVE = OUT / 'authored/pre-ranger-v4'
ARCHIVE.mkdir(exist_ok=True)
RECIPE = Path(__file__).with_name('material-recipes.json')

for p in [RECIPE, PACK/'replacements'/(STEM+'.png'),
          PACK/'materials'/('pbr_'+STEM+'_n.png'),
          PACK/'materials'/('pbr_'+STEM+'_orm.png')]:
    dest = ARCHIVE/p.name
    if not dest.exists():
        shutil.copy2(p, dest)

colour = Image.open(ART).convert('RGB').resize((1024,1024), Image.Resampling.LANCZOS)
alpha = Image.open(ARCHIVE/(STEM+'.png')).getchannel('A')
if alpha.size != colour.size:
    raise ValueError('Unexpected source alpha size')
colour.putalpha(alpha)
dest = OUT/'authored/ranger-albedo-v4.png'
colour.save(dest)
shutil.copy2(dest, PACK/'replacements'/(STEM+'.png'))

recipes = json.loads(RECIPE.read_text())
target = next(t for t in recipes['targets'] if t['model']=='ranger')
# Per-asset scale calibration. The old profile's normal_strength had no effect
# once a named finish was selected; explicit finish_overrides now apply it.
calibration = {
    'enamel':dict(normal_gain=.16, normal_highpass=.005, repeats=6,
                  roughness=.39, variation=.06, roughness_range=[.34,.44]),
    'powder_coat':dict(normal_gain=.24, normal_highpass=.005, repeats=7,
                       roughness=.53, variation=.08, roughness_range=[.46,.60]),
    'brushed_steel':dict(normal_gain=.12, normal_highpass=.006, repeats=4,
                         tool_marks=.008, grooves=280, roughness=.34,
                         variation=.065, roughness_range=[.28,.40]),
    'cast_alloy':dict(normal_gain=.18, normal_highpass=.006, repeats=6,
                      roughness=.45, variation=.07, roughness_range=[.38,.52]),
    'rubber':dict(normal_gain=.22, normal_highpass=.006, repeats=5,
                  roughness=.78, variation=.07, roughness_range=[.73,.84]),
}
for profile in target['profile_overrides'].values():
    finish = profile.get('finish')
    if finish in calibration:
        profile['finish_overrides'] = calibration[finish]

# These masks follow the existing olive panels. They distinguish a protective
# coating from the forest-green enamel without introducing foreign panel art.
coating_panels = [
    dict(label='V4 top protective strip', shape='rect', bounds=[47,3,109,18]),
    dict(label='V4 upper shell protective rim', shape='polygon', points=[
        [130,44],[162,33],[194,26],[195,3],[202,3],[202,24],[220,35],
        [211,45],[219,52],[225,48],[234,51],[221,61],[216,79],
        [204,72],[196,62],[186,57],[172,56],[159,59],[144,65],[133,72],[130,68]]),
    dict(label='V4 left vent cover', shape='polygon', points=[
        [29,97],[78,97],[78,105],[77,112],[76,129],[75,138],[29,138]]),
    dict(label='V4 waist protective band', shape='rect', bounds=[79,147,109,154]),
]
for layer in coating_panels:
    layer.update(finish='powder_coat', finish_overrides=calibration['powder_coat'],
                 metalness=0, bevel=.25)

layers = [l for l in target['surface_layers'] if not l['label'].startswith('V4 ')]
for layer in layers:
    if layer.get('finish') in calibration:
        layer['finish_overrides'] = calibration[layer['finish']]
    # A uniform tall relief ridge had made small fasteners look soft/swollen.
    if layer['label'].startswith('Machined fastener'):
        layer.update(height=.18, bevel=.20)
    if layer['label']=='Fitted green socket insert':
        layer.pop('roughness', None)
        layer.update(finish='enamel', finish_overrides=calibration['enamel'])
target['surface_layers'] = coating_panels + layers
target['state'] = 'Ranger v4 colour and finish correction; native acceptance inspection pending'
RECIPE.write_text(json.dumps(recipes, indent=2)+'\n')
meta = json.loads((OUT/'authored/ranger-albedo-v4.json').read_text())
meta.update(output=str(dest), conversion='1024-square sRGB; original candidate GS alpha retained byte-for-byte',
            installed_sha256=hashlib.sha256(dest.read_bytes()).hexdigest(),
            geometry='Existing edited Ranger, 2167 triangles; no geometry changes',
            material_calibration=calibration, status=target['state'])
(OUT/'authored/ranger-albedo-v4.json').write_text(json.dumps(meta,indent=2)+'\n')
manifest_path = OUT/'candidate/manifest.json'
manifest = json.loads(manifest_path.read_text())
manifest['authored_colour_ranger'] = 'authored/ranger-albedo-v4.json'
manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
print('Ranger v4 colour compiled; source opacity preserved; material recipe updated; mesh unchanged.')
