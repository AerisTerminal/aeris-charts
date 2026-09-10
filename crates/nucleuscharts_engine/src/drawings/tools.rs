//! Compile-time drawing-tool catalog.
//!
//! Tool semantics belong here rather than in browser/native hosts.  The catalog is deliberately
//! static: Nucleus needs one deterministic implementation shared by every backend, not a runtime
//! plugin registry.  New built-in tools should describe their placement/editing invariants here
//! and keep only genuinely tool-specific geometry/math in the drawing engine.

use super::DrawingKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawingPlacement {
    /// Place a fixed number of anchors from ordinary click/tap activations.
    ClickAnchors { count: u8 },
    /// One click commits a tool-specific preset geometry around that semantic origin. The preset
    /// owns its generated defining points; the host still forwards an ordinary activation.
    SingleClickPreset { points: u8 },
    /// Place a fixed number of anchors immediately from pointer press.  This is currently the text
    /// tool so the platform editor can open without a trailing compatibility click.
    PressAnchors { count: u8 },
    /// Repeated clicks/taps append anchors until an explicit finish action.
    MultiClick { minimum: u8 },
    /// Pointer-down / move / pointer-up capture with engine-owned sample decimation.
    Freehand { minimum: u8 },
}

impl DrawingPlacement {
    pub(crate) const fn minimum_points(self) -> usize {
        match self {
            Self::ClickAnchors { count }
            | Self::PressAnchors { count }
            | Self::SingleClickPreset { points: count } => count as usize,
            Self::MultiClick { minimum } | Self::Freehand { minimum } => minimum as usize,
        }
    }

    pub(crate) const fn valid_point_count(self, count: usize) -> bool {
        match self {
            Self::ClickAnchors { count: exact }
            | Self::PressAnchors { count: exact }
            | Self::SingleClickPreset { points: exact } => count == exact as usize,
            Self::MultiClick { minimum } | Self::Freehand { minimum } => count >= minimum as usize,
        }
    }

    pub(crate) const fn is_sequence(self) -> bool {
        matches!(self, Self::MultiClick { .. })
    }

    pub(crate) const fn is_freehand(self) -> bool {
        matches!(self, Self::Freehand { .. })
    }

    pub(crate) const fn places_on_press(self) -> bool {
        matches!(self, Self::PressAnchors { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawingHandleMode {
    None,
    Anchors,
    Endpoints,
    RectangleBounds,
    Position,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawingMovementAxis {
    Both,
    HorizontalOnly,
    VerticalOnly,
}

impl DrawingMovementAxis {
    pub(crate) const fn constrain(self, dx: f64, dy: f64) -> (f64, f64) {
        match self {
            Self::Both => (dx, dy),
            Self::HorizontalOnly => (dx, 0.0),
            Self::VerticalOnly => (0.0, dy),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawingStraightenMode {
    None,
    Segment45,
    Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawingLogicalExtent {
    Finite,
    Full,
    FromFirst,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawingPriceExtent {
    Finite,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DrawingToolSpec {
    pub(crate) kind: DrawingKind,
    pub(crate) wire_id: u8,
    pub(crate) name: &'static str,
    pub(crate) placement: DrawingPlacement,
    pub(crate) handles: DrawingHandleMode,
    pub(crate) movement_axis: DrawingMovementAxis,
    pub(crate) straighten: DrawingStraightenMode,
    pub(crate) logical_extent: DrawingLogicalExtent,
    pub(crate) price_extent: DrawingPriceExtent,
    /// Conservative semantic-bounds expansion for curved/freehand interpolation.
    pub(crate) bounds_padding_ratio: f64,
    pub(crate) default_width: f64,
    /// Placement commits directly into a platform text-edit session.  The editor itself remains a
    /// host concern, but the decision that this tool requests one is canonical engine metadata.
    pub(crate) requests_text_editor: bool,
}

const TREND_LINE: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::TrendLine,
    wire_id: 0,
    name: "trend_line",
    placement: DrawingPlacement::ClickAnchors { count: 2 },
    handles: DrawingHandleMode::Anchors,
    movement_axis: DrawingMovementAxis::Both,
    straighten: DrawingStraightenMode::Segment45,
    logical_extent: DrawingLogicalExtent::Finite,
    price_extent: DrawingPriceExtent::Finite,
    bounds_padding_ratio: 0.0,
    default_width: 2.0,
    requests_text_editor: false,
};

const HORIZONTAL_LINE: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::HorizontalLine,
    wire_id: 1,
    name: "horizontal_line",
    placement: DrawingPlacement::ClickAnchors { count: 1 },
    handles: DrawingHandleMode::Anchors,
    movement_axis: DrawingMovementAxis::VerticalOnly,
    straighten: DrawingStraightenMode::None,
    logical_extent: DrawingLogicalExtent::Full,
    price_extent: DrawingPriceExtent::Finite,
    bounds_padding_ratio: 0.0,
    default_width: 2.0,
    requests_text_editor: false,
};

const HORIZONTAL_RAY: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::HorizontalRay,
    wire_id: 2,
    name: "horizontal_ray",
    placement: DrawingPlacement::ClickAnchors { count: 1 },
    handles: DrawingHandleMode::Anchors,
    movement_axis: DrawingMovementAxis::Both,
    straighten: DrawingStraightenMode::None,
    logical_extent: DrawingLogicalExtent::FromFirst,
    price_extent: DrawingPriceExtent::Finite,
    bounds_padding_ratio: 0.0,
    default_width: 2.0,
    requests_text_editor: false,
};

const VERTICAL_LINE: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::VerticalLine,
    wire_id: 3,
    name: "vertical_line",
    placement: DrawingPlacement::ClickAnchors { count: 1 },
    handles: DrawingHandleMode::Anchors,
    movement_axis: DrawingMovementAxis::HorizontalOnly,
    straighten: DrawingStraightenMode::None,
    logical_extent: DrawingLogicalExtent::Finite,
    price_extent: DrawingPriceExtent::Full,
    bounds_padding_ratio: 0.0,
    default_width: 2.0,
    requests_text_editor: false,
};

const RECTANGLE: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::Rectangle,
    wire_id: 4,
    name: "rectangle",
    placement: DrawingPlacement::ClickAnchors { count: 2 },
    handles: DrawingHandleMode::RectangleBounds,
    movement_axis: DrawingMovementAxis::Both,
    straighten: DrawingStraightenMode::Square,
    logical_extent: DrawingLogicalExtent::Finite,
    price_extent: DrawingPriceExtent::Finite,
    bounds_padding_ratio: 0.0,
    default_width: 1.0,
    requests_text_editor: false,
};

const TEXT: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::Text,
    wire_id: 5,
    name: "text",
    placement: DrawingPlacement::PressAnchors { count: 1 },
    handles: DrawingHandleMode::None,
    movement_axis: DrawingMovementAxis::Both,
    straighten: DrawingStraightenMode::None,
    logical_extent: DrawingLogicalExtent::Finite,
    price_extent: DrawingPriceExtent::Finite,
    bounds_padding_ratio: 0.0,
    default_width: 2.0,
    requests_text_editor: true,
};

const BRUSH: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::Brush,
    wire_id: 6,
    name: "brush",
    placement: DrawingPlacement::Freehand { minimum: 2 },
    handles: DrawingHandleMode::Endpoints,
    movement_axis: DrawingMovementAxis::Both,
    straighten: DrawingStraightenMode::None,
    logical_extent: DrawingLogicalExtent::Finite,
    price_extent: DrawingPriceExtent::Finite,
    bounds_padding_ratio: 0.25,
    default_width: 2.0,
    requests_text_editor: false,
};

const PATH: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::Path,
    wire_id: 7,
    name: "path",
    placement: DrawingPlacement::MultiClick { minimum: 2 },
    handles: DrawingHandleMode::Anchors,
    movement_axis: DrawingMovementAxis::Both,
    straighten: DrawingStraightenMode::None,
    logical_extent: DrawingLogicalExtent::Finite,
    price_extent: DrawingPriceExtent::Finite,
    bounds_padding_ratio: 0.0,
    default_width: 2.0,
    requests_text_editor: false,
};

const LONG_POSITION: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::LongPosition,
    wire_id: 8,
    name: "long_position",
    placement: DrawingPlacement::SingleClickPreset { points: 3 },
    handles: DrawingHandleMode::Position,
    movement_axis: DrawingMovementAxis::Both,
    straighten: DrawingStraightenMode::None,
    logical_extent: DrawingLogicalExtent::Finite,
    price_extent: DrawingPriceExtent::Finite,
    bounds_padding_ratio: 0.0,
    default_width: 1.0,
    requests_text_editor: false,
};

const SHORT_POSITION: DrawingToolSpec = DrawingToolSpec {
    kind: DrawingKind::ShortPosition,
    wire_id: 9,
    name: "short_position",
    placement: DrawingPlacement::SingleClickPreset { points: 3 },
    handles: DrawingHandleMode::Position,
    movement_axis: DrawingMovementAxis::Both,
    straighten: DrawingStraightenMode::None,
    logical_extent: DrawingLogicalExtent::Finite,
    price_extent: DrawingPriceExtent::Finite,
    bounds_padding_ratio: 0.0,
    default_width: 1.0,
    requests_text_editor: false,
};

pub(crate) const DRAWING_TOOL_SPECS: [DrawingToolSpec; 10] = [
    TREND_LINE,
    HORIZONTAL_LINE,
    HORIZONTAL_RAY,
    VERTICAL_LINE,
    RECTANGLE,
    TEXT,
    BRUSH,
    PATH,
    LONG_POSITION,
    SHORT_POSITION,
];

impl DrawingKind {
    pub(crate) const fn spec(self) -> &'static DrawingToolSpec {
        match self {
            Self::TrendLine => &TREND_LINE,
            Self::HorizontalLine => &HORIZONTAL_LINE,
            Self::HorizontalRay => &HORIZONTAL_RAY,
            Self::VerticalLine => &VERTICAL_LINE,
            Self::Rectangle => &RECTANGLE,
            Self::Text => &TEXT,
            Self::Brush => &BRUSH,
            Self::Path => &PATH,
            Self::LongPosition => &LONG_POSITION,
            Self::ShortPosition => &SHORT_POSITION,
        }
    }
}
