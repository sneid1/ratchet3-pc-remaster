"""Refine the inspected original surfaces in Blender, retaining their UVs and form.

Curved patches follow the source vertex normals; flat patches stay flat. Each
new vertex keeps its exact original triangle binding for recorded-pose preview.
This is a contour pass on existing assets, not a replacement character generator.
"""
import argparse
import bpy
import json
from mathutils import Vector
from mathutils.bvhtree import BVHTree
from pathlib import Path
import sys
import struct

HERE=Path(__file__).resolve().parent
OUT=HERE.parents[1]/'captures/rift-texture-review'
p=argparse.ArgumentParser()
p.add_argument('--model',required=True)
p.add_argument('--blend',type=Path,required=True)
p.add_argument('--segments',type=int,default=4)
p.add_argument('--strength',type=float,default=1)
p.add_argument('--method',choices=['adaptive','uniform'],default='adaptive')
p.add_argument('--max-ratio',type=float,default=1.75,help='Maximum triangle multiplier for selective contour refinement')
p.add_argument('--min-offset',type=float,default=.0008,help='Minimum useful edge displacement as a fraction of source height')
p.add_argument('--lock-boundary',action='store_true',help='Keep terrain chunk boundaries coincident with their neighbouring original chunks')
a=p.parse_args(sys.argv[sys.argv.index('--')+1:])
if not 2<=a.segments<=8 or not 0<=a.strength<=1:raise ValueError('Invalid surface refinement')
if not 1<=a.max_ratio<=4 or not 0<a.min_offset<.05:raise ValueError('Invalid adaptive budget')
bpy.ops.wm.open_mainfile(filepath=str(a.blend.resolve()))
obj=bpy.data.objects['UYA_'+a.model+'_captured_pose']
source=obj.data
source.calc_loop_triangles()
surface=BVHTree.FromPolygons([v.co for v in source.vertices],[tuple(t.vertices) for t in source.loop_triangles],all_triangles=True)
constraints=json.loads((HERE/'surface-constraints.json').read_text()).get(a.model,[])
if a.method=='adaptive' and constraints:
    raise ValueError('Preserve accepted constrained geometry; this model needs a separate constrained topology pass')
constrained={c['label']:0 for c in constraints}
def matches(rule,tri):
    x0,y0,x1,y1=rule['uv_bounds']
    coords=[source.uv_layers.active.data[i].uv for i in tri.loops]
    return source.materials[tri.material_index].name.startswith(rule['material_prefix']) and min(source.vertices[i].co.z for i in tri.vertices)>=rule['minimum_source_z'] and all(x0<=t.x%1<=x1 and y0<=(1-t.y)%1<=y1 for t in coords)
layer_trees={}
for label in {r['above_surface'] for r in constraints if 'above_surface' in r}:
    rule=next(r for r in constraints if r['label']==label)
    triangles=[tuple(t.vertices) for t in source.loop_triangles if matches(rule,t)]
    if not triangles:raise ValueError('Layer constraint matches no source geometry: '+label)
    layer_trees[label]=BVHTree.FromPolygons([v.co for v in source.vertices],triangles,all_triangles=True)
layer_projected=0;layer_max_lift=0
source_lighting=(OUT/f'candidate-blender-{a.model}.vertex-lighting.bin').read_bytes()
positions=[];normals=[];uvs=[];bindings=[];faces=[];materials=[]
height=max(v.co.z for v in source.vertices)-min(v.co.z for v in source.vertices)
limit=height*.02
max_offset=0;curved=0;collision_limited=0
clearance=max(height*1e-5,1e-6)
edge_uses={}
def position_key(point):return tuple(round(v,5) for v in point)
if a.lock_boundary:
    for tri in source.loop_triangles:
        ps=[source.vertices[i].co for i in tri.vertices]
        for i,j in ((0,1),(1,2),(2,0)):
            key=tuple(sorted((position_key(ps[i]),position_key(ps[j]))))
            edge_uses[key]=edge_uses.get(key,0)+1
adaptive_report={}
if a.method=='adaptive':
    sys.path.insert(0,str(HERE))
    from adaptive_refinement import build
    values,adaptive_report=build(source,source_lighting,surface,a.strength,a.max_ratio,a.min_offset)
    positions,normals,uvs,bindings,faces,materials=values
    max_offset=adaptive_report['maximum_offset'];curved=adaptive_report['curved_vertices']
    collision_limited=adaptive_report['collision_limited_vertices']
for tri in source.loop_triangles if a.method=='uniform' else []:
    ids=list(tri.vertices)
    points=[source.vertices[i].co.copy() for i in ids]
    ns=[source.corner_normals[i].vector.copy().normalized() for i in tri.loops]
    tex=[source.uv_layers.active.data[i].uv.copy() for i in tri.loops]
    refine_scale=1.0;layer_rule=None
    material_name=source.materials[tri.material_index].name
    for rule in constraints:
        # Bounds are in the source atlas's image coordinates (V downward).
        # A small set of layered surfaces shares a silhouette but has separate
        # UVs. Preserve their exact source separation instead of bulging each.
        if matches(rule,tri):
            refine_scale=min(refine_scale,rule['position_refinement']);constrained[rule['label']]+=1
            if 'above_surface' in rule:layer_rule=rule
    if (points[1]-points[0]).cross(points[2]-points[0]).length_squared<1e-12:continue
    controls={}
    for i,j in ((0,1),(1,0),(1,2),(2,1),(2,0),(0,2)):
        boundary=a.lock_boundary and edge_uses[tuple(sorted((position_key(points[i]),position_key(points[j]))))]==1
        correction=Vector() if boundary else ns[i]*(points[j]-points[i]).dot(ns[i])
        controls[i,j]=(2*points[i]+points[j]-correction)/3
    edge=sum(controls.values(),Vector())/6
    middle=edge+(edge-sum(points,Vector())/3)*.5
    def patch(w):
        result=sum((points[i]*w[i]**3 for i in range(3)),Vector())
        for (i,j),control in controls.items():result+=control*(3*w[i]**2*w[j])
        return result+middle*(6*w[0]*w[1]*w[2])
    grid={}
    for i in range(a.segments+1):
        for j in range(a.segments+1-i):
            w=(1-(i+j)/a.segments,i/a.segments,j/a.segments)
            linear=sum((points[k]*w[k] for k in range(3)),Vector())
            offset=(patch(w)-linear)*a.strength*refine_scale
            # Original unlit overlays (eyes and lamps) can sit on another
            # surface. Curving their independent triangle strips creates depth
            # intersections. Retain their exact source surface in this pass.
            if not (struct.unpack_from('<I',source_lighting,ids[0]*20+12)[0] & 0x80000000):offset=Vector()
            if offset.length>limit:offset=offset.normalized()*limit
            # Original layered surfaces (eyes, armour overlays, lips, straps)
            # can be closer together than an otherwise small contour change.
            # Stop before another source triangle; never infer safe clearance
            # from the character's overall size or from its colour texture.
            if offset.length>clearance:
                direction=offset.normalized()
                hit,_,hit_index,hit_distance=surface.ray_cast(linear+direction*clearance,direction,offset.length)
                if hit is not None and hit_index!=tri.index and hit_distance+clearance<offset.length:
                    offset=direction*max(0,hit_distance*.5-clearance);collision_limited+=1
            if layer_rule:
                # Repair an existing intersection, rather than preserving it.
                # Project only this fitted source layer in front of its named
                # underlying surface, with a small physical separation.
                normal=sum((ns[k]*w[k] for k in range(3)),Vector()).normalized()
                tree=layer_trees[layer_rule['above_surface']];start=linear+offset
                hit,_,_,depth=tree.ray_cast(start+normal*clearance,normal,layer_rule['projection_limit'])
                if hit is not None:lift=depth+clearance+layer_rule['minimum_clearance']
                else:
                    hit,_,_,depth=tree.ray_cast(start-normal*clearance,-normal,layer_rule['projection_limit'])
                    lift=max(0,layer_rule['minimum_clearance']-depth-clearance) if hit is not None else 0
                if lift:
                    offset+=normal*lift;layer_projected+=1;layer_max_lift=max(layer_max_lift,lift)
            max_offset=max(max_offset,offset.length)
            curved+=offset.length>1e-6
            grid[i,j]=len(positions);positions.append(tuple(linear+offset))
            normals.append(tuple(sum((ns[k]*w[k] for k in range(3)),Vector()).normalized()))
            uvs.append(tuple(sum((tex[k]*w[k] for k in range(3)),Vector((0,0)))))
            bindings.append((ids,w))
    for i in range(a.segments):
        for j in range(a.segments-i):
            faces.append((grid[i,j],grid[i+1,j],grid[i,j+1]));materials.append(tri.material_index)
            if i+j<a.segments-1:
                faces.append((grid[i+1,j],grid[i+1,j+1],grid[i,j+1]));materials.append(tri.material_index)
mesh=bpy.data.meshes.new(a.model+'_refined_original')
mesh.from_pydata(positions,[],faces);mesh.update()
for mat in source.materials:mesh.materials.append(mat)
layer=mesh.uv_layers.new(name='Original_UYA_UV')
for loop in mesh.loops:layer.data[loop.index].uv=uvs[loop.vertex_index]
for polygon,index in zip(mesh.polygons,materials):polygon.material_index=index;polygon.use_smooth=True
mesh.normals_split_custom_set([normals[l.vertex_index] for l in mesh.loops])
for k in range(3):
    ids=mesh.attributes.new('review_src'+str(k),'INT','POINT')
    ids.data.foreach_set('value',[b[0][k] for b in bindings])
    weights=mesh.attributes.new('review_bary'+str(k),'FLOAT','POINT')
    weights.data.foreach_set('value',[b[1][k] for b in bindings])
obj.data=mesh;obj.hide_set(False);obj.hide_render=False
obj['Refinement']='Original silhouette and UV layout; curvature from authored corner normals; exact original triangle bindings.'
obj['Maximum surface displacement']=max_offset
target=OUT/'blender/refined';target.mkdir(exist_ok=True)
report={'model':a.model,'source_triangles':len(source.loop_triangles),'refined_triangles':len(faces),'maximum_offset':max_offset,'curved_vertices':curved,'strength':a.strength,'segments':a.segments,'lock_boundary':a.lock_boundary,'constrained_source_triangles':constrained,'collision_limited_vertices':collision_limited}
report['method']=a.method
report.update(adaptive_report)
report.update(layer_projected_vertices=layer_projected,layer_maximum_lift=layer_max_lift)
bpy.ops.wm.save_as_mainfile(filepath=str(target/(a.model+'.blend')))
(target/(a.model+'.json')).write_text(json.dumps(report,indent=2))
print(json.dumps(report))
