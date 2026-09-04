//! Primitive command-buffer decoder (plugin platform Phase C-a).
//!
//! JS pane primitives never touch a canvas: their renderer calls the draw functions on the
//! host-built context, each of which records one plain JS object into a command array. After
//! the renderer returns, the host JSON-stringifies that array (one marshalling pass per
//! primitive per frame) and this module decodes it into the backend-neutral [`Prim`] IR the
//! WebGPU and Canvas2D executors share — so plugin content is pixel-identical across backends
//! by construction.
//!
//! The decoder is pure (no JS, no DOM): it takes the JSON text plus the pane's shared point
//! pool and returns prims plus human-readable warnings for skipped input, so it is fully
//! host-testable. Coordinates arrive in absolute bitmap px (the draw context's converters
//! already applied the pane's pixel ratios and offset); integer prims round here exactly like
//! the engine's own geometry.

use nucleuscharts_engine::line_style_from_u8;
use nucleuscharts_render::color::Color;
use nucleuscharts_render::draw_list::{Gradient, IRect, LineType, Prim, TextAlign};

/// Defaults the host folds into `text` commands (the draw context has no font state of its
/// own): the layout font family, the layout font size scaled to the pane's bitmap px, and the
/// layout text color — the same sources the engine's own axis labels draw with.
#[derive(Clone, Debug)]
pub struct TextDefaults {
    pub family: String,
    /// Bitmap px (the command coordinate space): `layout.fontSize × dpr`.
    pub size: f32,
    pub color: Color,
}

/// Decode result: the prims produced (in command order) plus one warning per skipped command.
#[derive(Clone, Debug, Default)]
pub struct DecodedCommands {
    pub prims: Vec<Prim>,
    pub warnings: Vec<String>,
}

impl DecodedCommands {
    fn warn(&mut self, index: usize, detail: String) {
        self.warnings.push(format!("command {index}: {detail}"));
    }
}

fn num(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key)?.as_f64().filter(|v| v.is_finite())
}

fn color(value: &serde_json::Value, key: &str) -> Option<Color> {
    Color::parse_css(value.get(key)?.as_str()?)
}

/// Optional CSS color slot (`undefined`/`null`/absent → None; unparseable → None).
fn optional_color(value: &serde_json::Value, key: &str) -> Option<Color> {
    value.get(key)?.as_str().and_then(Color::parse_css)
}

fn style(value: &serde_json::Value) -> nucleuscharts_render::draw_list::LineStyle {
    line_style_from_u8(num(value, "style").unwrap_or(0.0).clamp(0.0, 255.0) as u8)
}

/// Push `count` points from a flat `[x0,y0,x1,y1,...]` JSON array into the shared pool.
/// Returns the `(first_point, point_count)` window, or `None` when the array is malformed or
/// holds fewer than two points (both executors ignore degenerate runs, but skipping keeps the
/// pool clean).
fn push_points(value: &serde_json::Value, pool: &mut Vec<[f32; 2]>) -> Option<(u32, u32)> {
    let flat = value.get("points")?.as_array()?;
    let first = pool.len();
    for pair in flat.as_chunks::<2>().0 {
        let (Some(x), Some(y)) = (pair[0].as_f64(), pair[1].as_f64()) else {
            pool.truncate(first);
            return None;
        };
        if !x.is_finite() || !y.is_finite() {
            pool.truncate(first);
            return None;
        }
        pool.push([x as f32, y as f32]);
    }
    let count = pool.len() - first;
    if count < 2 {
        pool.truncate(first);
        return None;
    }
    Some((first as u32, count as u32))
}

fn decode_one(
    command: &serde_json::Value,
    pool: &mut Vec<[f32; 2]>,
    text_defaults: &TextDefaults,
) -> Result<Prim, String> {
    let kind = command
        .get("c")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "missing \"c\" kind".to_string())?;
    match kind {
        "rect" => Ok(Prim::Rect {
            rect: IRect {
                x: num(command, "x").ok_or("x")?.round() as i32,
                y: num(command, "y").ok_or("y")?.round() as i32,
                w: num(command, "w").ok_or("w")?.round() as i32,
                h: num(command, "h").ok_or("h")?.round() as i32,
            },
            color: color(command, "color").ok_or("color")?,
        }),
        "rect_frame" => Ok(Prim::RectFrame {
            rect: IRect {
                x: num(command, "x").ok_or("x")?.round() as i32,
                y: num(command, "y").ok_or("y")?.round() as i32,
                w: num(command, "w").ok_or("w")?.round() as i32,
                h: num(command, "h").ok_or("h")?.round() as i32,
            },
            border: num(command, "line_width")
                .ok_or("line_width")?
                .round()
                .max(1.0) as i32,
            color: color(command, "color").ok_or("color")?,
        }),
        "hline" => Ok(Prim::HLine {
            y: num(command, "y").ok_or("y")?.round() as i32,
            x0: num(command, "x1").ok_or("x1")?.round() as i32,
            x1: num(command, "x2").ok_or("x2")?.round() as i32,
            width: num(command, "width").ok_or("width")?.round().max(1.0) as i32,
            style: style(command),
            color: color(command, "color").ok_or("color")?,
        }),
        "vline" => Ok(Prim::VLine {
            x: num(command, "x").ok_or("x")?.round() as i32,
            y0: num(command, "y1").ok_or("y1")?.round() as i32,
            y1: num(command, "y2").ok_or("y2")?.round() as i32,
            width: num(command, "width").ok_or("width")?.round().max(1.0) as i32,
            style: style(command),
            color: color(command, "color").ok_or("color")?,
        }),
        "polyline" => {
            let (first_point, point_count) = push_points(command, pool)
                .ok_or("points (need a flat [x,y,...] array of 2+ points)")?;
            Ok(Prim::Polyline {
                first_point,
                point_count,
                width: num(command, "width").ok_or("width")? as f32,
                style: style(command),
                line_type: LineType::Simple,
                color: color(command, "color").ok_or("color")?,
            })
        }
        "area_fill" => {
            let (first_point, point_count) = push_points(command, pool)
                .ok_or("points (need a flat [x,y,...] array of 2+ points)")?;
            Ok(Prim::AreaFill {
                first_point,
                point_count,
                base_y: num(command, "base_y").ok_or("base_y")? as f32,
                line_type: LineType::Simple,
                gradient: Gradient {
                    top: color(command, "top_color").ok_or("top_color")?,
                    bottom: color(command, "bottom_color").ok_or("bottom_color")?,
                },
            })
        }
        "circle" => Ok(Prim::Circle {
            cx: num(command, "x").ok_or("x")? as f32,
            cy: num(command, "y").ok_or("y")? as f32,
            radius: num(command, "r").ok_or("r")? as f32,
            fill: color(command, "fill_color").ok_or("fill_color")?,
            stroke_width: num(command, "border_width").unwrap_or(0.0) as f32,
            stroke: optional_color(command, "border_color").unwrap_or(Color::rgba(0, 0, 0, 0)),
        }),
        "round_rect" => {
            let r = num(command, "r").ok_or("r")? as f32;
            Ok(Prim::RoundRect {
                x: num(command, "x").ok_or("x")? as f32,
                y: num(command, "y").ok_or("y")? as f32,
                w: num(command, "w").ok_or("w")? as f32,
                h: num(command, "h").ok_or("h")? as f32,
                radii: [r, r, r, r],
                fill: color(command, "color").ok_or("color")?,
                border_width: 0.0,
                border_color: Color::rgba(0, 0, 0, 0),
            })
        }
        "triangle" => Ok(Prim::Triangle {
            a: [
                num(command, "x1").ok_or("x1")? as f32,
                num(command, "y1").ok_or("y1")? as f32,
            ],
            b: [
                num(command, "x2").ok_or("x2")? as f32,
                num(command, "y2").ok_or("y2")? as f32,
            ],
            c: [
                num(command, "x3").ok_or("x3")? as f32,
                num(command, "y3").ok_or("y3")? as f32,
            ],
            color: color(command, "color").ok_or("color")?,
        }),
        // Text runs decode fully: the anchor (x = aligned edge, y = vertical center — the
        // axis labels' middle-baseline convention), the run, and the font components, with the
        // host's layout defaults folded in so the prim is backend-ready. Both backends paint
        // it in layer order through the browser's text engine (Canvas2D `fillText` directly,
        // WebGPU via a host-rasterized atlas quad of the same run).
        "text" => Ok(Prim::Text {
            x: num(command, "x").ok_or("x")? as f32,
            y: num(command, "y").ok_or("y")? as f32,
            text: command
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            color: optional_color(command, "color").unwrap_or(text_defaults.color),
            size: num(command, "size")
                .filter(|s| *s > 0.0)
                .unwrap_or(f64::from(text_defaults.size)) as f32,
            family: command
                .get("font")
                .and_then(serde_json::Value::as_str)
                .filter(|f| !f.is_empty())
                .unwrap_or(&text_defaults.family)
                .to_string(),
            align: match command.get("align").and_then(serde_json::Value::as_str) {
                Some("center") => TextAlign::Center,
                Some("right") => TextAlign::Right,
                _ => TextAlign::Left,
            },
            // Numeric CSS weight (100–900) wins; the boolean `bold` shorthand maps to 700.
            weight: command
                .get("weight")
                .and_then(serde_json::Value::as_u64)
                .map(|w| (w as u16).clamp(100, 900))
                .unwrap_or(
                    if command
                        .get("bold")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                    {
                        700
                    } else {
                        400
                    },
                ),
            italic: command
                .get("italic")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }),
        other => Err(format!("unknown command {other:?}")),
    }
}

/// Decode one renderer's JSON command array into prims, appending polyline/area points to
/// `pool`. Malformed JSON, non-array input, and per-command problems skip with a warning
/// rather than failing the frame — a broken plugin must never take the chart down.
pub fn decode_commands(
    json: &str,
    pool: &mut Vec<[f32; 2]>,
    text_defaults: &TextDefaults,
) -> DecodedCommands {
    let mut out = DecodedCommands::default();
    let parsed = match serde_json::from_str::<serde_json::Value>(json) {
        Ok(value) => value,
        Err(error) => {
            out.warnings
                .push(format!("malformed command buffer: {error}"));
            return out;
        }
    };
    let Some(commands) = parsed.as_array() else {
        out.warnings
            .push("command buffer is not an array".to_string());
        return out;
    };
    for (index, command) in commands.iter().enumerate() {
        match decode_one(command, pool, text_defaults) {
            Ok(prim) => out.prims.push(prim),
            Err(detail) => out.warn(index, format!("skipped ({detail})")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleuscharts_render::draw_list::LineStyle;

    fn decode(json: &str) -> (Vec<Prim>, Vec<[f32; 2]>, Vec<String>) {
        let mut pool = Vec::new();
        let defaults = TextDefaults {
            family: "DefaultFamily".into(),
            size: 24.0,
            color: Color::rgb(0x11, 0x22, 0x33),
        };
        let out = decode_commands(json, &mut pool, &defaults);
        (out.prims, pool, out.warnings)
    }

    #[test]
    fn rect_and_rect_frame_decode_to_integer_prims() {
        let (prims, _, warnings) = decode(
            r##"[
                {"c":"rect","x":10.4,"y":20.6,"w":100.0,"h":50.0,"color":"#ff0000"},
                {"c":"rect_frame","x":1.0,"y":2.0,"w":30.0,"h":40.0,"color":"rgba(0,128,0,0.5)","line_width":2.0}
            ]"##,
        );
        assert!(warnings.is_empty());
        assert_eq!(
            prims,
            vec![
                Prim::Rect {
                    rect: IRect {
                        x: 10,
                        y: 21,
                        w: 100,
                        h: 50
                    },
                    color: Color::rgb(0xff, 0x00, 0x00),
                },
                Prim::RectFrame {
                    rect: IRect {
                        x: 1,
                        y: 2,
                        w: 30,
                        h: 40
                    },
                    border: 2,
                    color: Color::rgba(0, 128, 0, 128),
                },
            ]
        );
    }

    #[test]
    fn hline_and_vline_decode_with_style_and_min_width() {
        let (prims, _, warnings) = decode(
            r##"[
                {"c":"hline","y":50.5,"x1":0.0,"x2":640.0,"color":"#2196f3","width":0.0,"style":2},
                {"c":"vline","x":100.0,"y1":10.0,"y2":490.0,"color":"#9598a1","width":2.0,"style":4}
            ]"##,
        );
        assert!(warnings.is_empty());
        assert_eq!(
            prims,
            vec![
                Prim::HLine {
                    y: 51,
                    x0: 0,
                    x1: 640,
                    width: 1,
                    style: LineStyle::Dashed,
                    color: Color::rgb(0x21, 0x96, 0xf3),
                },
                Prim::VLine {
                    x: 100,
                    y0: 10,
                    y1: 490,
                    width: 2,
                    style: LineStyle::Dotted,
                    color: Color::rgb(0x95, 0x98, 0xa1),
                },
            ]
        );
    }

    #[test]
    fn polyline_appends_points_to_the_shared_pool() {
        let (prims, pool, warnings) = decode(
            r##"[
                {"c":"polyline","points":[0,0, 10.5,20.25, 30,40],"color":"#0000ff","width":2.5,"style":0},
                {"c":"polyline","points":[1,1, 2,2],"color":"#0000ff","width":1.0,"style":1}
            ]"##,
        );
        assert!(warnings.is_empty());
        assert_eq!(
            pool,
            vec![
                [0.0, 0.0],
                [10.5, 20.25],
                [30.0, 40.0],
                [1.0, 1.0],
                [2.0, 2.0]
            ]
        );
        assert_eq!(
            prims,
            vec![
                Prim::Polyline {
                    first_point: 0,
                    point_count: 3,
                    width: 2.5,
                    style: LineStyle::Solid,
                    line_type: LineType::Simple,
                    color: Color::rgb(0x00, 0x00, 0xff),
                },
                Prim::Polyline {
                    first_point: 3,
                    point_count: 2,
                    width: 1.0,
                    style: LineStyle::Dotted,
                    line_type: LineType::Simple,
                    color: Color::rgb(0x00, 0x00, 0xff),
                },
            ]
        );
    }

    #[test]
    fn area_fill_decodes_gradient_and_base() {
        let (prims, pool, warnings) = decode(
            r##"[{"c":"area_fill","points":[0,10, 20,30],"base_y":50.5,"top_color":"#2edc8766","bottom_color":"rgba(40,221,100,0)"}]"##,
        );
        assert!(warnings.is_empty());
        assert_eq!(pool, vec![[0.0, 10.0], [20.0, 30.0]]);
        assert_eq!(
            prims,
            vec![Prim::AreaFill {
                first_point: 0,
                point_count: 2,
                base_y: 50.5,
                line_type: LineType::Simple,
                gradient: Gradient {
                    top: Color::rgba(0x2e, 0xdc, 0x87, 0x66),
                    bottom: Color::rgba(40, 221, 100, 0),
                },
            }]
        );
    }

    #[test]
    fn circle_round_rect_triangle_and_text_decode() {
        let (prims, _, warnings) = decode(
            r##"[
                {"c":"circle","x":100.0,"y":50.0,"r":8.0,"fill_color":"#ff0000","border_color":"#000000","border_width":2.0},
                {"c":"circle","x":1.0,"y":2.0,"r":3.0,"fill_color":"#00ff00"},
                {"c":"round_rect","x":10.0,"y":20.0,"w":80.0,"h":30.0,"r":6.0,"color":"#123456"},
                {"c":"triangle","x1":0.0,"y1":0.0,"x2":10.0,"y2":0.0,"x3":5.0,"y3":8.0,"color":"#abcdef"},
                {"c":"text","x":40.0,"y":60.0,"text":"hello","color":"#191919","size":12,"align":"center","bold":true}
            ]"##,
        );
        assert!(warnings.is_empty());
        assert_eq!(
            prims,
            vec![
                Prim::Circle {
                    cx: 100.0,
                    cy: 50.0,
                    radius: 8.0,
                    fill: Color::rgb(0xff, 0x00, 0x00),
                    stroke_width: 2.0,
                    stroke: Color::rgb(0, 0, 0),
                },
                Prim::Circle {
                    cx: 1.0,
                    cy: 2.0,
                    radius: 3.0,
                    fill: Color::rgb(0x00, 0xff, 0x00),
                    stroke_width: 0.0,
                    stroke: Color::rgba(0, 0, 0, 0),
                },
                Prim::RoundRect {
                    x: 10.0,
                    y: 20.0,
                    w: 80.0,
                    h: 30.0,
                    radii: [6.0, 6.0, 6.0, 6.0],
                    fill: Color::rgb(0x12, 0x34, 0x56),
                    border_width: 0.0,
                    border_color: Color::rgba(0, 0, 0, 0),
                },
                Prim::Triangle {
                    a: [0.0, 0.0],
                    b: [10.0, 0.0],
                    c: [5.0, 8.0],
                    color: Color::rgb(0xab, 0xcd, 0xef),
                },
                Prim::Text {
                    x: 40.0,
                    y: 60.0,
                    text: "hello".into(),
                    color: Color::rgb(0x19, 0x19, 0x19),
                    size: 12.0,
                    family: "DefaultFamily".into(),
                    align: TextAlign::Center,
                    weight: 700,
                    italic: false,
                },
            ]
        );
    }

    #[test]
    fn text_decodes_defaults_and_explicit_font_family() {
        let (prims, _, warnings) = decode(
            r##"[
                {"c":"text","x":1.0,"y":2.0,"text":"bare"},
                {"c":"text","x":3.0,"y":4.0,"text":"styled","size":9.5,"font":"Custom","align":"right","bold":true,"color":"#ff0000"},
                {"c":"text","x":5.0,"y":6.0,"text":"badalign","align":"justify"},
                {"c":"text","x":7.0,"y":8.0,"text":"badsize","size":0}
            ]"##,
        );
        assert!(warnings.is_empty());
        assert_eq!(
            prims,
            vec![
                Prim::Text {
                    x: 1.0,
                    y: 2.0,
                    text: "bare".into(),
                    color: Color::rgb(0x11, 0x22, 0x33),
                    size: 24.0,
                    family: "DefaultFamily".into(),
                    align: TextAlign::Left,
                    weight: 400,
                    italic: false,
                },
                Prim::Text {
                    x: 3.0,
                    y: 4.0,
                    text: "styled".into(),
                    color: Color::rgb(0xff, 0x00, 0x00),
                    size: 9.5,
                    family: "Custom".into(),
                    align: TextAlign::Right,
                    weight: 700,
                    italic: false,
                },
                Prim::Text {
                    x: 5.0,
                    y: 6.0,
                    text: "badalign".into(),
                    color: Color::rgb(0x11, 0x22, 0x33),
                    size: 24.0,
                    family: "DefaultFamily".into(),
                    align: TextAlign::Left,
                    weight: 400,
                    italic: false,
                },
                Prim::Text {
                    x: 7.0,
                    y: 8.0,
                    text: "badsize".into(),
                    color: Color::rgb(0x11, 0x22, 0x33),
                    size: 24.0,
                    family: "DefaultFamily".into(),
                    align: TextAlign::Left,
                    weight: 400,
                    italic: false,
                },
            ]
        );
    }

    #[test]
    fn unknown_commands_are_skipped_with_a_warning() {
        let (prims, _, warnings) = decode(
            r##"[
                {"c":"rect","x":0,"y":0,"w":10,"h":10,"color":"#000000"},
                {"c":"sparkle","x":1},
                {"c":"hline","y":5,"x1":0,"x2":10,"color":"#000000","width":1,"style":0}
            ]"##,
        );
        assert_eq!(prims.len(), 2);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("unknown command"));
        assert!(warnings[0].contains("command 1"));
    }

    #[test]
    fn malformed_input_never_panics() {
        // Not JSON at all.
        let (prims, _, warnings) = decode("not json");
        assert!(prims.is_empty());
        assert_eq!(warnings.len(), 1);
        // Valid JSON but not an array.
        for input in ["{}", "42", "\"text\"", "null"] {
            let (prims, _, warnings) = decode(input);
            assert!(prims.is_empty(), "input {input}");
            assert_eq!(warnings.len(), 1, "input {input}");
        }
        // Missing fields, wrong types, non-finite-as-null, odd point arrays, too few points.
        let (prims, pool, warnings) = decode(
            r##"[
                {"c":"rect"},
                {"c":"rect","x":"10","y":0,"w":10,"h":10,"color":"#000"},
                {"c":"rect","x":null,"y":0,"w":10,"h":10,"color":"#000"},
                {"c":"rect","x":0,"y":0,"w":10,"h":10,"color":"not-a-color"},
                {"c":"hline","y":5,"x1":0,"x2":10,"color":"#000"},
                {"c":"polyline","points":[0,0,1],"color":"#000","width":1,"style":0},
                {"c":"polyline","points":[0,0],"color":"#000","width":1,"style":0},
                {"c":"polyline","points":"nope","color":"#000","width":1,"style":0},
                {"c":"circle","x":0,"y":0,"fill_color":"#000"},
                {"c":"text","x":0,"y":0},
                {"c":"rect","x":0,"y":0,"w":10,"h":10,"color":"#000000"}
            ]"##,
        );
        // Only the final well-formed rect and the defaulted text survive.
        assert_eq!(prims.len(), 2);
        assert!(matches!(prims[0], Prim::Text { .. }));
        assert!(matches!(prims[1], Prim::Rect { .. }));
        assert!(pool.is_empty());
        assert_eq!(warnings.len(), 9);
    }

    #[test]
    fn pool_is_rolled_back_when_a_point_run_is_malformed() {
        let mut pool = vec![[9.0, 9.0]];
        let defaults = TextDefaults {
            family: "DefaultFamily".into(),
            size: 24.0,
            color: Color::rgb(0, 0, 0),
        };
        let out = decode_commands(
            r##"[
                {"c":"polyline","points":[0,0, 1,1],"color":"#000","width":1,"style":0},
                {"c":"polyline","points":[2,2, "bad",3],"color":"#000","width":1,"style":0}
            ]"##,
            &mut pool,
            &defaults,
        );
        assert_eq!(out.prims.len(), 1);
        assert_eq!(out.warnings.len(), 1);
        // The failed run must not leave half-pushed points in the pool.
        assert_eq!(pool, vec![[9.0, 9.0], [0.0, 0.0], [1.0, 1.0]]);
    }
}
