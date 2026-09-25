//! Per-direction color-override resolution for the series color options.
//!
//! The wasm boundary distinguishes three JS inputs so a pinned color can be *un-pinned* back to
//! its follow state (reference parity): `undefined` = keep the current value, `""` = clear the
//! override (follow the direction's default/body color), any string = pin it **verbatim**
//! (reference `series.options()` returns the applied string). The string is parsed only at render
//! time, where an unparseable value falls back to the default — so named colors and future
//! CSS forms round-trip through `options()` exactly as applied.

/// Apply an `Option<String>` color input to a verbatim override `slot`. See the module docs
/// for the keep/clear/pin contract.
pub(crate) fn update_color_slot(slot: &mut Option<String>, value: Option<String>) {
    match value {
        None => {}
        Some(v) if v.is_empty() => *slot = None,
        Some(v) => *slot = Some(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_clears_and_pins_verbatim() {
        let mut slot: Option<String> = None;

        // `undefined` (None) leaves the slot untouched.
        update_color_slot(&mut slot, None);
        assert_eq!(slot, None);

        // A CSS color pins the override, stored verbatim (case and form preserved).
        update_color_slot(&mut slot, Some("#FF0000".to_string()));
        assert_eq!(slot.as_deref(), Some("#FF0000"));

        // `undefined` still keeps the pinned color (the case the TS `?? ""` fallback broke).
        update_color_slot(&mut slot, None);
        assert_eq!(slot.as_deref(), Some("#FF0000"));

        // An empty string clears the override back to its follow state.
        update_color_slot(&mut slot, Some(String::new()));
        assert_eq!(slot, None);

        // A string the renderer cannot parse is still stored verbatim (render falls back to
        // the default), so `options()` returns exactly what was applied.
        update_color_slot(&mut slot, Some("not-a-color".to_string()));
        assert_eq!(slot.as_deref(), Some("not-a-color"));
    }
}
