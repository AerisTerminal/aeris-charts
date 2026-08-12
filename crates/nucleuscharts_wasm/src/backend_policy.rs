//! Platform-neutral classification for WebGPU surface failures.
//!
//! Keeping this policy outside the browser-only chart module makes recovery semantics testable on
//! the host while the actual surface/canvas transition remains in the WASM adapter.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SurfaceErrorAction {
    Reconfigure,
    SkipFrame,
    Fallback,
}

pub(crate) fn surface_error_action(error: &wgpu::SurfaceError) -> SurfaceErrorAction {
    match error {
        wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => SurfaceErrorAction::Reconfigure,
        wgpu::SurfaceError::Timeout => SurfaceErrorAction::SkipFrame,
        wgpu::SurfaceError::OutOfMemory | wgpu::SurfaceError::Other => SurfaceErrorAction::Fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::{surface_error_action, SurfaceErrorAction};

    #[test]
    fn recoverable_surface_errors_reconfigure_once() {
        assert_eq!(
            surface_error_action(&wgpu::SurfaceError::Lost),
            SurfaceErrorAction::Reconfigure
        );
        assert_eq!(
            surface_error_action(&wgpu::SurfaceError::Outdated),
            SurfaceErrorAction::Reconfigure
        );
    }

    #[test]
    fn timeout_skips_only_the_current_frame() {
        assert_eq!(
            surface_error_action(&wgpu::SurfaceError::Timeout),
            SurfaceErrorAction::SkipFrame
        );
    }

    #[test]
    fn terminal_surface_errors_fall_back() {
        assert_eq!(
            surface_error_action(&wgpu::SurfaceError::OutOfMemory),
            SurfaceErrorAction::Fallback
        );
        assert_eq!(
            surface_error_action(&wgpu::SurfaceError::Other),
            SurfaceErrorAction::Fallback
        );
    }
}
