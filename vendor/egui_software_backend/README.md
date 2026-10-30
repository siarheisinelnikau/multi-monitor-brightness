# Vendored egui software rasterizer

Source: `egui_software_backend` 0.0.2, https://github.com/DGriffin91/egui_software_backend.
License: MIT OR Apache-2.0; original license files are included.

This copy contains the CPU rasterizer only. The optional upstream window host, examples, and test-image tooling are omitted. The egui dependency is adapted to the application's pinned 0.37.0 version, including ordered batched image deltas per texture and the removal of the old egui ahash re-export. Window management and Windows GDI presentation are implemented by `src/host.rs` in this application. No GPU backend is selected or linked.
