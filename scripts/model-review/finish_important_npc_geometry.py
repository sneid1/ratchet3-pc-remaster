"""Run existing original-mesh finishing stages for the important NPC batch."""
import argparse
import runpy
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE.parents[1] / 'captures/rift-texture-review'
p = argparse.ArgumentParser()
p.add_argument('--model', required=True, choices=['source_c7662', 'source_c7109', 'source_c7969'])
a = p.parse_args(sys.argv[sys.argv.index('--') + 1:])
sys.path.insert(0, str(HERE))

def stage(script, *args):
    sys.argv = ['blender', '--', *map(str, args)]
    runpy.run_path(str(HERE / script), run_name='__main__')

stage('smooth_character_surfaces.py', '--model', a.model, '--blend', OUT/'blender'/(a.model+'.blend'), '--angle', 35)
stage('refine_models.py', '--model', a.model, '--blend', OUT/'blender/smooth'/(a.model+'.blend'),
      '--segments', 2, '--strength', .6, '--method', 'adaptive', '--max-ratio', 1.5)
stage('blender_roundtrip.py', '--model', a.model, '--blend', OUT/'blender/refined'/(a.model+'.blend'))
stage('sync_blender_materials.py', '--model', a.model, '--delivery', OUT/'deliverables/imagegen-layers/models')
