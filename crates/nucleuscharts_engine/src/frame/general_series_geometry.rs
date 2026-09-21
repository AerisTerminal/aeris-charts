use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{IRect, Prim};

use crate::{ChartEngine, GeneralSeriesKind, DEFAULT_LINE_COLOR};

impl ChartEngine {
    pub(super) fn build_general_series_frame(
        &self,
        pane_index: usize,
        hpr: f64,
        vpr: f64,
        out: &mut Vec<Prim>,
    ) {
        let Some(pane_id) = self.pane_stable_id(pane_index) else {
            return;
        };
        for series in self
            .general_series_iter()
            .filter(|series| series.visible() && series.pane_id() == pane_id)
        {
            match series.kind() {
                GeneralSeriesKind::Column => {
                    let color = series
                        .color()
                        .and_then(Color::parse_css)
                        .unwrap_or(DEFAULT_LINE_COLOR);
                    self.visit_general_columns(series, |geometry| {
                        let left = (geometry.left * hpr).round() as i32;
                        let right = (geometry.right * hpr).round() as i32;
                        let top = (geometry.top * vpr).round() as i32;
                        let bottom = (geometry.bottom * vpr).round() as i32;
                        let width = (right - left).max(1);
                        let height = bottom - top;
                        if height <= 0 {
                            return;
                        }
                        out.push(Prim::Rect {
                            rect: IRect {
                                x: left,
                                y: top,
                                w: width,
                                h: height,
                            },
                            color,
                        });
                    });
                }
            }
        }
    }
}
