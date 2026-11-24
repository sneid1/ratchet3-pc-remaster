"""Extend existing source-reviewed surface families to unmapped moby props.

Uses the repository's original surface classifications, never colour brightness
or filename guesses. Mixed character/weapon atlases and effects stay separate.
"""
import csv,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review';PACK=OUT/'candidate/0-riftapart'
coverage=json.loads((OUT/'moby-coverage.json').read_text())
catalogue={r['id']:r for r in json.loads((OUT/'asset-catalogue.json').read_text())['uya_targets']}
def table(path,delimiter='\t'):
    rd=csv.DictReader((s for s in path.read_text().splitlines() if s and not s.startswith('#')),delimiter=delimiter)
    return rd.fieldnames,list(rd)
measurements={r['name'].removesuffix('.png'):r for r in table(ROOT/'docs/surfaces-objects.csv',',')[1]}
# Fine material detail only. Designed panels retain their own seams and colour;
# no foreign panel layout, dirt multiplier or colour-derived cavity is added.
families={
    'metal':('review_satin_alloy',1,.18,.22),
    'panel':('review_painted_metal',0,.18,.35),
    'rock':('detail_concretepockmarks_01',0,.15,.5),
    'floor':('concrete_001',0,.12,.5),
    # This coarse source category also contains background rock silhouettes.
    # Retain geometric normals until the individual surface is identified;
    # never emboss speculative wood grain onto those meshes.
    'wood':('wood_grain_001',0,0,.5),
    'sand':('sand_001',0,.12,.5),
    'ice':('ice_sheet_001',0,.12,.7),
}
fields,rows=table(PACK/'surfaces.tsv');mapped={r['stem'] for r in rows}
mapped.update(r['stem'] for r in table(PACK/'regions.tsv')[1])
missing={t for a in coverage['assets'] for t in a['unmapped_atlases']}
report_path=OUT/'moby-prop-families.json'
previous=json.loads(report_path.read_text())['applied'] if report_path.exists() else []
report={r['stem']:r for r in previous}
for row in rows:
    if row['stem'] in report and report[row['stem']]['source_classification']=='wood':row['normal']=0
for stem in sorted(missing-mapped):
    target=catalogue.get(stem,{})
    kind=target.get('kind')
    if kind not in families or target.get('label','').startswith('props2 '):continue
    material,metal,normal,repeat=families[kind]
    if not (PACK/'materials'/(material+'.png')).is_file():raise ValueError('Missing existing kit material '+material)
    measured=measurements[stem]
    def tiles(axis):
        value=measured.get('repeat_'+axis)
        return round(min(16,max(1,float(value)/repeat)),3) if value else 1
    rows.append(dict(stem=stem,material=material,tiles_u=tiles('u'),tiles_v=tiles('v'),reflectance=.2,grain=0,normal=normal,specular=1,metal=metal))
    report[stem]={'stem':stem,'source_classification':kind,'material':material,'classification_evidence':'docs/surfaces-objects.csv; textures/uya-surface-kinds.txt','colour_and_alpha':'unchanged','ao':'Geometry/depth only','state':'Shared family pass; individual model review still pending'}
with (PACK/'surfaces.tsv').open('w',newline='') as f:
    w=csv.DictWriter(f,fields,delimiter='\t',lineterminator='\n');w.writeheader();w.writerows(rows)
report_path.write_text(json.dumps({'policy':'Existing coarse surface classes; no automatic classification of object atlases, lights, water or effects. Ambiguous wood/background-rock classes retain geometric normals.','applied':list(report.values())},indent=2)+'\n')
print(f'Applied existing physical material families to {len(report)} additional moby prop surfaces')
