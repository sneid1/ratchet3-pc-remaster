"""Inspection-only hooks in the frozen renderer; never edits port/ sources."""
from pathlib import Path


def apply(src):
    native=src/'native.rs'
    text=native.read_text()
    text=text.replace('&pic.tv, &pic.dv, &pic.view, &pic.view_bg, self.size)?;',
                      '&pic.tv, &pic.dv, &pic.depth, &self.ao, &pic.view, &pic.view_bg, self.size)?;')
    text=text.replace('eye: [cam.eye[0], cam.eye[1], cam.eye[2], 0.0],',
                      'eye: [cam.eye[0], cam.eye[1], cam.eye[2], crate::material_review::channel()],')
    native.write_text(text)
    objects=src/'objects.rs'
    text=objects.read_text()
    text=text.replace('vec3<f32>(255.0), t.a); }','vec3<f32>(m.ao * 255.0), t.a); }')
    marker='    // Review channel output from the actual sampled material.\n'
    if marker not in text:
        anchor='    let rgb = select(floor((c.rgb * i.f + view.fog_colour.rgb * (255.0 - i.f)) / 256.0), c.rgb, i.f > 255.5);'
        if text.count(anchor)!=1:raise RuntimeError('Character channel hook changed')
        debug=marker+'''    let channel = u32(view.eye.w);
    if (channel == 1u) { return vec4<f32>(t.rgb, t.a); }
    if (channel == 2u) { return vec4<f32>(vec3<f32>(material_metal(m) * 255.0), t.a); }
    if (channel == 3u) { return vec4<f32>(vec3<f32>((1.0 - m.map.z) * 255.0), t.a); }
    if (channel == 4u) { return vec4<f32>((n * 0.5 + 0.5) * 255.0, t.a); }
    if (channel == 5u) { return vec4<f32>(vec3<f32>(m.ao * 255.0), t.a); }
'''
        text=text.replace(anchor,debug+anchor)
    objects.write_text(text)
    text=objects.read_text()
    anchor='    // Review channel output from the actual sampled material.\n'
    diagnostic='    if (u32(view.eye.w)==6u) { return vec4<f32>(round(i.colour.rgb*255.0),255.0); }\n'
    if diagnostic not in text:text=text.replace(anchor,diagnostic+anchor)
    # Fur shells only in the lit pictures: channel 0 and channel 8 ("game", lit without the review-only PBR response).
    text=text.replace('    if (u32(view.eye.w)>0u) { discard; }\n','    if (u32(view.eye.w)>0u && u32(view.eye.w)!=8u) { discard; }\n')
    text=text.replace('@fragment fn fs_fur(i: VOut) -> FOut {\n', '@fragment fn fs_fur(i: VOut) -> FOut {\n    if (u32(view.eye.w)>0u && u32(view.eye.w)!=8u) { discard; }\n') if 'if (u32(view.eye.w)>0u && u32(view.eye.w)!=8u) { discard; }' not in text else text
    objects.write_text(text)


if __name__=='__main__':
    apply(Path(__file__).resolve().parents[2]/'captures/rift-texture-review/viewer-build/src')
