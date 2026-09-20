# Nucleus Charts for Rust

Nucleus Charts is a high-performance financial chart engine with deterministic state, professional
interactions, drawings, indicators, backend-neutral frame construction, and native, WebGPU, and
WebAssembly rendering paths.

## Getting started

Use the headless engine directly:

```sh
cargo add nucleuscharts_engine
```

Add the renderer required by the host:

```sh
cargo add nucleuscharts_render_wgpu
# or
cargo add nucleuscharts_native
```

The current release is `0.2.0`. All published `nucleuscharts_*` crates in a coordinated release use
the same version.

## Crates

| Crate | Purpose |
| --- | --- |
| [`nucleuscharts_engine`](https://crates.io/crates/nucleuscharts_engine) | Chart state, interactions, drawings, indicators, and frame construction |
| [`nucleuscharts_core`](https://crates.io/crates/nucleuscharts_core) | Platform-free data, scales, options, validation, and formatting |
| [`nucleuscharts_indicators`](https://crates.io/crates/nucleuscharts_indicators) | Pure Rust technical-indicator calculations |
| [`nucleuscharts_render`](https://crates.io/crates/nucleuscharts_render) | Backend-neutral draw-list contract and rendering math |
| [`nucleuscharts_render_wgpu`](https://crates.io/crates/nucleuscharts_render_wgpu) | WebGPU executor |
| [`nucleuscharts_native`](https://crates.io/crates/nucleuscharts_native) | Native tiny-skia rasterizer and server-side PNG rendering |
| [`nucleuscharts_wasm`](https://crates.io/crates/nucleuscharts_wasm) | WebAssembly browser host |

The optional GPUI executor is available from the repository but is not published to crates.io. It
tracks a reviewed Zed commit whose API differs from the registry `gpui` release.

## License

Nucleus Charts is source-available under the
[PolyForm Noncommercial License 1.0.0](https://github.com/Axiusflowhq/financial-charts/blob/main/LICENSE).
Personal and noncommercial use is free. Business or commercial use requires a paid Commercial
License. Commercial licensing and custom engineering are available through the
[project repository](https://github.com/Axiusflowhq/financial-charts).
