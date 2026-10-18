# Changelog

Notable changes to Lifeline. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and version numbers
follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Place search in *Plan*: find any spot by name through OpenStreetMap
  Nominatim, cached for a day.
- Tactical 1 km graticule on the map (heavy lines every 5 km with kilometre
  labels) behind a new *Ops grid* toggle.
- Map markers — depot, requests and observed areas — are keyboard accessible:
  focus with Tab, move with the arrow keys (Shift for coarse steps), Enter
  opens a request.
- Hypsometric elevation legend on the map key, matching the terrain tint.
- Assistant provider presets for DeepSeek, Cerebras, xAI, GitHub Models and
  NVIDIA NIM.
- Satellite base layer reports a failure to load instead of silently falling
  back to relief.

### Changed

- Terrain relief re-coloured with a full hypsometric ramp (forest floor to
  ridgeline per theme) and a two-light hillshade so lee slopes stay readable.

### Fixed

- Rapid area loads or flood checks could interleave and apply an older result
  over a newer one; both now guard against out-of-order completion.
- Satellite tile decoding moved off the render path; a corrupt or missing
  image no longer lingers.
- Duplicate exports launched parallel processes when pressed repeatedly;
  exports honour the busy state.
- Map label cache no longer grows without bound over a long session.
- Backend error objects stringify into readable text instead of `{}`.

## [0.2.0] - 2026-10-16

### Added

- *Supply* workspace: lot-tracked stock by the ten classes of supply,
  cold-chain and controlled-item rules, tamper-evident ledger with two-person
  sign-off, FEFO allocation to the current plan, CSV import/export, example
  stock.
- *Area intel* workspace: Sentinel-2 (2017–), Landsat (1982–) and Sentinel-1
  radar through Planetary Computer; true colour, infrared, agriculture,
  vegetation, moisture, water, burn and radar products; two-date change maps;
  A1–H8 grid; apply a scene as the Plan map base.
- *Assistant* workspace: optional chat over the current plan, parse radio-style
  text into a request, read a satellite scene; cloud or local models; off by
  default with a local-only switch.
- Plan staging: after a flood check, list requests that lost truck access and
  recommend drone or aircraft.
- Plan exports as GeoJSON, KML and Cursor-on-Target (ATAK).
- New app icon (medical cross with lifeline) for Windows, macOS and Linux.

### Changed

- Planning and map: line-of-sight computed once and cached, map payloads sent
  only when they change, roads drawn in fewer passes, pan/zoom once per frame.
- Top bar holds five workspaces; brand mark uses the new icon.

## [0.1.0] - 2026-10-15

First public release.

### Added

- Desktop app for Windows, macOS and Linux with two workspaces: *Plan* and
  *Airdrop lab*.
- Real areas from open data: elevation from AWS Terrain Tiles, roads and
  settlements from OpenStreetMap, wind from MET Norway or Open-Meteo, all
  cached on disk for offline use. Presets for Gorkha (Nepal), Dadu
  (Pakistan) and Kahramanmaraş (Türkiye), any latitude and longitude, or a
  generated valley.
- Truck, drone and airdrop options for every request, with travel time,
  trips, probability of arrival and cost, and a scheduler that serves
  requests by priority and deadline within a risk tolerance.
- Line-of-sight exposure from observed areas for ground vehicles, drones
  and aircraft; hazard zones that close roads or add risk.
- Satellite flood check on Sentinel-2 L2A imagery from Microsoft Planetary
  Computer, with an MNDWI water index and an optional U-Net run through
  ONNX. Training, evaluation and export code in `ml/`.
- Airdrop physics: point mass with drag through the International Standard
  Atmosphere and a wind profile, parachute inflation, Euler and RK4
  integrators, release-point solver and Monte Carlo landing dispersion.
- `lifeline` command-line tool with `example`, `plan`, `wind`, `flood` and
  `airdrop` subcommands, reading the same scenario JSON the app saves.
- Installers built by GitHub Actions for each tagged release.

[0.2.0]: https://github.com/sekaikx/lifeline/releases/tag/v0.2.0
[0.1.0]: https://github.com/sekaikx/lifeline/releases/tag/v0.1.0
