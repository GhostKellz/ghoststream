# Changelog

## [0.1.0] - 2026-06-17

### Added

- Added the project documentation layout under `docs/` with `docs/README.md` as the index and lowercase descriptive pages grouped under `getting-started/`, `guides/`, `reference/`, `internals/`, and `advisories/`.
- Added root project policy documents: `CONTRIBUTING.md` and `SECURITY.md`.
- Added GitHub issue templates for bug reports and feature requests.
- Added Dependabot configuration for GitHub Actions and Cargo updates.
- Enabled GitHub vulnerability alerts and automated security fixes for the repository.
- Added Cargo audit advisory documentation under `docs/advisories/`.
- Added a root `PKGBUILD` as the single package build file.
- Added `Cargo.lock` for reproducible application builds.

### Changed

- Updated Rust dependencies with `cargo update`.
- Updated PipeWire and libspa dependencies to the `0.9` series.
- Migrated PipeWire call sites to the current `pipewire` `0.9` API.
- Configured `ort` for dynamic ONNX Runtime loading with the required `std`, `load-dynamic`, and `api-24` features plus CUDA and TensorRT support.
- Cleaned `.gitignore` so `Cargo.lock` is no longer ignored and local task scratch stays out of commits.

### Removed

- Removed the empty/random GitHub Actions workflow.
- Removed the root `SPEC.md`; ignored planning scratch remains under `tasks/`.

### Security

- Ran `cargo audit`; the only advisory currently accepted is `RUSTSEC-2024-0436` for transitive `paste` usage through `rav1e`/`ravif`/`image`.

### Verified

- `cargo check` passes.
- `cargo test` passes.
- `cargo audit` passes with the documented accepted advisory.
