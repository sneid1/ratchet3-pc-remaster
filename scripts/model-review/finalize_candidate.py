"""Record current pack references and provenance, without rerendering or rebaking."""
import csv,hashlib,json
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review';PACK=OUT/'candidate/0-riftapart'
def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def table(name):return list(csv.DictReader((s for s in (PACK/(name+'.tsv')).read_text().splitlines() if s and not s.startswith('#')),delimiter='\t'))
counts={};references=set()
for name in ['surfaces','regions','pbr']:
    rows=table(name);counts[name]=len(rows)
    if len({r['stem'] for r in rows})!=len(rows):raise ValueError('Duplicate '+name+' row')
    for row in rows:
        if name=='regions':references.add(PACK/'regions'/row['map'])
        if name=='pbr':references.update(PACK/'materials'/(row[k]+'.png') for k in ['normal_map','orm_map','emissive_map'] if row.get(k))
        else:
            for suffix in (['','1','2','3'] if name=='regions' else ['']):
                material=row['material'+suffix]
                if material not in ['-','fur']:references.add(PACK/'materials'/(material+'.png'))
                if not 0<=float(row['metal'+suffix])<=1:raise ValueError('Metalness outside physical range')
missing=[str(p) for p in references if not p.is_file()]
if missing:raise ValueError('Missing material data: '+str(missing))
manifest_path=OUT/'candidate/manifest.json';manifest=json.loads(manifest_path.read_text());baseline=Path(manifest['baseline'])
manifest['files']=[]
for path in sorted(p for p in PACK.rglob('*') if p.is_file()):
    rel=path.relative_to(PACK).as_posix();old=baseline/rel;digest=sha(path);previous=sha(old) if old.exists() else None
    manifest['files'].append({'path':rel,'bytes':path.stat().st_size,'sha256':digest,'baseline_sha256':previous,'changed':digest!=previous})
manifest['coverage']={'atlas_aliases':'moby-atlas-aliases.json','prop_families':'moby-prop-families.json','source_classes':'moby-coverage.json','art_pass':'Incomplete; coverage is not individual approval'}
manifest_path.write_text(json.dumps(manifest,indent=2)+'\n')
build=OUT/'viewer-build'
provenance={'executable':sha(build/'uya-model-review.exe'),'owned_sources':{p.name:sha(p) for p in Path(__file__).parent.iterdir() if p.is_file()},'scope':'Isolated renderer; no active game deployment'}
(build/'review-build-sha256.json').write_text(json.dumps(provenance,indent=2)+'\n')
report={'tables':counts,'resolved_material_files':len(references),'missing_references':missing,'manifest_files':len(manifest['files']),'visual_evidence':'See docs/RIFT-TEXTURE-REVIEW.md; this checks references and provenance only.','art_pass':'incomplete'}
(OUT/'candidate-integrity-current.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
