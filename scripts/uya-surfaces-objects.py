"""Makes docs/surfaces-objects.csv (plan item 9.2): every texture the gameplay frames draw in the world, the objects' too.

Usage:
  python -I scripts/uya-surfaces-objects.py <measure dir> <docs/surfaces.csv> <out.csv>

<measure dir> is scripts/surfaces-measure.ps1's folder of `uya native --surfaces` counts on the gameplay dumps, one a level,
with the mobies' and shrubs' textures counted (layer `objects`, port/uya/src/surfaces.rs). The output has docs/surfaces.csv's
columns and first line: per texture its levels (those where it covers at least 0.1% of the picture, else the level of its
largest share), its largest share of a level's picture, its size, its world size per repeat (on the level of its largest share
where measured there, else on another level), its mean colour and kind from docs/surfaces.csv when that lists it, and kind
`object` for a mobies' or shrubs' texture that it does not (what an object is, the matching says).
"""
import csv
import sys
from collections import defaultdict
from pathlib import Path

measure, old_csv, out = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3])
old_lines = old_csv.read_text(encoding='utf-8').splitlines()
if not old_lines[0].startswith('# units per metre '):
    sys.exit(f'{old_csv}: the first line is not the units per metre')
old = {r['name']: r for r in csv.DictReader(old_lines[1:])}
per = defaultdict(dict)
for tsv in sorted(measure.glob('f*-*.tsv')):
    level = int(tsv.stem.split('-')[1])
    lines = tsv.read_text().splitlines()
    if lines[1].split('\t') != ['stem', 'layer', 'width', 'height', 'pixels', 'share', 'repeat_u', 'repeat_v']:
        sys.exit(f'{tsv}: columns {lines[1]!r}')
    for l in lines[2:]:
        stem, layer, w, h, px, share, ru, rv = l.split('\t')
        per[stem][level] = (layer, int(w), int(h), float(share), ru, rv)
rows = []
for stem, by_level in per.items():
    best = max(by_level, key=lambda lv: by_level[lv][3])
    layer, w, h, share, ru, rv = by_level[best]
    # A texture no saved frame drew: kept when it is an object's (the level loads it for a moby or a shrub: every level's props,
    # shrubs and enemies, PLAN 9.2), with share 0, every level that loads it and no repeat unless a draw measured one.
    if share == 0 and layer != 'objects':
        continue
    if not ru:
        ru, rv = next(((v[4], v[5]) for v in by_level.values() if v[4]), ('', ''))
    levels = sorted(lv for lv, v in by_level.items() if v[3] >= 0.001) or ([best] if share > 0 else sorted(by_level))
    o = old.get(stem + '.png') or old.get(stem)
    kind = o['kind'] if o else ('object' if layer == 'objects' else '')
    if not kind:
        sys.exit(f'{stem}: a {layer} texture with no kind in {old_csv}')
    rows.append(dict(name=stem, levels=';'.join(map(str, levels)), share=f'{share:.5f}', width=w, height=h, repeat_u=ru, repeat_v=rv,
                     L=o['L'] if o else '', a=o['a'] if o else '', b=o['b'] if o else '', kind=kind))
rows.sort(key=lambda r: -float(r['share']))
with open(out, 'w', newline='', encoding='utf-8') as f:
    f.write(old_lines[0] + '\n')
    wr = csv.DictWriter(f, fieldnames=['name', 'levels', 'share', 'width', 'height', 'repeat_u', 'repeat_v', 'L', 'a', 'b', 'kind'], lineterminator='\n')
    wr.writeheader()
    wr.writerows(rows)
print(f'{len(rows)} textures drawn ({sum(1 for r in rows if r["kind"] == "object")} objects): {out}')
