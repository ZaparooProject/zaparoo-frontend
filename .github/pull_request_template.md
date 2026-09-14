<!-- Thanks for the pull request. Please fill this out before requesting review. -->

## Summary

<!-- What changed, and why? One or two sentences is usually enough. -->

## Motivation

<!-- Link the issue or discussion that led to this. Delete this section if it does not apply. -->

## Screenshots / recordings

<!-- Required for visual changes. Include 720p and, if possible, 240p. `just snapshots` renders every screen offline. -->

## Test plan

<!-- How did you verify this? Manual steps and automated tests are both fine. -->

## Checklist

- [ ] `just lint` is green (zero warnings)
- [ ] `just test` passes
- [ ] If this touches animation or overlays, I kept the software renderer's dirty region small (see `docs/slint-gotchas.md`)
- [ ] If this could affect the MiSTer build, I considered ARM32 implications (see `docs/architecture.md`)
- [ ] If this adds user-visible strings, they are wrapped in `@tr()` and `just tr-extract` was run
- [ ] I have signed the [CLA](../.github/CLA.md) (first-time contributors only)
