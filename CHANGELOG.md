# Changelog

Notable changes to Lifeline. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and version numbers
follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Satellite explorer engine: Sentinel-2 (2017–), Landsat (1982–) and
  Sentinel-1 radar through Planetary Computer, with true colour, infrared,
  agriculture, vegetation, moisture, water and burn products, plus two-date
  change maps for flooding, vegetation loss and burns.
- Supply ledger crate: lot-tracked stock by the ten classes of supply,
  cold-chain and controlled-item rules, a tamper-evident movement log,
  two-person sign-off for controlled items, FEFO allocation to deliveries,
  and CSV import/export.
- Optional AI assistant crate: cloud (Gemini, OpenRouter, Groq, Mistral) or
  local (Ollama, LM Studio, llama.cpp) providers, separate models for chat,
  vision and small jobs, off by default, with a local-only switch.
- App workspaces for *Area intel*, *Supply* and *Assistant*, wired to the
  engines above. Plan uses the chosen satellite base and supply catalogue.
- After a flood check, Plan lists requests that lost truck access and
  suggests drone or aircraft staging.
- Plan exports as GeoJSON, KML and Cursor-on-Target (ATAK).
- New app icon (medical cross with lifeline) for Windows, macOS and Linux;
  README banners and the in-app brand mark use the same mark.

### Changed

- Planning and map: line-of-sight computed once and cached, map payloads
  sent only when they change, roads drawn in fewer passes, pan/zoom
  updates once per frame.

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

[0.1.0]: https://github.com/sekaikx/lifeline/releases/tag/v0.1.0
