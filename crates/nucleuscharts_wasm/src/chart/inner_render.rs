//! `ChartInner` rendering: WebGPU/Canvas2D pane execution, axis overlay painting, backend
//! failover, and browser text measurement.

use nucleuscharts_render::draw_list::Prim;

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
        // `frame_stats().cpu_ms` covers this whole function: layout, axis-frame construction,
        // engine frame build, plugin passes, and command encoding — the host-side CPU cost of
        // producing a frame. Two clock reads per frame; the record itself is fixed-size.
        let frame_start = self.clock.as_ref().map(|clock| clock.now());
        self.telemetry.reset_canvas2d_ops();
        self.telemetry.set_gpu_resources(0, 0, 0);
        self.telemetry.set_browser_rebuilds(0, 0);
        let outcome = self.render_inner();
        if let (Some(clock), Some(start)) = (self.clock.as_ref(), frame_start) {
            self.telemetry.set_cpu_ms(clock.now() - start);
        }
        outcome
    }

    fn render_inner(&mut self) -> Result<(), JsValue> {
        self.engine.begin_frame_build();
        // Series primitives (plugin platform Phase C-b): pull this frame's autoscale
        // contributions from the plugin hooks before any layout/autoscale pass runs, so the
        // axis-width negotiation, axis frame, and pane frame all see the merged ranges.
        self.collect_series_primitive_autoscale();
        // Custom series (Phase C-c): same pre-layout collection point — the visible items'
        // price values become autoscale contributions and the engine's custom frame values.
        self.collect_custom_series_autoscale();
        let plugin_active = !self.primitives.is_empty()
            || !self.series_primitives.is_empty()
            || !self.custom_series.is_empty();
        if plugin_active {
            // Plugins append transient labels to `axis_frame`; rebuild its engine-owned base on
            // every conservative plugin frame so labels neither duplicate nor survive detach.
            self.engine.invalidate_axis_frame();
        }

        // Feed the engine clock for the candle-close countdown labels: the host-pinned value
        // when `set_now_seconds` installed one (the package's 1s countdown timer), else the
        // browser's system time — the engine itself is headless and owns no clock. This must
        // happen before layout so the first rendered countdown participates in width negotiation.
        let now = self
            .now_override
            .unwrap_or_else(|| js_sys::Date::now() / 1000.0);
        self.engine.set_now_seconds(now);

        // ---- layout (price axis width negotiated against the price labels) ----
        if self.engine.frame_requires_layout() {
            self.recompute_layout(false);
        }

        // Settle layout, autoscale, and retained pane layers before building axis labels so the
        // axis consumes the same finalized scale ranges as the canonical pane frame.
        self.engine.build_frame_into_accumulating(&mut self.frame);
        self.telemetry.set_rebuilds(self.engine.frame_build_stats());

        // Build the retained axis labels only when an axis input changed.
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
        if self.engine.frame_requires_axis() {
            let next_axis_frame = self.engine.build_axis_frame(max_label_width, |text| {
                measure_text_ctx(axis_ctx, dpr, &font_family, font_size, text)
            });
            if next_axis_frame != self.axis_frame {
                self.axis_frame = next_axis_frame;
            }
            // Axis lowering also consumes option colors that are not stored in `AxisFrame`.
            // Rebuild its primitives whenever the engine invalidates the axis, even when label
            // geometry itself compares equal (for example a separator-only theme change).
            self.axis_dirty = true;
        }

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
        if !self.primitives.is_empty()
            || !self.series_primitives.is_empty()
            || !self.custom_series.is_empty()
        {
            self.axis_dirty = true;
        }
        // Axis labels contributed by primitives are now complete; convert the whole top layer once
        // and feed it to whichever backend executes this frame.
        let axis_rebuilt = self.axis_dirty;
        self.build_axis_prims();
        let text_rasterizations_before = self
            .text_runs
            .as_ref()
            .map_or(0, TextRunStore::rasterizations);

        if self
            .gfx
            .as_ref()
            .is_some_and(|gfx| gfx.device_lost.load(Ordering::Acquire))
        {
            self.activate_canvas2d("device_lost", "WebGPU device was lost");
        }

        let bg = resolved_surface_color(&self.opts().layout.background.color);
        // Arm GPU timestamp collection on the first frame after the host reads `frame_stats()`.
        // `GpuTimer::new` is a feature-flag check plus (once) a query set, so an unsupported
        // device just keeps answering `None` and `gpu_ms` stays null.
        if self.telemetry.stats_requested() {
            if let Some(gfx) = self.gfx.as_mut() {
                if gfx.timer.is_none() {
                    gfx.timer = GpuTimer::new(&gfx.shared.device, &gfx.shared.queue);
                }
            }
        }

        let pane_outcome = if self.gfx.is_some() {
            let engine_frame = &self.frame;
            let pane_count = engine_frame.panes.len();
            let pane_group_count = if plugin_active {
                pane_count
            } else {
                (0..pane_count)
                    .map(|pane| 6 + self.engine.frame_series_segments(pane).len())
                    .sum()
            };
            self.gpu_groups
                .resize_with(pane_group_count + 1, DrawGroup::default);
            self.gpu_groups.truncate(pane_group_count + 1);
            let Some(gfx) = self.gfx.as_mut() else {
                return Err(JsValue::from_str("WebGPU state disappeared mid-render"));
            };
            let shared = Rc::clone(&gfx.shared);
            let renderers = Rc::clone(&gfx.renderers);
            let text_runs = &mut self.text_runs;
            let mut atlas = shared.atlas.borrow_mut();
            let mut image_atlas = shared.image_atlas.borrow_mut();
            atlas.begin_frame();
            image_atlas.begin_frame();
            let atlas_changed = self.gpu_atlas_epoch != atlas.epoch();
            let image_atlas_changed = self.gpu_image_atlas_epoch != image_atlas.epoch();
            self.gpu_atlas_epoch = atlas.epoch();
            self.gpu_image_atlas_epoch = image_atlas.epoch();
            if !atlas_changed
                && self
                    .gpu_groups
                    .iter()
                    .any(|group| !group.tex_quads.is_empty())
            {
                atlas.protect_retained_frame_slots();
            }
            if !image_atlas_changed
                && self
                    .gpu_groups
                    .iter()
                    .any(|group| !group.image_quads.is_empty())
            {
                image_atlas.protect_retained_frame_slots();
            }
            let queue = &shared.queue;
            if plugin_active {
                for (pane, pane_frame) in engine_frame.panes.iter().enumerate() {
                    let group = &mut self.gpu_groups[pane];
                    group.key = 0x1000_0000 | pane as u64;
                    group.scissor = Some(pane_frame.scissor);
                    group.clear();
                    let mut resolve_text = |prim: &Prim| {
                        text_runs
                            .as_mut()
                            .and_then(|runs| runs.resolve(&mut atlas, queue, prim))
                    };
                    let mut resolve_image =
                        |prim: &Prim| super::image_runs::resolve(&mut image_atlas, queue, prim);
                    prims_to_group(
                        &pane_frame.under,
                        &pane_frame.points,
                        group,
                        &mut resolve_text,
                        &mut resolve_image,
                    );
                    prims_to_group(
                        &pane_frame.main,
                        &pane_frame.points,
                        group,
                        &mut resolve_text,
                        &mut resolve_image,
                    );
                    prims_to_group(
                        &pane_frame.top_prims,
                        &pane_frame.points,
                        group,
                        &mut resolve_text,
                        &mut resolve_image,
                    );
                }
            } else {
                let mut build_group = |group: &mut DrawGroup,
                                       key: u64,
                                       source_revision: u64,
                                       scissor: Option<[u32; 4]>,
                                       prims: &[Prim],
                                       points: &[[f32; 2]]| {
                    if !atlas_changed
                        && !image_atlas_changed
                        && group.key == key
                        && group.source_revision == source_revision
                        && group.scissor == scissor
                    {
                        return;
                    }
                    group.scissor = scissor;
                    group.rebuild(key, source_revision);
                    let mut resolve_text = |prim: &Prim| {
                        text_runs
                            .as_mut()
                            .and_then(|runs| runs.resolve(&mut atlas, queue, prim))
                    };
                    let mut resolve_image =
                        |prim: &Prim| super::image_runs::resolve(&mut image_atlas, queue, prim);
                    prims_to_group(prims, points, group, &mut resolve_text, &mut resolve_image);
                };
                let mut group_index = 0;
                for (pane, pane_frame) in engine_frame.panes.iter().enumerate() {
                    let segments = self.engine.frame_pane_segments(pane).unwrap_or_default();
                    let main = &pane_frame.main;
                    build_group(
                        &mut self.gpu_groups[group_index],
                        0x2000_0000 | (pane as u64) << 4,
                        segments.under_revision,
                        Some(pane_frame.scissor),
                        &pane_frame.under[..segments.under_end.min(pane_frame.under.len())],
                        &pane_frame.points,
                    );
                    group_index += 1;
                    for series in self.engine.frame_series_segments(pane) {
                        let key = match series.series_id {
                            Some(id) => 0x3000_0000 | (pane as u64) << 32 | u64::from(id),
                            None => 0x4000_0000 | pane as u64,
                        };
                        build_group(
                            &mut self.gpu_groups[group_index],
                            key,
                            series.revision,
                            Some(pane_frame.scissor),
                            &main[series.start.min(main.len())..series.end.min(main.len())],
                            &pane_frame.points,
                        );
                        group_index += 1;
                    }
                    build_group(
                        &mut self.gpu_groups[group_index],
                        0x2000_0005 | (pane as u64) << 4,
                        segments.trading_revision,
                        Some(pane_frame.scissor),
                        &main[segments.series_end.min(main.len())
                            ..segments.trading_regions_end.min(main.len())],
                        &pane_frame.points,
                    );
                    group_index += 1;
                    build_group(
                        &mut self.gpu_groups[group_index],
                        0x2000_0002 | (pane as u64) << 4,
                        segments.drawings_revision,
                        Some(pane_frame.scissor),
                        &main[segments.trading_regions_end.min(main.len())
                            ..segments.drawings_end.min(main.len())],
                        &pane_frame.points,
                    );
                    group_index += 1;
                    build_group(
                        &mut self.gpu_groups[group_index],
                        0x2000_0006 | (pane as u64) << 4,
                        segments.trading_revision,
                        Some(pane_frame.scissor),
                        &main[segments.drawings_end.min(main.len())
                            ..segments.trading_end.min(main.len())],
                        &pane_frame.points,
                    );
                    group_index += 1;
                    build_group(
                        &mut self.gpu_groups[group_index],
                        0x2000_0003 | (pane as u64) << 4,
                        segments.overlay_revision,
                        Some(pane_frame.scissor),
                        &main[segments.trading_end.min(main.len())
                            ..segments.overlay_end.min(main.len())],
                        &pane_frame.points,
                    );
                    group_index += 1;
                    build_group(
                        &mut self.gpu_groups[group_index],
                        0x2000_0004 | (pane as u64) << 4,
                        segments.top_revision,
                        Some(pane_frame.scissor),
                        &pane_frame.top_prims,
                        &pane_frame.points,
                    );
                    group_index += 1;
                }
            }
            // Final unscissored top-layer group: watermark, axis chrome and axis/crosshair labels.
            // It is submitted in this same pass after every pane group, so no engine Canvas2D paint
            // follows a WebGPU frame.
            let axis_group = &mut self.gpu_groups[pane_group_count];
            if atlas_changed
                || image_atlas_changed
                || axis_group.key != u64::MAX
                || axis_group.source_revision != self.axis_revision
            {
                axis_group.scissor = None;
                axis_group.rebuild(u64::MAX, self.axis_revision);
                let mut resolve_text = |prim: &Prim| {
                    text_runs
                        .as_mut()
                        .and_then(|runs| runs.resolve(&mut atlas, queue, prim))
                };
                let mut resolve_image =
                    |prim: &Prim| super::image_runs::resolve(&mut image_atlas, queue, prim);
                prims_to_group(
                    &self.axis_prims,
                    &[],
                    axis_group,
                    &mut resolve_text,
                    &mut resolve_image,
                );
            }
            let atlas_valid = atlas.frame_valid() && image_atlas.frame_valid();
            drop(atlas);
            drop(image_atlas);
            if !atlas_valid {
                PaneRenderOutcome::Canvas2d
            } else {
                let groups = &self.gpu_groups[..];
                gfx.msaa.ensure(
                    &shared.device,
                    gfx.config.format,
                    gfx.config.width,
                    gfx.config.height,
                );

                let acquired = match gfx.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(Some(frame)),
                    error => match surface_error_action(&error) {
                        SurfaceErrorAction::Reconfigure => {
                            // Resize and suspend/resume can invalidate only the swapchain. Reconfigure
                            // and retry once; if that fails, the warm Canvas2D pane takes over.
                            gfx.surface.configure(&shared.device, &gfx.config);
                            match gfx.surface.get_current_texture() {
                                wgpu::CurrentSurfaceTexture::Success(frame)
                                | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => Ok(Some(frame)),
                                retry_error
                                    if surface_error_action(&retry_error)
                                        == SurfaceErrorAction::SkipFrame =>
                                {
                                    Ok(None)
                                }
                                _ => Err("surface remained unavailable after reconfiguration"),
                            }
                        }
                        SurfaceErrorAction::SkipFrame => Ok(None),
                        SurfaceErrorAction::Fallback => Err("surface validation failed"),
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
                        let resources_before = gfx.frame_resources.stats();
                        let draw_calls = render_frame(
                            &shared.device,
                            &shared.queue,
                            gfx.msaa.view(),
                            &view,
                            gfx.config.width,
                            gfx.config.height,
                            bg_clear,
                            &renderers.quad,
                            &renderers.tex,
                            &renderers.image,
                            &renderers.tri,
                            groups,
                            &mut gfx.frame_resources,
                            gfx.timer.as_ref(),
                        );
                        let resources_after = gfx.frame_resources.stats();
                        self.telemetry.set_gpu_resources(
                            resources_after.allocations - resources_before.allocations,
                            resources_after.write_calls - resources_before.write_calls,
                            resources_after.uploaded_bytes - resources_before.uploaded_bytes,
                        );
                        self.telemetry.set_draw_calls(draw_calls);
                        shared.queue.present(frame);
                        PaneRenderOutcome::Presented
                    }
                    Ok(None) => PaneRenderOutcome::Timeout,
                    Err(error) => PaneRenderOutcome::Fallback(format!(
                        "WebGPU surface acquisition failed after recovery: {error}"
                    )),
                }
            }
        } else {
            PaneRenderOutcome::Canvas2d
        };

        let text_rasterizations_after = self
            .text_runs
            .as_ref()
            .map_or(0, TextRunStore::rasterizations);
        self.telemetry.set_browser_rebuilds(
            u64::from(axis_rebuilt),
            text_rasterizations_after.saturating_sub(text_rasterizations_before),
        );

        match pane_outcome {
            PaneRenderOutcome::Presented => {}
            PaneRenderOutcome::Timeout => {
                // Keep the last complete frame. The next animation/input repaint retries.
                // This is exactly `frame_stats().dropped_frames`: encoded but never presented.
                self.telemetry.count_dropped();
                return Ok(());
            }
            PaneRenderOutcome::Fallback(reason) => {
                self.activate_canvas2d("surface_acquisition_failed", &reason);
                self.render_canvas2d()?;
            }
            PaneRenderOutcome::Canvas2d => self.render_canvas2d()?,
        }

        self.paint_primitive_text_overlay()?;
        self.telemetry.count_presented();
        Ok(())
    }

    // ---- Shared axis/top-layer frame ----

    /// Convert the engine-owned [`AxisFrame`] plus watermark into the shared backend-neutral
    /// axis/top layer. The engine owns this policy; this browser host contributes only its native
    /// Canvas text ink metric.
    fn build_axis_prims(&mut self) {
        if !self.axis_dirty {
            return;
        }
        let axis_ctx = &self.axis_ctx;
        let layout = self.opts().layout.clone();
        let dpr = self.dpr;
        // `measure_text_ctx` leaves the measurement canvas in bitmap-font space. Axis primitive
        // placement is expressed in logical pixels until the engine performs the single DPR
        // conversion, so normalize the browser's bitmap ink metrics before returning them.
        axis_ctx.set_font(&format!(
            "{}px {}",
            layout.font_size * dpr,
            layout.font_family
        ));
        axis_ctx.set_text_baseline("middle");
        self.engine
            .build_axis_primitives_into(&self.axis_frame, &mut self.axis_prims, |text| {
                axis_ctx
                    .measure_text(text)
                    .ok()
                    .map(|metrics| {
                        crate::text_cache::logical_midpoint_correction(
                            metrics.actual_bounding_box_ascent(),
                            metrics.actual_bounding_box_descent(),
                            dpr,
                        )
                    })
                    .unwrap_or(0.0)
            });
        self.axis_revision = self.axis_revision.wrapping_add(1).max(1);
        self.axis_dirty = false;
    }

    // ---- Legacy Canvas2D plugin-text escape hatch ----

    fn paint_primitive_text_overlay(&mut self) -> Result<(), JsValue> {
        if self.primitive_texts.is_empty() {
            if self.overlay_had_plugin_text {
                self.axis_ctx
                    .clear_rect(0.0, 0.0, self.bitmap_w as f64, self.bitmap_h as f64);
                self.overlay_had_plugin_text = false;
                self.telemetry.add_canvas2d_ops(1);
            }
            return Ok(());
        }
        self.axis_ctx
            .clear_rect(0.0, 0.0, self.bitmap_w as f64, self.bitmap_h as f64);
        let ops = 1 + self.draw_primitive_overlay_texts(self.dpr)?;
        self.overlay_had_plugin_text = true;
        self.telemetry.add_canvas2d_ops(ops);
        Ok(())
    }

    #[allow(dead_code)]
    fn draw_axes_2d(&self, axis_frame: &AxisFrame) -> Result<(), JsValue> {
        let ctx = &self.axis_ctx;
        let dpr = self.dpr;
        let bitmap_w = self.bitmap_w as f64;
        let bitmap_h = self.bitmap_h as f64;
        let pane_left = self.pane_left;
        let pane_w = self.pane_w;
        let pane_h = self.pane_h;

        // Every op below is tallied into `frame_stats().canvas2d_ops` — the metric that proves
        // (Item 4) the WebGPU path issues no Canvas2D work per frame. Accumulated locally and
        // published once so the counting itself costs one `Cell` write per frame.
        let mut ops = 1u32; // the clear_rect
        ctx.clear_rect(0.0, 0.0, bitmap_w, bitmap_h);
        let border_w = 1f64.max(dpr.floor());

        let options = self.opts();
        // Watermark paints first so it sits below the axis borders, labels, and crosshair chrome.
        ops += self.draw_watermark(&options.watermark, dpr)?;
        // The primitives' `text_views` overlay draws share the watermark's slot (Phase 3.5):
        // in-pane plugin text, below the axis chrome, identical on both backends.
        ops += self.draw_primitive_overlay_texts(dpr)?;

        // Axis borders come from the options store (reference `borderColor`/`borderVisible` per strip);
        // an unparseable color falls back to the reference default.
        let border = nucleuscharts_core::style::DEFAULT_BORDER_RGB;
        let fallback = Color::rgb(border.0, border.1, border.2);
        let left_border = Color::parse_css(&options.left_price_scale.border_color)
            .unwrap_or(fallback)
            .to_hex();
        let right_border = Color::parse_css(&options.right_price_scale.border_color)
            .unwrap_or(fallback)
            .to_hex();
        let time_border = Color::parse_css(&options.time_scale.border_color)
            .unwrap_or(fallback)
            .to_hex();

        for band in &axis_frame.bands {
            ctx.set_fill_style_str(&band.color.to_css());
            ctx.fill_rect(
                (band.x * dpr).round(),
                (band.y * dpr).round(),
                (band.width * dpr).round(),
                (band.height * dpr).round(),
            );
            ops += 1;
        }

        if self.left_axis_w > 0.0 && options.left_price_scale.border_visible {
            ctx.set_fill_style_str(&left_border);
            ctx.fill_rect(
                (pane_left * dpr).round() - border_w,
                0.0,
                border_w,
                (pane_h * dpr).round(),
            );
            ops += 1;
        }
        if self.axis_w > 0.0 && options.right_price_scale.border_visible {
            ctx.set_fill_style_str(&right_border);
            ctx.fill_rect(
                ((pane_left + pane_w) * dpr).round(),
                0.0,
                border_w,
                (pane_h * dpr).round(),
            );
            ops += 1;
        }
        if options.time_scale.border_visible && self.engine.time_axis_visible {
            ctx.set_fill_style_str(&time_border);
            ctx.fill_rect(0.0, (pane_h * dpr).round(), bitmap_w, border_w);
            ops += 1;
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
            let color = if tick.left {
                if !left_ticks {
                    continue;
                }
                &left_border
            } else {
                if !right_ticks {
                    continue;
                }
                &right_border
            };
            ctx.set_fill_style_str(color);
            ctx.fill_rect(
                (tick.x * dpr).round(),
                (tick.y * dpr).round() - tick_off,
                tick_len,
                tick_h,
            );
            ops += 1;
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
                ops += 1;
            }
        }

        // Separators between stacked panes (roadmap Phase B1): a border line at each pane
        // boundary. An unset `layout.panes.separatorColor` follows the price-axis border color
        // (theme-aware like the axis chrome); an explicit value pins it. Painted regardless of
        // the time-axis border's visibility since they are functional dividers, not axis chrome.
        // The divider is structural, so it spans the complete bitmap width — through every
        // visible price-scale strip — matching the hover band and the separator hit test.
        let separator_color = Color::parse_css(&options.layout.panes.separator_color)
            .unwrap_or_else(|| {
                Color::parse_css(&options.right_price_scale.border_color).unwrap_or(fallback)
            })
            .to_hex();
        ctx.set_fill_style_str(&separator_color);
        for separator in &axis_frame.separators {
            let y = (separator * dpr).round();
            ctx.fill_rect(0.0, y, bitmap_w, (PANE_SEPARATOR * dpr).max(border_w));
            ops += 1;
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
            ops += 1;
        }

        ops += self.draw_axis_labels(
            axis_frame,
            dpr,
            &options.layout.font_family,
            options.layout.font_size,
        )?;
        self.telemetry.add_canvas2d_ops(ops);
        Ok(())
    }

    /// Paint the `watermark` label onto the overlay, anchored inside the pane per `horzAlign`/
    /// `vertAlign`. Drawn in media coordinates (context scaled by DPR) like the axis labels; the
    /// CSS color string is passed through verbatim so alpha is preserved.
    #[allow(dead_code)]
    fn draw_watermark(&self, wm: &WatermarkOptions, dpr: f64) -> Result<u32, JsValue> {
        if !wm.visible || wm.text.is_empty() {
            return Ok(0);
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
        let result = ctx.fill_text(&wm.text, x, y).map(|_| 1);
        ctx.restore();
        result
    }

    /// Paint the primitives' `text_views` overlay draws (plugin platform Phase 3.5) in media
    /// coordinates (context scaled by DPR) like the axis labels. Each draw carries its own
    /// fully-resolved font, color, and canvas alignment keywords; colors pass through verbatim
    /// so alpha is preserved (same rule as the watermark).
    fn draw_primitive_overlay_texts(&self, dpr: f64) -> Result<u32, JsValue> {
        if self.primitive_texts.is_empty() {
            return Ok(0);
        }
        let ctx = &self.axis_ctx;
        ctx.save();
        if let Err(error) = ctx.scale(dpr, dpr) {
            ctx.restore();
            return Err(error);
        }
        let mut ops = 0;
        let mut draw_result = Ok(0);
        for text in &self.primitive_texts {
            ctx.save();
            ctx.begin_path();
            ctx.rect(text.clip[0], text.clip[1], text.clip[2], text.clip[3]);
            ctx.clip();
            ctx.set_font(&text.font);
            ctx.set_fill_style_str(&text.color);
            ctx.set_text_align(&text.align);
            ctx.set_text_baseline(&text.baseline);
            let result = ctx.fill_text(&text.text, text.x, text.y).map(|_| ());
            ctx.restore();
            if let Err(error) = result {
                draw_result = Err(error);
                break;
            }
            ops += 1;
            draw_result = Ok(ops);
        }
        ctx.restore();
        draw_result
    }

    #[allow(dead_code)]
    fn draw_axis_labels(
        &self,
        axis_frame: &AxisFrame,
        dpr: f64,
        font_family: &str,
        font_size: f64,
    ) -> Result<u32, JsValue> {
        let ctx = &self.axis_ctx;

        // Z-order matters: boxed labels (last value, price lines, crosshair) must fully cover any
        // ordinary tick label they overlap, exactly like reference where each axis view paints its
        // background and text as one unit in view order. Painting all backgrounds first and all
        // texts second lets tick glyphs bleed onto the boxes, so paint in two ordered layers:
        // plain tick text first, then each boxed label's background + text.
        let mut ops = self.draw_axis_label_texts(
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
                // Sizes come from the box's rounded far EDGE minus its rounded offset, so adjacent
                // boxes (e.g. the outside title chip vs the in-strip price chip) keep a
                // deterministic device-px gap instead of breathing ±1px from independent
                // offset/width rounding.
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
                ops += 1;
                if let Some((border_width, border_color)) = label.border {
                    let line_width = (border_width * dpr).max(1.0);
                    ctx.set_fill_style_str(&border_color.to_css());
                    if label.background_corners.is_empty() {
                        ctx.fill_rect(bx, by, bw, bh);
                    } else {
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
                    ctx.set_fill_style_str(&color.to_css());
                    let inner_corners = label.background_corners;
                    if inner_corners.is_empty() {
                        ctx.fill_rect(
                            bx + line_width,
                            by + line_width,
                            (bw - line_width * 2.0).max(0.0),
                            (bh - line_width * 2.0).max(0.0),
                        );
                    } else {
                        fill_boxed_label_background(
                            ctx,
                            bx + line_width,
                            by + line_width,
                            (bw - line_width * 2.0).max(0.0),
                            (bh - line_width * 2.0).max(0.0),
                            (2.0 * dpr - line_width).max(0.0),
                            inner_corners,
                        );
                    }
                    ops += 1;
                } else if label.background_corners.is_empty() {
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
            ops +=
                self.draw_axis_label_texts(std::iter::once(label), dpr, font_family, font_size)?;
        }
        Ok(ops)
    }

    /// Draws label glyphs in media-coordinate space: the context is scaled by DPR while the font
    /// stays at the configured CSS px size. Using an independently hinted size*dpr bitmap font is
    /// observably different at fractional DPR even when every logical coordinate is identical.
    #[allow(dead_code)]
    fn draw_axis_label_texts<'l>(
        &self,
        labels: impl Iterator<Item = &'l AxisLabel>,
        dpr: f64,
        font_family: &str,
        font_size: f64,
    ) -> Result<u32, JsValue> {
        let ctx = &self.axis_ctx;
        ctx.save();
        if let Err(error) = ctx.scale(dpr, dpr) {
            ctx.restore();
            return Err(error);
        }
        ctx.set_text_baseline("middle");
        let mut ops = 0;
        let mut draw_result = Ok(0);
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
            ops += 1;
            draw_result = Ok(ops);
        }
        ctx.restore();
        draw_result
    }

    /// Permanently switch this chart instance to its already-initialized Canvas2D pane.
    fn activate_canvas2d(&mut self, stable_reason: &'static str, detail: &str) {
        if self.gfx.take().is_some() {
            set_backend_visibility(self.gpu_pane.as_ref(), self.fallback_pane.as_ref(), false);
            self.backend_status
                .runtime_fallback(stable_reason, detail.to_string());
            warn_backend_fallback(&self.backend_status);
        }
    }

    /// Execute the exact same retained frame consumed by WebGPU through Canvas2D.
    pub(super) fn render_canvas2d(&self) -> Result<(), JsValue> {
        self.render_canvas2d_with_axis(true)
    }

    /// Execute the retained pane frame through Canvas2D, optionally including the shared
    /// watermark/axis top layer. Screenshot capture passes `false` to preserve the established
    /// `take_screenshot(add_top_layer = false)` behavior now that axes no longer live on the
    /// transparent overlay canvas.
    pub(super) fn render_canvas2d_with_axis(&self, include_axis: bool) -> Result<(), JsValue> {
        let ctx = &self.pane_ctx;
        let width = self.bitmap_w as f64;
        let height = self.bitmap_h as f64;
        ctx.clear_rect(0.0, 0.0, width, height);
        let bg = self.opts().layout.background.color;
        ctx.set_fill_style_str(&bg);
        ctx.fill_rect(0.0, 0.0, width, height);
        let mut image_store = self.canvas_images.borrow_mut();
        let mut target = crate::canvas2d_target::WasmCanvas2d::with_images(ctx, &mut image_store);
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
        if include_axis {
            execute_canvas2d(&self.axis_prims, &[], &mut target, viewport);
        }
        // The `clear_rect` + background `fill_rect` above, plus every executed prim.
        self.telemetry.add_canvas2d_ops(2 + target.ops());
        Ok(())
    }
}

fn resolved_surface_color(css: &str) -> Color {
    let fallback = nucleuscharts_core::style::DEFAULT_SURFACE_RGB;
    Color::parse_css(css).unwrap_or(Color::rgb(fallback.0, fallback.1, fallback.2))
}

#[cfg(test)]
mod style_tests {
    use super::*;

    #[test]
    fn malformed_surface_css_falls_back_to_the_canonical_nucleus_surface() {
        let expected = nucleuscharts_core::style::DEFAULT_SURFACE_RGB;
        assert_eq!(
            resolved_surface_color("not-a-color"),
            Color::rgb(expected.0, expected.1, expected.2)
        );
    }
}

/// Fill a boxed axis label's background with per-corner rounding (TradingView-style side
/// radius): the path is built manually from lines and quadratic arcs — corners flagged in
/// `corners` get `radius`, the rest stay sharp. The radius clamps to half the box so thin
/// boxes keep a well-formed path. Coordinates are bitmap px (the axis context is unscaled here).
#[allow(dead_code)]
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
