"""Recover the exact shared normal-angle table from existing saved skin jobs.

Gameplay RAM dumps may omit boot ranges. Cross-check independent level captures
instead of decoding normals from those zero-filled ranges. No game execution.
"""
import hashlib,json,struct
from pathlib import Path
OUT=Path(__file__).resolve().parents[2]/'captures/rift-texture-review'

def tables(path):
    b=path.read_bytes();offset=16
    magic,version,_,count=struct.unpack_from('<4I',b)
    if magic!=int.from_bytes(b'UYAJ','little') or version!=3:raise ValueError('Expected captured version 3 skin jobs')
    for _ in range(count):
        kind,_,_,n=struct.unpack_from('<4I',b,offset);offset+=16
        if kind==1:
            offset+=n*4;m,=struct.unpack_from('<I',b,offset);offset+=4+m*8
            k,=struct.unpack_from('<I',b,offset);offset+=4+k*16
        elif kind==2:offset+=n*16
        else:raise ValueError('Unknown captured job')
    count,=struct.unpack_from('<I',b,offset);offset+=4
    for _ in range(count):
        lo,hi,n=struct.unpack_from('<3I',b,offset);offset+=12+n*12
        size,=struct.unpack_from('<I',b,offset);offset+=4+size
        size,=struct.unpack_from('<I',b,offset);offset+=4
        raw=b[offset:offset+size];offset+=size
        if size!=2048:raise ValueError('Incomplete normal-angle table')
        values=struct.unpack('<512f',raw)
        if max(abs(values[i]**2+values[i+1]**2-1) for i in range(0,512,2))>1e-5:
            raise ValueError('Invalid normal-angle table')
        yield raw,lo+(hi<<32)
    if offset!=len(b):raise ValueError('Trailing job data')

def extract():
    reference=None;sources=[]
    for level in [1,2,3]:
        path=next(iter(sorted((OUT/f'poses-L{level:02}').glob('jobs*.bin'))))
        count=0
        for raw,field in tables(path):
            if reference is None:reference=raw
            if raw!=reference:raise ValueError('Captured normal-angle tables disagree')
            count+=1
        if not count:raise ValueError('No saved skin jobs')
        sources.append(dict(path=str(path),skin_fields=count))
    (OUT/'source-skin-trig.bin').write_bytes(reference)
    (OUT/'source-skin-trig.json').write_text(json.dumps(dict(sources=sources,sha256=hashlib.sha256(reference).hexdigest(),
        scope='Exact original normal-angle table, identical across saved skin jobs in three levels. No new capture.'),indent=2)+'\n')

if __name__=='__main__':extract()
