# System Logo Audit

Missing or needs-review system SVGs from the black/white logo migration.

Source: `frontend.log` after switching `resources/images/systems/` to SVG, plus manual review notes.

## Missing from bundled SVG set

These IDs appeared in the catalog but did not have matching `resources/images/systems/<id>.svg` files:

- Apogee — no authentic SVG yet; uses intentional centered text fallback (`АПОГЕЙ · БК 01` style would need a future trace).
- BK0011M — intentionally uses centered text fallback; available logo is just the system name.
- EDSAC — intentionally uses centered text fallback; no real logo found.
- Galaksija — intentionally uses centered text fallback; no real logo found.
- Interact — intentionally uses centered text fallback; only machine-photo logo found so far.
- Jupiter — intentionally uses centered text fallback; only Jupiter Ace machine-photo logo found so far.
- Laser
- MultiComp
- Orao
- QL
- RX78 — intentionally uses centered text fallback; Bandai company logo is not strongly associated with the old computer.
- Specialist
- TatungEinstein
- UK101
- Vector06C — intentionally uses centered text fallback; only PNG/unclear-source clear logo found.

## Resolved from external sources

- `AliceMC10` → `resources/images/systems/AliceMC10.svg`, grayscale/tintable TRS-80 MC-10 badge from Wikimedia Commons (`PD-textlogo`, trademarked logo notice applies).
- `AmstradPCW` → `resources/images/systems/AmstradPCW.svg`, grayscale/tintable 1980s Amstrad logo from Wikimedia Commons (`PD-textlogo`, trademarked logo notice applies).
- `CoCo2` → `resources/images/systems/CoCo2.svg`, tintable TRS-80 series logo from Wikimedia Commons (public-domain text/logo, trademarked logo notice applies).
- `PDP1` → `resources/images/systems/PDP1.svg`, tintable PDP-1 wordmark from Wikimedia Commons (`PD-textlogo`, trademarked logo notice applies).
- `VC4000` → `resources/images/systems/VC4000.svg`, tintable Interton Electronic wordmark from Wikimedia Commons (public-domain/trademarked logo notice applies).

## Resolved by artwork alias

- `MacPlus` → aliases to `systems/MacOS` in `Resources.qml`; Core/system catalog should probably treat `MacPlus` as a Mac OS artwork alias.
- `SVI328` → aliases to `systems/Spectravideo` in `Resources.qml`, using the local PRO100BYTE logo-pack asset.

## Resolved variant composites

- `Gameboy2P` → updated with original platform logo plus a 2-player/team mark from Noun Project (`team` by AbtoCreative, CC BY 3.0).
- `GBA2P` → updated with original platform logo plus a 2-player/team mark from Noun Project (`team` by AbtoCreative, CC BY 3.0).

## Initial upstream notes

Quick search of PRO100BYTE/console-logos found likely candidates only for:

- `AliceMC10` → resolved with TRS-80 MC-10 badge from Wikimedia Commons
- `MacPlus` → Apple Mac OS / Macintosh-style logo candidate
- `SVI328` → SpectraVideo candidate
- `CoCo2` → resolved with TRS-80 series logo from Wikimedia Commons

Most other missing IDs had no clear match in the PRO100BYTE main SVG set and likely need an external source, alias decision, or deliberate fallback. Missing system logos now render a large centered text fallback instead of a blank tile.

## Follow-up candidate notes

- `PDP1` → resolved with Wikimedia Commons `PDP-1 wordmark horizontal.svg`.
- `SVI328` → resolved with local `Spectravideo.svg` artwork alias; Wikimedia Commons also has `SVI 328 ID plate.svg` if a more exact ID-plate style is wanted later.
- `RX78` → Wikimedia Commons has `BANDAI.svg`, but the Bandai company wordmark is not strongly associated with this old computer; skip for now.
- `VC4000` → resolved with Wikimedia Commons Interton Electronic SVG wordmark.
- `Vector06C` → LaunchBox has a clear-logo PNG, but source/licensing is not obvious; skip for now.

## Regional logo variants

Systems where the logo itself differs by region (not just the name). Registered in
`REGIONAL_LOGOS` in `rust/frontend/src/system_logos.rs`; variant SVGs live alongside
the base art under `resources/images/systems/`.

Naming convention: `{id}.{region}.svg` where `{region}` is `us`, `eu`, or `jp`.

| System ID | US variant | EU variant | JP variant | Status |
|---|---|---|---|---|
| `Genesis` | `Genesis` (base) | `Genesis.eu` (Mega Drive) | `Genesis.jp` (Mega Drive) | resolved |
| `SNES` | `SNES` (base) | `SNES` (base) | `SNES.jp` (Super Famicom) | resolved |
| `NES` | `NES` (base) | `NES` (base) | `NES.jp` (Famicom) | resolved |
| `MasterSystem` | `MasterSystem` (base) | `MasterSystem` (base) | `MasterSystem.jp` (Mark III) | resolved |
| `MegaCD` | `MegaCD.us` (Sega CD) | `MegaCD` (base) | `MegaCD` (base) | resolved |
| `Sega32X` | `Sega32X` (base) | `Sega32X` (base) | `Sega32X.jp` (Super 32X) | placeholder - Wikimedia `Sega_32X_logo.svg`; no upstream Super 32X JP art found |
| `TurboGrafx16` | `TurboGrafx16` (base) | `TurboGrafx16.eu` (PC Engine) | `TurboGrafx16.jp` (PC Engine) | resolved |
| `TurboGrafx16CD` | `TurboGrafx16CD` (base) | `TurboGrafx16CD.eu` (PC Engine CD) | `TurboGrafx16CD.jp` (PC Engine CD) | resolved |
