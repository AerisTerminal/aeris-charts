//! The orbit camera of a 3D pane (complete build): ECharts GL `viewControl` angles, distance and
//! projection, their validation, and the world-to-screen mapping that frames and hits share.
//!
//! Angles follow ECharts GL's `OrbitControl`: `alpha` raises the eye above the box (negative
//! values look from below) and `beta` turns it around the vertical axis, so `beta = 0` looks at
//! the front of the box and `beta = 90` at its right side. The eye always looks at the box center
//! with the world `y` axis up. The view basis comes from the angles directly, so looking straight
//! down at `alpha = 90` is not degenerate.

use crate::{ChartError, ErrorCode};

const MIN_ALPHA: f64 = -90.0;
const MAX_ALPHA: f64 = 90.0;
const MIN_DISTANCE: f64 = 40.0;
const MAX_DISTANCE: f64 = 400.0;
const MIN_ORTHOGRAPHIC_SIZE: f64 = 20.0;
const MAX_ORTHOGRAPHIC_SIZE: f64 = 400.0;
/// Vertical field of view of the perspective camera in degrees (claygl's default, which ECharts
/// GL keeps).
const FIELD_OF_VIEW: f64 = 50.0;
/// Nearest view depth a perspective frame draws; geometry closer to the eye is clipped away.
pub(super) const NEAR: f64 = 0.1;
/// Eye distance of the orthographic camera, beyond every box corner so all depths are positive.
const ORTHOGRAPHIC_EYE: f64 = 1_000.0;

/// How a 3D pane projects its box (complete build).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection3d {
    #[default]
    Perspective,
    Orthographic,
}

/// Orbit camera of a 3D pane (complete build), ECharts GL `viewControl` shaped. Every 3D series
/// of a pane stores the same camera.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Camera3d {
    /// Elevation of the eye in degrees, `-90..=90`.
    pub alpha: f64,
    /// Rotation of the eye around the vertical axis in degrees, above `-180` through `180`.
    pub beta: f64,
    /// Eye distance from the box center in box units, `40..=400` (perspective).
    pub distance: f64,
    pub projection: Projection3d,
    /// Height of the visible volume in box units, `20..=400` (orthographic).
    pub orthographic_size: f64,
}

impl Default for Camera3d {
    fn default() -> Self {
        Self {
            alpha: 20.0,
            beta: 40.0,
            distance: 200.0,
            projection: Projection3d::Perspective,
            orthographic_size: 150.0,
        }
    }
}

impl Camera3d {
    pub(crate) fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// This camera with its angles, distance and size in range; `beta` wraps around.
    fn clamped(self) -> Self {
        let mut beta = (self.beta + 180.0).rem_euclid(360.0) - 180.0;
        if beta <= -180.0 {
            beta += 360.0;
        }
        Self {
            alpha: self.alpha.clamp(MIN_ALPHA, MAX_ALPHA),
            beta,
            distance: self.distance.clamp(MIN_DISTANCE, MAX_DISTANCE),
            projection: self.projection,
            orthographic_size: self
                .orthographic_size
                .clamp(MIN_ORTHOGRAPHIC_SIZE, MAX_ORTHOGRAPHIC_SIZE),
        }
    }

    /// This camera turned by the given degrees.
    pub(super) fn rotated(self, alpha: f64, beta: f64) -> Self {
        Self {
            alpha: self.alpha + alpha,
            beta: self.beta + beta,
            ..self
        }
        .clamped()
    }

    /// This camera moved `factor` times closer (perspective) or with a `factor` times smaller
    /// volume (orthographic).
    pub(super) fn zoomed(self, factor: f64) -> Self {
        Self {
            distance: self.distance / factor,
            orthographic_size: self.orthographic_size / factor,
            ..self
        }
        .clamped()
    }
}

pub(in crate::general_layout) fn validate_camera(camera: Camera3d) -> Result<(), ChartError> {
    let finite = [
        camera.alpha,
        camera.beta,
        camera.distance,
        camera.orthographic_size,
    ]
    .iter()
    .all(|value| value.is_finite());
    if finite && camera.clamped() == camera {
        Ok(())
    } else {
        Err(ChartError::new(
            ErrorCode::InvalidOptions,
            "3D cameras need alpha within -90..=90, beta above -180 through 180, distance within 40..=400 and orthographic size within 20..=400",
        ))
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// The world-to-screen mapping of one camera over one plot rectangle. World coordinates are box
/// units around the box center; screen coordinates are media px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct View3d {
    eye: [f64; 3],
    right: [f64; 3],
    up: [f64; 3],
    /// Unit vector from the box center toward the eye.
    back: [f64; 3],
    center: [f64; 2],
    /// Media px per box unit: at unit depth for perspective, everywhere for orthographic.
    scale: f64,
    perspective: bool,
}

impl View3d {
    pub(super) fn new(camera: &Camera3d, rect: [f64; 4]) -> Self {
        let (alpha, beta) = (camera.alpha.to_radians(), camera.beta.to_radians());
        let (sa, ca, sb, cb) = (alpha.sin(), alpha.cos(), beta.sin(), beta.cos());
        let back = [ca * sb, sa, ca * cb];
        let perspective = camera.projection == Projection3d::Perspective;
        let height = rect[3] - rect[1];
        let (distance, scale) = if perspective {
            (
                camera.distance,
                height / 2.0 / (FIELD_OF_VIEW.to_radians() / 2.0).tan(),
            )
        } else {
            (ORTHOGRAPHIC_EYE, height / camera.orthographic_size)
        };
        Self {
            eye: back.map(|axis| axis * distance),
            right: [cb, 0.0, -sb],
            up: [-sa * sb, ca, -sa * cb],
            back,
            center: [(rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0],
            scale,
            perspective,
        }
    }

    /// View coordinates of a world point: right, up, and depth in front of the eye.
    pub(super) fn view_point(&self, p: [f64; 3]) -> [f64; 3] {
        let v = sub(p, self.eye);
        [dot(v, self.right), dot(v, self.up), -dot(v, self.back)]
    }

    /// Whether view point `v` is in front of the near plane.
    pub(super) fn visible(&self, v: [f64; 3]) -> bool {
        !self.perspective || v[2] >= NEAR
    }

    /// Screen position of view point `v`, which must be [`visible`](Self::visible).
    pub(super) fn screen(&self, v: [f64; 3]) -> [f64; 2] {
        let scale = if self.perspective {
            self.scale / v[2]
        } else {
            self.scale
        };
        [self.center[0] + v[0] * scale, self.center[1] - v[1] * scale]
    }

    /// Screen position of world point `p`, or `None` behind the near plane.
    pub(super) fn project(&self, p: [f64; 3]) -> Option<[f64; 2]> {
        let v = self.view_point(p);
        self.visible(v).then(|| self.screen(v))
    }

    /// Whether a surface at `point` with outward `normal` faces the eye.
    pub(super) fn faces_eye(&self, point: [f64; 3], normal: [f64; 3]) -> bool {
        if self.perspective {
            dot(normal, sub(self.eye, point)) > 0.0
        } else {
            dot(normal, self.back) > 0.0
        }
    }

    pub(super) fn eye(&self) -> [f64; 3] {
        self.eye
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECT: [f64; 4] = [0.0, 0.0, 400.0, 300.0];

    #[test]
    fn default_camera_matches_echarts_gl_view_control() {
        let camera = Camera3d::default();
        assert!(validate_camera(camera).is_ok());
        let view = View3d::new(&camera, RECT);
        // OrbitControl: dir = (sin θ' cos φ', -cos θ', sin θ' sin φ') with θ' = α + π/2 and
        // φ' = π/2 - β, scaled by the distance.
        let (theta, phi) = (
            20f64.to_radians() + std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2 - 40f64.to_radians(),
        );
        let expected = [
            theta.sin() * phi.cos() * 200.0,
            -theta.cos() * 200.0,
            theta.sin() * phi.sin() * 200.0,
        ];
        for (axis, want) in expected.iter().enumerate() {
            assert!((view.eye()[axis] - want).abs() < 1e-9);
        }
        // The box center lands on the plot center and world up points up the screen.
        let center = view.project([0.0; 3]).unwrap();
        assert!((center[0] - 200.0).abs() < 1e-9 && (center[1] - 150.0).abs() < 1e-9);
        let up = view.project([0.0, 10.0, 0.0]).unwrap();
        assert!(up[1] < 150.0 && (up[0] - 200.0).abs() < 1e-9);
    }

    #[test]
    fn cameras_validate_clamp_and_wrap() {
        for invalid in [
            Camera3d {
                alpha: 91.0,
                ..Camera3d::default()
            },
            Camera3d {
                beta: -180.0,
                ..Camera3d::default()
            },
            Camera3d {
                distance: 39.0,
                ..Camera3d::default()
            },
            Camera3d {
                orthographic_size: f64::NAN,
                ..Camera3d::default()
            },
        ] {
            assert!(validate_camera(invalid).is_err(), "{invalid:?}");
        }
        let turned = Camera3d::default().rotated(100.0, 150.0);
        assert_eq!(turned.alpha, 90.0);
        assert_eq!(turned.beta, -170.0);
        assert!(validate_camera(turned).is_ok());
        let near = Camera3d::default().zoomed(100.0);
        assert_eq!((near.distance, near.orthographic_size), (40.0, 20.0));
    }

    #[test]
    fn straight_down_views_and_orthographic_scale_are_well_defined() {
        let top = View3d::new(
            &Camera3d {
                alpha: 90.0,
                ..Camera3d::default()
            },
            RECT,
        );
        let p = top.project([10.0, 0.0, 0.0]).unwrap();
        assert!(p.iter().all(|value| value.is_finite()));
        let ortho = View3d::new(
            &Camera3d {
                alpha: 0.0,
                beta: 0.0,
                projection: Projection3d::Orthographic,
                ..Camera3d::default()
            },
            RECT,
        );
        // 150 box units fill the 300 px plot height: 2 px per unit, independent of depth.
        assert_eq!(ortho.project([10.0, 0.0, 40.0]), Some([220.0, 150.0]));
        assert_eq!(ortho.project([10.0, 0.0, -40.0]), Some([220.0, 150.0]));
        assert!(ortho.faces_eye([0.0, 0.0, 50.0], [0.0, 0.0, 1.0]));
        assert!(!ortho.faces_eye([0.0, 0.0, -50.0], [0.0, 0.0, -1.0]));
    }
}
