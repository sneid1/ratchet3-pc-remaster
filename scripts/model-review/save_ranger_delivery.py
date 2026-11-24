"""Save the current Ranger as a self-contained editable Blender delivery."""
from pathlib import Path
import bpy

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT/'captures/rift-texture-review'
DEST = OUT/'deliverables/ranger-v4'
DEST.mkdir(parents=True, exist_ok=True)
bpy.ops.wm.open_mainfile(filepath=str(OUT/'blender/edited/ranger.blend'))
obj = bpy.data.objects['UYA_ranger_captured_pose']
packed = set()
for mat in obj.data.materials:
    for node in mat.node_tree.nodes:
        if node.type=='TEX_IMAGE' and node.get('review_pbr') and node.image:
            node.image.pack()
            packed.add(node.image.name)
bpy.ops.wm.save_as_mainfile(filepath=str(DEST/'ranger.blend'))
print('Ranger delivery saved with', len(packed), 'embedded material textures.')
