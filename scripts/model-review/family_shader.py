"""Use the sandbox PBR response for mapped material families, including terrain."""
from pathlib import Path
SRC=Path(__file__).resolve().parents[2]/'captures/rift-texture-review/viewer-build/src'
def apply():
    path=SRC/'objects.rs';text=path.read_text()
    old='        if (mat.rift.w > 2.5) {\n            let nx=dpdx(n);'
    # Channel 8 ("game") skips this review-only response: the game lights fitted atlases with its own light pass.
    new='        if (mat.rift.w > 0.5 && u32(view.eye.w) != 8u) {\n            let nx=dpdx(n);'
    text=text.replace('        if (mat.rift.w > 0.5 && m.furw < 0.01) {\n            let nx=dpdx(n);',new)
    text=text.replace('        if (mat.rift.w > 0.5) {\n            let nx=dpdx(n);',new)
    if old in text:text=text.replace(old,new)
    elif new not in text:raise ValueError('Object PBR family hook changed')
    # PS2 skin/rigid lighting stores inward normals and incoming light rays.
    # Their diffuse dot product is unchanged by reversing both, but the GGX
    # view/half vectors require outward normals and directions toward lights.
    old_call='c=vec4<f32>(material_pbr(t.rgb/255.0,n,e,l.c0.rgb*k*i.ln.w,l.d0,l.c1.rgb*k*i.ln.w,l.d1,ambient,m,normal_variance),c.a);'
    new_call='''let physical_sign=select(1.0,-1.0,mode>=2u);
            c=vec4<f32>(material_pbr(t.rgb/255.0,n*physical_sign,e,l.c0.rgb*k*i.ln.w,l.d0*physical_sign,l.c1.rgb*k*i.ln.w,l.d1*physical_sign,ambient,m,normal_variance),c.a);'''
    if old_call in text:text=text.replace(old_call,new_call)
    elif new_call not in text:raise ValueError('Moby physical-normal hook changed')
    old_normal='    let n = select(renormal(i.ln.xyz), material_normal(i.ln.xyz, dp1, dp2, duv1, duv2, m), shaded);'
    new_normal='''    var n = select(renormal(i.ln.xyz), material_normal(i.ln.xyz, dp1, dp2, duv1, duv2, m), shaded);
    // Sandbox shrub meshes are uncullled, two-sided surfaces. Their original
    // strips did not encode an outward side; face the physical normal toward
    // the visible side without changing the source topology or cutout alpha.
    if ((i.light & (1u<<18u))!=0u && dot(n,view.eye.xyz-i.wpos)<0.0) {n=-n;}'''
    if old_normal in text:text=text.replace(old_normal,new_normal)
    elif new_normal not in text:raise ValueError('Two-sided shrub normal hook changed')
    old_ambient='            let ambient=max(i.colour.rgb*(255.0/128.0)-k*(l.c0.rgb*i.ls.x+l.c1.rgb*i.ls.y),vec3<f32>(0.0));'
    new_ambient='''            var ambient=max(i.colour.rgb*(255.0/128.0)-k*(l.c0.rgb*i.ls.x+l.c1.rgb*i.ls.y),vec3<f32>(0.0));
            if ((i.light & (1u<<18u))!=0u) {
                // Reconstruct the ambient share using the SAME visible-side
                // reference normal as the two-sided BRDF. Keeping the old
                // inward-side terms produces bright triangles at strip joins.
                var reference_n=renormal(i.ln.xyz);
                if (dot(reference_n,e)<0.0) {reference_n=-reference_n;}
                let terms=vec2<f32>(max(dot(reference_n,l.d0),0.0),max(dot(reference_n,l.d1),0.0));
                ambient=max(i.colour.rgb*(255.0/128.0)-(l.c0.rgb*terms.x+l.c1.rgb*terms.y),vec3<f32>(0.0));
            }'''
    if old_ambient in text:text=text.replace(old_ambient,new_ambient)
    elif new_ambient not in text:raise ValueError('Shrub ambient reconstruction hook changed')
    text=text.replace('if (channel == 4u) { return vec4<f32>((n * 0.5 + 0.5) * 255.0, t.a); }',
        'if (channel == 4u) { let physical_n=n*select(1.0,-1.0,((i.light>>16u)&3u)>=2u); return vec4<f32>((physical_n * 0.5 + 0.5) * 255.0, t.a); }')
    # Native skin normals point into the body: shell displacement must use
    # the physical outward normal, without changing captured diffuse terms.
    start=text.index('@vertex fn vs_moby_fur(')
    end=text.index('fn fur_hash(',start)
    block=text[start:end]
    block=block.replace('let n = renormal(v.ln.xyz);','let n = renormal(v.ln.xyz) * select(1.0,-1.0,((v.light>>16u)&3u)>=2u);')
    text=text[:start]+block+text[end:]
    path.write_text(text)
    path=SRC/'lighting.rs';text=path.read_text()
    start=text.index('    if (rift) {',text.index('fn gs_lit('))
    end=text.index('    return p;',start)
    replacement='''    // Family PBR: source normal/gloss and explicit physical metalness.
    if (rift) {
        let e=renormal(view.eye.xyz-i.wpos);
        var texel:vec4<f32>;
        if (mat.extra.x>0.5) {
            texel=textureSampleGrad(tex,smp,i.uv,dpdx(i.uv),dpdy(i.uv));
        } else {
            let lod=min(log2(i.depth)+mat.qlog+mat.k,mat.last);
            texel=textureSampleLevel(tex,smp,i.uv,lod);
        }
        let base=clamp(texel.rgb*material_grain(m),vec3<f32>(0.0),vec3<f32>(1.0));
        let nx=dpdx(n);let ny=dpdy(n);
        let variance=0.5*(dot(nx,nx)+dot(ny,ny));
        var colour=material_pbr(base,n,e,i.col0.rgb,i.dir0,i.col1.rgb,i.dir1,i.base.rgb/128.0,m,variance);
        if (mat.emit.x>0.0) {colour=max(colour,min(base*255.0*mat.emit.x,vec3<f32>(255.0)));}
        let fogged=floor((colour*i.f+view.fog_colour.rgb*(255.0-i.f))/256.0);
        p=vec4<f32>(fogged,p.a);
    }
'''
    text=text[:start]+replacement+text[end:];path.write_text(text)
if __name__=='__main__':apply()
