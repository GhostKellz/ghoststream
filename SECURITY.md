# Security Policy

## Reporting Vulnerabilities

Please report vulnerabilities privately. Do not open public GitHub issues for security problems.

Include affected versions, platform details, reproduction steps, logs, and whether the issue requires local access.

## Supported Versions

GhostStream is pre-1.0 and under active development.

| Version | Supported |
|---------|-----------|
| 0.1.x | Yes |

## Security Considerations

- Screen and window capture should go through xdg-desktop-portal consent.
- Audio capture may include desktop or microphone content.
- Stream URLs and keys are sensitive.
- Virtual camera output exposes frames to local applications.
- Support bundles and logs should be reviewed before public sharing.

## Dependency Auditing

```bash
cargo audit
```

Advisory tracking lives in [docs/advisories/](docs/advisories/).
