# Contributing to Zaparoo Frontend

Thanks for taking the time to contribute. This guide covers the parts that
matter before a pull request: the CLA, local setup, and the checks we expect
you to run.

## Contributor License Agreement

Every contributor signs the [CLA](.github/CLA.md) once. It gives
**Wizzo Pty Ltd** (the legal entity behind the project) the license it needs
to use and sublicense your contribution. You keep your copyright; this is not a
copyright transfer. The CLA lets the company offer commercial licenses to
friendly partners without renegotiating every past contribution.

To sign, open your first pull request and post this exact comment:

> I have read the CLA Document and I hereby sign the CLA

CLA Assistant Lite records your signature in
`.github/contributors/signatures.json`. After that, future PRs are recognized
automatically.

## Development setup

Use [`docs/quickstart.md`](docs/quickstart.md) to get from a fresh clone to a
running frontend. You do not need MiSTer hardware. The repo includes a mock
Zaparoo Core you can start with `just mock-core`.

For the MiSTer ARM32 cross-build, sanitizer builds, and deployment, see
[`docs/building.md`](docs/building.md).

### Supported host platforms

- **Linux (x86_64)** is the main development target.
- **macOS** is best-effort. It should work, but CI does not cover it. Report
  breakage; patches welcome.
- **Windows** is not tested or actively supported. Use WSL2 instead.

## Before you open a pull request

Run these locally. CI runs the same checks and blocks merge when they fail.

```bash
just lint    # clang-format, clang-tidy, qmllint, rustfmt, clippy, cargo-deny
just test    # ctest + cargo nextest
```

Zero lint warnings is the bar. If a rule is wrong for the change you are
making, do not disable it quietly; call it out in the PR.

## Pull request conventions

The [PR template](.github/pull_request_template.md) asks for the details we
need. Two points matter most:

- **Explain why the change exists.** The diff already shows what changed.
- **Include screenshots or recordings for visual changes**, with the FPS
  counter visible at 720p and, if possible, 240p. It must stay green (≥55) at
  720p+ and not go red (<30) at 240p.

### Commit messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` for a new user-visible feature
- `fix:` for a bug fix
- `refactor:` for a code change that neither fixes a bug nor adds a feature
- `docs:` for documentation-only changes
- `test:` for adding or updating tests
- `chore:` for build, tooling, deps, or other housekeeping

Scopes are optional. Use one when it makes the summary clearer:
`feat(ui): add settings screen`, `fix(rust): handle empty catalog`.

### Branch naming

Use `feat/<short-description>`, `fix/<short-description>`,
`docs/<short-description>`, etc. Keep it readable; use hyphens, not
underscores.

## Main branch is protected

`main` only accepts changes through pull requests. Every PR needs:

1. All CI jobs green: Rust lint, Rust tests, desktop build + ctest + lint,
   ARM32 cross-build, and the CLA check.
2. A CLA signature recorded for the PR author.
3. At least one approving review from a maintainer other than the PR
   author.
4. The branch up to date with `main`.

Force-pushing and direct pushes to `main` are blocked.

## Bugs and feature requests

Open a GitHub issue. For bugs, include repro steps, expected behavior, actual
behavior, whether it reproduces on desktop and/or MiSTer, and a log excerpt
(`~/.local/share/zaparoo/logs/frontend.log` on desktop;
`/tmp/zaparoo/frontend.log` on MiSTer). For feature requests, say what you want
and why. If you plan to implement it, say that too so we can agree on scope
first.

## Questions?

- Architecture or design questions: open a GitHub Discussion or issue so the
  answer is searchable later.
- CLA or licensing: [legal@zaparoo.org](mailto:legal@zaparoo.org).
