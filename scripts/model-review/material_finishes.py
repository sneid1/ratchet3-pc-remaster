"""Calibrated fine finishes from inspected kit textures; no baked colour grime."""
from functools import lru_cache
import hashlib
from pathlib import Path
import numpy as np
from PIL import Image, ImageFilter


def spread(values):
    lo, hi = np.percentile(values, [5, 95])
    if hi - lo < 1 / 255:
        return np.zeros_like(values)
    return np.clip((values - (lo + hi) * .5) / (hi - lo), -.75, .75)


def sample(path, size, repeats=1, rotation=0):
    # Data channels must be resized independently, without alpha premultiplication.
    image = Image.open(path).convert('RGBA')
    tile_size = max(16, round(size / repeats))
    channels = [np.asarray(c.resize((tile_size, tile_size), Image.Resampling.BILINEAR),
                           dtype=np.float32) / 255 for c in image.split()]
    tile = np.stack(channels, axis=2)
    tile = np.rot90(tile, rotation // 90)
    count = int(np.ceil(size / tile_size))
    return np.tile(tile, (count, count, 1))[:size, :size]


@lru_cache(maxsize=32)
def load(recipe_json, size):
    import json
    recipe = json.loads(recipe_json)
    repeats = recipe.get('repeats', 1)
    rotation = recipe.get('rotation', 0)
    normal = np.zeros((size, size, 2), dtype=np.float32)
    if 'normal_source' in recipe:
        normal = (sample(recipe['normal_source'], size, repeats, rotation)[..., :2] * 2 - 1)
        if rotation == 90:normal = np.stack((-normal[..., 1], normal[..., 0]), axis=2)
        elif rotation == 180:normal = -normal
        elif rotation == 270:normal = np.stack((normal[..., 1], -normal[..., 0]), axis=2)
        normal *= recipe.get('normal_gain', 1)
        if recipe.get('normal_highpass'):
            radius=recipe['normal_highpass']*size
            for channel in range(2):
                packed=Image.fromarray(np.uint8(np.clip(normal[...,channel]*.5+.5,0,1)*255+.5))
                low=np.asarray(packed.filter(ImageFilter.GaussianBlur(radius)),dtype=np.float32)/255*2-1
                normal[...,channel]-=low
    texture = sample(recipe['variation_source'], size, repeats, rotation)
    channel = recipe.get('variation_channel', 0)
    if recipe.get('variation_mode')=='normal_slope':
        # Explicit authored finish modulation for normal-only detail sources.
        # It is not decoded gloss, baked AO, or height inferred from colour.
        variation=spread(np.linalg.norm(texture[...,:2]*2-1,axis=2))
    else:variation = spread(texture[..., channel])
    low, high = recipe['roughness_range']
    roughness = np.clip(recipe['roughness'] + variation * recipe.get('variation', .1), low, high)
    # Directional tool marks are a geometric surface finish, independent of
    # colour brightness. Their orientation follows the inspected brushed kit.
    if recipe.get('tool_marks'):
        axis = np.arange(size, dtype=np.float32) / size
        phase = axis * recipe.get('grooves', 180) * (2 * np.pi)
        slope = (np.sin(phase) + .35 * np.sin(phase * 1.73)) * recipe['tool_marks']
        if rotation in (90, 270):normal[..., 0] += slope[None, :]
        else:normal[..., 1] += slope[:, None]
    length = np.linalg.norm(normal, axis=2)
    normal *= np.minimum(1, .35 / np.maximum(length, 1e-6))[..., None]
    return normal, roughness


def provenance(name, recipe):
    paths = {recipe[k] for k in ('normal_source', 'variation_source') if k in recipe}
    return dict(finish=name, recipe=recipe, sources=[dict(path=p, sha256=hashlib.sha256(Path(p).read_bytes()).hexdigest()) for p in sorted(paths)])
