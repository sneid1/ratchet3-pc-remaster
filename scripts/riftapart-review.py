"""Build an isolated, complete character-material candidate from a baseline pack.

No player-profile writes. Original atlases/colors and good existing replacements
are retained. Review-specific maps derive from inspected Rift Apart material maps;
all changed files and source hashes are recorded beside the candidate.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import shutil
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "captures/rift-texture-review"
RATCHET = "9a6d927dedfae75e-284e78b48088f99a-00002213"

# source packed material, normal multiplier, grain multiplier, gloss scale,
# gloss bounds, reflectance root, specular strength, metalness.
MATERIALS = {
    "review_ty_skin": ("detail_skinreptile_02", .22, .13, 0, (24,24), .20, .45, 0),
    "review_satin_alloy": ("steel_001", .2, 0, .62, (105,150), .52, .6, 1),
    "review_dark_alloy": ("steel_carbon_001", .25, .10, .65, (65,110), .38, .5, .8),
    "review_lens": ("plastic_004", 0, 0, 0, (205,205), .20, .65, 0),
    "review_soft_leather": ("detail_leather_02", .32, .15, 0, (30,30), .20, .35, 0),
}


def digest(path):
    h=hashlib.sha256()
    with open(path,"rb") as stream:
        for chunk in iter(lambda:stream.read(1024*1024),b""):
            h.update(chunk)
    return h.hexdigest()


def read_tsv(path):
    lines=[line for line in path.read_text(encoding="utf-8").splitlines() if line and not line.startswith("#")]
    reader=csv.DictReader(lines,delimiter="\t")
    return reader.fieldnames,list(reader)


def write_tsv(path, fields, rows):
    with path.open("w",encoding="utf-8",newline="") as stream:
        stream.write("# Isolated character review candidate; scripts/riftapart-review.py\n")
        writer=csv.DictWriter(stream,fields,delimiter="\t",lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)


def slot(row, index, material, tiles=None, catch=None):
    suffix=str(index) if index else ""
    row["material"+suffix]=material
    if material in MATERIALS:
        source,n,g,gs,limits,reflectance,specular,metal=MATERIALS[material]
        for key,value in (("reflectance",reflectance),("specular",specular),("metal",metal),("grain",1),("fur",0)):
            row[key+suffix]=str(value)
    if tiles is not None:
        row["tiles_u"+suffix]=row["tiles_v"+suffix]=str(tiles)
    if catch is not None:
        row["catchlight"+suffix]=str(int(catch))


def build(args):
    baseline=args.baseline.resolve()
    candidate=args.out.resolve()
    if candidate == baseline or baseline in candidate.parents:
        raise ValueError("Candidate must be separate from baseline")
    if not candidate.is_relative_to(ROOT):
        raise ValueError("Candidate must stay in this workspace")
    # Each run restores the baseline files, so transformations never accumulate.
    shutil.copytree(baseline,candidate,dirs_exist_ok=True)
    provenance=[]
    for name,(source,norm,grain,scale,(lo,hi),_,_,_) in MATERIALS.items():
        path=baseline/"materials"/(source+".png")
        image=np.asarray(Image.open(path).convert("RGBA").resize((512,512),Image.Resampling.LANCZOS),dtype=np.float32)
        image[...,:2]=127.5+(image[...,:2]-127.5)*norm
        image[...,2]=np.clip(image[...,2]*scale,lo,hi)
        image[...,3]=102+(image[...,3]-102)*grain
        Image.fromarray(np.clip(image+.5,0,255).astype(np.uint8)).save(candidate/"materials"/(name+".png"))
        provenance.append({"output":"materials/"+name+".png","source":str(path),"sha256":digest(path),
                           "normal_scale":norm,"grain_scale":grain,"gloss_scale":scale,"gloss_bounds":[lo,hi],"size":512})

    # A plain painted area from the inspected architectural tile: only its fine
    # surface texture, never its panels, bolts or broad hexagonal markings.
    source=args.export/"metal/steel_painted_panels_002_n.png"
    # This interior is within the large lower-left panel, away from its seams.
    crop=(256,1088,640,1472)
    nm=np.asarray(Image.open(source).convert("RGB").crop(crop).resize((256,256),Image.Resampling.LANCZOS),dtype=np.float32)
    # Mirror the normal with its corresponding slope sign, making the crop tile.
    x=nm[...,:2]-127.5
    right=x[:,::-1].copy();right[...,0]*=-1
    top=np.concatenate([x,right],axis=1)
    bottom=top[::-1].copy();bottom[...,1]*=-1
    xy=127.5+np.concatenate([top,bottom],axis=0)*.25
    packed=np.empty((512,512,4),np.uint8)
    packed[...,:2]=np.clip(xy+.5,0,255)
    packed[...,2]=95
    packed[...,3]=102
    Image.fromarray(packed).save(candidate/"materials/review_painted_metal.png")
    provenance.append({"output":"materials/review_painted_metal.png","source":str(source),"sha256":digest(source),
                       "crop":crop,"normal_scale":.25,"gloss":95,"grain":1,"mirrored_with_normal_signs":True})

    fields,rows=read_tsv(baseline/"regions.tsv")
    _,descriptions=read_tsv(ROOT/"textures/riftapart-regions.tsv")
    descriptions={r["stem"]:str(r) for r in descriptions}
    changed=[]
    for row in rows:
        stem=row["stem"]
        text=stem+" "+descriptions.get(stem,"")
        ratchet=stem==RATCHET
        suit=stem.startswith("4d392437d864304a")
        helmet=stem.startswith("f76fd4297c61a9d7")
        clank=stem.startswith(("407dd834b00a78e5","5e837b1a0322e50f"))
        small=any(p in text for p in ("1327fb77a992df53","47a27d18e4242bb1","9fef15fa081ab9a4"))
        ranger=any(p in text for p in ("efa796ee9a43f16c","f29031bb271eb6f2"))
        mech=any(p in text for p in ("e7a10620a3d903e5","211295e6748c07d"))
        if not any((ratchet,suit,helmet,clank,small,ranger,mech)):
            continue
        before=dict(row)
        row["normal"]="1"
        for index in range(4):
            suffix=str(index) if index else ""
            material=row["material"+suffix]
            if material=="-" or material=="fur":
                continue
            if ratchet or suit:
                if material=="detail_leather_02":slot(row,index,"review_soft_leather",tiles=2)
                elif material in ("steel_001","chromium_001"):slot(row,index,"review_satin_alloy",tiles=2)
                elif row["catchlight"+suffix]=="1":slot(row,index,"review_lens",tiles=1,catch=True)
            elif helmet:
                if material!="plastic_004":slot(row,index,"review_satin_alloy",tiles=2)
            elif clank:
                slot(row,index,"review_lens" if row["catchlight"+suffix]=="1" else "review_satin_alloy",tiles=1)
                if row["catchlight"+suffix]!="1":
                    # The legacy atlas already contains bright painted lighting;
                    # a full additive metal response clips it under Veldin's sun.
                    row["reflectance"+suffix]="0.22"
                    row["specular"+suffix]="0.18"
                    row["metal"+suffix]="0.25"
            elif material.startswith("detail_skin"):
                slot(row,index,"review_ty_skin",tiles=6)
            elif material=="plastic_004":
                slot(row,index,"review_lens",tiles=1)
            elif ranger or mech:
                if material in ("mol_arch_painted_metal_04","steel_painted_panels_002","anodized_metal_01"):
                    slot(row,index,"review_painted_metal",tiles=4)
                    for key,value in (("reflectance",.2),("grain",1),("specular",.55),("metal",.1)):
                        row[key+suffix]=str(value)
                else:slot(row,index,"review_dark_alloy",tiles=4)
        changed.append({"stem":stem,"before":before,"after":dict(row)})

    # Correct pale fur classified as metal by the old color-cluster assignment.
    # Coordinates are on the inspected original 256px UYA atlas, not the RA UVs.
    row=next(r for r in rows if r["stem"]==RATCHET)
    original=args.original_ratchet
    low=Image.open(original).convert("RGBA")
    if low.size!=(256,256):raise ValueError("Expected the inspected 256x256 original Ratchet atlas")
    size=2048
    original_hd=low.resize((size,size),Image.Resampling.LANCZOS)
    rgb=np.asarray(original_hd,dtype=np.float32)
    spatial=Image.new("L",(256,256))
    draw=ImageDraw.Draw(spatial)
    draw.polygon([(67,0),(115,0),(117,125),(50,127),(0,108),(0,80),(43,70),(67,54)],fill=255)
    draw.polygon([(177,66),(204,76),(254,84),(255,120),(230,139),(209,153),(186,138),(173,121)],fill=255)
    draw.polygon([(215,0),(256,0),(256,34),(246,36)],fill=255)
    area=np.asarray(spatial.resize((size,size),Image.Resampling.NEAREST))>0
    fur=area&(rgb[...,0]>=rgb[...,1]*.95)&(rgb[...,1]>rgb[...,2]*1.16)
    weights=np.asarray(Image.open(baseline/"regions"/row["map"]).convert("RGBA").resize((size,size),Image.Resampling.NEAREST)).copy()
    fur &= weights[...,3]<128 # Preserve the fitted eye region.
    weights[fur]=[0,0,255,0]
    # Four independent material weights, not colour with transparency. RGBA
    # filtering would premultiply by the lens weight and erase leather/metal/fur.
    Image.merge('RGBA',tuple(channel.resize((1024,1024),Image.Resampling.BOX)
                            for channel in Image.fromarray(weights).split())).save(candidate/"regions"/row["map"])

    # Use real painted fur microstructure, with source colors and low-frequency
    # shading removed. Do not interpret the RA strand-control texture as diffuse.
    source=args.export/"more/characters/hero/hero_ratchet/textures/hero_ratchet_head_c.png"
    crop=(450,300,670,780)
    detail=Image.open(source).convert("L").crop(crop)
    local=np.asarray(detail,dtype=np.float32)
    smooth=np.asarray(detail.filter(ImageFilter.GaussianBlur(5)),dtype=np.float32)
    ratio=np.clip((local+8)/(smooth+8),.82,1.18)
    # A mirrored, fine repeat; preserves the UYA face, stripes and palette.
    top=np.concatenate([ratio,ratio[:,::-1]],axis=1)
    tile=np.concatenate([top,top[::-1]],axis=0)
    detail=np.tile(tile,(int(np.ceil(size/tile.shape[0])),int(np.ceil(size/tile.shape[1]))))[:size,:size]
    installed=np.asarray(Image.open(baseline/"replacements"/(RATCHET+".png")).convert("RGBA")).copy()
    sharpen=np.asarray(original_hd.filter(ImageFilter.UnsharpMask(radius=1.2,percent=65,threshold=3)),dtype=np.float32)
    improved=np.clip(sharpen[...,:3]*detail[...,None],0,255)
    installed[fur,:3]=improved[fur].astype(np.uint8)
    Image.fromarray(installed).save(candidate/"replacements"/(RATCHET+".png"))
    provenance.append({"output":"replacements/"+RATCHET+".png","source":str(source),"sha256":digest(source),
                       "original_uv_atlas":str(original),"original_sha256":digest(original),"crop":crop,
                       "detail_ratio_bounds":[.82,1.18],"control_map_used":False,"outside_fur_unchanged":True})
    write_tsv(candidate/"regions.tsv",fields,rows)

    # Actual file comparison, including every unmodified file in this complete pack.
    manifest=[]
    for path in sorted(candidate.rglob("*")):
        if not path.is_file():continue
        rel=path.relative_to(candidate).as_posix()
        old=baseline/rel
        new_hash=digest(path)
        old_hash=digest(old) if old.is_file() else None
        manifest.append({"path":rel,"bytes":path.stat().st_size,"sha256":new_hash,
                         "baseline_sha256":old_hash,"changed":new_hash!=old_hash})
    report={"baseline":str(baseline),"candidate":str(candidate),"provenance":provenance,"regions":changed,"files":manifest}
    (candidate.parent/"manifest.json").write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")
    print(f"Candidate: {candidate}; {len(changed)} region rows; {sum(x['changed'] for x in manifest)} changed/new files",flush=True)


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline",type=Path,default=OUT/"profile/texture_replacements/0-riftapart")
    parser.add_argument("--out",type=Path,default=OUT/"candidate/0-riftapart")
    parser.add_argument("--export",type=Path,default=Path("C:/riftapart-export"))
    parser.add_argument("--original-ratchet",type=Path,default=Path.home()/"Games/Ratchet and Clank 3/texture_dumps/b9283d76f354a794-284e78b48088f99a-0000221b.png")
    build(parser.parse_args())
