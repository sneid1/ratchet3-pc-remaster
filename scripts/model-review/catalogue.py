"""Index the complete available source library and UYA target coverage.

This is an inventory, not an automatic visual match. Source families and filename
roles are search aids; a mapping requires explicit reviewed provenance.
"""
import csv
from collections import Counter,defaultdict
import json
from pathlib import Path
import sys

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
sys.path.insert(0,str(ROOT/'scripts'))
import riftapart

def table(path,delimiter=','):
    return list(csv.DictReader((l for l in path.read_text(encoding='utf-8-sig').splitlines() if l and not l.startswith('#')),delimiter=delimiter))

def main():
    toc=riftapart.Toc();dag=riftapart.Dag()
    export_paths=defaultdict(list)
    for p in Path('C:/riftapart-export').rglob('*.png'):
        if 'sheets' not in p.parts:export_paths[p.stem.lower()].append(str(p))
    texture_rows=table(Path('C:/riftapart-export/catalogue.csv'))
    basename_count=Counter(r['name'].split('\\')[-1].removesuffix('.texture').lower() for r in texture_rows)
    models=[]
    for name in dag.names:
        if not name.endswith('.model'):continue
        records=[a for a in toc.by_id.get(riftapart.asset_id(name),()) if a.span==0]
        if not records:continue
        asset=records[0];parts=name.split('\\')
        models.append({'id':f'{asset.id:016x}','path':name,'family':'/'.join(parts[:3]),'category':parts[0],
                       'archive':toc.archives[asset.archive],'bytes':asset.size,'state':'available; not visually reviewed'})
    textures=[]
    for row in texture_rows:
        basename=row['name'].split('\\')[-1].removesuffix('.texture')
        candidates=export_paths[basename.lower()]
        preview=candidates[0] if len(candidates)==1 and basename_count[basename.lower()]==1 else None
        textures.append({'id':row['id'],'path':row['name'],'family':row['group'],'category':row['kind'],
                         'role_hint':row['role'],'width':int(row['width']),'height':int(row['height']),
                         'format':row['format'],'slices':int(row['slices']),'preview':preview,
                         'channel_semantics':'unverified: suffix alone does not establish packing'})
    labels={r['stem']:r for r in table(ROOT/'textures/riftapart-regions.tsv','\t')}
    matches={r['stem']:r for r in table(ROOT/'textures/riftapart-matches.tsv','\t')}
    targets=[]
    recipes={r['stem']:r for r in json.loads(Path(__file__).with_name('material-recipes.json').read_text())['targets']}
    for row in table(ROOT/'docs/surfaces-objects.csv'):
        stem=row['name'].removesuffix('.png');region=labels.get(stem,{});match=matches.get(stem,{})
        targets.append({'id':stem,'kind':row['kind'],'label':region.get('shows') or match.get('shows') or '',
                        'levels':row['levels'].split(';'),'size':[int(row['width']),int(row['height'])],
                        'existing_region_mapping':region or None,'existing_surface_mapping':match or None,
                        'pbr_recipe':recipes.get(stem),
                        'state':recipes[stem]['state'] if stem in recipes else 'needs close visual review'})
    result={'schema':1,'scope':'All installed named RA models and catalogue textures; all existing UYA surface targets.',
            'matching_policy':'Keep UYA identity. Review source fit, geometry/UV compatibility and all material channels. Never apply by filename similarity alone.',
            'sources':{'models':models,'textures':textures},'uya_targets':targets}
    (OUT/'asset-catalogue.json').write_text(json.dumps(result,separators=(',',':')))
    print(f'Indexed {len(models)} installed models, {len(textures)} source textures, {len(targets)} UYA targets. No automatic substitutions.')

if __name__=='__main__':main()
