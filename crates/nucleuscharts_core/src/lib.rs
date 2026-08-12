//! nucleuscharts_core — platform-free chart model.
//!
//! Faithful port of the reference charting library model layer. All model math is `f64` (matching
//! JavaScript semantics); conversion to backend coordinate formats happens at render encoding.

pub mod format;
pub mod helpers;
pub mod model;
pub mod options;
pub mod scale;
pub mod style;

/// Media-space (CSS px) coordinate. Bitmap conversion happens at encode time only.
pub type Coordinate = f64;

/// Integer index into the merged time-scale point list. May be negative in logical space
/// (positions left of the first bar) — matches the reference charting library's `TimePointIndex`/`Logical`.
pub type TimePointIndex = i64;
