"""Assemble existing, unretouched native captures into a local progress gallery."""
from pathlib import Path
import html
import json
import math
import shutil
from PIL import Image, ImageDraw, ImageFont, ImageChops

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'captures/rift-texture-review'
OUT = SOURCE / 'progress-gallery'
OUT.mkdir(exist_ok=True)

# These are implemented passes, not a list of final-approved assets.
ITEMS = [
    ('ranger', 'Galactic Ranger — material depth', 'Characters', 'candidate-depth-after-ranger-close.png',
     '2,167 triangles', 'Fitted bevel/vent relief, cavity AO, edge wear, finish variety and explicit amber lights. Antenna excluded from emission. Embedded Blender delivery saved.',
     'Original low-count silhouette retained. The game port and full animation approval remain outstanding.'),
    ('ranger-side', 'Ranger — second angle', 'Characters', 'candidate-depth-final-ranger-side.png',
     'Same 2,167-triangle edit', 'Second view of the current low-count Ranger.',
     'This is the same asset, not an additional completed model.'),
    ('ratchet', 'Ratchet & attached Clank', 'Characters', 'candidate-reduction-after-front.png',
     '16,884 triangles — reduced from 154,368', 'Selective original-surface refinement with current material depth. Accepted eye-layer records preserved byte-for-byte. Front and recorded-pose views inspected.',
     'Full animation approval and the wider character pass remain unfinished.'),
    ('clank', 'Clank — backpack model', 'Characters', 'candidate-emission-fixed-clank.png',
     '3,598 triangles', 'Explicit green eye and red antenna emission; fitted mechanical relief and differentiated alloy response.',
     'Backpack variant only. Original low-count silhouette and colour detail remain.'),
    ('small-ty', 'Small Tyhrranoid', 'Characters', 'finish-v2-others/small_ty_near.png',
     '2,105 triangles', 'Verified original near mesh, selective refinement and finer skin/roughness response.',
     'Original spotted colour design retained; this is a surface pass, not a new sculpt.'),
    ('mecha-ty', 'Mecha-Tyhrranoid', 'Characters', 'candidate-emission-fixed-mecha.png',
     '3,548 triangles', 'Only the hand cannons emit. Feet, body, eyes and antenna explicitly excluded. Existing mechanical materials and animated UV strips retained.',
     'Still based on the captured distant mesh. Near-mesh joint inputs remain unresolved.'),
    ('wasp', 'Florana wasp', 'Characters', 'finish-v1-others/florana_creature.png',
     '800 triangles', 'Chitin finish, fitted relief and inspected Rift Apart normal-detail reuse.',
     'Original wing resolution and low-count silhouette remain limiting.'),
    ('qwark', 'Captain Qwark — jungle outfit', 'Characters', 'finish-v2-others/phoenix_crew.png',
     '6,446 triangles', 'Separate skin, leather and cloth response; selective geometry refinement.',
     'Original colour texture remains soft. Not a fully repainted NPC.'),
    ('skidd-imagegen', 'Skidd — detailed texture layers', 'Characters', 'candidate-layers-reviewed-candidate-source_c7012-45.png',
     '3,930 original triangles / 2 atlas sets', 'Authored colour and height, fine cloth/leather detail, physical material separation and protected HD emblem. Thirteen separate layer types.',
     'Bind-space source model; rig and animation approval remain outstanding.'),
    ('armoured-ty-imagegen', 'Armoured Tyhrranoid — detailed texture layers', 'Characters', 'candidate-layers-reviewed-candidate-source_c7074-45.png',
     '1,760 original triangles / 1 atlas set', 'Fitted armour/vent detail, differentiated alloy and coating, separate normal/roughness/AO. Generated skin-like paint marks rejected.',
     'Bind-space source model; lamps are not enabled without a part-specific emission assignment.'),
    ('qwark-imagegen', 'Qwark — hero uniform', 'Characters', 'candidate-layers-reviewed-candidate-source_c7000-45.png',
     '3,190 original triangles / 2 atlas sets', 'Fine suit weave, differentiated boots and hardware, sharp original HD emblem. Separate local AO and height detail.',
     'Original bind-space proportions and topology retained; no rig/animation approval.'),
    ('sasha-imagegen', 'Sasha — detailed texture layers', 'Characters', 'candidate-layers-reviewed-candidate-source_c7013-45.png',
     '5,808 original triangles / 2 atlas sets', 'Tailored cloth, leather, short facial fur and directional hair detail. Incorrect skin finish on uniform removed.',
     'Original bind-space model. Mirrored UVs limit geometry-baked AO.'),
    ('al-imagegen', 'Big Al — detailed texture layers', 'Characters', 'candidate-layers-reviewed-candidate-source_c6501-45.png',
     '3,568 original triangles / 2 atlas sets', 'Cotton coat seams and pocket relief, skin/hair detail and separate material channels. Source normal decoder fixed; bright triangle fault removed.',
     'Constant-palette shirt UVs use a neutral normal; original silhouette and rig remain.'),
    ('blue-bloom', 'Veldin blue bloom', 'Environment & props', 'family-expansion/veldin-bloom-1206.png',
     'Colour + normal/ORM pass', 'New 1,024-square colour artwork, fitted veins, cuticle roughness and original cutout coverage.',
     'Thin original leaves, soft edges and low-sided flower centre remain.'),
    ('red-bloom', 'Veldin red bloom', 'Environment & props', 'family-expansion/veldin-bloom-30.png',
     'Colour + normal/ORM pass', 'New petal detail and roughness; two-sided normal/ambient correction removes triangular shading patches.',
     'Central flower sprite and original flat petal construction still need work.'),
    ('rail', 'Veldin rail assembly', 'Environment & props', 'candidate-gallery-rail.png',
     'Shared material pass — partial', 'Isolated source kit with calibrated coating and metal response.',
     'Change is subtle. Original geometry and colour resolution remain; not a prop remake.'),
    ('cliff-kit', 'Veldin cliff kit', 'Environment & props', 'candidate-gallery-cliff-kit.png',
     'Material assignment correction — partial', 'Removed incorrect wood classification and applied subdued stone detail.',
     'Still faceted and soft at close range. No completed environment remodel.'),
    ('ground-ring', 'Veldin ground ring prop', 'Environment & props', 'candidate-gallery-ground-ring.png',
     'Shared material pass — partial', 'Original design rendered through the current material/lighting path.',
     'No claim of a major visible improvement; individual prop work is still needed.'),
    ('terrain', 'Veldin eroded terrain chunk', 'Environment & props', 'candidate-gallery-terrain.png',
     'Terrain material pass — partial', 'Original chunk, vertex normals and captured level lighting with corrected rock assignments.',
     'An isolated terrain fragment, not a completed level. Original cuts and low-resolution colour remain.'),
    ('vendor', 'Phoenix Gadgetron vendor', 'Environment & props', 'candidate-gallery-vendor.png',
     'Shared material pass — partial', 'Separated material response on an original captured moby prop.',
     'Panel artwork, transparency and silhouette still require individual work.'),
    ('weapon', 'Phoenix weapon display', 'Environment & props', 'candidate-family-reviewed-phoenix_mechanism.png',
     'Shared material pass — partial', 'Distinct paint, exposed metal and rubber response.',
     'Original texture blur and faceted shape remain visible.'),
]

def font(size, bold=False):
    return ImageFont.truetype('C:/Windows/Fonts/' + ('segoeuib.ttf' if bold else 'segoeui.ttf'), size)

def fit_subject(im, target):
    # Only remove empty black margins. No relighting, sharpening or repainting.
    rgb = im.convert('RGB')
    mask = rgb.convert('L').point(lambda v: 255 if v > 10 else 0)
    box = mask.getbbox()
    if box:
        x0, y0, x1, y1 = box
        pad = 22
        rgb = rgb.crop((max(0, x0-pad), max(0, y0-pad), min(rgb.width, x1+pad), min(rgb.height, y1+pad)))
    rgb.thumbnail(target, Image.Resampling.LANCZOS)
    canvas = Image.new('RGB', target, 'black')
    canvas.paste(rgb, ((target[0]-rgb.width)//2, (target[1]-rgb.height)//2))
    return canvas

def sheet(category, filename):
    rows = [r for r in ITEMS if r[2] == category]
    width, cell_h, top = 1600, 485, 110
    result = Image.new('RGB', (width, top + math.ceil(len(rows)/2)*cell_h), '#111517')
    d = ImageDraw.Draw(result)
    d.text((28, 17), category + ' — work implemented so far', font=font(34, True), fill='#eff4ed')
    d.text((28, 65), 'Native sandbox captures / captured level lighting / partial work is labelled', font=font(22), fill='#b7c1c7')
    for i, r in enumerate(rows):
        key, title, _, src, status, _, _ = r
        x, y = (i%2)*800, top + (i//2)*cell_h
        im = Image.open(SOURCE/src)
        result.paste(fit_subject(im, (780, 390)), (x+10, y))
        d.text((x+20, y+398), title, font=font(26, True), fill='#eff4ed')
        d.text((x+20, y+435), status, font=font(21), fill='#b5d0a9' if 'partial' not in status else '#e8c28a')
    result.save(OUT/filename, quality=94)

cards = []
manifest = []
for key, title, category, src, status, done, remaining in ITEMS:
    path = SOURCE/src
    if not path.is_file():
        raise FileNotFoundError(path)
    full = key + '.png'
    thumb = key + '-thumb.jpg'
    shutil.copy2(path, OUT/full)
    fit_subject(Image.open(path), (900, 650)).save(OUT/thumb, quality=94)
    capture_note = ('Source normal decode revision 2. Original bind-space near mesh; saved level directional sets with nearest-terrain blend/ambient. This is not actor-specific captured lighting or animation approval.' if key.endswith('-imagegen') else
                    'Current material/emission revision inspected in the native sandbox with captured Veldin lighting. See material-depth delivery and matched comparisons.' if key in ['ranger','ranger-side','ratchet','clank','mecha-ty'] else
                    'Latest inspected character capture, made before the final shared-family bake.'
                    if category == 'Characters' else
                    'Current isolated sandbox capture; source geometry retained.')
    row = dict(id=key, title=title, category=category, source=str(path), status=status,
               implemented=done, remaining=remaining, capture_note=capture_note)
    manifest.append(row)
    esc = html.escape
    cards.append(f'''<article data-group="{esc(category)}" id="{key}">
      <button class="picture" data-full="{full}" data-title="{esc(title)}" aria-label="Enlarge {esc(title)}"><img src="{thumb}" alt="{esc(title)} — native sandbox render" loading="lazy"></button>
      <div class="copy"><div class="status">{esc(status)}</div><h2>{esc(title)}</h2>
      <p>{esc(done)}</p><p class="remaining"><b>Still limited by:</b> {esc(remaining)}</p>
      <details><summary>Capture provenance</summary><p>{esc(capture_note)}</p><code>{esc(src)}</code><p><a href="{full}">Open original PNG</a></p></details></div></article>''')

for source, dest in [('family-expansion/foliage-comparison.png', 'foliage-comparison.png'),
                     ('deliverables/material-depth/ranger-comparison.png', 'ranger-material-comparison.png'),
                     ('deliverables/material-depth/ratchet-comparison.png', 'ratchet-material-comparison.png'),
                     ('deliverables/material-depth/report.json', 'material-depth-report.json'),
                     ('family-expansion/report.json', 'family-report.json'),
                     ('adaptive-model-pass.json', 'geometry-report.json'),
                     ('finish-report.json', 'character-material-report.json')]:
    shutil.copy2(SOURCE/source, OUT/dest)
shutil.copy2(ROOT/'docs/RIFT-TEXTURE-REVIEW.md', OUT/'full-work-log.md')
(OUT/'manifest.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')

page = '''<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Ratchet 3 — current asset work</title><style>
*{box-sizing:border-box}body{margin:0;background:#101315;color:#eef1ec;font:16px/1.55 system-ui,sans-serif}main{max-width:1500px;margin:auto;padding:40px 28px 80px}a{color:#b8dda9}h1{font-size:clamp(30px,4vw,52px);line-height:1.1;margin:14px 0 22px;letter-spacing:-.03em}h2{font-size:22px;margin:6px 0 12px}.eyebrow,.status{font-size:12px;text-transform:uppercase;letter-spacing:.09em;color:#b8dda9}.intro{max-width:900px;color:#c4cccd}.notice{padding:18px 22px;border-left:3px solid #ddba7c;background:#24231e;margin:26px 0}.toolbar{position:sticky;top:0;z-index:2;background:#101315f5;display:flex;gap:8px;flex-wrap:wrap;padding:15px 0;border-bottom:1px solid #384046;margin-bottom:24px}button{font:inherit;cursor:pointer}.filter{background:#20272b;color:#d8e0df;border:1px solid #4b565b;border-radius:4px;padding:9px 16px}.filter[aria-pressed=true]{background:#c2d7b4;color:#162213;border-color:#c2d7b4}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,380px),1fr));gap:22px}article{border:1px solid #354046;background:#171c20;border-radius:5px;overflow:hidden}article[hidden]{display:none}.picture{display:block;width:100%;border:0;background:#000;padding:0}.picture img{width:100%;aspect-ratio:900/650;object-fit:contain;display:block}.copy{padding:20px}.copy p{font-size:14px;color:#c3cccd}.copy .remaining{color:#ddc19a}summary{cursor:pointer;font-size:13px;color:#aab7be}code{font-size:12px;overflow-wrap:anywhere}section{margin-top:50px}.wide{width:100%;height:auto;border:1px solid #354046}.comparison{display:grid;grid-template-columns:1fr 1fr;gap:24px}table{width:100%;border-collapse:collapse;font-size:14px}td,th{text-align:left;vertical-align:top;padding:15px 12px;border-bottom:1px solid #384046}th{color:#b8dda9}td:first-child{width:26%}dialog{width:96vw;max-width:1700px;max-height:96vh;background:#101315;color:white;border:1px solid #536065;padding:12px}dialog::backdrop{background:#000c}dialog img{display:block;max-width:100%;max-height:80vh;margin:auto;object-fit:contain}.dialoghead{display:flex;justify-content:space-between;align-items:center;padding:4px 8px 10px}.dialoghead button{background:#d4dfce;border:0;padding:8px 15px}footer{color:#a6b0b5;font-size:13px;margin-top:35px}@media(max-width:700px){main{padding:24px 16px}.comparison{grid-template-columns:1fr}table{font-size:12px}td,th{padding:10px 6px}}
</style><main><div class="eyebrow">Local sandbox / 16 November 2026 / progress review</div>
<h1>Everything implemented so far.</h1>
<p class="intro">Seven captured character/enemy edits, five additional source-model texture passes, two foliage passes and six prop/terrain previews. Click any image for the original full-size native capture. The second Ranger view is the same asset.</p>
<div class="notice"><b>The full asset pass is incomplete.</b> These are implemented changes with visual evidence. Original silhouettes, some colour detail, rigs and flower centres still limit quality. Shared atlas coverage is not individual model completion.</div>
<p class="intro">Lighting uses saved level data. The five imagegen additions are bind-space source models with corrected decoded normals and nearest-terrain light blends. The earlier seven characters use captured poses. Veldin has a captured LDR reflection probe; blackroom captures omit surrounding world shadowcasters. Shrub normals are reconstructed.</p>
<p><a href="http://127.0.0.1:8768/">Open interactive sandbox</a> &nbsp;·&nbsp; <a href="http://127.0.0.1:8768/assets">Inspected source library</a> &nbsp;·&nbsp; <a href="full-work-log.md">Full work log</a></p>
<nav class="toolbar" aria-label="Filter assets"><button class="filter" data-filter="All" aria-pressed="true">All 20 assets / variants</button><button class="filter" data-filter="Characters" aria-pressed="false">12 character / enemy variants</button><button class="filter" data-filter="Environment &amp; props" aria-pressed="false">8 foliage / props / terrain</button><a class="filter" href="#broader">Shared work &amp; remaining scope</a></nav>
<div class="grid">''' + '\n'.join(cards) + '''</div>
<section><h2>Matched comparisons</h2><p>Earlier pass on the left; current inspected pass on the right. These compare iterations, not an untouched game against a completed remaster.</p><div class="comparison">
<div><h3>Foliage</h3><a href="foliage-comparison.png"><img class="wide" src="foliage-comparison.png" alt="Earlier and current blue and red blooms"></a></div>
<div><h3>Ranger finish depth</h3><a href="ranger-material-comparison.png"><img class="wide" src="ranger-material-comparison.png" alt="Ranger previous and current finish at the same camera and triangle count"></a></div>
<div><h3>Ratchet leather and harness</h3><a href="ratchet-material-comparison.png"><img class="wide" src="ratchet-material-comparison.png" alt="Ratchet before and after the mask repair and fitted material depth pass"></a></div></div></section>
<section id="broader"><h2>Work beyond the pictured assets</h2><table><thead><tr><th>Area</th><th>Implemented</th><th>Limit</th></tr></thead><tbody>
<tr><td>Material atlases</td><td>Nine earlier character sets with 178 fitted UV features; two foliage sets; nine new detailed layer sets for five source models. The candidate has 485 PBR registrations including the shared material pass.</td><td>Atlas registrations and source previews are not finished-model counts. The full all-moby art pass remains unfinished.</td></tr>
<tr><td>Shared material families</td><td>107 packed material maps; 38 calibrated finish definitions. Latest batch changed 77 existing map files. Paint, metals, rubber, skin, cloth, leather, rock, soil, bark and foliage have differentiated responses.</td><td>4,148 assigned slots / 3,400 source texture stems describe reach, not individual visual completion.</td></tr>
<tr><td>Coverage</td><td>324 verified atlas variants inherit compatible assignments; 241 additional moby prop surfaces use existing coarse classifications. Source inventory and remaining-work list created.</td><td>1,056 unique atlases remain unmapped. Runtime, cutscene, multiplayer and uncaptured rig gaps persist. Counts overlap and cannot be added.</td></tr>
<tr><td>Geometry / Blender</td><td>Selective refinement replaces excessive subdivision. Ratchet is now 16,884 triangles, down from 154,368, with accepted eye records exact. Five additional source models retain original topology and have editable Blender materials.</td><td>These are surface refinements and texture work, not complete new sculpts. New source-model Blender copies are bind-space assets without animation approval.</td></tr>
<tr><td>Sandbox renderer</td><td>Physical normal/light fixes, sRGB colour, linear normal/ORM, GGX lighting, geometry/depth AO, outward PBR fur, Veldin reflection probe, two-sided shrub shading and corrected ambient reconstruction.</td><td>Other levels lack the reflection probe. No production game integration or full animation approval.</td></tr>
<tr><td>Original / Rift Apart assets</td><td>Source library, UYA extraction, shrub isolation, Blender roundtrip; Rift Apart Ratchet, Clank and Ranger hero armour/helmet inspected/imported; wasp normal detail reused.</td><td>No complete Rift Apart model swap or rig retarget. Source inspection is not a finished replacement.</td></tr>
</tbody></table><p><a href="material-depth-report.json">Current material-depth report</a> · <a href="family-report.json">Earlier family report</a> · <a href="geometry-report.json">Geometry counts</a> · <a href="manifest.json">Image provenance</a></p></section>
<footer>All images are native sandbox output. Presentation thumbnails only crop empty black margins and resize; original PNGs are unchanged. No game launch, in-game review, world review or timing run was used for this gallery.</footer></main>
<dialog><div class="dialoghead"><span id="imageTitle"></span><button id="close">Close</button></div><img id="large" alt=""></dialog>
<script>const dialog=document.querySelector('dialog');document.querySelectorAll('.picture').forEach(b=>b.onclick=()=>{document.querySelector('#large').src=b.dataset.full;document.querySelector('#large').alt=b.dataset.title;document.querySelector('#imageTitle').textContent=b.dataset.title;dialog.showModal()});document.querySelector('#close').onclick=()=>dialog.close();dialog.onclick=e=>{if(e.target===dialog)dialog.close()};document.querySelectorAll('[data-filter]').forEach(b=>b.onclick=()=>{document.querySelectorAll('[data-filter]').forEach(x=>x.setAttribute('aria-pressed',String(x===b)));document.querySelectorAll('article').forEach(a=>a.hidden=b.dataset.filter!=='All'&&a.dataset.group!==b.dataset.filter)});</script></html>'''
(OUT/'index.html').write_text(page, encoding='utf-8')
sheet('Characters', 'characters.jpg')
sheet('Environment & props', 'environment-props.jpg')
print(json.dumps({'gallery':str(OUT/'index.html'), 'asset_count':20, 'images':len(ITEMS), 'all_source_images_present':True}))
