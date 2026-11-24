"""Compile opt-in imagegen height art over existing fitted material recipes."""
import hashlib
import json
from pathlib import Path
import numpy as np
from PIL import Image, ImageFilter
from surface_layers import mask_for


def blur(a, radius):
    return np.asarray(Image.fromarray(np.uint8(np.clip(a, 0, 1)*255+.5)).filter(
        ImageFilter.GaussianBlur(radius)), np.float32)/255


def apply(spec, target, weights, normal, rough, ao, root):
    source = root/spec['source']
    size = len(rough)
    height = np.asarray(Image.open(source).convert('L').resize((size, size),
                        Image.Resampling.LANCZOS), np.float32)/255
    eligible = np.clip(sum(weights[..., i] for i, strength in enumerate(spec['strength']) if strength), 0, 1)
    for region in spec.get('protect', []):
        eligible *= 1-mask_for(dict(region, bevel=1), size)
    for region in target.get('emissive_regions', []):
        eligible *= 1-mask_for(dict(region, bevel=1), size)
    # Generated bevels can drift several pixels. Explicit recipe features own
    # construction boundaries; only independently authored interior relief is
    # admitted here. Erode both semantic borders and large height transitions.
    dy, dx = np.gradient(blur(height, 2))
    edge = np.hypot(dx, dy) > spec.get('edge_threshold', .018)
    safe = Image.fromarray(np.uint8((eligible > .98) & ~edge)*255)
    mask = np.asarray(safe.filter(ImageFilter.MinFilter(13)), np.float32)/255
    surface = blur(height, .45)-blur(height, spec.get('highpass', 8))
    dy, dx = np.gradient(surface)
    strength = sum(weights[..., i]*s for i, s in enumerate(spec['strength']))
    relief = np.stack((-dx, dy), axis=2)*strength[..., None]*mask[..., None]
    z = np.sqrt(np.maximum(1-(normal*normal).sum(axis=2), .01))
    slopes = normal/z[..., None]+relief
    normal[:] = slopes/np.sqrt(1+(slopes*slopes).sum(axis=2))[..., None]
    cavity = np.maximum(-surface, 0)*mask
    crest = np.maximum(surface, 0)*mask
    rough[:] = np.clip(rough+cavity*.65-crest*.45, .12, .94)
    ao *= np.clip(1-cavity*.7, .9, 1)
    return dict(height=height, detail=relief, wear=crest, grime=cavity,
                source=str(source), sha256=hashlib.sha256(source.read_bytes()).hexdigest())


def colour(spec, weights, root, destination):
    size = len(weights)
    def read(path):
        return np.stack([np.asarray(c.resize((size, size), Image.Resampling.LANCZOS), np.float32)/255
                         for c in Image.open(root/path).convert('RGBA').split()], axis=2)
    base = read(spec['baseline'])
    art = read(spec['source'])
    mask = np.clip(sum(weights[..., i] for i in spec['channels']), 0, 1)
    if spec.get('coverage')=='atlas':mask[:]=1
    for region in spec.get('protect', []):
        mask *= 1-mask_for(dict(region, bevel=0), size)
    mask = np.asarray(Image.fromarray(np.uint8(mask*255)).filter(ImageFilter.MinFilter(5)), np.float32)/255
    amount=spec.get('blend', .7)
    if isinstance(amount, list):amount=sum(weights[..., i]*v for i,v in enumerate(amount))[..., None]
    blend = mask[..., None]*amount
    result = base.copy()
    result[..., :3] = base[..., :3]*(1-blend)+art[..., :3]*blend
    result[..., 3] = read(spec['opacity_source'])[..., 3]
    for region in spec.get('opacity_edges', []):
        # Tighten only the low-opacity fringe of an explicitly selected cutout.
        # Interior transmission and fully opaque texels retain source opacity.
        scope=mask_for(dict(region, bevel=0), size)
        alpha=result[..., 3]
        limit=region.get('limit',64/255)
        narrowed=limit*np.clip(alpha/limit,0,1)**region.get('power',2)
        result[..., 3]=np.where(alpha<limit,alpha*(1-scope)+narrowed*scope,alpha)
    Image.fromarray(np.uint8(np.clip(result, 0, 1)*255+.5)).save(destination)


def publish(data, folder, colour, normal, orm, emission, weights):
    folder.mkdir(parents=True, exist_ok=True)
    detail = data['detail']
    detail = np.concatenate((detail, np.ones((*detail.shape[:2], 1))), axis=2)
    detail /= np.linalg.norm(detail, axis=2, keepdims=True)
    layers = {'base-colour': colour[..., :3], 'normal': normal[..., :3],
              'roughness': orm[..., 1], 'metalness': orm[..., 2], 'ao': orm[..., 0],
              'height': data['height'], 'detail-normal': detail*.5+.5,
              'emission': emission[..., :3], 'opacity': colour[..., 3],
              'wear-mask': data['wear'], 'grime-mask': data['grime'],
              'material-masks': weights, 'orm-packed': orm[..., :3]}
    for name, values in layers.items():
        Image.fromarray(np.uint8(np.clip(values, 0, 1)*255+.5)).save(folder/(name+'.png'))
    (folder/'layers.json').write_text(json.dumps(dict(
        source=data['source'], source_sha256=data['sha256'],
        method='Independent imagegen height; explicit fitted seams retained; generated boundary drift excluded',
        emission='Linear native RGB / 4; multiply by 4 for radiance',
        files={name: hashlib.sha256((folder/(name+'.png')).read_bytes()).hexdigest()
               for name in layers}), indent=2)+'\n')
