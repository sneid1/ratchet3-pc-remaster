# Improved texture redraw prompts

Revised 24 November 2026 for the Ratchet & Clank: Up Your Arsenal texture batch.

## Recommendation

Use one reconstruction prompt with a treatment selected from the visible texture. Keep crisp reconstruction as an explicit option. Preserve intentional soft features at their existing widths. Do not send the whole queue through the soft metal-panel recipe.

The ready-to-use colour prompt is in `colour-template.txt`; treatments are in `treatments.json`. The pack also contains retry wording, optional image-role clauses and filled colour prompts for all eleven supplied examples. Ten examples are ready to submit under the current one-reference contract. The conditional q05309 example is separated into `examples/needs-role-confirmation/`.

These revisions were derived from the [supplied prompt pack](https://chatgpt.com/library?file_id=libfile_fde3_f-47b04dff0e088191b2e153ddeebfa648_231ff9dc024c8191b011ce6a13b1c4b7&account_id=personal), the reviewed outputs in the current conversation, and the [shared texture session](https://chatgpt.com/share/6b051a80-47c4-83ec-aa34-03420e14673a). They have not been run through Qwen Image 2.1 Turbo here. No new image-generation run was performed for this review.

## What the other session establishes

The shared session contains two different tasks. For the grey floor panel, its response reports that the first two attempts remained too blurry and the third was closer after the prompt requested the existing painted marks reconstructed in focus while preserving broad faded boundaries. It also explicitly reports residual darkening and interpreted edges. That supports preserving edge width and existing shading, not treating the result as an exact restoration or a proven first-pass recipe.

Later, for q04508 and q02318, the user explicitly rejected restrained treatment. The visible work log then calls for sharp redraws: reconstruct q04508's blocky patches, especially at the lower right, and retain q02318's bright greens and exact layout. It reports a further correction to q04508 for a grey region and two small grille details. The useful lessons are to reconstruct real details clearly, protect local colours, and describe a specific missing feature on retry. The shared page does not expose the full final per-texture generation prompts or a final user approval, so the templates below are a synthesis rather than claimed verbatim successful Qwen prompts.

In this conversation's fifteen-texture batch, grain and smooth gradients also needed different handling. Targeted revisions restored grain in q00070 and removed invented cloudiness from the deliberately uniform q00260 gradient. Adding the hard-pixel source on some grain retries was useful evidence in those attempts, but prompt wording and inputs changed together; this was not an isolated A/B test.

## Why the current prompts fail

| Current instruction | Failure it can encourage | Replacement |
| --- | --- | --- |
| Every shape, crack, rivet and panel edge, only sharper | Invites those feature types on textures that do not contain them; narrows painted bands into seams | Preserve visible features and each boundary's actual character; request sharp reconstruction only for genuine hard features |
| All panel divisions are wide painted tonal bands | False for mechanical atlases, graphics, bark and genuine crack networks | Select a treatment from the visible image |
| Fine matte paint texture on everything | Adds grain to clean graphics/effects, while erasing coarse natural texture | Preserve existing grain; add only supported finer definition |
| Do not add gradients or blotches | Can conflict with retaining existing painted shading and mottling | Keep the original variation at full strength; add no new illumination or patterns |
| Average colour plus “only” a small brightness spread | Can flatten local colours, desaturate stripes or hide atlas-region drift | Match local RGB appearance; retain the mean as a secondary check; remove range from the prose prompt |
| An alpha file shows where it is drawn | Refers to an image the model does not receive | Describe only the supplied RGB reference and handle the actual alpha separately |
| Match the accepted variant's drawing and detail | May copy its extra features, colour drift or layout errors | Source controls structure and colours; variant supplies compatible technique only |
| Retry with the whole soft prompt | Can fix an invented seam while losing real grilles or grain | Add one diagnosis-specific correction to the original reconstruction prompt |

## 1. Main colour prompt

```text
Redraw Image 1 as the same 2D colour texture at native 1024 x 1024 resolution, retaining the original Ratchet & Clank: Up Your Arsenal art style. Source: {describe}.

Reconstruct the artwork rather than enlarging its pixel blocks. Remove enlargement blur from genuine hard contours and resolve existing small details clearly at native resolution. Preserve intentional soft shading, painted transitions and glow at their original widths. Different features in the same texture can require different edge treatment.

Keep every visible feature's position, count, shape, size, spacing and orientation. Preserve empty areas and the exact crop. Retain the original grain and mottling, including their recognizable clusters, scale, direction and contrast; do not erase them or replace them with unrelated fine noise. Keep plain regions plain. Additional fine definition must stay within the existing design and must not introduce new features when reduced to the original dimensions.

Match colour locally throughout the image, including each separate atlas piece. Preserve hue, saturation, brightness and all existing painted light and dark variation. The source's whole-image average is {mean}; this is a reference check, not a colour to apply everywhere. Preserve the original lighting without adding new illumination, reflections, contrast enhancement or a vignette.

{treatment}

Copy any existing lettering, numbers and logos as drawn shapes, preserving their strokes, gaps, spacing and placement. Do not retype, correct, translate or guess unclear characters.

{tiling}
{variant}
{pixel_reference}
{alpha}
{retry_note}

No crop, zoom, rotation, perspective, rearrangement or added decoration. Reduced to the original dimensions, the result should closely reproduce the source's design and colour distribution. Output only the square colour texture, filling the frame, at 1024 x 1024.
```

This deliberately contains both positive reconstruction instructions and preservation rules. It does not ask for a blurrier image, a sterile plate, or a universal low-contrast matte finish.

## 2. Choose one main treatment

Select from appearance and known texture role. Do not choose from the material label or brightness range alone. A labelled metal texture may be a simple gradient, a graphic, a complex atlas or a grained surface. For a mixed atlas, use the atlas treatment and put concise, regional instructions in `{describe}`.

### A. Structured surfaces and real hard details

Use `treatment = "structured"`.

```text
Reconstruct the existing design with clean continuous contours and sharply resolved internal detail. Resolve real slots, grille openings, fasteners and markings individually where the source shows them, retaining their exact counts, locations and thicknesses. Replace blocky sampling with coherent drawn features, without inventing missing parts. Keep existing painted bands and curved shading at their own widths; do not sharpen them into new seams. Do not omit small features simply because they are faint.
```

### B. Character and mechanical UV atlases

Use `treatment = "uv_atlas"`.

```text
This is a flat UV texture atlas. Preserve every separate piece's outline, size, position, orientation, existing distortion and surrounding padding. Reconstruct the real hard features and small mechanical or graphic details clearly; retain soft organic shading and painted transitions where the source has them. Preserve each piece's own colours independently. Keep facial features and markings inside their original pieces. Do not rearrange or join pieces, straighten distorted features, fill padding, complete unseen parts or render an assembled character or object.
```

### C. Broad painted bands and soft material transitions

Use `treatment = "soft_bands"`.

```text
The existing broad bands and gradients are painted colour transitions. Keep their exact positions, widths, shapes and light-to-dark profiles. Do not collapse them into thin dark seam lines, outline them or turn them into physical frames or bevels. Resolve the existing painted contours in focus while retaining softly feathered transitions; do not apply global blur. Preserve all source mottling at full strength. Add finer matte definition only where an existing painted surface supports it; a plain gradient can remain plain.
```

### D. Grain, bark and natural surfaces

Use `treatment = "grain"`.

```text
The original grain and mottling are essential to this surface. Keep the major patches, furrows, crack paths and grain groupings in place, preserving their scale, direction, density and contrast. Resolve their smaller contours and internal colour variation at native resolution. Keep rough source texture rough where shown. Do not denoise it into smooth paint, replace it with generic noise, add new crack branches or exaggerate relief. Preserve any clearly drawn structural boundaries separately from the finer grain.
```

### E. Painted graphics and stripes

Use `treatment = "flat_graphic"`.

```text
Preserve the exact count, order, width, position and colour of the graphic regions and lines. Reconstruct their drawn contours cleanly without shifting them. Keep flat fills flat and any deliberate colour transitions smooth. Do not add material grain, surface shading, bevels, outlines, seams or decorative detail.
```

### F. Light bars and soft colour effects

Use `treatment = "soft_effect"`.

```text
Preserve the exact footprint, paths, spacing, colours, brightness distribution and falloff of the visible light or effect pattern. Reconstruct its existing forms with smooth continuous colour transitions, keeping soft edges soft at their original widths. Do not turn luminous bands or painted networks into solid hardware, cracks, raised grids or crisp outlines. Do not add a housing, light source, new glow, material grain or a different background.
```

### G. Deliberately smooth gradients

Use `treatment = "smooth_gradient"`.

```text
This is a deliberately smooth colour swatch. Match the original gradient direction, transition positions and tonal profile. Keep uniform regions uniform and straight transitions straight. Do not add grain, mottling, clouds, surface relief, sharpening lines or a material interpretation. Preserve any existing narrow tonal step at its actual width without turning it into a seam.
```

## 3. Placeholder changes

| Placeholder | How to populate it |
| --- | --- |
| `{describe}` | Retain original dimensions, actual role/kind and a brief visible description. Remove the embedded tiling sentence and the claim that an unseen alpha file is supplied. For uncatalogued textures, describe visible forms without guessing a material. |
| `{mean}` | Keep the existing source average in #rrggbb format as a secondary reference. Do not impose this colour on each region. Ensure its calculation uses the same RGB/alpha convention as the reference. |
| `{range}` | Remove from generated prompt text. Keep the value in logging and QA. It is a reported scalar brightness spread, not a saturation limit or reliable texture classifier. No code was supplied to establish its exact formula. |
| `{treatment}` | New. Insert one treatment above. Default to the structured treatment only when genuine hard construction is visible; an unknown visual role should get a brief source description and the base preservation rules rather than a guessed material. Empty is valid. |
| `{tiling}` | Keep, once only. Empty unless repeat axes are known. Select the x, y or xy clause below. |
| `{variant}` | Keep. Empty unless an accepted same-layout variant is actually attached as Image 2. Use the narrower clause below. |
| `{pixel_reference}` | New optional block. Empty unless a hard-pixel source is attached. Insert the clause below after filling its `{pixel_reference_index}` with the actual image index. |
| `{alpha}` | New optional block. Empty for opaque inputs. For the supplied RGB-only soft-alpha examples, insert the honest RGB clause below. This does not recreate the missing alpha. |
| `{retry_note}` | New optional block. Empty on the first attempt. Insert a verified, specific correction on retry. |

The filled example prompts in this pack use only Image 1, matching their supplied reference files. They do not assume variants, alpha masks or hard-pixel sources have also been sent.

### Missing descriptions

Add a short observation such as “pale aqua field with winding bright soft bands”, rather than inferring an object from a vague queue category. Useful metadata is: layout type, visible colour regions, hard versus soft boundaries, grain character, text/markings present, alpha-reference status and confirmed repeat axes. Store an uncertain material identity as unknown. This can be a small visual classification pass; the prompt does not require a long caption.

### Why the range sentence should go

q03684 is visibly green and purple with strong stripe boundaries, yet its prompt reports a spread of 11. In the supplied pixels, that figure is consistent with the spread of an arithmetic RGB average; the precise pipeline formula is not shown. Whatever the formula, it is not a description of the strong hue difference. q03077 reports 192 but receives the same “only ... subtle” wording. q03408 reports just 27 while containing important cracks. Do not use this scalar to desaturate an image, classify it as plain, or suppress its structure.

## 4. Image and tiling clauses

### Existing accepted variant, Image 2

```text
Image 2 is a previously accepted redraw of the same layout in another colour scheme. Use its compatible drawing technique and corresponding detail only where those features agree with Image 1. Image 1 controls every feature's position, size, colours, brightness, shading and intentional edge softness. Do not import Image 2's palette, extra features, layout errors or lighting changes.
```

Only use a variant that has passed layout and local-colour checks against its own source. Avoid supplying failed outputs as style references. A failed candidate may be supplied explicitly for a limited repair, using the separate repair contract below.

### Current RGB reference for an alpha-bearing texture

```text
This texture uses transparency in the game, but the supplied reference is RGB and contains no alpha mask. Reconstruct the supplied colour image only. Preserve visible silhouettes, edge colours and soft colour fades at their original widths. Do not infer transparent holes from black pixels, invent a cutout, harden fading edges or add a new background.
```

All twenty-two supplied reference and hard-pixel PNGs are RGB, including q03939 and q03930. The actual opacity channel is therefore not available in this example pack. Preserve and resample the original alpha separately when it exists, then combine it with the colour output using the renderer's established straight/premultiplied-alpha convention. This is a proposed pipeline policy, not a claim that the current runner already does it. Do not derive opacity from black pixels. If opacity itself must be redrawn, supply the actual mask with an explicit image role and validate it separately.

### Confirmed horizontal repeat

```text
This texture repeats horizontally. Preserve the existing wrap across the left and right edges, including the positions and widths of features meeting there. Do not add a border or alter the interior layout to disguise a mismatch.
```

Use the corresponding y or xy clause from `optional-input-clauses.txt` for other confirmed axes. None of the eleven supplied default example prompts asserts tiling, so their revised prompts leave this empty. Do not infer both-axis repetition solely because a texture is a level surface. If known repeat metadata conflicts with an obvious source gradient or atlas, review that metadata instead of asking the model to create a return gradient. Visual seamlessness does not require the first and last sampled pixels to be exactly equal.

### Optional additional hard-pixel source

```text
Image {pixel_reference_index} is the same source enlarged with hard pixels. Use it to identify the original small-feature layout, colour boundaries and grain scale. Reconstruct those marks at native resolution without drawing the square pixel grid. It supplies source evidence, not a different texture or a style to copy.
```

This is an optional input experiment, not required by the revised default. Preserve the current Image 2 variant contract: without a variant, a hard-pixel source can be Image 2; with a variant, append it as Image 3. Populate `{pixel_reference_index}` from the actual attachment order, then insert the expanded clause into `{pixel_reference}`. Leave that block empty when there is no additional source image. Do not describe Image 2 as both a variant and the original. Use the same source crop and orientation. Keep it an additional source reference, not a substitute for native-resolution reconstruction. Try this specifically on grain or unresolved small-detail failures before adding it to every request.

## 5. Retry the actual failure

Do not automatically switch every failed attempt to the soft treatment. Use the source and the same treatment again, with one short note grounded in the actual mismatch. A low edge-correlation score alone does not identify whether the cause is lost grain, moved geometry, altered colour boundaries or a bad plain-texture metric.

The current retry can remain source-first, using the same attachment order as the first attempt. The note describes what to draw from the source; it does not pretend the model can see an unattached failed candidate.

### Invented detail

```text
In {region}, the source contains {source_feature}. Keep that form at its original width and contrast. Do not add {unwanted_feature} there. Preserve the source's surrounding features.
```

### Missing detail

```text
Preserve {feature} in {region}, including its original count, location and outline. Resolve it clearly rather than merging it into the surrounding fill. Do not compensate by inventing detail elsewhere.
```

### Lost grain

```text
Restore the original grain in {region}, retaining its recognizable clusters, scale, direction and strength. This grain must remain visible when reduced to the original dimensions. Much finer unrelated noise does not replace it.
```

### Moved feature

```text
Keep {feature} in {region} at its exact source position, size and thickness. Preserve its original asymmetry and curvature. Do not straighten, regularise or symmetrise it.
```

### Colour drift

```text
Match the source's {region} locally. Preserve its {source_colour_description}; avoid the observed {drift_description}. Keep its internal light/dark relationships. Do not compensate by changing another part of the image.
```

### Hardened fade

```text
In {region}, retain the source's soft colour falloff across its full original width. Do not add a hard outline or narrow boundary. Keep the original centre brightness and footprint.
```

All placeholders inside these retry snippets describe observed facts: region, feature, actual source form or measured drift. If only a global error is known, use an honest global correction; do not invent a location or a missing feature. Recheck the full image after retry because a targeted instruction can still affect other areas.

### Optional limited repair of a mostly correct candidate

This changes the input contract deliberately: **Image 1 = failed candidate; Image 2 = original source**. Do not append this to the normal colour prompt or retain its Image 2 variant clause.

```text
Image 1 is the current high-resolution colour candidate. Image 2 is the original texture and the authority for the design and colours. Make this limited correction to Image 1: {correction}.

Use Image 2 to recover the specified feature's location, count, shape, width and colours. Preserve Image 1's composition and all correctly reconstructed regions outside {region}. Do not apply a new global style, lighting change or colour grade. Do not add replacement details elsewhere. Output only the corrected 1024 x 1024 colour texture.
```

`{correction}` is the verified action; `{region}` is its location. For example, a note can request restoration of a grey patch and two missing grille details when those facts are established by the comparison. The shared session provides that kind of correction as useful evidence; it does not prove a universal repair success rate.

## 6. Routing for the supplied examples

| ID | Treatment | What the prompt protects |
| --- | --- | --- |
| q03939 | UV atlas | Every character island, original skin/clothing colours, facial/painted details and padding; alpha must be handled separately |
| q03215 | UV atlas | Crisp real mechanical features, red armour, grey housings, small grilles/fasteners and green indicator |
| q03684 | Flat graphic | Strong purple/green colours, exact stripe order and widths; no new grain or relief |
| q03077 | Soft effect | Broad green light bars, pale cores, spacing and original glow falloff |
| q03998 | Structured | Existing red rim and recessed surround, with a plain red centre; no new frame or fasteners |
| q03210 | Soft effect | Aqua field and pale interconnected light-like paths, preserving their soft widths without guessing a material |
| q04980 | Grain with visible-shape description | Pointed brown vertical forms, broad dark divisions and modest original grain |
| q05309 | Confirm role first; soft effect only if colour | Radial pink/blue pattern; preserve vivid colours if it is colour artwork. If encoded material data, exclude it from artistic colour prompting |
| q03408 | Grain with protected cracks | Existing crack branches and junctions plus the faint background grain |
| q04334 | Grain | Strong coarse bark texture, vertical furrows and warm multicolour mottling |
| q03930 | Soft effect with explicit two-part layout | Both purple networks, every junction, their soft falloff and the large intervening gap; separate alpha handling |

Do not claim q05309 is a normal map from its palette alone. Its shader/material role is unknown in the supplied data. The conditional prompt is supplied for convenience but is not marked ready for automatic submission.

## 7. Small runner changes that matter as much as wording

### Keep structure, grain and nearly-flat checks separate

Keep the existing one-original-pixel movement tolerance and 0.6 structural correlation as starting gates for real features. Include colour boundaries, not only one brightness channel. Separate recognizable crack paths, UV boundaries, stripes, grilles and lettering from stochastic grain. A modest structural filter must not erase tiny markings; retain dedicated checks for protected fine graphics.

For a genuinely unstructured grained surface, compare grain strength, scale, orientation and coarse distribution separately. Do not demand pixel-for-pixel alignment of every random fleck, and do not reward adding unrelated grain merely to make an edge score larger. If a source has no coherent structure above its noise floor, structural correlation should be not applicable, with colour, texture and invented-feature checks still active. Low global brightness spread alone is insufficient to make this exception.

Keep detection of invented long lines, frames, repeated dots and new shapes active on plain surfaces. Ratios become unreliable when original edge energy is almost zero; use an absolute floor and coherent new-feature checks in that case. This is a proposed validator change; the actual implementation was not supplied.

### Check local colour

A correct global mean can hide a brightened metal island and a darkened green island. Compare a coarse spatial colour map or known UV regions as well as the whole-image mean, with the same source coordinates and resampling convention. Retain the existing 24/255 limit initially as a severe-drift guard, then calibrate tighter regional review criteria against accepted examples. No universal new numeric threshold has been validated here.

Use the original alpha or known valid regions for visible-colour scoring when available. Fully transparent RGB should not dominate a visible-colour score, while edge padding/colour bleed remains a separate filtering concern. Check small bright marks separately because global 2nd/98th percentiles can ignore them.

### Keep alpha independent

Compare opacity coverage, contour position and soft transition width separately from RGB. Inspect reconstructed RGBA against light and dark backgrounds for fringes. Preserve the renderer's alpha convention; simply copying RGB from a black-composited preview into an alpha texture can create dark fringes. The supplied images do not establish whether their RGB was composited, so inspect the actual extraction/rendering path before choosing that reconstruction step.

### Check provider prompt rewriting once

Qwen's official editing guide says `prompt_extend` rewrites prompts and recommends disabling it for detailed instructions. It also says image indices follow attachment order and the 2.1 series does not support a separate `negative_prompt` field. Keep exclusions in the main prompt. [Qwen documentation](https://www.alibabacloud.com/help/en/model-studio/qwen-image-edit-guide)

For OpenRouter, inspect the selected endpoint's allowed provider options before setting an upstream flag; do not assume the Alibaba parameter is exposed on that route. Its endpoint capability records are the documented authority for allowed passthrough keys. [OpenRouter documentation](https://openrouter.ai/blog/announcements/image-api/)

## 8. Rollout and evidence

Start with the ten ready examples plus representative grain/gradient cases from the completed batch. Compare the old default, old soft prompt and the appropriate new treatment using the same model/provider, input images, output size and evaluation. If reproducible seeds are supported, keep a fixed seed set; otherwise use a small repeat set to avoid choosing on a lucky render. Evaluate first-pass rate and retry rate separately, plus visible quality and the incidence of lost details, invented structure, colour drift and alpha damage.

Keep prompt-only changes separate from optional extra-reference or validator changes when measuring their effect. A looser validator can raise the reported pass rate without improving artwork. Do not present this review as a measured Qwen improvement until that comparison has been run.

For the near-term queue, prioritize the character/mechanical atlas treatment and a short source description. Keep the soft-band treatment specific to actual soft bands, and keep natural bark/grain out of it.

## Sources and limits

- [User-provided prompt pack and eleven example pairs](https://chatgpt.com/library?file_id=libfile_fde3_f-47b04dff0e088191b2e153ddeebfa648_231ff9dc024c8191b011ce6a13b1c4b7&account_id=personal). All pixel-format and appearance observations above were checked directly against those files.
- [Shared session supplied by the user](https://chatgpt.com/share/6b051a80-47c4-83ec-aa34-03420e14673a). Read through the rendered shared conversation, including its visible progress messages. Full final per-asset generation prompts were not visible.
- Current conversation's fifteen-texture redraws and local comparison records. These were generated with the built-in image tool, not the user's Qwen endpoint, and some required targeted retries.
- [Qwen editing guide](https://www.alibabacloud.com/help/en/model-studio/qwen-image-edit-guide) and [OpenRouter Image API documentation](https://openrouter.ai/blog/announcements/image-api/), checked on 24 November 2026 for the narrow API notes above.

No runner source code, original alpha files, shader bindings, or labelled full-queue evaluation set were supplied. Recommendations about those components are explicit proposed changes rather than claims about the existing implementation.
