wgpu-core 30.0.1 as shipped on crates.io, with FOUR changes for the port (MIT OR Apache-2.0, see LICENSE.MIT / LICENSE.APACHE).
The port's native renderer (port/uya) and its play window draw through wgpu 30; this copy is the one the port's workspace builds
(port/Cargo.toml, [patch.crates-io]).

What changed (each place marked "PORT CHANGE")
  1. src/command/bundle.rs, `State::flush_bindings`: a bind group's buffer and texture memory init actions join the bundle the
     first time that group is bound in it (`State::registered_groups`), and each action once (`seen_buffer_actions`,
     `seen_texture_actions`: same buffer or texture, same range, same kind, the first kept, in order). Until v0.1.5 every
     binding appended all of them and `RenderBundleEncoder::finish` dropped the duplicates with a set and a retain.
  2. src/command/pass.rs, `flush_bindings_helper`: a bind group's buffer and texture init actions are registered the first
     time the group is bound in a pass, not at every binding (`PassState::registered_groups`, set empty where render.rs and
     compute.rs begin a pass).
  3. src/command/bundle.rs, `set_bind_group`: a group already in the bundle's usage scope (`State::scoped_groups`) is not
     merged into it again (the merge of a group already there changes nothing).
  4. src/command/render.rs, `execute_bundle`: the pass's stencil reference (`State::stencil_reference`, 0 unless the pass set
     another) is set on the command encoder before a bundle's commands run. A pass sets it when one of its own pipelines that
     reads it is set (`set_pipeline`); a bundle's `SetPipeline` never does, so a bundle's draws with a stencil test compared
     against whatever reference the command buffer last held (WebGPU: a bundle's draws use the pass's). The port's sky, drawn
     where the terrain's cover left the stencil 0 (port/uya/src/native.rs `Cover`), was right in a dump's first frame and gone in
     the next (the shadow steps' reference left over).
  Nothing else differs from the published crate (compare with `diff -r` against the crate unpacked from crates.io; only the .cargo-ok
  marker file is missing here).

Why
  Every bind group set in a render bundle appends all of that group's init actions to the bundle, so a bundle that switches
  between groups holds each group's actions once per switch, and every execution of the bundle registers every one of them
  (`CommandBufferTextureMemoryActions::register_init_action`: a lock and a check each). The port's presenter executes the
  static world's bundles (terrain, ties, shrubs, and their visible-surface copies) for every picture it shows between two game
  frames, so on Daxx this registration was the presenter's largest single cost (a CPU profile of a hidden Daxx run: 29% of
  the command encoder's finish, which was most of a picture). Registering the same action twice does nothing the first did not
  (the first clears any discarded surface it covers; the second finds none), so the duplicates are dropped once, when the
  bundle is made. The same holds for a group bound again and again in one pass (the moby draws and the EE's steps switch
  between a few groups hundreds of times a picture): nothing in a pass discards a surface before the pass ends.
  Measured on hidden paced Daxx runs (fields 2400-4800), the presenter's encode time a picture (median): 5.29 ms before,
  4.14 ms with change 1 (3083 -> 4439 pictures drawn over the same fields), and 4.07 -> 3.62 ms for change 2 on top of it.
  Recording the bundles is the frame's pool threads' work too (the ties', terrain's and shrubs' bundles, every frame): appending
  every group's actions at each rebind and retaining them at the end, and merging a group's usages again at each rebind, were
  58 % of those threads' time on Veldin. With change 1 as it is now and change 3, `uya native` on a Veldin dump at 1080p
  (medians of 27 redraws): the ties' thread 1.98 -> 1.70 ms, their recording 1.96 -> 1.46 ms, the frame 2.93 -> 2.49 ms; the
  picture byte-identical.

How it is used
  port/Cargo.toml: [patch.crates-io] wgpu-core = { path = "vendor/wgpu-core" } (cargo reads [patch] only at the workspace root).
