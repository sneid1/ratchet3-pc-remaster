"""Find source-atlas aliases using image evidence and actual source class IDs.

Only the existing material assignment is inherited. Never replace the target's
colour/alpha, and never call an image match a model or visual approval.
"""
import csv,json,hashlib
from collections import defaultdict
from pathlib import Path
import numpy as np
from PIL import Image
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review';PACK=OUT/'candidate/0-riftapart'
def table(path):
    reader=csv.DictReader((s for s in path.read_text().splitlines() if s and not s.startswith('#')),delimiter='\t')
    return reader.fieldnames,list(reader)
def path_for(stem):
    return next((p for p in [OUT/'library/uya-mobies/textures'/(stem+'.png'),OUT/'dumps'/(stem+'.png'),Path.home()/'Games/Ratchet and Clank 3/texture_dumps'/(stem+'.png')] if p.is_file()),None)
coverage=json.loads((OUT/'moby-coverage.json').read_text())
classes=defaultdict(set)
missing=set()
for a in coverage['assets']:
    missing.update(a['unmapped_atlases'])
    for t in a['atlases']:classes[t].update(o['class_id'] for o in a['occurrences'])
tables={f:table(PACK/(f+'.tsv')) for f in ['regions','surfaces']}
sources={r['stem']:(f,r) for f,(_,rows) in tables.items() for r in rows}
# Original mappings only: inherited rows must not accumulate confidence by chaining.
old=OUT/'moby-atlas-aliases.json'
if old.exists():
    inherited={r['target'] for r in json.loads(old.read_text())['accepted']}
    sources={s:r for s,r in sources.items() if s not in inherited}
    missing.update(inherited)
arrays={}
for s in sources:
    path=path_for(s)
    if path:
        im=Image.open(path).convert('RGBA')
        arrays[s]=np.asarray(im.resize((32,32),Image.Resampling.BOX),dtype=np.float32)
accepted=[];proposed=[]
for stem in sorted(missing):
    path=path_for(stem)
    if path is None:continue
    im=Image.open(path).convert('RGBA');a=np.asarray(im.resize((32,32),Image.Resampling.BOX),dtype=np.float32)
    # Flat fills and isolated light gradients are not atlas identity evidence.
    if min(im.size)<16 or float(a[...,:3].std(axis=(0,1)).max())<15:continue
    candidates=list(arrays)
    if not candidates:continue
    scored=sorted((float(np.mean((a-arrays[s])**2)**.5),s) for s in candidates)
    rms,source=scored[0]
    if rms>14:continue
    entry={'target':stem,'source':source,'source_classes':sorted(classes[stem]&classes[source]),'rgba32_rms':round(rms,4),
           'source_image_sha256':hashlib.sha256(path_for(source).read_bytes()).hexdigest(),
           'target_image_sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
    # Very close RGBA agreement, a common real class, and enough spatial detail.
    # Higher-error/palette variants stay proposals for visual region inspection.
    if rms<=1.0 or (rms<=3.0 and entry['source_classes']):
        family,row=sources[source];entry['table']=family
        copy=dict(row);copy['stem']=stem
        tables[family][1][:]=[r for r in tables[family][1] if r['stem']!=stem]
        tables[family][1].append(copy);accepted.append(entry)
    else:proposed.append(entry)
for name,(fields,rows) in tables.items():
    with (PACK/(name+'.tsv')).open('w',newline='') as f:
        w=csv.DictWriter(f,fields,delimiter='\t',lineterminator='\n');w.writeheader();w.writerows(rows)
old.write_text(json.dumps({'policy':'RGBA source image agreement <=1 RMS at 32px, or <=3 with a common decoded class ID; spatial detail required. Source colour and alpha unchanged. No chained inheritance.','accepted':accepted,'needs_visual_region_review':proposed},indent=2)+'\n')
print(json.dumps({'material_aliases_applied':len(accepted),'proposals_for_visual_review':len(proposed)}))
