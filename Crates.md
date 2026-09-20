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

The repository is preparing coordinated release `0.3.0`. All published `nucleuscharts_*` crates in
a coordinated release use the same version. Previously published artifacts retain the license
bundled with their release.

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

Nucleus Charts is open-source software under the
[GNU Affero General Public License v3.0](https://github.com/Axiusflowhq/financial-charts/blob/main/LICENSE),
identified by `AGPL-3.0-only`. A separate commercial license is available for proprietary,
OEM/embedded, and white-label use; see the
[commercial licensing notice](https://github.com/Axiusflowhq/financial-charts/blob/main/COMMERCIAL_LICENSE.md).
