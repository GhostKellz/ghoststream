# Contributing to GhostStream

## Development

```bash
cargo fmt
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
```

Keep changes focused and update documentation when behavior, configuration, or public APIs change.

## Documentation

Keep `docs/README.md` as an index only. Put documentation under topic folders with lowercase, hyphenated Markdown filenames.

Use `docs/advisories/` for RustSec and dependency audit notes.

## Issues

Include OS, compositor, PipeWire version, GPU/driver, FFmpeg version, command line, and relevant logs for capture, encode, or output failures.

## License

By contributing, you agree that your contributions are licensed under the MIT License.
