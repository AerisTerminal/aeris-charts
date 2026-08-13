//! Canvas2D-exact source-over for the fixed-function premultiplied-alpha blend pipe.
//!
//! Measured Chrome (accelerated Canvas2D) integer math per 8-bit channel — the reference the
//! parity gates compare against (`?feature=volume` probe, 5632 sweeps, 0 model misses):
//!
//! ```text
//! a = 255 -> out = c                                   (opaque src replaces dst)
//! else    -> out = ((c*a + 127)/255) + ((d*(256-a))>>8)
//!            ^premul, round-half-up     ^dst scale, FLOOR
//! ```
//!
//! The hardware blend (`src + dst*(1-src.a)`, f32, one round-to-nearest at the unorm8 store)
//! reproduces that exactly for every channel whose premultiplied value rounds to ≥ 1:
//!
//! * Premultiply and quantize like the browser: `sp = (c*a + 127)/255` (round-half-up; ties are
//!   impossible since 255 is odd), done in the fragment shader in exact f32 integer math.
//! * Feed `sp - 255/512` (not `sp`) as the src color. The hardware's round-half-up then lands on
//!   the browser's floor: `round(sp - 0.498.. + d*q) = sp + floor(d*q)` because `d*q` fractions
//!   are multiples of `1/256` and `1/512` clears both the `frac = 0` and `frac = 255/256` edges
//!   (≥ 1/512 margin, vs ~1e-5 f32 error; no exact ties, so round-half-even GPUs agree).
//! * Use dst scale `q = (256-a)/256` by outputting `src.a = a/256`.
//!
//! Alphas 128 and 254 are special: there the classic `s = (255-a)/255` dst scale already matches
//! the browser's floor at every dst (verified exhaustively), including the `sp = 0` channels
//! where the shift would clamp at 0 — so those two alphas keep the unshifted `/255` path.
//!
//! Residual (hardware-bound, documented): channels with `sp = 0` (src contributes nothing:
//! `c*a < 127.5`) at alphas other than 128/254 cannot be shifted (fragment output clamps at 0),
//! so their dst scale rounds to nearest instead of flooring — off by at most 1/255, only when
//! `frac(d*(256-a)/256) >= 0.5`, i.e. at most a ±1 step on a channel the source barely touches.
//!
//! Colors that are not on the 8-bit grid (interpolated gradient pixels) keep the legacy f32
//! path — the browser quantizes per pixel too, and those pixels already match it.
//!
//! The clear color is opaque, so every dst under translucent prims starts on the exact 8-bit
//! grid this math assumes; 4x MSAA blends identically per sample on pixel-aligned quads, so the
//! resolve is a no-op for them.

/// WGSL twin of [`blend_output`]; concatenated into the quad and tri pipeline shaders.
pub(crate) const SOURCE_OVER_WGSL: &str = r#"
// Canvas2D-exact source-over for 8-bit-grid colors (see crates/nucleuscharts_render_wgpu/src/blend.rs).
fn nucleuscharts_source_over(c: vec4<f32>) -> vec4<f32> {
    let a255 = c.a * 255.0;
    let a8 = floor(a255 + 0.5);
    let c255 = c.rgb * 255.0;
    let c8 = floor(c255 + 0.5);
    let on_grid = abs(a255 - a8) < 0.001 && all(abs(c255 - c8) < vec3<f32>(0.001));
    if (!on_grid || a8 >= 255.0) {
        // interpolated (gradient) or fully opaque colors: legacy f32 premultiply
        return vec4<f32>(c.rgb * c.a, c.a);
    }
    if (a8 <= 0.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let sp = floor((c8 * a8 + 127.0) / 255.0);
    let exact255 = a8 == 128.0 || a8 == 254.0;
    let delta = select(0.498046875, 0.0, exact255);
    let inv_a = select((256.0 - a8) / 256.0, (255.0 - a8) / 255.0, exact255);
    let rgb = max(sp - vec3<f32>(delta), vec3<f32>(0.0)) / 255.0;
    return vec4<f32>(rgb, 1.0 - inv_a);
}
"#;

/// `255/512 = 0.5 - 1/512` — the src shift turning the hardware's round-to-nearest into the
/// browser's floor (see module docs). Dyadic, exact in f32.
#[cfg(test)]
pub(crate) const GRID_SHIFT: f32 = 255.0 / 512.0;

/// The browser's per-channel result (reference model, from the measured probe table).
#[cfg(test)]
pub(crate) fn chrome_source_over_channel(c: u8, a: u8, d: u8) -> u8 {
    if a == 255 {
        return c;
    }
    ((c as u32 * a as u32 + 127) / 255 + (d as u32 * (256 - a as u32)) / 256) as u8
}

/// Fragment-output twin of `SOURCE_OVER_WGSL` for an on-grid straight color: premultiplied rgb
/// (shifted) plus the alpha that makes the hardware dst factor match the browser's.
#[cfg(test)]
pub(crate) fn blend_output(c: [u8; 3], a: u8) -> [f32; 4] {
    if a == 255 {
        return [
            c[0] as f32 / 255.0,
            c[1] as f32 / 255.0,
            c[2] as f32 / 255.0,
            1.0,
        ];
    }
    if a == 0 {
        return [0.0; 4];
    }
    let premul = |ch: u8| (ch as u32 * a as u32 + 127) / 255;
    // alphas whose /255 dst scale matches the browser's floor at every dst, sp=0 included
    let exact255 = a == 128 || a == 254;
    let delta = if exact255 { 0.0 } else { GRID_SHIFT };
    let inv_a = if exact255 {
        (255 - a) as f32 / 255.0
    } else {
        (256 - a as u16) as f32 / 256.0
    };
    let channel = |i: usize| (premul(c[i]) as f32 - delta).max(0.0) / 255.0;
    [channel(0), channel(1), channel(2), 1.0 - inv_a]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulate the fixed-function pipe: premultiplied blend `src + dst*(1-src.a)` with one
    /// round-half-up at the 8-bit store.
    fn hw_channel(c: u8, a: u8, d: u8) -> u8 {
        let out = blend_output([c, c, c], a);
        let v = out[0] as f64 * 255.0 + d as f64 * (1.0 - out[3] as f64);
        (v + 0.5).floor() as u8
    }

    #[test]
    fn wgsl_function_present() {
        assert!(SOURCE_OVER_WGSL.contains("fn nucleuscharts_source_over"));
    }

    #[test]
    fn exhaustive_match_except_documented_sp_zero_residual() {
        let mut residual = 0u32;
        for a in 1..=254u8 {
            for c in 0..=255u8 {
                let sp = (c as u32 * a as u32 + 127) / 255;
                for d in 0..=255u8 {
                    let want = chrome_source_over_channel(c, a, d);
                    let got = hw_channel(c, a, d);
                    if want != got {
                        assert_eq!(
                            sp, 0,
                            "mismatch outside sp=0 residual: c={c} a={a} d={d} want={want} got={got}"
                        );
                        assert_eq!(
                            (want as i16 - got as i16).abs(),
                            1,
                            "residual larger than 1/255: c={c} a={a} d={d}"
                        );
                        residual += 1;
                    }
                }
            }
        }
        // precomputed offline: only sp=0 channels at alphas other than 128/254, ≤ 1/255 each
        assert_eq!(residual, 113_792);
    }

    #[test]
    fn alphas_128_and_254_exact_at_every_dst() {
        for a in [128u8, 254] {
            for c in 0..=255u8 {
                for d in 0..=255u8 {
                    assert_eq!(hw_channel(c, a, d), chrome_source_over_channel(c, a, d));
                }
            }
        }
    }

    #[test]
    fn volume_fixture_colors_match_chrome() {
        // Representative translucent market colors over two neutral destinations.
        let cases: [([u8; 3], [u8; 3], [u8; 3]); 4] = [
            ([247, 82, 95], [30, 30, 30], [139, 56, 63]),
            ([8, 153, 129], [30, 30, 30], [19, 92, 80]),
            ([247, 82, 95], [12, 12, 12], [130, 47, 54]),
            ([8, 153, 129], [12, 12, 12], [10, 83, 71]),
        ];
        for (src, dst, want) in cases {
            for ch in 0..3 {
                let got = hw_channel(src[ch], 128, dst[ch]);
                assert_eq!(got, want[ch], "src={src:?} dst={dst:?} ch={ch}");
                assert_eq!(got, chrome_source_over_channel(src[ch], 128, dst[ch]));
            }
        }
    }

    #[test]
    fn alpha_stays_opaque_over_opaque_dst() {
        for a in 1..=254u8 {
            let out = blend_output([7, 7, 7], a);
            let alpha = (out[3] as f64 * 255.0 + 255.0 * (1.0 - out[3] as f64)) + 0.5;
            assert_eq!(alpha.floor() as u8, 255, "a={a}");
        }
    }

    #[test]
    fn opaque_replaces_and_transparent_keeps_dst() {
        for c in 0..=255u8 {
            assert_eq!(hw_channel(c, 255, 18), c);
        }
        for d in 0..=255u8 {
            assert_eq!(hw_channel(0, 0, d), d);
        }
    }
}
