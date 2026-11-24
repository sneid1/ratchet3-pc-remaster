"""Join actual source classes to material coverage without calling maps art approval.

Every level occurrence is retained; identical source fingerprints share a row.
Unknown/runtime materials and undecoded source containers remain explicit work.
"""
import csv
import json
from collections import defaultdict
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart'
INDEX=OUT/'library/uya-mobies'
def table(path):
    return list(csv.DictReader((s for s in path.read_text().splitlines() if s and not s.startswith('#')),delimiter='\t'))
mapped={}
for name in ['surfaces','regions','pbr']:
    for row in table(PACK/(name+'.tsv')):mapped[row['stem']]=name
catalogue=json.loads((OUT/'asset-catalogue.json').read_text())
labels={r['id']:r['label'] for r in catalogue['uya_targets']}
preview=json.loads(Path(__file__).with_name('review-assets.json').read_text())
groups=defaultdict(list)
for row in table(INDEX/'occurrences.tsv'):groups[row['asset_id']].append(row)
result=[]
for key,uses in groups.items():
    textures=sorted({t for r in uses for t in r['textures'].split(',') if t})
    runtime=[t for t in textures if t.startswith('runtime-material-')]
    atlas=[t for t in textures if t not in runtime]
    missing=[t for t in atlas if t not in mapped]
    candidates=[name for name,model in preview.items() if model.get('source_layer')!='terrain' and any(t.startswith(tuple(model['prefixes'].split(','))) for t in atlas)]
    result.append({'asset_id':key,'occurrences':[{'level':int(r['level']),'class_id':int(r['class_id']),'slot':int(r['slot'])} for r in uses],
        'near_triangles':int(uses[0]['near_triangles']),'all_meshes':int(uses[0]['all_meshes']),
        'atlases':atlas,'runtime_materials':runtime,'unmapped_atlases':missing,
        'labels':list(dict.fromkeys(labels[t] for t in atlas if labels.get(t))),
        'material_mapping':'partial' if missing or runtime else 'mapped' if atlas else 'no static atlas',
        'related_capture_candidates':candidates,
        'geometry_state':'Shared original-surface refinement available in the native sandbox for every decoded draw; individual source-class art pass pending',
        'visual_state':'not individually approved'})
result.sort(key=lambda r:(bool(r['related_capture_candidates']),r['material_mapping']=='mapped',-r['near_triangles']))
unresolved=table(INDEX/'unresolved-sources.tsv')
(OUT/'moby-coverage.json').write_text(json.dumps({'scope':'Every decoded LEVEL core class, every source occurrence and LOD/variant; unresolved source containers listed separately',
    'renderer':'Sandbox PBR consumes corrected physical moby normals/directions, outward fur shells and mapped material families',
    'completion':'Incomplete: material mapping is not model or visual completion',
    'unresolved_sources':unresolved,'assets':result},indent=2)+'\n')
with (OUT/'remaining-mobies.tsv').open('w',newline='') as f:
    w=csv.writer(f,delimiter='\t',lineterminator='\n');w.writerow(['asset_id','levels','class_ids','near_triangles','material_mapping','unmapped_atlases','runtime_materials','related_previews'])
    for r in result:w.writerow([r['asset_id'],','.join(map(str,sorted({o['level'] for o in r['occurrences']}))),','.join(map(str,sorted({o['class_id'] for o in r['occurrences']}))),r['near_triangles'],r['material_mapping'],','.join(r['unmapped_atlases']),','.join(r['runtime_materials']),','.join(r['related_capture_candidates'])])
print(json.dumps({'source_occurrences':sum(len(a['occurrences']) for a in result),'source_assets':len(result),
    'mapped_assets':sum(a['material_mapping']=='mapped' for a in result),'unique_unmapped_atlases':len({t for a in result for t in a['unmapped_atlases']}),
    'unresolved_source_records':len(unresolved),'art_pass':'incomplete'}))
