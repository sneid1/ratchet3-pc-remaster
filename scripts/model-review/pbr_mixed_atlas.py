"""Retain explicit fur and lens regions when a mixed atlas gets fitted PBR maps."""
from emissive_shader import SRC, once

def apply():
    path=SRC/'texnames.rs';s=path.read_text()
    # Independent of atlas sampling scale: original fur tiles must not become
    # one giant strand pattern when normal/ORM maps use one atlas repeat.
    s=once(s,'    pbr: bool,','    pbr: bool,\n    fur_tiles: [f32;2],') if s.count('    pbr: bool,')==1 else s
    if '    fur_tiles: [f32;2],' not in s:
        assert s.count('    pbr: bool,')==2
        s=s.replace('    pbr: bool,','    pbr: bool,\n    fur_tiles: [f32;2],')
    s=once(s,'normal:1.0, pbr:true })','normal:1.0, pbr:true, fur_tiles:[0.0;2] })')
    s=once(s,'normal: num(normal)?, pbr: false })','normal: num(normal)?, pbr: false, fur_tiles:[0.0;2] })') if s.count('normal: num(normal)?, pbr: false })')==1 else s
    s=s.replace('normal: num(normal)?, pbr: false })','normal: num(normal)?, pbr: false, fur_tiles:[0.0;2] })')
    s=once(s,'normal: s.normal, pbr: s.pbr }))','normal: s.normal, pbr: s.pbr, fur_tiles:s.fur_tiles }))')
    s=once(s,'[t(3)[0], t(3)[1], 0.0, 0.0]','[t(3)[0], t(3)[1], r.fur_tiles[0], r.fur_tiles[1]]')
    s=once(s,'let (stem, s) = match (file, &f[..]) {','let (stem, mut s) = match (file, &f[..]) {')
    anchor='            // A PBR atlas deliberately replaces the old tiled material row.'
    if 's.slots[0].fur=old.slots' not in s:s=once(s,anchor,'''            // Fitted mixed atlases keep the original explicit shell length and
            // lens strength. Their per-pixel weights are baked in normal/ORM A.
            if s.pbr {
                if let Some(old)=out.get(&n) {
                    let old:&RiftSurface=old;
                    s.slots[0].fur=old.slots.iter().map(|slot|slot.fur).fold(0.0f32,f32::max);
                    s.slots[0].catchlight=old.slots.iter().map(|slot|slot.catchlight).fold(0.0f32,f32::max);
                }
            }
'''+anchor)
    path.write_text(s)
    # Also works when updating a snapshot that already has the mask hook.
    s=once(s,'                    let old:&RiftSurface=old;',
           '                    let old:&RiftSurface=old;\n                    s.fur_tiles=old.slots.iter().find(|slot|slot.fur>0.0).map_or([0.0;2],|slot|slot.tiles);')
    path.write_text(s)
    path=SRC/'lighting.rs';s=path.read_text()
    s=once(s,'0.0,0.0,0.0,metal,m1.r,select(vec3<f32>(0.0),m2.rgb*4.0,mat.rift.w>3.5)',
           'mat.furs.x*m0.a,select(0.0,m0.a,mat.furs.x>0.0),mat.catches.x*m1.a,metal,m1.r,select(vec3<f32>(0.0),m2.rgb*4.0,mat.rift.w>3.5)')
    path.write_text(s)
    path=SRC/'objects.rs';s=path.read_text()
    s=once(s,'    let tiles = mat.rift.xy * f.x + mat.tiles.xy * f.y + mat.tiles.zw * f.z + mat.tiles3.xy * f.w;',
        '    let region_tiles = mat.rift.xy * f.x + mat.tiles.xy * f.y + mat.tiles.zw * f.z + mat.tiles3.xy * f.w;\n    let tiles=select(region_tiles,mat.tiles3.zw,mat.rift.w>2.5);')
    path.write_text(s)

if __name__=='__main__':apply()
