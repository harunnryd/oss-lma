# Contributing

## Setup

```bash
uv sync            # python workspace
cargo build        # rust workspace
```

## Ground rules

- `contracts/` is the single source of truth for event schemas and the
  error catalog. Change it first; regenerate/validate both language sides
  in the same change.
- TDD: write the failing test before the implementation. Ported pure
  algorithms get characterization tests against known-good fixtures before
  anything is rewritten.
- Code carries no comments or docstrings — names and structure explain
  intent.
- Keep provider-specific logic inside adapters; nothing outside
  `python/lma_stt` may import a vendor SDK.

## Pull requests

1. Tests green across both languages (`pytest`, `cargo test`)
2. Contract schemas validated on both sides
3. Docs updated when behavior described in `docs/` changes
