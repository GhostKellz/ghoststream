# Accepted Advisories

Audit performed: 2026-06-17

Run `cargo audit` after dependency updates and record any accepted advisories here.

| Advisory | Crate | Severity | Source | Disposition |
|----------|-------|----------|--------|-------------|
| RUSTSEC-2024-0436 | `paste 1.0.15` | Warning, unmaintained | `rav1e` via `ravif` and `image` | Accepted until upstream image/AVIF dependencies remove `paste` |
