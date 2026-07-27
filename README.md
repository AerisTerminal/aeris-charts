# Origin Charts

A production trading-chart engine in **Rust + WebGPU + WASM**, pixel-faithful to
the reference charting library with a long-term
trajectory toward a full TradingView-class charting platform.

## Documents

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — system design, crate layout, roadmap.
- [docs/RENDERING_SPEC.md](docs/RENDERING_SPEC.md) — exact pixel math ported from
  the reference charting library (the fidelity contract; every renderer must match it).

## Workspace

| Crate | Purpose |
|---|---|
| `crates/origin_core` | Platform-free chart model: scales, ranges, invalidation, formatters |
| `crates/origin_engine` | Headless chart instance: panes, series, data, layout, interaction, frame production |
| `crates/origin_render` | Draw-list IR + rendering math (bar widths, primitives) |
| `crates/origin_render_wgpu` | WebGPU backend (pipelines, glyph atlas) — WIP |
| `crates/origin_wasm` | wasm-bindgen host shell (DOM, events, RAF) — WIP |
| `packages/charts` | Public TypeScript API (`@origin/charts`), snake_case — WIP |

Naming convention: **snake_case everywhere**, including the public TS API.

## Install

The package is published privately to **GitHub Packages** (not the public npm registry).
Authenticate with a PAT that has `read:packages`, then:

```sh
bun add @origin/charts   # primary (Bun — see packages/charts/README.md for bunfig.toml)
npm install @origin/charts
```

The published package ships prebuilt JS + WASM — no Rust toolchain needed to consume it.

## Development

```sh
cargo test          # core math + rendering math unit tests
cargo clippy --all-targets
```

The vendored study copy of the reference charting library in `tmp/` is for reference only and is git-ignored.

## License

Proprietary. Origin Charts is commercial software; a paid commercial license is
required for any use beyond the 30-day internal evaluation period. See [LICENSE](LICENSE).
