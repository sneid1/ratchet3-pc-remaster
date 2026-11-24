"""Save each source model through Blender so relative image paths stay valid."""
import bpy,json
from pathlib import Path
OUT=Path(__file__).resolve().parents[2]/'captures/rift-texture-review'
assets=json.loads((OUT/'imagegen-layer-assets.json').read_text())
for key in assets:
    bpy.ops.wm.open_mainfile(filepath=str(OUT/'blender/imagegen-roster.blend'))
    for image in bpy.data.images:
        if image.filepath:image.filepath=bpy.path.abspath(image.filepath)
    obj=bpy.data.objects['UYA_'+key+'_captured_pose']
    for other in list(bpy.data.objects):
        if other.type=='MESH' and other!=obj:bpy.data.objects.remove(other,do_unlink=True)
    obj.hide_set(False);obj.hide_render=False
    bpy.context.scene['Source pose']='Original bind-space model; no animation/rig approval implied'
    bpy.ops.wm.save_as_mainfile(filepath=str(OUT/'blender/edited'/(key+'.blend')))
