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
- Migrated the crate to Rust 2024 edition.
- Updated package author metadata to `Christopher Kelley <ckelley@ghostkellz.sh>`.
- Updated PipeWire and libspa dependencies to the `0.10` series.
- Migrated PipeWire call sites to the current `pipewire` `0.10` API.
- Updated `ashpd`, `criterion`, `dirs`, `ndarray`, and `toml` to current Rust-compatible versions.
- Migrated portal screencast setup to the current `ashpd` options API.
- Configured `ort` for dynamic ONNX Runtime loading with the required `std`, `load-dynamic`, and `api-24` features plus CUDA and TensorRT support.
- Cleaned `.gitignore` so `Cargo.lock` is no longer ignored and local task scratch stays out of commits.

### Removed

- Removed the empty/random GitHub Actions workflow.
- Removed the root `SPEC.md`; ignored planning scratch remains under `tasks/`.

### Security

- Restricted `image` features to the formats Ghoststream uses, removing the transitive `ravif`/`rav1e`/`paste` advisory path.
- Closed superseded Dependabot pull requests after folding their dependency updates into `main`.
- Ran `cargo audit`; no vulnerabilities or accepted advisories remain.

### Verified

- `cargo check` passes.
- `cargo test` passes.
- `cargo audit` passes.
