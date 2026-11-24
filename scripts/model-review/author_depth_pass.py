"""Fitted construction relief on the inspected Ranger/Ratchet source UV kit.

No remesh, colour-height conversion or blanket noise. Keep previous recipes and
maps for a matched native comparison; every added feature is explicit UV art.
"""
import json,shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'captures/rift-texture-review'
PACK=OUT/'candidate/0-riftapart';RECIPE=Path(__file__).with_name('material-recipes.json')
archive=OUT/'authored/pre-depth-pass';archive.mkdir(exist_ok=True)
recipes=json.loads(RECIPE.read_text())
for source in [RECIPE,PACK/'pbr.tsv',*PACK.joinpath('materials').glob('pbr_*')]:
    if not (archive/source.name).exists():shutil.copy2(source,archive/source.name)

def line(label,points,width=.8,height=-.6,bevel=.45,**kw):
    return dict(label='Depth: '+label,shape='line',points=points,width=width,height=height,bevel=bevel,**kw)
def shape(label,bounds,height=.65,bevel=.55,kind='rect',**kw):
    return dict(label='Depth: '+label,shape=kind,bounds=bounds,height=height,bevel=bevel,**kw)
def polygon(label,points,height=.8,bevel=.7,**kw):
    return dict(label='Depth: '+label,shape='polygon',points=points,height=height,bevel=bevel,**kw)

ranger=next(t for t in recipes['targets'] if t['model']=='ranger')
ranger['surface_layers']=[l for l in ranger['surface_layers'] if not l['label'].startswith('Depth: ')]
for l in ranger['surface_layers']:
    if 'height' in l:
        old=l.setdefault('pre_depth_height',l['height'])
        l['height']=old*(2.0 if old<0 else 1.65)
        l['edge_ao']=.64 if old<0 else .75;l['edge_width']=.9;l['edge_wear']=.7
        if 'cavity' in l:l['cavity']=min(l['cavity'],.55 if old<-.4 else .74)
    elif l['label'].startswith('V4'):
        l.update(height=.85,edge_ao=.72,edge_width=.85,edge_wear=.7)
for x in [43,81]:
    ranger['surface_layers'].append(line('machined plate perimeter',[[x,33],[x,63],[x+22,63],[x+22,33],[x,33]],.55,-.45,.25,cavity=.68,roughness=.64))
ranger['surface_layers'] += [
    line('cannon service cover joint',[[173,165],[194,157],[237,157],[243,162],[243,195],[174,195]],.8,-1.0,.4,cavity=.54,roughness=.72),
    line('cannon machined index groove',[[193,190],[199,180]],.65,-.7,.3,cavity=.62),
    line('cannon parallel index groove',[[196,190],[202,180]],.65,-.7,.3,cavity=.62),
    polygon('left greave raised face',[[30,168],[48,168],[48,198],[41,201],[29,187]],1.0,.9,edge_ao=.68,edge_wear=.65),
    polygon('right greave raised face',[[58,168],[79,168],[93,201],[74,205],[58,199]],1.0,.9,edge_ao=.68,edge_wear=.65),
    line('bottom rail channel',[[100,239],[242,239]],2.0,-1.1,.5,cavity=.46,roughness=.76),
]
# Sparse service scratches, restricted to the exposed cannon alloy and shin
# coatings. Different lengths/directions avoid a repeating all-over dirt stamp.
for i,points in enumerate([[[148,184],[151,181]],[[151,189],[156,183]],[[175,199],[179,198]],[[225,160],[230,161]],[[73,199],[77,198]],[[33,182],[36,185]]]):
    ranger['surface_layers'].append(line('edge abrasion '+str(i),points,.35,-.18,.18,roughness=.3))
for region in ranger['emissive_regions']:
    if 'antenna' in region['label']:region['label']='Shared orange light swatch; antenna excluded by source-face policy'

body_stem='9a6d927dedfae75e-284e78b48088f99a-00002213'
body=next((t for t in recipes['targets'] if t['stem']==body_stem),None)
if body is None:
    body=dict(stem=body_stem,model='ratchet',overrides=[],surface_layers=[])
    recipes['targets'].append(body)
body['surface_layers']=[]
body['region_overrides']=[dict(label='Glove leather; original painted highlights are not metal studs',shape='polygon',bevel=.15,channel=0,points=points) for points in [
    [[117,3],[171,3],[175,9],[172,29],[167,39],[180,45],[191,46],[197,55],[198,62],[184,66],[175,64],[176,97],[171,105],[159,109],[145,110],[129,108],[119,99],[115,74],[116,43]],
    [[110,112],[154,112],[155,122],[151,143],[153,155],[171,158],[181,169],[181,173],[170,176],[161,172],[162,208],[159,218],[146,222],[133,221],[121,218],[109,211],[106,197],[106,172],[109,150]]]]
body['profile_overrides']={
    'review_soft_leather':dict(metalness=0,finish='leather',roughness=dict(mode='constant',value=.56),
        finish_overrides=dict(normal_gain=.32,normal_highpass=.006,repeats=8,roughness=.61,variation=.15,roughness_range=[.5,.73])),
    'review_satin_alloy':dict(metalness=1,finish='brushed_steel',roughness=dict(mode='constant',value=.37),
        finish_overrides=dict(normal_gain=.3,repeats=5,roughness=.37,variation=.12,roughness_range=[.29,.48]))}
L=body['surface_layers']
# Broad leather folds on the two glove islands: recessed valleys, neighbouring
# rounded crests and contact grime; never apply these to skin, iris or fur.
for group,(x,y) in enumerate([(120,6),(110,116)]):
    for i,(dy,w) in enumerate([(7,29),(15,33),(23,27),(31,25)]):
        pts=[[x+4,y+dy],[x+12,y+dy-1],[x+w-5,y+dy+1],[x+w,y+dy]]
        L.append(line(f'glove {group} flex crease {i}',pts,.7,-.55,.6,cavity=.72,roughness=.7))
        L.append(line(f'glove {group} fold crest {i}',[[a,b+1.1] for a,b in pts],1.6,.4,1.0,edge_wear=.3))
    L.append(line(f'glove {group} cuff welt',[[x+1,y+2],[x+26,y+2],[x+40,y+3]],1.0,.7,.5,edge_ao=.68,edge_width=.7,roughness=.5))
# Palm/finger channels and knuckle flexion lines follow the existing painted
# contours, giving the original forms moving highlights under level lighting.
for x,top,bottom in [(128,63,101),(143,63,107),(157,62,103),(118,173,211),(131,176,218),(144,176,219),(158,173,214)]:
    L.append(line('finger separation',[[x,top],[x-1,(top+bottom)*.5],[x,bottom]],.7,-.58,.6,cavity=.7,roughness=.68))
for x in [113,126,139,152]:
    for y in [185,200]:
        L.append(line('knuckle flexion seam',[[x,y],[x+4,y+1],[x+9,y]],.55,-.4,.45,cavity=.77))
L += [
    line('upper glove thumb root',[[167,43],[169,51],[168,58]],.8,-.7,.8,cavity=.72),
    line('lower glove thumb root',[[157,154],[163,160],[166,169]],.8,-.7,.8,cavity=.72),
    polygon('boot upper inset',[[2,166],[20,164],[44,165],[59,167],[82,166],[98,168],[100,185],[2,185]],.75,1.1,edge_ao=.72,edge_wear=.5),
    shape('boot collar rib housing',[2,129,83,158],.8,.7,edge_ao=.65,roughness=.58),
    shape('boot shin inset',[18,132,47,153],-.9,.6,cavity=.65,roughness=.65),
    line('boot toe perimeter',[[5,183],[7,176],[14,173],[24,173],[29,179],[29,188]],1.1,.7,.6,edge_ao=.65,edge_wear=.6),
    polygon('harness left bevel',[[165,140],[171,139],[191,159],[199,169],[191,173],[180,161]],.85,.65,edge_ao=.68,edge_wear=.55),
    polygon('harness cross strap bevel',[[162,175],[184,179],[224,162],[230,166],[232,177],[198,192],[179,189],[163,187]],.8,.65,edge_ao=.68,edge_wear=.55),
    line('harness folded edge',[[173,143],[186,156],[192,168]],.65,-.65,.4,cavity=.65),
    line('harness transverse joint',[[170,179],[181,182],[219,168]],.65,-.65,.4,cavity=.65),
    shape('chest clasp raised frame',[23,234,53,252],1.15,.7,edge_ao=.6,edge_wear=.75),
    shape('chest clasp inset',[30,239,48,251],-1.2,.6,cavity=.58,roughness=.42),
]
for x in [6,12,56,62,69,75]:
    L.append(shape('boot collar rib',[x,131,x+3,157],.65,.5,edge_ao=.76,edge_width=.5,edge_wear=.5))
for x,y in [(27,145),(40,145),(167,142),(187,139),(173,179),(179,187),(196,186),(208,178)]:
    L.append(shape('recessed harness fastener',[x-2,y-2,x+2,y+2],-.6,.35,kind='ellipse',cavity=.6,roughness=.43))
# Cloth at shoulder/sides: weave and seams are distinct from leather gloves.
L.append(polygon('shoulder woven panel',[[193,3],[235,37],[226,58],[214,73],[202,65],[200,49],[173,41]],.25,.8,
                 finish='woven_cloth',metalness=0,edge_ao=.8,edge_width=.7))
for points in [[[176,39],[193,25],[201,16]],[[195,45],[207,28],[211,23]]]:
    L.append(line('woven panel stitching',points,.45,.25,.25,roughness=.74))
# Fine raised stitches on cuff seams; reuse this scale on matching leather kit.
for x,y in [(125,9),(115,119)]:
    for dx in range(0,28,3):
        L.append(line('cuff stitch',[[x+dx,y],[x+dx+1.2,y+.7]],.3,.16,.15,roughness=.7))

# Mechanical islands and thumb/cuff islands are neighbours in this compact UV
# atlas. Restrict hardware relief to the inspected alloy mask: without this,
# a few harness fastener recesses leaked into the glove seen from the side.
for layer in L:
    label=layer['label'].lower()
    hardware=any(term in label for term in ['harness','chest clasp','boot collar','boot shin'])
    layer['scope_channel']=1 if hardware else 0

for target in recipes['targets']:
    if target['model'] not in ['ranger','ratchet']:continue
    target['state']='Fitted depth revision; native visual comparison required; full all-moby pass remains incomplete'
    if target['model']=='ratchet' and target['stem']!=body_stem:
        for l in target['surface_layers']:
            if 'height' in l:
                l['height']=l.setdefault('pre_depth_height',l['height'])*1.65
                l.update(edge_ao=.7,edge_width=.9,edge_wear=.6)
recipes['channel_contract']['normal']='Linear tangent XYZ; A retains the original explicit fur mask (zero elsewhere)'
recipes['channel_contract']['orm']='Linear R=AO G=roughness B=metalness A=explicit lens catchlight mask'
RECIPE.write_text(json.dumps(recipes,indent=2)+'\n')
print({t['stem'][:8]:len(t['surface_layers']) for t in recipes['targets'] if t['model'] in ['ranger','ratchet']})
