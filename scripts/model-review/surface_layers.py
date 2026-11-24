"""Bake explicitly authored UV surface features into linear PBR channels.

Shapes describe real panel boundaries/vents in the inspected atlas, in 256-unit
image coordinates. Colour brightness is never treated as height or metalness.
"""
import numpy as np
import json
from PIL import Image, ImageDraw, ImageFilter
from material_finishes import load as load_finish


def mask_for(layer, size):
    scale = size / 256
    image = Image.new('L', (size, size))
    draw = ImageDraw.Draw(image)
    kind = layer['shape']
    if kind in ('rect', 'ellipse'):
        bounds = tuple(x * scale for x in layer['bounds'])
        getattr(draw, 'rectangle' if kind == 'rect' else 'ellipse')(bounds, fill=255)
    elif kind == 'polygon':
        draw.polygon([tuple(x * scale for x in p) for p in layer['points']], fill=255)
    elif kind == 'line':
        draw.line([tuple(x * scale for x in p) for p in layer['points']],
                  fill=255, width=max(1, round(layer.get('width', 1) * scale)), joint='curve')
    else:
        raise ValueError('Unknown authored surface shape: ' + kind)
    bevel = layer.get('bevel', .5) * scale
    if bevel:
        image = image.filter(ImageFilter.GaussianBlur(bevel))
    return np.asarray(image, dtype=np.float32) / 255


def apply(layers, normal, rough, metal, ao, finishes=None, weights=None):
    size = len(rough)
    height = np.zeros_like(rough)
    for layer in layers:
        mask = mask_for(layer, size)
        if 'scope_channel' in layer:
            if weights is None:raise ValueError('Scoped relief requires its authored material weights')
            mask*=weights[...,int(layer['scope_channel'])]
        height += mask * layer.get('height', 0)
        if 'finish' in layer:
            spec=dict(finishes[layer['finish']])
            spec.update(layer.get('finish_overrides',{}))
            detail,finish_rough=load_finish(json.dumps(spec,sort_keys=True),size)
            normal[:]=normal*(1-mask[...,None])+detail*mask[...,None]
            rough[:]=rough*(1-mask)+finish_rough*mask
        for field, target in [('roughness', rough), ('metalness', metal)]:
            if field in layer:
                target[:] = target * (1 - mask) + layer[field] * mask
        # Only authored recesses attenuate indirect light. Never darken albedo.
        if 'cavity' in layer:
            ao *= 1 - mask * (1 - layer['cavity'])
        if 'edge_ao' in layer or 'edge_wear' in layer:
            # Contact around fitted construction edges, not diffuse brightness.
            # Broad dirt stays in crevices; exposed lips receive a finer finish.
            core=Image.fromarray(np.uint8(np.clip(mask,0,1)*255))
            radius=max(1,round(layer.get('edge_width',1.2)*size/256))
            expanded=np.asarray(core.filter(ImageFilter.MaxFilter(radius*2+1)),dtype=np.float32)/255
            inset=np.asarray(core.filter(ImageFilter.MinFilter(radius*2+1)),dtype=np.float32)/255
            contact=np.clip(expanded-mask,0,1)
            lip=np.clip(mask-inset,0,1)
            ao*=1-contact*(1-layer.get('edge_ao',1))
            if layer.get('edge_ao',1)<1:
                rough[:]=rough*(1-contact)+np.maximum(rough,.76)*contact
            if 'edge_wear' in layer:
                # A subtle break in roughness makes the bevel respond to light;
                # this does not expose metal through dielectric leather/cloth.
                wear=lip*layer['edge_wear']
                rough[:]=rough*(1-wear)+np.maximum(.22,rough-.16)*wear
    dy, dx = np.gradient(height, 256 / size)
    # Native UV V runs downward; its cotangent frame flips the normal Y channel.
    relief = np.stack((-dx, dy, np.ones_like(dx)), axis=2)
    relief /= np.linalg.norm(relief, axis=2, keepdims=True)
    # Combine normal slopes so fine material grain follows the authored relief.
    z = np.sqrt(np.maximum(1 - (normal * normal).sum(axis=2), .01))
    slopes = normal / z[..., None] + relief[..., :2] / relief[..., 2:3]
    combined = np.concatenate((slopes, np.ones((size, size, 1))), axis=2)
    combined /= np.linalg.norm(combined, axis=2, keepdims=True)
    normal[:] = combined[..., :2]
    return {'features': len(layers), 'finishes':sorted({l['finish'] for l in layers if 'finish' in l}), 'height_range': [float(height.min()), float(height.max())],
            'normal_xy_peak': float(np.linalg.norm(normal, axis=2).max())}
