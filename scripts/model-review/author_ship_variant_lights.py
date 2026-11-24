"""Reuse the inspected engine-core light assignment on the two other ships."""
import hashlib,json,struct
from pathlib import Path
import numpy as np

ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
manifest=Path(__file__).with_name('review-assets.json');assets=json.loads(manifest.read_text())
for model in ('ratchet_ship_2','ratchet_ship_3'):
    path=OUT/f'candidate-blender-{model}.obj';positions=[];uv=[]
    for line in path.read_text().splitlines():
        p=line.split()
        if p and p[0]=='v':positions.append(list(map(float,p[1:4])))
        elif p and p[0]=='vt':uv.append([float(p[1])%1,(1-float(p[2]))%1])
    assembly=json.loads(path.with_suffix('.assembly.json').read_text())
    engine=next(p for p in assembly['parts'] if p['name']=='Twin engine assembly')
    assert engine['class_id']==4312 and engine['end']-engine['first']==452
    first=engine['first']+392;end=first+12
    v=np.array(positions).reshape(-1,3,3);u=np.array(uv).reshape(-1,3,2)*256
    assert len(v)==assembly['parts'][-1]['end']
    assert np.all((u[first:end,:,0]>=129)&(u[first:end,:,0]<=147))
    assert np.all((u[first:end,:,1]>=29)&(u[first:end,:,1]<=47))
    assert np.ptp(v[first:end,:,0])<.001
    flags=np.zeros(len(v),np.uint8);flags[first:end]=1
    target=OUT/'blender/edited'/(model+'.emission.bin');target.parent.mkdir(exist_ok=True)
    target.write_bytes(b'R3EMIT01'+struct.pack('<I',len(v)*3)+bytes(flags))
    target.with_suffix('.json').write_text(json.dumps(dict(model=model,source_obj_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),source_vertices=len(v)*3,allowed_source_faces=list(range(first,end)),description='Only the original class 4312 engine-core fans emit. Shared ship_engine_emit atlas; all hull, wing, weapon and cockpit surfaces disabled.'),indent=2)+'\n')
    assets[model]['emission_policy']=target.relative_to(OUT).as_posix()
    print(model,len(v),'source triangles; engine-only faces',first,end)
manifest.write_text(json.dumps(assets,indent=2)+'\n')
