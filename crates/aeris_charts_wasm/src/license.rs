//! Runtime license notice printed to the browser console. NOTICE section 7(b) makes this a
//! required legal notice; only commercial licensees may suppress it through `licenseKey`.

pub(crate) const NOTICE: &str = "Powered by Aeris Charts (AGPL-3.0-only). Commercial license \
     required for closed-source deployment: integrations@aeristerminal.com | \
     https://aeristerminal.com";

/// True at most once per module instance, and never for a licensed caller.
fn take_announcement(announced: &std::sync::atomic::AtomicBool, licensed: bool) -> bool {
    !licensed && !announced.swap(true, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn license_notice(licensed: bool) {
    static ANNOUNCED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if take_announcement(&ANNOUNCED, licensed) {
        web_sys::console::info_1(&NOTICE.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn announces_once_and_never_for_licensed_hosts() {
        let announced = AtomicBool::new(false);
        assert!(!take_announcement(&announced, true));
        assert!(take_announcement(&announced, false));
        assert!(!take_announcement(&announced, false));
    }

    #[test]
    fn notice_carries_commercial_contact() {
        assert!(NOTICE.contains("integrations@aeristerminal.com"));
    }
}
