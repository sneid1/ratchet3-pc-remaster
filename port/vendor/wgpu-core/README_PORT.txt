wgpu-core 30.0.1 as shipped on crates.io, with TWO changes for the port (MIT OR Apache-2.0, see LICENSE.MIT / LICENSE.APACHE).
The port's native renderer (port/uya) and its play window draw through wgpu 30; this copy is the one the port's workspace builds
(port/Cargo.toml, [patch.crates-io]).

What changed (each place marked "PORT CHANGE")
  1. src/command/bundle.rs, in `RenderBundleEncoder::finish`: the bundle's buffer and texture memory init actions keep only
     the first of each (same buffer or texture, same range, same kind), in order.
  2. src/command/pass.rs, `flush_bindings_helper`: a bind group's buffer and texture init actions are registered the first
     time the group is bound in a pass, not at every binding (`PassState::registered_groups`, set empty where render.rs and
     compute.rs begin a pass).
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

How it is used
  port/Cargo.toml: [patch.crates-io] wgpu-core = { path = "vendor/wgpu-core" } (cargo reads [patch] only at the workspace root).
