"""Optional explicit emissive atlas and restrained native glow in the sandbox."""
from pathlib import Path
import shutil

HERE=Path(__file__).resolve().parent
SRC=HERE.parents[1]/'captures/rift-texture-review/viewer-build/src'
def once(text,old,new):
    if new in text:return text
    if text.count(old)!=1:raise ValueError('Emission hook changed: '+old[:80])
    return text.replace(old,new,1)
def apply():
    path=SRC/'texnames.rs';s=path.read_text()
    s=once(s,'if r.pbr { 3.0 } else if r.region.is_some()',
           'if r.pbr { if r.slots.len()>2 {4.0} else {3.0} } else if r.region.is_some()')
    s=once(s,'        if lines.next() != Some(header) {',
        '''        let actual_header=lines.next();
        if actual_header != Some(header) && !(file=="pbr.tsv" && actual_header==Some("stem\\tnormal_map\\torm_map\\temissive_map")) {''')
    s=once(s,'("pbr.tsv", [stem, normal_map, orm_map]) => {',
        '("pbr.tsv", [stem, _normal_map, _orm_map, rest @ ..]) if rest.len()<=1 => {')
    s=once(s,'                    for map in [normal_map, orm_map] {',
        '''                    for (index,map) in f[1..].iter().enumerate() {
                        if index==2 && (map.is_empty() || *map=="-") {continue;}''')
    path.write_text(s)
    path=SRC/'lighting.rs';s=path.read_text()
    s=once(s,'metal: f32, ao: f32 };','metal: f32, ao: f32, emission: vec3<f32>, glow: f32 };')
    if 'm2.rgb*4.0' not in s:
        s=once(s,'0.0,0.0,0.0,metal,m1.r);',
            '0.0,0.0,0.0,metal,m1.r,select(vec3<f32>(0.0),m2.rgb*4.0,mat.rift.w>3.5),select(0.0,m2.a,mat.rift.w>3.5));')
    s=once(s,'dot(ws, mat.metals),1.0);','dot(ws, mat.metals),1.0,vec3<f32>(0.0),0.0);')
    s=once(s,'material_to_srgb(direct+indirect)','material_to_srgb(direct+indirect+m.emission)')
    path.write_text(s)
    path=SRC/'objects.rs';s=path.read_text()
    start=s.index('fn gs(i: VOut)');end=s.index('// The pixel as the GS writes',start)
    block=once(s[start:end],'    let m = material_at(uv);','''    var m = material_at(uv);
    if ((i.light & (1u<<19u))!=0u) {m.emission=vec3<f32>(0.0);m.glow=0.0;}''')
    s=s[:start]+block+s[end:]
    sampling='    // A Rift Apart surface: the texel times its material\'s grain, its alpha the texture\'s; lit per pixel, the light pass with'
    s=once(s,sampling,'''    // Explicit authored source-face emitters, for shared-UV cannon cells.
    if ((i.light & (1u<<20u))!=0u) {
        m.emission=material_to_linear(t.rgb/255.0)*1.8;m.glow=0.7;m.metal=0.0;m.ao=1.0;
    }
'''+sampling)
    marker='    if (u32(view.eye.w)==6u) { return vec4<f32>(round(i.colour.rgb*255.0),255.0); }'
    s=once(s,marker,'''    // Self-illumination also applies to the source's unlit optic overlays.
    // The explicit mask excludes yellow/olive armour; no hue classifier.
    if (m.glow>0.0) {
        c=vec4<f32>(max(c.rgb,255.0*min(material_to_srgb(m.emission),vec3<f32>(1.0))),max(c.a,128.0+127.0*m.glow));
    }
    if (u32(view.eye.w)==7u) {return vec4<f32>(255.0*min(material_to_srgb(m.emission),vec3<f32>(1.0)),t.a);}
'''+marker)
    block='    if (u32(view.eye.w)==7u) {return vec4<f32>(255.0*min(material_to_srgb(m.emission),vec3<f32>(1.0)),t.a);}'
    s=once(s,block,'    if ((i.light & (1u<<19u))!=0u) {c.a=min(c.a,128.0);}\n'+block)
    path.write_text(s)
    path=SRC/'native.rs';s=path.read_text()
    s=once(s,'&pic.tv, &pic.dv, &pic.depth, &self.ao,','&pic.tv, &pic.dv, &pic.tex, &pic.depth, &self.ao,')
    path.write_text(s)
    path=SRC/'lib.rs';s=path.read_text()
    if 'mod review_emission;' not in s:path.write_text(s+'\nmod review_emission;\n')
    shutil.copy2(HERE/'review_emission.rs',SRC/'review_emission.rs')
if __name__=='__main__':apply()
