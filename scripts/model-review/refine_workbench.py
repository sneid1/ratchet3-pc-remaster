"""Apply the shared surface correction to existing authored models once."""
import argparse,json,runpy,sys
from pathlib import Path
HERE=Path(__file__).resolve().parent;OUT=HERE.parents[1]/'captures/rift-texture-review'
p=argparse.ArgumentParser();p.add_argument('models',nargs='+');p.add_argument('--method',choices=['adaptive','uniform'],default='adaptive')
p.add_argument('--max-ratio',type=float,default=1.75)
a=p.parse_args(sys.argv[sys.argv.index('--')+1:])
for model in a.models:
    if not model.replace('_','').isalnum():raise ValueError('Invalid model key')
    recipe=json.loads((OUT/'blender/refined'/(model+'.json')).read_text())
    source=OUT/'blender/smooth'/(model+'.blend')
    if not source.is_file():raise ValueError('No existing smoothed original source: '+model)
    sys.argv=['blender','--','--model',model,'--blend',str(source),'--segments',str(recipe['segments']),'--strength',str(recipe['strength']),'--method',a.method,'--max-ratio',str(a.max_ratio)]
    runpy.run_path(str(HERE/'refine_models.py'),run_name='__main__')
    sys.argv=['blender','--','--model',model,'--blend',str(OUT/'blender/refined'/(model+'.blend'))]
    runpy.run_path(str(HERE/'blender_roundtrip.py'),run_name='__main__')
