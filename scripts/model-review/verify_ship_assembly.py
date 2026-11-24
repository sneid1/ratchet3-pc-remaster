"""Record the original ship kit's scale and handedness from existing saved RAM.

No game process is launched. The known L01 field 1001 records are checked against
their class IDs, class-header scales, common origin and relative orientation.
Small animation differences are reported; the art asset uses canonical bind axes.
"""
import hashlib,json,struct
from pathlib import Path
import numpy as np

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'captures/rift-texture-review'
path=OUT/'poses-L01/ram01001.bin';ram=path.read_bytes()
def record(address):
    cls=struct.unpack_from('<h',ram,address+0xaa)[0]
    ptr=struct.unpack_from('<I',ram,address+0x24)[0]
    return dict(address=hex(address),class_id=cls,
        scale=struct.unpack_from('<f',ram,address+0x2c)[0],
        class_scale=struct.unpack_from('<f',ram,ptr+0x24)[0],
        origin=list(struct.unpack_from('<3f',ram,address+0x10)),
        axes=np.array(struct.unpack_from('<12f',ram,address+0xc0)).reshape(3,4)[:,:3])
hull=record(0x1d63d00);assert hull['class_id']==3412
parts=[]
for address,cls,mirror in [(0x1d63f00,4308,False),(0x1d64000,4308,True),
                          (0x1d64100,4312,False),(0x1d64200,4314,False),(0x1d64300,4314,True)]:
    part=record(address);assert part['class_id']==cls
    assert abs(part['scale']-part['class_scale'])<1e-7
    relative=part.pop('axes')@hull['axes'].T
    expected=np.diag([1,-1 if mirror else 1,1])
    error=float(np.max(np.abs(relative-expected)))
    origin_error=float(np.linalg.norm(np.array(part['origin'])-hull['origin']))
    assert error<.001 and origin_error<.01
    part.update(mirror_y=mirror,scale_relative_to_hull=part['scale']/hull['scale'],
                observed_relative_axes=relative.tolist(),maximum_axis_error=error,origin_error=origin_error)
    parts.append(part)
hull['axes']=hull['axes'].tolist()
report=dict(source=str(path.relative_to(ROOT)),sha256=hashlib.sha256(ram).hexdigest(),
    hull=hull,parts=parts,excluded='4318 and 4325 are projectile meshes, not structural parts.',
    canonicalization='Use exact class-header scale ratios and mirror Y; remove sub-millimetre animation-axis/origin differences for the bind-space art assembly.')
(OUT/'ship-assembly.json').write_text(json.dumps(report,indent=2)+'\n')
# The runtime class table is sorted differently from the disc's source slots.
# Resolve by ID, never by the disc slot. These addresses are documented in
# uyafmt::levelcode::GadgetAddrs for the saved Veldin capture.
kit=[]
for slot in range(400):
    cls=struct.unpack_from('<h',ram,0x1fda70+slot*2)[0]
    if cls not in (4308,4309,4310,4312,4314):continue
    pointer=struct.unpack_from('<I',ram,0x1fd200+slot*4)[0]
    assert 0x100000<=pointer<len(ram)-0x48
    scale=struct.unpack_from('<f',ram,pointer+0x24)[0]
    assert .1<scale<.3
    kit.append(dict(class_id=cls,runtime_slot=slot,pointer=hex(pointer),scale=scale))
assert sorted(r['class_id'] for r in kit)==[4308,4309,4310,4312,4314]
(OUT/'ship-kit-scales.json').write_text(json.dumps(dict(source=report['source'],sha256=report['sha256'],classes=kit),indent=2)+'\n')
print('Verified hull and five original ship kit instances against saved RAM.')
