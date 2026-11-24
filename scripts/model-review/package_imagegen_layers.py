"""Package aligned texture layers, editable models and unretouched comparisons."""
import hashlib,json,shutil
from pathlib import Path
from PIL import Image,ImageDraw,ImageFont
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review';DEST=OUT/'deliverables/imagegen-layers'
MODELS={'source_c7012':('Skidd',3930),'source_c7074':('Armoured Tyhrranoid',1760),'source_c7000':('Qwark',3190),
        'source_c7013':('Sasha',5808),'source_c6501':('Big Al',3568)}
FONT=ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf',24)
review=DEST/'review';review.mkdir(exist_ok=True)

def subject(image):
    rgb=image.convert('RGB');box=rgb.convert('L').point(lambda v:255 if v>7 else 0).getbbox()
    x0,y0,x1,y1=box
    return (max(0,x0-18),max(0,y0-18),min(rgb.width,x1+18),min(rgb.height,y1+18))

records=[]
for model,(name,triangles) in MODELS.items():
    sheet=Image.new('RGB',(1500,1700),'#14191d');draw=ImageDraw.Draw(sheet)
    evidence=[]
    for col,angle in enumerate([45,225]):
        paths=[OUT/f'candidate-layers-reviewed-{pack}-{model}-{angle}.png' for pack in ['before','candidate']]
        ims=[Image.open(p).convert('RGB') for p in paths];boxes=[subject(im) for im in ims]
        # Identical crop and scaling on both versions; no retouch or relighting.
        crop=(min(b[0] for b in boxes),min(b[1] for b in boxes),max(b[2] for b in boxes),max(b[3] for b in boxes))
        for row,(im,path) in enumerate(zip(ims,paths)):
            im=im.crop(crop);im.thumbnail((730,780),Image.Resampling.LANCZOS)
            sheet.paste(im,(col*750+(750-im.width)//2,row*850+50))
            draw.text((col*750+14,row*850+10),f'{name} / {angle} deg / '+('Previous pack' if row==0 else 'Current layers'),font=FONT,fill='white')
            target=review/(model+f'-{angle}-'+('before' if row==0 else 'current')+'.png')
            shutil.copy2(path,target);evidence.append(dict(file=target.name,sha256=hashlib.sha256(target.read_bytes()).hexdigest()))
    sheet.save(review/(model+'-comparison.png'))
    records.append(dict(model=model,name=name,triangles=triangles,geometry='Original source near mesh, unchanged',captures=evidence,
        lighting='Actual level directional sets; nearest-terrain blend/ambient, not captured actor-specific light blend',
        pose='Bind-space source mesh; rig and animation approval remain outstanding',normal_decode_revision=2))
layers=json.loads((DEST/'report.json').read_text())
prompts=DEST/'prompts';prompts.mkdir(exist_ok=True)
for p in (OUT/'authored/roster').glob('*.json'):shutil.copy2(p,prompts/p.name)
for name in ['source-normal-validation.json','source-skin-trig.json']:shutil.copy2(OUT/name,DEST/name)
(DEST/'model-evidence.json').write_text(json.dumps(records,indent=2)+'\n')
text='''# Detailed imagegen texture layers

Eight imagegen-authored colour atlases and eight independently generated technical height atlases, plus one original constant-colour shirt swatch. Nine aligned 1,024-square layer sets cover five source models. Built-in imagegen was used; prompts and generation records are in `prompts/`.

Each atlas folder contains base colour, tangent normal, roughness, metalness, AO, height, detail normal, emission, opacity, wear mask, grime mask, semantic material masks and packed ORM. `layers.json` records provenance and hashes. Colour retains the original alpha convention; normal/ORM channels are linear. Semantic mask RGBA channels are independent weights, not transparency. ORM packs AO / roughness / metalness in RGB.

The native candidate consumes colour, combined normal, ORM and explicit emission. Height, detail-normal and wear/grime masks are editable source layers; no geometric displacement or tessellation is enabled. Emission is black for these five models because no new lamp regions were approved. Existing Ranger, Clank and Mecha emission policies remain separate and unchanged.

Local geometry AO was baked in Blender at 32 samples and 2% body-height distance. Only unique UV areas use that bake. Mirrored/stacked UVs use local height cavity and native depth contact AO. No colour-derived painted AO. Big Al's constant-palette shirt has a deliberately neutral normal instead of a stretched cloth pattern.

`models/` contains five editable Blender files with embedded colour and physical material maps. `review/` contains original native front/rear PNGs and matched comparisons. All five retain their original near-mesh triangle counts:

| Model | Triangles |
|---|---:|
'''
text+=''.join(f'| {r["name"]} | {r["triangles"]:,} |\n' for r in records)
text+='''
Source normals use the exact original angle table cross-checked across saved skin jobs in three levels. Earlier source-only previews with invalid normals are superseded. The previous seven captured-pose models were unaffected by that source-viewer fault.

Lighting uses actual saved level directional sets and nearest-terrain ambient/blend. It is not the actor-specific blend or a studio light rig. These are source bind-space models, not validated animations. No gameplay, world review or timing runs were used.

The full all-enemy/NPC/environment task remains incomplete. This package is an implemented material batch, not a claim that every source model is finished. See `docs/RIFT-TEXTURE-REVIEW.md` in the workspace for the broader coverage and remaining gaps.
'''
(DEST/'README.md').write_text(text,encoding='utf-8')
print(json.dumps(dict(models=len(records),atlases=len(layers),layer_types=len(layers[0]['layer_types']),destination=str(DEST))))
