dlss_wgpu 6.0.0 as shipped on crates.io, with ONE change for the port (MIT OR Apache-2.0, see LICENSE-MIT / LICENSE-APACHE).
The port's native renderer (port/uya, src/dlss.rs) makes its Vulkan instance and device through it; this copy is the one the
port's workspace builds (port/Cargo.toml, [patch.crates-io]).

What changed (each place marked "PORT CHANGE", all in src/initialization.rs)
  When NGX answers a feature's extension query with an error of its own (a DlssError: FeatureNotSupported on a GPU without DLSS),
  `register_instance_extensions` and `register_device_extensions` mark that feature unsupported in `FeatureSupport` and keep
  NGX's answer in its new `super_resolution_error` / `ray_reconstruction_error`, as the crate already does when a required
  extension is missing; the instance or device is made without that feature's extensions. A Vulkan error still fails the call.
  Nothing else differs from the published crate (compare with `diff -r` against the crate unpacked from crates.io; its
  Cargo.lock, Dockerfile, .gitignore and the registry's marker files are not copied).

Why
  dlss_wgpu 6.0.0 ("Fix dlss_wgpu incorrectly turning errors into Result::Ok(())", its CHANGELOG) returns NGX's errors from these
  queries, so `request_device` failed on every GPU NGX says has no DLSS: the game stopped before its window opened. A player's
  report on 25 November 2026 (v0.1.2, GeForce GTX 1650 Ti, driver 617.14: "it appears DLSS is a requitement and not optional as
  stated in the readme"): Streamline's log showed the device made (its vkCreateDevice) and nothing after it, and NGX answered
  0xbad00001 (FeatureNotSupported) for that GPU.
