"""Budgeted edge refinement for the original Blender triangle stream.

Only agreed, curved manifold edges are split. Both adjoining triangles use the
same midpoint; UVs, corner normals and source-pose bindings remain independent.
"""
import math
import struct
from mathutils import Vector

EDGES = ((0, 1), (1, 2), (2, 0))


def triangles_for_edges(split):
    """Original corners 0..2, optional edge midpoints 3..5; preserve winding."""
    count = sum(split)
    if count == 0:
        return [(0, 1, 2)]
    if count == 3:
        return [(0, 3, 5), (3, 1, 4), (5, 4, 2), (3, 4, 5)]
    if count == 1:
        edge = split.index(True)
        a, b, c = edge, (edge + 1) % 3, (edge + 2) % 3
        return [(a, 3 + edge, c), (3 + edge, b, c)]
    # Rotate so the two split edges are a-b and b-c. Their common corner
    # gets a small triangle; the remaining quad uses one consistent diagonal.
    edge = next(i for i in range(3) if split[i] and split[(i + 1) % 3])
    a, b, c = edge, (edge + 1) % 3, (edge + 2) % 3
    ab, bc = 3 + edge, 3 + (edge + 1) % 3
    return [(ab, b, bc), (a, ab, c), (ab, bc, c)]


def build(source, lighting, surface, strength, max_ratio, min_offset_ratio,
          protected_source_faces=(), triangle_limit=None):
    def point_key(p):
        return tuple(round(v, 5) for v in p)

    def key_for(a, b):
        return tuple(sorted((point_key(a), point_key(b))))

    height = max(v.co.z for v in source.vertices) - min(v.co.z for v in source.vertices)
    clearance = max(height * 1e-5, 1e-6)
    threshold = height * min_offset_ratio
    records, uses = [], {}
    for tri in source.loop_triangles:
        ids = tuple(tri.vertices)
        ps = [source.vertices[i].co.copy() for i in ids]
        if (ps[1] - ps[0]).cross(ps[2] - ps[0]).length_squared < 1e-12:
            continue
        ns = [source.corner_normals[i].vector.copy().normalized() for i in tri.loops]
        uv = [source.uv_layers.active.data[i].uv.copy() for i in tri.loops]
        words = [struct.unpack_from('<I', lighting, i * 20 + 12)[0] for i in ids]
        eligible = (len(set(words)) == 1 and bool(words[0] & 0x80000000)
                    and min(ids)//3 not in protected_source_faces)
        record = dict(ids=ids, ps=ps, ns=ns, uv=uv, material=tri.material_index,
                      index=tri.index, eligible=eligible)
        records.append(record)
        for a, b in EDGES:
            key = key_for(ps[a], ps[b])
            aa, bb = (a, b) if point_key(ps[a]) <= point_key(ps[b]) else (b, a)
            uses.setdefault(key, []).append((record, aa, bb))

    candidates = []
    collisions = 0
    for key, adjacent in uses.items():
        if len(adjacent) != 2 or not all(r['eligible'] for r, _, _ in adjacent):
            continue
        r, a, b = adjacent[0]
        other, oa, ob = adjacent[1]
        if r['material'] != other['material']:
            continue
        na, nb = r['ns'][a], r['ns'][b]
        if na.dot(other['ns'][oa]) < .9999 or nb.dot(other['ns'][ob]) < .9999:
            continue
        edge = r['ps'][b] - r['ps'][a]
        offset = -.125 * (na * edge.dot(na) - nb * edge.dot(nb)) * strength
        cap = min(height * .02, edge.length * .04)
        if offset.length > cap:
            offset = offset.normalized() * cap
        middle = (r['ps'][a] + r['ps'][b]) * .5
        if offset.length > clearance:
            direction = offset.normalized()
            hit, _, index, distance = surface.ray_cast(middle + direction * clearance, direction, offset.length)
            if hit is not None and index not in (r['index'], other['index']) and distance + clearance < offset.length:
                offset = direction * max(0, distance * .5 - clearance)
                collisions += 1
        if offset.length >= threshold:
            candidates.append((offset.length, key, middle + offset))
    # One shared edge adds exactly one triangle to each incident source face.
    # Rank by geometric deviation, never by source triangle order or texture.
    triangle_cap=math.floor(len(records)*max_ratio)
    if triangle_limit is not None:triangle_cap=min(triangle_cap,triangle_limit)
    edge_budget = max(0, (triangle_cap - len(records)) // 2)
    candidates.sort(key=lambda item: (-item[0], item[1]))
    chosen = candidates[:edge_budget]
    midpoints = {key: p for _, key, p in chosen}
    positions, normals, uvs, bindings, faces, materials = [], [], [], [], [], []
    unchanged = 0
    for r in records:
        local = {}
        split = [key_for(r['ps'][a], r['ps'][b]) in midpoints for a, b in EDGES]
        unchanged += not any(split)
        samples = [(i, tuple(1.0 if i == k else 0.0 for k in range(3)), r['ps'][i]) for i in range(3)]
        for i, (a, b) in enumerate(EDGES):
            if split[i]:
                w = tuple(.5 if k in (a, b) else 0.0 for k in range(3))
                samples.append((3 + i, w, midpoints[key_for(r['ps'][a], r['ps'][b])]))
        for slot, w, position in samples:
            local[slot] = len(positions)
            positions.append(tuple(position))
            normals.append(tuple(sum((r['ns'][k] * w[k] for k in range(3)), Vector()).normalized()))
            uvs.append(tuple(sum((r['uv'][k] * w[k] for k in range(3)), Vector((0, 0)))))
            bindings.append((r['ids'], w))
        for tri in triangles_for_edges(split):
            faces.append(tuple(local[i] for i in tri))
            materials.append(r['material'])
    expected = len(records) + 2 * len(chosen)
    if len(faces) != expected or len(faces) > math.floor(len(records) * max_ratio):
        raise ValueError('Adaptive triangle budget/topology invariant failed')
    if not all(math.isfinite(v) for p in positions for v in p):
        raise ValueError('Non-finite adaptive vertex')
    report = dict(method='adaptive', source_nondegenerate_triangles=len(records),
                  max_triangle_ratio=max_ratio, min_offset_height_ratio=min_offset_ratio,
                  candidate_edges=len(candidates), split_edges=len(chosen),
                  unchanged_source_triangles=unchanged, collision_limited_vertices=collisions,
                  maximum_offset=max((item[0] for item in chosen), default=0),
                  curved_vertices=2 * len(chosen), shared_edge_midpoints='identical on both adjoining faces')
    return (positions, normals, uvs, bindings, faces, materials), report
