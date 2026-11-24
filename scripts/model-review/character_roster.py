"""Keep every source occurrence/variant; expose a resumable source review queue.

Joint count is a prioritisation aid, never an exclusion or a character label.
SCENE and other undecoded containers remain explicit unresolved scope.
"""
import argparse,csv,json
from collections import defaultdict
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
def table(path):return list(csv.DictReader(path.read_text().splitlines(),delimiter='\t'))
def build():
    old_path=OUT/'character-roster.json'
    prior={a['asset_id']:a for a in json.loads(old_path.read_text())['assets']} if old_path.exists() else {}
    rows=table(OUT/'library/uya-mobies/occurrences.tsv');groups=defaultdict(list)
    for r in rows:groups[r['asset_id']].append(r)
    pbr={r['stem'] for r in table(OUT/'candidate/0-riftapart/pbr.tsv')}
    assets=[];classes=defaultdict(list)
    for key,uses in groups.items():
        r=uses[0];atlases=[s for s in r['textures'].split(',') if s and not s.startswith('runtime-')]
        a=dict(asset_id=key,occurrences=[{k:int(u[k]) for k in ['level','slot','class_id']} for u in uses],
            near_triangles=int(r['near_triangles']),joints=int(r['near_joints']),atlases=atlases,
            pbr_atlases=[s for s in atlases if s in pbr],unmapped_pbr=[s for s in atlases if s not in pbr],
            identity='Unclassified; atlas prose is not model identity evidence',
            fitted_art='Pending individual work',visual_review='Pending',geometry='Original source topology')
        # Preserve actual inspection evidence for unchanged source fingerprints.
        # A refreshed coverage count must not erase work or trigger re-review.
        if key in prior:
            for field in ['identity','fitted_art','visual_review','review_evidence','limitations']:
                if field in prior[key]:a[field]=prior[key][field]
        assets.append(a)
        if a['near_triangles']>150 and a['joints']>=2 and atlases:
            for u in uses:classes[int(u['class_id'])].append((a,u))
    manifest={};queue=[]
    for class_id,variants in sorted(classes.items()):
        candidates=[]
        for a,u in variants:
            folder=ROOT/f'captures/gameplay/L{int(u["level"]):02}'
            if list(folder.glob('ram*.bin')) and list(folder.glob('jobs*.bin')):candidates.append((a,u,folder))
        if not candidates:continue
        a,u,folder=max(candidates,key=lambda v:(v[0]['near_triangles'],-int(v[1]['level'])))
        name=f'source_c{class_id}'
        manifest[name]=dict(name=f'Source class {class_id}',prefixes=','.join(s.split('-')[0] for s in a['atlases']),
            instance=0,level=int(u['level']),poses=str(folder.resolve()),whole=True,source_layer='source',source_id=int(u['slot']),
            asset_id=a['asset_id'],class_id=class_id,
            lighting_note='Saved level directional sets; original terrain light blend/ambient nearest the saved camera. Not the original actor-specific light blend.',
            pose_note='Original bind-space near mesh. Runtime-only materials and UVs fail explicitly.')
        queue.append(dict(name=name,model=name,yaw=45,pitch=8,zoom=1.25))
    queue.sort(key=lambda r:(manifest[r['model']]['level'],r['model']))
    (OUT/'character-source-assets.json').write_text(json.dumps(manifest,indent=2)+'\n')
    (OUT/'character-source-requests.json').write_text(json.dumps(queue,indent=2)+'\n')
    result=dict(scope='All decoded LEVEL source assets, including every occurrence and variant; no non-jointed asset silently excluded',
        completion='Incomplete: source review and shared PBR are not finished enemy/NPC art',
        unresolved_sources=table(OUT/'library/uya-mobies/unresolved-sources.tsv'),assets=assets,
        first_review_queue='One highest-detail representative per class with >=2 joints and >150 triangles. Remaining variants and unclassified meshes remain in assets.',
        representative_classes=len(queue))
    (OUT/'character-roster.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(dict(source_assets=len(assets),jointed_assets=sum(a['joints']>1 and a['near_triangles']>0 for a in assets),representative_classes=len(queue))))

def render(start,count):
    import viewer
    viewer.MODELS=json.loads((OUT/'character-source-assets.json').read_text())
    queue=json.loads((OUT/'character-source-requests.json').read_text())[start:start+count]
    args=argparse.Namespace(height=640,export_obj=True,limit_mb=3200,batch=True,requests=True,poses=None)
    renderer=viewer.Renderer(args)
    directory=OUT/'roster-review';directory.mkdir(exist_ok=True)
    report=directory/'results.json';records=json.loads(report.read_text()) if report.exists() else {}
    try:
        for request in queue:
            name=request['name'];path=directory/(name+'.png')
            if name in records and (records[name].get('normal_decode_revision')==2 or records[name]['state']=='Unresolved source preview'):continue
            try:
                renderer.render({**request,'pack':'candidate'},path)
                records[name]=dict(path=str(path),asset=viewer.MODELS[name],state='Rendered; individual visual review pending',normal_decode_revision=2,
                    normal_source='Exact original angle table cross-checked across saved skin jobs in three levels')
                print(name+': rendered',flush=True)
            except Exception as e:
                records[name]=dict(asset=viewer.MODELS[name],state='Unresolved source preview',error=str(e))
                print(name+': '+str(e),flush=True)
            report.write_text(json.dumps(records,indent=2)+'\n')
    finally:renderer.close()

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--render',action='store_true');p.add_argument('--start',type=int,default=0);p.add_argument('--count',type=int,default=40);a=p.parse_args()
    render(a.start,a.count) if a.render else build()
