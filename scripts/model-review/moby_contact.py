"""Numbered source-atlas contact sheets for explicit family review, not auto-tags."""
import json
from collections import Counter
from pathlib import Path
from PIL import Image,ImageDraw
OUT=Path(__file__).resolve().parents[2]/'captures/rift-texture-review'
data=json.loads((OUT/'moby-coverage.json').read_text())
catalogue={r['id']:r for r in json.loads((OUT/'asset-catalogue.json').read_text())['uya_targets']}
uses=Counter(t for a in data['assets'] for t in a['unmapped_atlases'])
rows=[]
for stem,count in uses.most_common():
    label=catalogue.get(stem,{}).get('label','')
    if label.startswith(('props2 none:','props2 light:','props2 water:')):continue
    path=next((p for p in [OUT/'library/uya-mobies/textures'/(stem+'.png'),OUT/'dumps'/(stem+'.png'),Path.home()/'Games/Ratchet and Clank 3/texture_dumps'/(stem+'.png')] if p.is_file()),None)
    if path is None:continue
    im=Image.open(path)
    rows.append({'stem':stem,'path':str(path),'size':im.size,'class_uses':count,'label':label})
(OUT/'moby-contact-index.json').write_text(json.dumps(rows,indent=2)+'\n')
for start in range(0,len(rows),64):
    page=Image.new('RGB',(1024,1184),(28,31,35));draw=ImageDraw.Draw(page)
    for n,row in enumerate(rows[start:start+64],start):
        x=(n%64)%8*128;y=(n%64)//8*148
        im=Image.open(row['path']).convert('RGB');im.thumbnail((124,124))
        page.paste(im,(x+(128-im.width)//2,y))
        draw.text((x+3,y+126),f'{n:03d}  {row["class_uses"]} classes',fill='white')
    page.save(OUT/f'moby-atlases-{start//64:02}.jpg',quality=92)
print(f'{len(rows)} source atlases; {(len(rows)+63)//64} contact sheets')
