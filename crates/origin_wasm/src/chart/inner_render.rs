//! `ChartInner` rendering: WebGPU/Canvas2D pane execution, axis overlay painting, backend
//! failover, and browser text measurement.

use origin_render::draw_list::Prim;

use super::*;

impl ChartInner {
    /// Reports the active pane backend for diagnostics and runtime-matrix tests.
    pub fn backend_kind(&self) -> String {
        if self.gfx.is_some() {
            "webgpu".into()
        } else {
            "canvas2d".into()
        }
    }

    pub fn render(&mut self) -> Result<(), JsValue> {
        // Series primitives (plugin platform Phase C-b): pull this frame's autoscale
        // contributions from the plugin hooks before any layout/autoscale pass runs, so the
        // axis-width negotiation, axis frame, and pane frame all see the merged ranges.
        self.collect_series_primitive_autoscale();
        // Custom series (Phase C-c): same pre-layout collection point — the visible items'
        // price values become autoscale contributions and the engine's custom frame values.
        self.collect_custom_series_autoscale();

        // ---- layout (price axis width negotiated against the price labels) ----
        self.recompute_layout(false);

        // Feed the engine clock for the candle-close countdown labels: the host-pinned value
        // when `set_now_seconds` installed one (the package's 1s countdown timer), else the
        // browser's system time — the engine itself is headless and owns no clock.
        let now = self
            .now_override
            .unwrap_or_else(|| js_sys::Date::now() / 1000.0);
        self.engine.set_now_seconds(now);

        // time tick marks: built once (needs &mut), shared by GPU grid + 2D labels.
        // Font comes from `layout` (reference `fontSize`/`fontFamily`): it drives the tick-density
        // estimate, host text measurement, and glyph drawing so all three agree. The label
        // width cap is reference `timeScale.tickMarkMaxCharacterLength` (default 8).
        let layout = self.opts().layout;
        let font_size = layout.font_size;
        let font_family = layout.font_family;
        let pixels_per_character = (font_size + 4.0) * 5.0 / 8.0;
        let max_label_width =
            pixels_per_character * f64::from(self.engine.tick_mark_max_character_length);
        let axis_ctx = &self.axis_ctx;
        let dpr = self.dpr;
        self.axis_frame = self.engine.build_axis_frame(max_label_width, |text| {
            measure_text_ctx(axis_ctx, dpr, &font_family, font_size, text)
        });

        // ---- GPU: one scissored draw group per stacked pane ----
        // The headless engine owns chart geometry. The WASM host only adds browser-adapter
        // concerns such as crosshair interaction and text labels.
        self.engine.build_frame_into(&mut self.frame);

        // Pane primitives (plugin platform Phase C-a): plugin renderers record Prim commands
        // into the pane layers and boxed labels into the axis frame, after the engine frame is
        // settled and before either backend consumes it. The Phase 3.5 overlay-text store is
        // cleared first so a detached primitive leaves no stale glyphs behind.
        self.primitive_texts.clear();
        self.run_pane_primitives();
        // Series primitives (Phase C-b): same pass, bound to each owning series' scale.
        self.run_series_primitives();
        // Custom series (Phase C-c): plugin renders splice into each pane's `main` layer at
        // the series' paint-order marks (same command-recording model).
        self.run_custom_series();

        if self
            .gfx
            .as_ref()
            .is_some_and(|gfx| gfx.device_lost.load(Ordering::Acquire))
        {
            self.activate_canvas2d("WebGPU device was lost");
        }

        let bg = Color::parse_css(&self.opts().layout.background.color)
            .unwrap_or(Color::rgb(0xff, 0xff, 0xff));
        let pane_outcome = if self.gfx.is_some() {
            let engine_frame = &self.frame;
            self.gpu_groups
                .resize_with(engine_frame.panes.len(), DrawGroup::default);
            self.gpu_groups.truncate(engine_frame.panes.len());
            let Some(gfx) = self.gfx.as_mut() else {
                return Err(JsValue::from_str("WebGPU state disappeared mid-render"));
            };
            let shared = Rc::clone(&gfx.shared);
            let renderers = Rc::clone(&gfx.renderers);
            let text_runs = &mut self.text_runs;
            let mut atlas = shared.atlas.borrow_mut();
            for (group, pane_frame) in self.gpu_groups.iter_mut().zip(&engine_frame.panes) {
                group.scissor = Some(pane_frame.scissor);
                group.clear();
                // Convert the shared frame only at the WebGPU backend boundary. The builder
                // walks each layer in the Canvas2D executor's order (under, then main, then
                // top; prims in list order within a layer) and records one run per maximal
                // same-pipeline block, so e.g. markers emitted after the candles paint over
                // the wicks on WebGPU exactly as they do on Canvas2D. Text prims resolve
                // through the host's browser-rasterized atlas cache (chart/text_runs.rs) and
                // schedule as tex-quad runs at their prim position in the same order.
                let queue = &shared.queue;
                let mut resolve_text = |prim: &Prim| {
                    text_runs
                        .as_mut()
                        .and_then(|runs| runs.resolve(&mut atlas, queue, prim))
                };
                prims_to_group(
                    &pane_frame.under,
                    &pane_frame.points,
                    group,
                    &mut resolve_text,
                );
                prims_to_group(
                    &pane_frame.main,
                    &pane_frame.points,
                    group,
                    &mut resolve_text,
                );
                prims_to_group(
                    &pane_frame.top_prims,
                    &pane_frame.points,
                    group,
                    &mut resolve_text,
                );
            }
            let groups = &self.gpu_groups[..];
            gfx.msaa.ensure(
                &shared.device,
                gfx.config.format,
                gfx.config.width,
                gfx.config.height,
            );

            let acquired = match gfx.surface.get_current_texture() {
                Ok(frame) => Ok(Some(frame)),
                Err(error) => match surface_error_action(&error) {
                    SurfaceErrorAction::Reconfigure => {
                        // Resize and suspend/resume can invalidate only the swapchain. Reconfigure
                        // and retry once; if that fails, the warm Canvas2D pane takes over.
                        gfx.surface.configure(&shared.device, &gfx.config);
                        match gfx.surface.get_current_texture() {
                            Ok(frame) => Ok(Some(frame)),
                            Err(retry_error)
                                if surface_error_action(&retry_error)
                                    == SurfaceErrorAction::SkipFrame =>
                            {
                                Ok(None)
                            }
                            Err(retry_error) => Err(retry_error),
                        }
                    }
                    SurfaceErrorAction::SkipFrame => Ok(None),
                    SurfaceErrorAction::Fallback => Err(error),
                },
            };

            match acquired {
                Ok(Some(frame)) => {
                    let view = frame
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    let bg_clear = wgpu::Color {
                        r: bg.r() as f64 / 255.0,
                        g: bg.g() as f64 / 255.0,
                        b: bg.b() as f64 / 255.0,
                        a: 1.0,
                    };
                    render_frame(
                        &shared.device,
                        &shared.queue,
                        gfx.msaa.view(),
                        &view,
                        gfx.config.width,
                        gfx.config.height,
                        bg_clear,
                        &renderers.quad,
                        &renderers.tex,
                        &renderers.tri,
                        groups,
                    );
                    frame.present();
                    PaneRenderOutcome::Presented
                }
                Ok(None) => PaneRenderOutcome::Timeout,
                Err(error) => PaneRenderOutcome::Fallback(format!(
                    "WebGPU surface acquisition failed after recovery: {error}"
                )),
            }
        } else {
            PaneRenderOutcome::Canvas2d
        };

        match pane_outcome {
            PaneRenderOutcome::Presented => {}
            PaneRenderOutcome::Timeout => {
                // Keep the last complete frame. The next animation/input repaint retries.
                return Ok(());
            }
            PaneRenderOutcome::Fallback(reason) => {
                self.activate_canvas2d(&reason);
                self.render_canvas2d()?;
            }
            PaneRenderOutcome::Canvas2d => self.render_canvas2d()?,
        }

        self.draw_axes_2d(&self.axis_frame)?;
        Ok(())
    }

    // --- data / scale bookkeeping ---

    pub(super) fn compute_price_axis_width(&mut self, target: PriceScaleTarget) -> f64 {
        let axis_ctx = self.axis_ctx.clone();
        let dpr = self.dpr;
        let layout = self.opts().layout;
        let font_size = layout.font_size;
        let font_family = layout.font_family;
        self.engine.optimal_price_axis_width_for(target, |text| {
            measure_text_ctx(&axis_ctx, dpr, &font_family, font_size, text)
        })
    }

    // ---- Canvas2D axis overlay ----

    fn draw_axes_2d(&self, axis_frame: &AxisFrame) -> Result<(), JsValue> {
        let ctx = &self.axis_ctx;
        let dpr = self.dpr;
        let bitmap_w = self.bitmap_w as f64;
        let bitmap_h = self.bitmap_h as f64;
        let pane_left = self.pane_left;
        let pane_w = self.pane_w;
        let pane_h = self.pane_h;

        ctx.clear_rect(0.0, 0.0, bitmap_w, bitmap_h);
        let border_w = 1f64.max(dpr.floor());

        let options = self.opts();
        // Watermark paints first so it sits below the axis borders, labels, and crosshair chrome.
        self.draw_watermark(&options.watermark, dpr)?;
        // The primitives' `text_views` overlay draws share the watermark's slot (Phase 3.5):
        // in-pane plugin text, below the axis chrome, identical on both backends.
        self.draw_primitive_overlay_texts(dpr)?;

        // Axis borders come from the options store (reference `borderColor`/`borderVisible` per strip);
        // an unparseable color falls back to the reference default.
        let fallback = Color::parse_css(BORDER_CSS).unwrap_or(Color::rgb(0x2b, 0x2b, 0x43));
        let left_border = Color::parse_css(&options.left_price_scale.border_color)
            .unwrap_or(fallback)
            .to_hex();
        let right_border = Color::parse_css(&options.right_price_scale.border_color)
            .unwrap_or(fallback)
            .to_hex();
        let time_border = Color::parse_css(&options.time_scale.border_color)
            .unwrap_or(fallback)
            .to_hex();

        if self.left_axis_w > 0.0 && options.left_price_scale.border_visible {
            ctx.set_fill_style_str(&left_border);
            ctx.fill_rect(
                (pane_left * dpr).round() - border_w,
                0.0,
                border_w,
                (pane_h * dpr).round(),
            );
        }
        if self.axis_w > 0.0 && options.right_price_scale.border_visible {
            ctx.set_fill_style_str(&right_border);
            ctx.fill_rect(
                ((pane_left + pane_w) * dpr).round(),
                0.0,
                border_w,
                (pane_h * dpr).round(),
            );
        }
        if options.time_scale.border_visible && self.engine.time_axis_visible {
            ctx.set_fill_style_str(&time_border);
            ctx.fill_rect(0.0, (pane_h * dpr).round(), bitmap_w, border_w);
        }

        // reference price-axis-widget.ts `_drawTickMarks`: 5 css px stubs from the pane edge into
        // the strip at each tick coordinate, in the strip's border color, gated on
        // `borderVisible && ticksVisible` (the engine already filtered on the latter).
        let tick_len = (5.0 * dpr).round();
        let tick_h = border_w;
        let tick_off = (dpr * 0.5).floor();
        let right_ticks = options.right_price_scale.border_visible;
        let left_ticks = options.left_price_scale.border_visible;
        for tick in &axis_frame.price_ticks {
            let (color, x) = if tick.left {
                if !left_ticks {
                    continue;
                }
                (&left_border, ((pane_left - 5.0) * dpr).round())
            } else {
                if !right_ticks {
                    continue;
                }
                (&right_border, ((pane_left + pane_w) * dpr).round())
            };
            ctx.set_fill_style_str(color);
            ctx.fill_rect(x, (tick.y * dpr).round() - tick_off, tick_len, tick_h);
        }

        // reference time-axis-widget.ts `_drawTickMarks`: 5 css px stubs down from the top of the
        // time strip, in the time-scale border color, same border/visibility gating.
        if options.time_scale.border_visible
            && self.engine.time_ticks_visible
            && self.engine.time_axis_visible
        {
            ctx.set_fill_style_str(&time_border);
            let y0 = (pane_h * dpr).round();
            for x in &axis_frame.time_ticks {
                ctx.fill_rect((x * dpr).round() - tick_off, y0, tick_h, tick_len);
            }
        }

        // Separators between stacked panes (roadmap Phase B1): a border line at each pane
        // boundary. An unset `layout.panes.separatorColor` follows the price-axis border color
        // (theme-aware like the axis chrome); an explicit value pins it. Painted regardless of
        // the time-axis border's visibility since they are functional dividers, not axis chrome.
        let separator_color = Color::parse_css(&options.layout.panes.separator_color)
            .unwrap_or_else(|| {
                Color::parse_css(&options.right_price_scale.border_color).unwrap_or(fallback)
            })
            .to_hex();
        ctx.set_fill_style_str(&separator_color);
        for separator in &axis_frame.separators {
            let y = (separator * dpr).round();
            ctx.fill_rect(
                (pane_left * dpr).round(),
                y,
                (pane_w * dpr).round(),
                (PANE_SEPARATOR * dpr).max(border_w),
            );
        }

        // reference pane-separator.ts hover handle (`top: -4px; height: 9px; width: 100%` over the
        // 1px separator cell): a full-width 9 css px band centered on the separator, painted
        // in `layout.panes.separatorHoverColor` while the host reports a hovered separator.
        if let Some(separator) = axis_frame
            .separator_hover
            .and_then(|i| axis_frame.separators.get(i))
        {
            ctx.set_fill_style_str(&options.layout.panes.separator_hover_color);
            ctx.fill_rect(
                0.0,
                ((separator - 4.0) * dpr).round(),
                bitmap_w,
                (9.0 * dpr).round(),
            );
        }

        self.draw_axis_labels(
            axis_frame,
            dpr,
            &options.layout.font_family,
            options.layout.font_size,
        )?;
        Ok(())
    }

    /// Paint the `watermark` label onto the overlay, anchored inside the pane per `horzAlign`/
    /// `vertAlign`. Drawn in media coordinates (context scaled by DPR) like the axis labels; the
    /// CSS color string is passed through verbatim so alpha is preserved.
    fn draw_watermark(&self, wm: &WatermarkOptions, dpr: f64) -> Result<(), JsValue> {
        if !wm.visible || wm.text.is_empty() {
            return Ok(());
        }
        let ctx = &self.axis_ctx;
        ctx.save();
        if let Err(error) = ctx.scale(dpr, dpr) {
            ctx.restore();
            return Err(error);
        }
        let font = if wm.font_style.is_empty() {
            format!("{}px {}", wm.font_size, wm.font_family)
        } else {
            format!("{} {}px {}", wm.font_style, wm.font_size, wm.font_family)
        };
        ctx.set_font(&font);
        ctx.set_fill_style_str(&wm.color);
        let (x, align) = match wm.horz_align.as_str() {
            "left" => (self.pane_left, "left"),
            "right" => (self.pane_left + self.pane_w, "right"),
            _ => (self.pane_left + self.pane_w / 2.0, "center"),
        };
        let (y, baseline) = match wm.vert_align.as_str() {
            "top" => (0.0, "top"),
            "bottom" => (self.pane_h, "bottom"),
            _ => (self.pane_h / 2.0, "middle"),
        };
        ctx.set_text_align(align);
        ctx.set_text_baseline(baseline);
        let result = ctx.fill_text(&wm.text, x, y).map(|_| ());
        ctx.restore();
        result
    }

    /// Paint the primitives' `text_views` overlay draws (plugin platform Phase 3.5) in media
    /// coordinates (context scaled by DPR) like the axis labels. Each draw carries its own
    /// fully-resolved font, color, and canvas alignment keywords; colors pass through verbatim
    /// so alpha is preserved (same rule as the watermark).
    fn draw_primitive_overlay_texts(&self, dpr: f64) -> Result<(), JsValue> {
        if self.primitive_texts.is_empty() {
            return Ok(());
        }
        let ctx = &self.axis_ctx;
        ctx.save();
        if let Err(error) = ctx.scale(dpr, dpr) {
            ctx.restore();
            return Err(error);
        }
        let mut draw_result = Ok(());
        for text in &self.primitive_texts {
            ctx.set_font(&text.font);
            ctx.set_fill_style_str(&text.color);
            ctx.set_text_align(&text.align);
            ctx.set_text_baseline(&text.baseline);
            if let Err(error) = ctx.fill_text(&text.text, text.x, text.y).map(|_| ()) {
                draw_result = Err(error);
                break;
            }
        }
        ctx.restore();
        draw_result
    }

    fn draw_axis_labels(
        &self,
        axis_frame: &AxisFrame,
        dpr: f64,
        font_family: &str,
        font_size: f64,
    ) -> Result<(), JsValue> {
        let ctx = &self.axis_ctx;

        // Z-order matters: boxed labels (last value, price lines, crosshair) must fully cover any
        // ordinary tick label they overlap, exactly like reference where each axis view paints its
        // background and text as one unit in view order. Painting all backgrounds first and all
        // texts second lets tick glyphs bleed onto the boxes, so paint in two ordered layers:
        // plain tick text first, then each boxed label's background + text.
        self.draw_axis_label_texts(
            axis_frame.labels.iter().filter(|l| l.background.is_none()),
            dpr,
            font_family,
            font_size,
        )?;
        let mut last_attach: Option<(u32, f64)> = None;
        for label in axis_frame.labels.iter().filter(|l| l.background.is_some()) {
            if let Some((x, y, w, h, color)) = label.background {
                // Backgrounds are bitmap-aligned geometry, matching the reference's bitmap-coordinate pass.
                // `to_css` keeps alpha so custom (e.g. price-line) label colors stay translucent.
                // Sizes come from the box's rounded far EDGE minus its rounded origin, so adjacent
                // boxes (e.g. the outside title chip vs the in-strip price chip) keep a
                // deterministic device-px gap instead of breathing ±1px from independent
                // origin/width rounding.
                ctx.set_fill_style_str(&color.to_css());
                let bx = (x * dpr).round();
                // Attached rows (the price chip + its countdown chip) share an edge: the next
                // box's top is the previous box's exact bottom — no per-box rounding gap between
                // them at fractional DPR.
                let by = match (label.attach_group, last_attach) {
                    (Some(group), Some((last_group, last_bottom))) if group == last_group => {
                        last_bottom
                    }
                    _ => (y * dpr).round(),
                };
                let bw = ((x + w) * dpr).round() - bx;
                let bh = ((y + h) * dpr).round() - by;
                last_attach = label.attach_group.map(|group| (group, by + bh));
                if label.background_corners.is_empty() {
                    ctx.fill_rect(bx, by, bw, bh);
                } else {
                    // TradingView-style side radius: only the engine-selected (axis-facing)
                    // corners round — 2 CSS px, scaled to bitmap px like the box itself.
                    fill_boxed_label_background(
                        ctx,
                        bx,
                        by,
                        bw,
                        bh,
                        2.0 * dpr,
                        label.background_corners,
                    );
                }
            } else {
                last_attach = None;
            }
            self.draw_axis_label_texts(std::iter::once(label), dpr, font_family, font_size)?;
        }
        Ok(())
    }

    /// Draws label glyphs in media-coordinate space: the context is scaled by DPR while the font
    /// stays at the configured CSS px size. Using an independently hinted size*dpr bitmap font is
    /// observably different at fractional DPR even when every logical coordinate is identical.
    fn draw_axis_label_texts<'l>(
        &self,
        labels: impl Iterator<Item = &'l AxisLabel>,
        dpr: f64,
        font_family: &str,
        font_size: f64,
    ) -> Result<(), JsValue> {
        let ctx = &self.axis_ctx;
        ctx.save();
        if let Err(error) = ctx.scale(dpr, dpr) {
            ctx.restore();
            return Err(error);
        }
        ctx.set_text_baseline("middle");
        let mut draw_result = Ok(());
        for label in labels {
            ctx.set_font(&if label.bold {
                format!("bold {font_size}px {font_family}")
            } else {
                format!("{font_size}px {font_family}")
            });
            ctx.set_text_align(match label.align {
                AxisTextAlign::Left => "left",
                AxisTextAlign::Right => "right",
                AxisTextAlign::Center => "center",
            });
            ctx.set_fill_style_str(&label.color.to_css());
            let metrics_text = match label.midpoint {
                AxisTextMidpoint::None => None,
                AxisTextMidpoint::Label => Some(label.text.as_str()),
                AxisTextMidpoint::StableTime => Some("Apr0"),
            };
            let y_mid_correction = metrics_text
                .and_then(|text| ctx.measure_text(text).ok())
                .map(|metrics| {
                    (metrics.actual_bounding_box_ascent() - metrics.actual_bounding_box_descent())
                        / 2.0
                })
                .unwrap_or(0.0);
            if let Err(error) = ctx.fill_text(&label.text, label.x, label.y + y_mid_correction) {
                draw_result = Err(error);
                break;
            }
        }
        ctx.restore();
        draw_result
    }

    /// Permanently switch this chart instance to its already-initialized Canvas2D pane.
    fn activate_canvas2d(&mut self, reason: &str) {
        if self.gfx.take().is_some() {
            set_backend_visibility(&self.gpu_pane, &self.fallback_pane, false);
            web_sys::console::warn_1(&format!("origin: {reason}; continuing with Canvas2D").into());
        }
    }

    /// Execute the exact same retained frame consumed by WebGPU through Canvas2D.
    pub(super) fn render_canvas2d(&self) -> Result<(), JsValue> {
        let ctx = &self.pane_ctx;
        let width = self.bitmap_w as f64;
        let height = self.bitmap_h as f64;
        ctx.clear_rect(0.0, 0.0, width, height);
        let bg = self.opts().layout.background.color;
        ctx.set_fill_style_str(&bg);
        ctx.fill_rect(0.0, 0.0, width, height);
        let mut target = crate::canvas2d_target::WasmCanvas2d::new(ctx);
        let viewport = CanvasViewport {
            width: width as f32,
            height: height as f32,
        };
        for pane in &self.frame.panes {
            target.save();
            let [x, y, w, h] = pane.scissor;
            target.clip_rect(x as f32, y as f32, w as f32, h as f32);
            execute_canvas2d(&pane.under, &pane.points, &mut target, viewport);
            execute_canvas2d(&pane.main, &pane.points, &mut target, viewport);
            execute_canvas2d(&pane.top_prims, &pane.points, &mut target, viewport);
            target.restore();
        }
        Ok(())
    }
}

/// Fill a boxed axis label's background with per-corner rounding (TradingView-style side
/// radius): the path is built manually from lines and quadratic arcs — corners flagged in
/// `corners` get `radius`, the rest stay sharp. The radius clamps to half the box so thin
/// boxes keep a well-formed path. Coordinates are bitmap px (the axis context is unscaled here).
fn fill_boxed_label_background(
    ctx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
    corners: AxisLabelCorners,
) {
    let r = radius.max(0.0).min(w / 2.0).min(h / 2.0);
    let pick = |on: bool| if on { r } else { 0.0 };
    let tl = pick(corners.top_left);
    let tr = pick(corners.top_right);
    let br = pick(corners.bottom_right);
    let bl = pick(corners.bottom_left);
    ctx.begin_path();
    ctx.move_to(x + tl, y);
    ctx.line_to(x + w - tr, y);
    if tr > 0.0 {
        ctx.quadratic_curve_to(x + w, y, x + w, y + tr);
    }
    ctx.line_to(x + w, y + h - br);
    if br > 0.0 {
        ctx.quadratic_curve_to(x + w, y + h, x + w - br, y + h);
    }
    ctx.line_to(x + bl, y + h);
    if bl > 0.0 {
        ctx.quadratic_curve_to(x, y + h, x, y + h - bl);
    }
    ctx.line_to(x, y + tl);
    if tl > 0.0 {
        ctx.quadratic_curve_to(x, y, x + tl, y);
    }
    ctx.close_path();
    ctx.fill();
}

pub(super) fn measure_text_ctx(
    ctx: &CanvasRenderingContext2d,
    dpr: f64,
    font_family: &str,
    font_size: f64,
    text: &str,
) -> f64 {
    ctx.set_font(&format!("{}px {font_family}", font_size * dpr));
    ctx.measure_text(text).map(|m| m.width()).unwrap_or(0.0) / dpr
}
