//! Interactive GPUI gallery for every general series family.
//! Run with: cargo run -p aeris_charts_render_gpui --features gpui-backend --example gpui_general_gallery

use aeris_charts_core::options::ChartTheme;
use aeris_charts_engine::{
    AxisDimension, CategoryScaleType, ChartEngine, ChartFrame, ContinuousScaleType,
    FinancialFrameRequest, FlowLink, FlowNode, FunnelStage, GeneralAxisOptions, GeneralLayoutInput,
    GeneralLayoutSeriesKind, GeneralLayoutSeriesOptions, GeneralScaleType, GeneralSeriesKind,
    GeneralSeriesOptions, GeneralXyInput, HierarchyNode, HorizontalDomain,
};
use aeris_charts_render::draw_list::Prim;
use aeris_charts_render_gpui::{
    AerisViewport, GpuiChartRenderer, PreparedAerisFrame,
    backend::measure_text,
    input::{GpuiChartInput, install_text_metrics},
};
use gpui::{
    App, Bounds, Context, Entity, MouseButton, Render, Window, WindowBounds, WindowOptions, canvas,
    div, prelude::*, px, rgb, size,
};
use gpui_platform::application;

const WIDTH: f32 = 1180.0;
const HEIGHT: f32 = 820.0;

fn hierarchy() -> GeneralLayoutInput {
    let node = |id: &str, parent: Option<&str>, label: &str, value: Option<f64>| HierarchyNode {
        id: id.into(),
        parent: parent.map(str::to_owned),
        label: label.into(),
        value,
    };
    GeneralLayoutInput::Hierarchy(vec![
        node("all", None, "Revenue", None),
        node("product", Some("all"), "Product", None),
        node("service", Some("all"), "Services", None),
        node("other", Some("all"), "Other", None),
        node("core", Some("product"), "Core", Some(44.0)),
        node("mobile", Some("product"), "Mobile", Some(24.0)),
        node("support", Some("service"), "Support", Some(18.0)),
        node("advisory", Some("service"), "Advisory", Some(10.0)),
        node("license", Some("other"), "Licensing", Some(8.0)),
        node("partner", Some("other"), "Partners", Some(6.0)),
    ])
}

fn input(kind: GeneralLayoutSeriesKind) -> GeneralLayoutInput {
    match kind {
        GeneralLayoutSeriesKind::Funnel => GeneralLayoutInput::Funnel(
            [
                ("visit", "Visitors", 120.0),
                ("interest", "Interested", 89.0),
                ("trial", "Trial", 63.0),
                ("proposal", "Proposal", 42.0),
                ("won", "Won", 27.0),
            ]
            .into_iter()
            .map(|(id, label, value)| FunnelStage {
                id: id.into(),
                label: label.into(),
                value,
            })
            .collect(),
        ),
        GeneralLayoutSeriesKind::Treemap | GeneralLayoutSeriesKind::Sunburst => hierarchy(),
        GeneralLayoutSeriesKind::Sankey => GeneralLayoutInput::Flow {
            nodes: ["Visitors", "Organic", "Paid", "Trial", "Direct", "Won"]
                .into_iter()
                .map(|id| FlowNode {
                    id: id.into(),
                    label: id.into(),
                })
                .collect(),
            links: [
                ("Visitors", "Organic", 64.0),
                ("Visitors", "Paid", 42.0),
                ("Organic", "Trial", 39.0),
                ("Paid", "Trial", 28.0),
                ("Organic", "Direct", 25.0),
                ("Paid", "Direct", 14.0),
                ("Trial", "Won", 45.0),
                ("Direct", "Won", 27.0),
            ]
            .into_iter()
            .map(|(source, target, value)| FlowLink {
                source: source.into(),
                target: target.into(),
                value,
            })
            .collect(),
        },
    }
}

fn general_input(kind: GeneralSeriesKind) -> GeneralXyInput {
    use GeneralSeriesKind as Kind;
    let x = (0..8).map(f64::from).collect::<Vec<_>>();
    let y = vec![22.0, 34.0, 27.0, 48.0, 41.0, 61.0, 54.0, 72.0];
    let categories = ["North", "South", "East", "West", "Online", "Partner"]
        .map(str::to_owned)
        .to_vec();
    match kind {
        Kind::XyLine | Kind::XyArea | Kind::Scatter => GeneralXyInput::Numeric {
            ids: None,
            x,
            y,
            y_valid: None,
        },
        Kind::Bubble => GeneralXyInput::Bubble {
            ids: None,
            x,
            y,
            y_valid: None,
            size: vec![12.0, 22.0, 17.0, 31.0, 26.0, 39.0, 32.0, 45.0],
            size_valid: None,
        },
        Kind::RangeArea => GeneralXyInput::RangeNumeric {
            ids: None,
            x,
            low: y.iter().map(|value| value - 8.0).collect(),
            low_valid: None,
            high: y.iter().map(|value| value + 9.0).collect(),
            high_valid: None,
        },
        Kind::RangeBar => GeneralXyInput::RangeCategory {
            ids: None,
            categories,
            category_indices: (0..6).collect(),
            low: vec![12.0, 22.0, 17.0, 31.0, 26.0, 39.0],
            low_valid: None,
            high: vec![35.0, 47.0, 41.0, 60.0, 55.0, 70.0],
            high_valid: None,
        },
        Kind::ErrorBar => GeneralXyInput::ErrorNumeric {
            ids: None,
            x: x.clone(),
            y: y.clone(),
            y_valid: None,
            x_low: x.iter().map(|value| value - 0.2).collect(),
            x_low_valid: None,
            x_high: x.iter().map(|value| value + 0.2).collect(),
            x_high_valid: None,
            y_low: y.iter().map(|value| value - 7.0).collect(),
            y_low_valid: None,
            y_high: y.iter().map(|value| value + 7.0).collect(),
            y_high_valid: None,
        },
        Kind::Column
        | Kind::HorizontalBar
        | Kind::Pie
        | Kind::Radar
        | Kind::RadialBar
        | Kind::PolarArea => GeneralXyInput::Category {
            ids: None,
            categories,
            category_indices: (0..6).collect(),
            y: vec![35.0, 56.0, 42.0, 68.0, 51.0, 74.0],
            y_valid: None,
        },
        Kind::BoxPlot => GeneralXyInput::BoxCategory {
            ids: None,
            categories,
            category_indices: (0..6).collect(),
            min: vec![12.0, 24.0, 17.0, 35.0, 29.0, 41.0],
            min_valid: None,
            q1: vec![19.0, 31.0, 25.0, 42.0, 36.0, 48.0],
            q1_valid: None,
            median: vec![26.0, 39.0, 32.0, 51.0, 43.0, 58.0],
            median_valid: None,
            q3: vec![33.0, 48.0, 39.0, 60.0, 50.0, 67.0],
            q3_valid: None,
            max: vec![41.0, 57.0, 46.0, 70.0, 59.0, 76.0],
            max_valid: None,
        },
        Kind::HeatmapGrid => {
            let mut x_indices = Vec::with_capacity(24);
            let mut y_indices = Vec::with_capacity(24);
            let mut values = Vec::with_capacity(24);
            for row in 0..4 {
                for column in 0..6 {
                    x_indices.push(column);
                    y_indices.push(row);
                    values.push(((column * 7 + row * 13 + 11) % 28 + 4) as f64);
                }
            }
            GeneralXyInput::HeatmapCategoryCategory {
                ids: None,
                x_categories: categories,
                x_category_indices: x_indices,
                y_categories: ["Mon", "Tue", "Wed", "Thu"].map(str::to_owned).to_vec(),
                y_category_indices: y_indices,
                value: values,
                value_valid: None,
            }
        }
    }
}
struct ChartCard {
    title: &'static str,
    engine: ChartEngine,
    renderer: GpuiChartRenderer,
    frame: ChartFrame,
    axis: Vec<Prim>,
    input: GpuiChartInput,
    built_for: (f32, f32, f32),
}

impl ChartCard {
    fn new_layout(kind: GeneralLayoutSeriesKind, title: &'static str, color: &str) -> Self {
        let mut engine =
            ChartEngine::new_with_initial_domain(520.0, 310.0, 1.0, HorizontalDomain::Layout)
                .expect("a layout pane can be the initial pane");
        engine.set_theme(ChartTheme::Light);
        let dataset = engine
            .create_general_layout_dataset(input(kind))
            .expect("gallery data is valid");
        let mut options = GeneralLayoutSeriesOptions::new(0, dataset, kind);
        options.title = title.into();
        options.color = color.into();
        options.data_labels = true;
        engine
            .add_general_layout_series(options)
            .expect("the gallery series binds");
        Self {
            title,
            engine,
            renderer: GpuiChartRenderer::new(),
            frame: ChartFrame::default(),
            axis: Vec::new(),
            input: GpuiChartInput::default(),
            built_for: (0.0, 0.0, 0.0),
        }
    }

    fn new_general(kind: GeneralSeriesKind, title: &'static str, color: &str, donut: bool) -> Self {
        use GeneralSeriesKind as Kind;
        let polar = matches!(
            kind,
            Kind::Pie | Kind::Radar | Kind::RadialBar | Kind::PolarArea
        );
        let category = matches!(
            kind,
            Kind::Column | Kind::RangeBar | Kind::BoxPlot | Kind::HeatmapGrid
        );
        let domain = if polar {
            HorizontalDomain::Polar
        } else if category {
            HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            }
        } else {
            HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            }
        };
        let mut engine = ChartEngine::new_with_initial_domain(520.0, 310.0, 1.0, domain)
            .expect("a general pane can be the initial pane");
        engine.set_theme(ChartTheme::Light);
        let (x_dimension, x_scale, y_dimension, y_scale) = if polar {
            (
                AxisDimension::Angle,
                GeneralScaleType::AngularCategory,
                AxisDimension::Radius,
                GeneralScaleType::RadialLinear,
            )
        } else if kind == Kind::HorizontalBar {
            (
                AxisDimension::X,
                GeneralScaleType::Linear,
                AxisDimension::Y,
                GeneralScaleType::Band,
            )
        } else if kind == Kind::HeatmapGrid {
            (
                AxisDimension::X,
                GeneralScaleType::Band,
                AxisDimension::Y,
                GeneralScaleType::Band,
            )
        } else {
            (
                AxisDimension::X,
                if category {
                    GeneralScaleType::Band
                } else {
                    GeneralScaleType::Linear
                },
                AxisDimension::Y,
                GeneralScaleType::Linear,
            )
        };
        let mut x_axis = GeneralAxisOptions::new("x", 0, x_dimension, x_scale);
        let mut y_axis = GeneralAxisOptions::new("y", 0, y_dimension, y_scale);
        // Cartesian and radar charts read against a grid; sector families show proportions
        // without one, and heatmap cells already tile the plot.
        if polar && kind != Kind::Radar {
            x_axis.visible = false;
            y_axis.visible = false;
        } else if kind != Kind::HeatmapGrid {
            engine
                .apply_options(
                    r#"{"grid":{"vertLines":{"visible":true},"horzLines":{"visible":true}}}"#,
                )
                .expect("gallery grid options");
        }
        engine.add_general_axis(x_axis).expect("gallery X axis");
        engine.add_general_axis(y_axis).expect("gallery Y axis");
        let dataset = engine
            .create_general_xy_dataset(general_input(kind))
            .expect("gallery general data");
        let mut options = match kind {
            Kind::XyLine => GeneralSeriesOptions::xy_line(0, dataset, "x", "y"),
            Kind::XyArea => GeneralSeriesOptions::xy_area(0, dataset, "x", "y"),
            Kind::RangeArea => GeneralSeriesOptions::range_area(0, dataset, "x", "y"),
            Kind::RangeBar => GeneralSeriesOptions::range_bar(0, dataset, "x", "y"),
            Kind::ErrorBar => GeneralSeriesOptions::error_bar(0, dataset, "x", "y"),
            Kind::Column => GeneralSeriesOptions::column(0, dataset, "x", "y"),
            Kind::HorizontalBar => GeneralSeriesOptions::horizontal_bar(0, dataset, "x", "y"),
            Kind::BoxPlot => GeneralSeriesOptions::box_plot(0, dataset, "x", "y"),
            Kind::HeatmapGrid => GeneralSeriesOptions::heatmap_grid(0, dataset, "x", "y"),
            Kind::Scatter => GeneralSeriesOptions::scatter(0, dataset, "x", "y"),
            Kind::Bubble => GeneralSeriesOptions::bubble(0, dataset, "x", "y"),
            Kind::Pie => GeneralSeriesOptions::pie(0, dataset, "x", "y"),
            Kind::Radar => GeneralSeriesOptions::radar(0, dataset, "x", "y"),
            Kind::RadialBar => GeneralSeriesOptions::radial_bar(0, dataset, "x", "y"),
            Kind::PolarArea => GeneralSeriesOptions::polar_area(0, dataset, "x", "y"),
        };
        options.title = title.into();
        options.color = if matches!(kind, Kind::Pie | Kind::PolarArea | Kind::RadialBar) {
            None
        } else {
            Some(color.into())
        };
        options.point_markers = matches!(kind, Kind::XyLine | Kind::XyArea);
        options.point_radius = if kind == Kind::Scatter { 6.0 } else { 4.0 };
        options.fill_opacity = if matches!(kind, Kind::XyArea | Kind::RangeArea | Kind::Radar) {
            0.42
        } else {
            options.fill_opacity
        };
        if donut {
            options.polar.inner_radius = 0.48;
        }
        if kind == Kind::HeatmapGrid {
            options.heatmap_low_color = Some("#e0f2fe".into());
            options.heatmap_high_color = Some("#0369a1".into());
        }
        engine
            .add_general_series(options)
            .expect("gallery general series");
        Self {
            title,
            engine,
            renderer: GpuiChartRenderer::new(),
            frame: ChartFrame::default(),
            axis: Vec::new(),
            input: GpuiChartInput::default(),
            built_for: (0.0, 0.0, 0.0),
        }
    }
    fn prepare(&mut self, bounds: Bounds<gpui::Pixels>, window: &Window) {
        let width: f32 = bounds.size.width.into();
        let height: f32 = bounds.size.height.into();
        let dpr = window.scale_factor();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        self.input.set_canvas_bounds(bounds);
        install_text_metrics(&mut self.engine, window);
        self.input.prepare_frame(&mut self.engine);
        let resized = self.built_for != (width, height, dpr);
        if resized && self.built_for.2 != 0.0 && self.built_for.2 != dpr {
            self.renderer.invalidate_caches();
        }
        let font = self.engine.options.get().layout.font_family.clone();
        let font = font.as_str();
        let axis_size = self.engine.axis_font_size() as f32;
        let countdown_size = self.engine.countdown_font_size() as f32;
        let measure = |size: f32| {
            move |text: &str, bold: bool| {
                f64::from(
                    measure_text(
                        window,
                        text,
                        font,
                        size,
                        if bold { 700 } else { 400 },
                        false,
                    )
                    .width,
                )
            }
        };
        self.engine.prepare_financial_frame_with_measure(
            FinancialFrameRequest {
                width: f64::from(width),
                height: f64::from(height),
                dpr: f64::from(dpr),
                force_layout: resized,
                allow_axis_shrink: resized,
                force_frame: false,
                force_axis: false,
                layout_only: false,
                fit_content: false,
                frame: &mut self.frame,
                axis_frame: None,
                axis_primitives: Some(&mut self.axis),
            },
            measure(axis_size),
            measure(countdown_size),
        );
        self.built_for = (width, height, dpr);
    }

    fn paint(&mut self, bounds: Bounds<gpui::Pixels>, window: &mut Window, cx: &mut App) {
        if self.frame.panes.is_empty() {
            return;
        }
        let viewport = AerisViewport::from_bounds(
            bounds.origin.x.into(),
            bounds.origin.y.into(),
            bounds.size.width.into(),
            bounds.size.height.into(),
        );
        let prepared =
            PreparedAerisFrame::from_engine(&self.frame, &self.engine).with_axis(&self.axis, &[]);
        if let Err(error) =
            self.renderer
                .paint_frame(&prepared, viewport, window.scale_factor(), window, cx)
        {
            eprintln!("general gallery paint failed: {error}");
        }
    }
}

impl Render for ChartCard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let move_entity = cx.entity();
        let down_entity = cx.entity();
        let up_entity = cx.entity();
        let outside_entity = cx.entity();
        let hover_entity = cx.entity();
        let prepaint = cx.entity();
        let paint = cx.entity();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(rgb(0xffffff))
            .rounded_lg()
            .border_1()
            .border_color(rgb(0xdfe6ef))
            .overflow_hidden()
            .p_2()
            .child(
                div()
                    .id(self.title)
                    .flex_1()
                    .min_h_0()
                    .on_hover(move |hovered, _, app| {
                        hover_entity.update(app, |card, cx| {
                            card.input.hover_changed(&mut card.engine, *hovered);
                            cx.notify();
                        });
                    })
                    .on_mouse_move(move |event, _, app| {
                        move_entity.update(app, |card, cx| {
                            card.input.mouse_move(&mut card.engine, event);
                            cx.notify();
                        });
                    })
                    .on_mouse_down(MouseButton::Left, move |event, _, app| {
                        down_entity.update(app, |card, cx| {
                            card.input.mouse_down(&mut card.engine, event);
                            cx.notify();
                        });
                    })
                    .on_mouse_up(MouseButton::Left, move |event, _, app| {
                        up_entity.update(app, |card, cx| {
                            card.input.mouse_up(&mut card.engine, event);
                            cx.notify();
                        });
                    })
                    .on_mouse_up_out(MouseButton::Left, move |event, _, app| {
                        outside_entity.update(app, |card, cx| {
                            card.input.mouse_up(&mut card.engine, event);
                            cx.notify();
                        });
                    })
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                prepaint.update(cx, |card, _| card.prepare(bounds, window));
                                bounds
                            },
                            move |_, bounds, window, cx| {
                                paint.update(cx, |card, cx| card.paint(bounds, window, cx));
                            },
                        )
                        .size_full(),
                    ),
            )
    }
}

struct Gallery {
    cards: Vec<Entity<ChartCard>>,
}

impl Gallery {
    fn new(cx: &mut App) -> Self {
        use GeneralSeriesKind as Kind;
        let mut cards = Vec::with_capacity(20);
        let general = [
            (Kind::XyLine, "Line", "#3b82f6", false),
            (Kind::XyArea, "Area", "#0ea5e9", false),
            (Kind::Column, "Columns", "#6366f1", false),
            (Kind::HorizontalBar, "Horizontal bars", "#14b8a6", false),
            (Kind::Scatter, "Scatter", "#f97316", false),
            (Kind::Bubble, "Bubble", "#8b5cf6", false),
            (Kind::RangeArea, "Range area", "#06b6d4", false),
            (Kind::RangeBar, "Range bars", "#64748b", false),
            (Kind::ErrorBar, "Error bars", "#ef4444", false),
            (Kind::BoxPlot, "Box plot", "#a855f7", false),
            (Kind::HeatmapGrid, "Heatmap", "#0284c7", false),
            (Kind::Pie, "Pie", "#f59e0b", false),
            (Kind::Pie, "Donut", "#f59e0b", true),
            (Kind::Radar, "Radar", "#10b981", false),
            (Kind::PolarArea, "Polar area", "#eab308", false),
            (Kind::RadialBar, "Radial bars", "#e11d48", false),
        ];
        for (kind, title, color, donut) in general {
            cards.push(cx.new(move |_| ChartCard::new_general(kind, title, color, donut)));
        }
        for (kind, title, color) in [
            (GeneralLayoutSeriesKind::Funnel, "Funnel", "#3b82f6"),
            (GeneralLayoutSeriesKind::Treemap, "Treemap", "#10b981"),
            (GeneralLayoutSeriesKind::Sankey, "Sankey", "#8b5cf6"),
            (GeneralLayoutSeriesKind::Sunburst, "Sunburst", "#f59e0b"),
        ] {
            cards.push(cx.new(move |_| ChartCard::new_layout(kind, title, color)));
        }
        Self { cards }
    }
}

impl Render for Gallery {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self
            .cards
            .chunks(2)
            .map(|pair| {
                div()
                    .flex()
                    .h(px(304.0))
                    .flex_shrink_0()
                    .gap_4()
                    .children(pair.iter().cloned())
            })
            .collect::<Vec<_>>();
        div().flex().flex_col().size_full().bg(rgb(0xf3f6fa)).child(
            div()
                .id("gallery-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_4()
                .child(div().flex().flex_col().gap_4().children(rows)),
        )
    }
}
fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title("Aeris Charts — General gallery");
                cx.new(|cx| Gallery::new(cx))
            },
        )
        .expect("the gallery window opens");
        cx.activate(true);
    });
}
