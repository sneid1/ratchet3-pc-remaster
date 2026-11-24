"""Writes the texture coverage report: which textures each capture area draws, by share of the picture,
and which of them the installed replacements cover.

Usage:
  python -I scripts/texture-coverage-report.py <report.md> <texture_replacements dir> <run dir> [<run dir> ...]
      [--dumps DIR]

Each <run dir> is a `uya frames ... --areas ...` run: its coverage.tsv gives, per area, every texture
id the shown picture was made of and its mean share of the picture (the port's texture-id plane, see
rac-rs gsfull/gs/texhook.rs). A texture is covered when a file under <texture_replacements dir>
(every installed selection, searched recursively, as the port searches it) has its PCSX2 key. When
the run itself drew replacements (run with --replace), its replacements.tsv says which files were
drawn and which were refused and why; the report lists the refusals.

The gap list orders the uncovered textures of every area by share of the picture: the order Phase 4
item 3 fills them in. Textures the run drew that no area's shown picture contains (boot and front-end
screens outside the areas, cutscenes, passes into off-screen targets) follow, from the run's
textures.tsv, by prims drawn. --selections (the textures/*.txt lists) says for each gap whether a pack
has a file for it and why that file was left out. --dumps (the run's --dump folder) checks each gap has
its dump.
"""
import argparse
import collections
import importlib.util
from pathlib import Path

_spec = importlib.util.spec_from_file_location('uya_texnames', Path(__file__).with_name('uya_texnames.py'))
names = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(names)

PSM = {0x00: 'CT32', 0x01: 'CT24', 0x02: 'CT16', 0x0a: 'CT16S', 0x13: 'T8', 0x14: 'T4', 0x1b: 'T8H', 0x24: 'T4HL', 0x2c: 'T4HH'}
SPECIAL = ('(untextured)', '(render target)')


def read_coverage(run):
    """{area: (header line, [rows])}, rows as dicts of the coverage.tsv columns."""
    areas = {}
    header = {}
    cols = None
    for line in (run / 'coverage.tsv').read_text(encoding='utf-8').splitlines():
        if line.startswith('# area '):
            name, rest = line[len('# area '):].split(':', 1)
            header[name] = rest.strip()
            continue
        if line.startswith('#'):
            continue
        f = line.split('\t')
        if cols is None:
            cols = f
            continue
        row = dict(zip(cols, f))
        row['share'] = float(row['share'])
        row['seen'] = float(row['seen'])
        areas.setdefault(row['area'], (header.get(row['area'], ''), []))[1].append(row)
    return areas


def installed_keys(folder):
    """{key: path} of every replacement file the port would load from the folder (first in sorted order wins)."""
    keys = {}
    for p in sorted(Path(folder).rglob('*')):
        if p.is_file():
            k = names.parse(p.name)
            if k is not None and k not in keys:
                keys[k] = p
    return keys


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('report', type=Path)
    ap.add_argument('replacements', type=Path)
    ap.add_argument('runs', type=Path, nargs='+')
    ap.add_argument('--dumps', type=Path)
    ap.add_argument('--selections', type=Path, nargs='*', default=[])
    args = ap.parse_args()

    # Pack files a selection left out, by key: "<selection>: <reason>".
    left_out = {}
    for sel in args.selections:
        reason = None
        for line in sel.read_text(encoding='utf-8').splitlines():
            if line.startswith('# Left out ('):
                reason = line.split('): ', 1)[1]
            elif not line.strip():
                reason = None
            elif reason and line.startswith('#   '):
                key = names.parse(line[4:].split('  (')[0].rsplit('/', 1)[-1])
                if key is not None:
                    left_out.setdefault(key, f'{sel.stem}: {reason}')

    def pack_status(stem):
        return left_out.get(names.parse(stem + '.png'), 'no file in any pack the selections were made from')

    keys = installed_keys(args.replacements) if args.replacements.is_dir() else {}
    sections = collections.Counter(p.relative_to(args.replacements).parts[0] for p in keys.values())
    out = ['# Texture coverage', '']
    out.append(f'Replacements: {len(keys)} keys under `{args.replacements}`'
               + (' (' + ', '.join(f'{s}: {n}' for s, n in sections.most_common()) + ')' if sections else '') + '.')
    out.append('')
    out.append('Share is the mean share of the shown picture over the area\'s samples, from the port\'s texture-id plane '
               '(last write wins, so a transparent effect counts where it draws over something else). '
               'Untextured and render-target pixels (screen copies, effects drawn from the frame) are listed apart: '
               'no texture pack can replace them.')
    out.append('')
    out.append('| Area | Samples | Textures drawn | Covered | Share of textured picture covered | Untextured | Render target |')
    out.append('| --- | --- | --- | --- | --- | --- | --- |')
    gaps = []
    refused = {}
    elsewhere = {}
    in_areas = set()
    for run in args.runs:
        rt = run / 'replacements.tsv'
        if rt.exists():
            for line in rt.read_text(encoding='utf-8').splitlines()[1:]:
                f = line.split('\t')
                if f[1] != 'ok':
                    refused[f[0]] = (f[1], f[6])
        for area, (header, rows) in read_coverage(run).items():
            tex = [r for r in rows if r['stem'] not in SPECIAL]
            special = {r['stem']: r['share'] for r in rows if r['stem'] in SPECIAL}
            textured = sum(r['share'] for r in tex)
            covered = [r for r in tex if names.parse(r['stem'] + '.png') in keys]
            cshare = sum(r['share'] for r in covered)
            out.append(f'| {area} | {header} | {len(tex)} | {len(covered)} | '
                       f'{(100 * cshare / textured if textured else 0):.1f}% ({cshare:.1f}% of {textured:.1f}%) | '
                       f'{special.get("(untextured)", 0):.1f}% | {special.get("(render target)", 0):.1f}% |')
            for r in tex:
                in_areas.add(r['stem'])
                if names.parse(r['stem'] + '.png') not in keys:
                    gaps.append((area, r))
        tt = run / 'textures.tsv'
        if tt.exists():
            lines = tt.read_text(encoding='utf-8').splitlines()
            cols = lines[0].split('\t')
            for line in lines[1:]:
                r = dict(zip(cols, line.split('\t')))
                if names.parse(r['stem'] + '.png') not in keys:
                    elsewhere.setdefault(r['stem'], r)
    out.append('')
    out.append(f'## Gap list ({len(gaps)} textures not covered)')
    out.append('')
    out.append('Largest share of the picture first. Size is the named rectangle in texels.')
    out.append('')
    out.append('| Area | Share | Seen | Texture | Format | Size | Dump | Packs |')
    out.append('| --- | --- | --- | --- | --- | --- | --- | --- |')

    def dump_of(stem):
        return ('yes' if (args.dumps / f'{stem}.png').exists() else 'no') if args.dumps else ''

    for area, r in sorted(gaps, key=lambda g: -g[1]['share']):
        psm = PSM.get(int(r['psm'], 16), r['psm'])
        out.append(f'| {area} | {r["share"]:.3f}% | {r["seen"]:.0f}% | `{r["stem"]}` | {psm} | {r["w"]}x{r["h"]} | '
                   f'{dump_of(r["stem"])} | {pack_status(r["stem"])} |')
    rest = [r for stem, r in elsewhere.items() if stem not in in_areas]
    out.append('')
    out.append(f'## Drawn outside the capture areas, not covered ({len(rest)})')
    out.append('')
    out.append('Textures the run drew that no area\'s shown picture contains, most prims first.')
    out.append('')
    out.append('| Prims | Texture | Format | Size | Dump | Packs |')
    out.append('| --- | --- | --- | --- | --- | --- |')
    for r in sorted(rest, key=lambda r: -int(r['prims'])):
        psm = PSM.get(int(r['psm'], 16), r['psm'])
        out.append(f'| {r["prims"]} | `{r["stem"]}` | {psm} | {r["w"]}x{r["h"]} | {dump_of(r["stem"])} | {pack_status(r["stem"])} |')
    if refused:
        out.append('')
        out.append(f'## Replacements refused in game ({len(refused)})')
        out.append('')
        for stem, (why, path) in sorted(refused.items()):
            out.append(f'- `{stem}`: {why} (`{path}`)')
    args.report.write_text('\n'.join(out) + '\n', encoding='utf-8')
    print(f'{len(gaps)} gaps -> {args.report}')


if __name__ == '__main__':
    main()
