//! Bounded, deterministic level-two order-book state.
//!
//! The book is the authoritative owner of sequence continuity, tick-grid normalization, current
//! levels, and time-bucketed historical snapshots. Hosts perform provider-specific decoding and
//! recovery; the engine reports a typed resync request instead of guessing across a gap.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use aeris_charts_render::draw_list::RasterImage;

pub const MAX_DEPTH_STREAMS: usize = 64;
pub const MAX_DEPTH_STREAM_KEY_BYTES: usize = 128;
pub const MAX_DEPTH_LEVELS_PER_SIDE: usize = 16_384;
pub const MAX_DEPTH_HISTORY_BUCKETS: usize = 8_192;
pub const MAX_DEPTH_HISTORY_CELLS: usize = 2_000_000;
pub const MAX_DEPTH_BATCH_UPDATES: usize = 1_000_000;
pub const MAX_DEPTH_EVENT_MARKERS: usize = 16_384;
pub const MAX_DEPTH_EVENT_LABEL_BYTES: usize = 256;
pub const MAX_DEPTH_REPLAY_UPDATES: usize = 1_000_000;
pub const MAX_DEPTH_HEATMAP_ROWS: usize = 4_096;
pub const MAX_DEPTH_HEATMAPS: usize = 16;
pub const MAX_DEPTH_EVENT_LAYERS: usize = 64;
pub const DEPTH_HEATMAP_CHUNK_COLUMNS: usize = 32;
const DEPTH_REPLAY_CHECKPOINT_INTERVAL: usize = 1_024;
const MAX_DEPTH_REPLAY_CHECKPOINTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DepthSide {
    Bid,
    Ask,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum DepthEventKind {
    IcebergRefill,
    PulledLiquidity,
    SizeCluster,
    Sweep,
    Mixed,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DepthMicrostructureEvent {
    pub timestamp_micros: i64,
    pub price: f64,
    pub size: f64,
    pub side: Option<DepthSide>,
    pub kind: DepthEventKind,
    pub host_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct DepthEventCluster {
    pub timestamp_micros: i64,
    pub price: f64,
    pub size: f64,
    pub side: Option<DepthSide>,
    pub kind: DepthEventKind,
    pub event_count: u32,
    pub host_label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DepthLevel {
    pub price: f64,
    pub size: f64,
    pub order_count: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DepthSnapshot {
    pub timestamp_micros: i64,
    pub sequence: u64,
    pub bids: Vec<DepthLevel>,
    pub asks: Vec<DepthLevel>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthUpdate {
    pub timestamp_micros: i64,
    pub sequence: u64,
    pub previous_sequence: u64,
    pub side: DepthSide,
    pub level: DepthLevel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DepthResyncRequest {
    pub expected_previous_sequence: u64,
    pub received_previous_sequence: u64,
    pub received_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct DepthBucket {
    pub start_micros: i64,
    pub bids: Vec<DepthLevel>,
    pub asks: Vec<DepthLevel>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct DepthLadderRow {
    pub price: f64,
    pub bid_size: Option<f64>,
    pub ask_size: Option<f64>,
    pub bid_order_count: Option<u32>,
    pub ask_order_count: Option<u32>,
    pub distance_from_touch_ticks: u32,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct DepthStudySnapshot {
    pub sequence: Option<u64>,
    pub best_bid: Option<DepthLevel>,
    pub best_ask: Option<DepthLevel>,
    pub bid_cumulative: Vec<DepthLevel>,
    pub ask_cumulative: Vec<DepthLevel>,
    pub imbalance: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DepthHeatmapOptions {
    pub pane_index: usize,
    pub price_min: f64,
    pub price_max: f64,
    pub minimum_size: f64,
    pub maximum_size: f64,
    pub bid_rgba: [u8; 4],
    pub ask_rgba: [u8; 4],
    pub opacity: f32,
}

impl Default for DepthHeatmapOptions {
    fn default() -> Self {
        Self {
            pane_index: 0,
            price_min: 0.0,
            price_max: 1_000_000.0,
            minimum_size: 0.0,
            maximum_size: 1_000.0,
            bid_rgba: [38, 166, 154, 220],
            ask_rgba: [239, 83, 80, 220],
            opacity: 0.72,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DepthEventLayerOptions {
    pub pane_index: usize,
    pub max_markers: usize,
}

impl Default for DepthEventLayerOptions {
    fn default() -> Self {
        Self {
            pane_index: 0,
            max_markers: 2_048,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct DepthEventLayer {
    pub stream_id: u64,
    pub options: DepthEventLayerOptions,
}

#[derive(Clone, Debug)]
pub(crate) struct DepthHeatmapColumn {
    pub start_micros: i64,
    pub image: RasterImage,
}

#[derive(Clone, Debug)]
pub(crate) struct DepthHeatmapChunk {
    pub start_micros: i64,
    pub bucket_starts: Vec<i64>,
    pub image: RasterImage,
}

#[derive(Clone, Debug)]
pub(crate) struct DepthHeatmap {
    pub id: u64,
    pub stream_id: u64,
    pub options: DepthHeatmapOptions,
    pub chunks: VecDeque<DepthHeatmapChunk>,
    pub active: Option<DepthHeatmapColumn>,
    history_signature: Option<(usize, i64, i64)>,
    next_image_revision: u64,
}

impl DepthHeatmap {
    fn new(id: u64, stream_id: u64, options: DepthHeatmapOptions) -> Self {
        Self {
            id,
            stream_id,
            options,
            chunks: VecDeque::new(),
            active: None,
            history_signature: None,
            next_image_revision: 1,
        }
    }

    fn image_key(&mut self) -> u64 {
        let revision = self.next_image_revision;
        self.next_image_revision = self.next_image_revision.saturating_add(1);
        (self.id << 32) | revision.min(u64::from(u32::MAX))
    }

    fn column(
        &mut self,
        book: &DepthBook,
        bucket: &DepthBucket,
    ) -> Result<DepthHeatmapColumn, DepthError> {
        let tick_size = book.options.tick_size;
        let first_tick = price_tick(tick_size, self.options.price_min)
            .map_err(|_| DepthError::InvalidOptions)?;
        let last_tick = price_tick(tick_size, self.options.price_max)
            .map_err(|_| DepthError::InvalidOptions)?;
        let rows = last_tick
            .checked_sub(first_tick)
            .and_then(|span| span.checked_add(1))
            .and_then(|rows| usize::try_from(rows).ok())
            .filter(|&rows| rows != 0 && rows <= MAX_DEPTH_HEATMAP_ROWS)
            .ok_or(DepthError::InvalidOptions)?;
        let mut pixels = vec![0_u8; rows.saturating_mul(4)];
        for (side, levels) in [
            (DepthSide::Bid, &bucket.bids),
            (DepthSide::Ask, &bucket.asks),
        ] {
            let color = match side {
                DepthSide::Bid => self.options.bid_rgba,
                DepthSide::Ask => self.options.ask_rgba,
            };
            for level in levels {
                if level.size < self.options.minimum_size {
                    continue;
                }
                let Ok(tick) = price_tick(tick_size, level.price) else {
                    continue;
                };
                let Some(row) = tick
                    .checked_sub(first_tick)
                    .and_then(|row| usize::try_from(row).ok())
                    .filter(|&row| row < rows)
                else {
                    continue;
                };
                let intensity = ((level.size - self.options.minimum_size)
                    / (self.options.maximum_size - self.options.minimum_size))
                    .clamp(0.0, 1.0);
                let offset = (rows - 1 - row) * 4;
                pixels[offset..offset + 3].copy_from_slice(&color[..3]);
                pixels[offset + 3] = (f64::from(color[3]) * intensity).round() as u8;
            }
        }
        let key = self.image_key();
        Ok(DepthHeatmapColumn {
            start_micros: bucket.start_micros,
            image: RasterImage {
                key,
                width: 1,
                height: rows as u32,
                pixels: Arc::from(pixels),
            },
        })
    }

    fn chunk(
        &mut self,
        book: &DepthBook,
        chunk_start_micros: i64,
        buckets: &[&DepthBucket],
    ) -> Result<DepthHeatmapChunk, DepthError> {
        let rows = self.row_count(book)?;
        let mut pixels = vec![
            0_u8;
            rows.saturating_mul(DEPTH_HEATMAP_CHUNK_COLUMNS)
                .saturating_mul(4)
        ];
        let interval = book.options.history_bucket_micros;
        for bucket in buckets {
            let offset = bucket
                .start_micros
                .saturating_sub(chunk_start_micros)
                .div_euclid(interval);
            let Ok(column) = usize::try_from(offset) else {
                continue;
            };
            if column >= DEPTH_HEATMAP_CHUNK_COLUMNS {
                continue;
            }
            self.paint_bucket(
                book,
                bucket,
                rows,
                column,
                DEPTH_HEATMAP_CHUNK_COLUMNS,
                &mut pixels,
            )?;
        }
        let key = self.image_key();
        Ok(DepthHeatmapChunk {
            start_micros: chunk_start_micros,
            bucket_starts: buckets.iter().map(|bucket| bucket.start_micros).collect(),
            image: RasterImage {
                key,
                width: DEPTH_HEATMAP_CHUNK_COLUMNS as u32,
                height: rows as u32,
                pixels: Arc::from(pixels),
            },
        })
    }

    fn row_count(&self, book: &DepthBook) -> Result<usize, DepthError> {
        let first_tick = price_tick(book.options.tick_size, self.options.price_min)
            .map_err(|_| DepthError::InvalidOptions)?;
        let last_tick = price_tick(book.options.tick_size, self.options.price_max)
            .map_err(|_| DepthError::InvalidOptions)?;
        last_tick
            .checked_sub(first_tick)
            .and_then(|span| span.checked_add(1))
            .and_then(|rows| usize::try_from(rows).ok())
            .filter(|&rows| rows != 0 && rows <= MAX_DEPTH_HEATMAP_ROWS)
            .ok_or(DepthError::InvalidOptions)
    }

    fn paint_bucket(
        &self,
        book: &DepthBook,
        bucket: &DepthBucket,
        rows: usize,
        column: usize,
        stride: usize,
        pixels: &mut [u8],
    ) -> Result<(), DepthError> {
        let tick_size = book.options.tick_size;
        let first_tick = price_tick(tick_size, self.options.price_min)
            .map_err(|_| DepthError::InvalidOptions)?;
        for (side, levels) in [
            (DepthSide::Bid, &bucket.bids),
            (DepthSide::Ask, &bucket.asks),
        ] {
            let color = match side {
                DepthSide::Bid => self.options.bid_rgba,
                DepthSide::Ask => self.options.ask_rgba,
            };
            for level in levels {
                if level.size < self.options.minimum_size {
                    continue;
                }
                let Ok(tick) = price_tick(tick_size, level.price) else {
                    continue;
                };
                let Some(row) = tick
                    .checked_sub(first_tick)
                    .and_then(|row| usize::try_from(row).ok())
                    .filter(|&row| row < rows)
                else {
                    continue;
                };
                let intensity = ((level.size - self.options.minimum_size)
                    / (self.options.maximum_size - self.options.minimum_size))
                    .clamp(0.0, 1.0);
                let offset = ((rows - 1 - row) * stride + column) * 4;
                pixels[offset..offset + 3].copy_from_slice(&color[..3]);
                pixels[offset + 3] = (f64::from(color[3]) * intensity).round() as u8;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DepthOptions {
    pub tick_size: f64,
    pub max_levels_per_side: usize,
    pub history_bucket_micros: i64,
    pub max_history_buckets: usize,
    pub max_history_cells: usize,
    pub max_event_markers: usize,
}

impl Default for DepthOptions {
    fn default() -> Self {
        Self {
            tick_size: 0.25,
            max_levels_per_side: 4_096,
            history_bucket_micros: 100_000,
            max_history_buckets: 3_600,
            max_history_cells: 500_000,
            max_event_markers: 4_096,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthError {
    InvalidStreamKey,
    StreamCapacity,
    UnknownStream(u64),
    InvalidOptions,
    InvalidTimestamp { index: usize },
    InvalidPrice { index: usize },
    OffGridPrice { index: usize },
    InvalidSize { index: usize },
    InvalidOrderCount { index: usize },
    DuplicateLevel { index: usize },
    CrossedSnapshot,
    LevelCapacity { side: DepthSide },
    BatchCapacity,
    StaleSequence,
    SequenceGap(DepthResyncRequest),
    ResyncRequired(DepthResyncRequest),
}

impl core::fmt::Display for DepthError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidStreamKey => write!(f, "depth stream key is empty or too long"),
            Self::StreamCapacity => write!(f, "depth stream capacity is exhausted"),
            Self::UnknownStream(id) => write!(f, "unknown depth stream {id}"),
            Self::InvalidOptions => write!(f, "depth options are invalid"),
            Self::InvalidTimestamp { index } => {
                write!(f, "depth row {index} has an invalid timestamp")
            }
            Self::InvalidPrice { index } => write!(f, "depth row {index} has an invalid price"),
            Self::OffGridPrice { index } => {
                write!(f, "depth row {index} price is not aligned to tick_size")
            }
            Self::InvalidSize { index } => write!(f, "depth row {index} has an invalid size"),
            Self::InvalidOrderCount { index } => {
                write!(f, "depth row {index} has an invalid order count")
            }
            Self::DuplicateLevel { index } => {
                write!(f, "depth row {index} duplicates a price level")
            }
            Self::CrossedSnapshot => write!(f, "depth snapshot is crossed or locked"),
            Self::LevelCapacity { side } => write!(f, "{side:?} depth level capacity is exhausted"),
            Self::BatchCapacity => write!(f, "depth update batch exceeds the bounded row limit"),
            Self::StaleSequence => write!(f, "depth sequence is stale or not increasing"),
            Self::SequenceGap(request) => write!(
                f,
                "depth sequence gap: expected previous {}, received previous {} for sequence {}",
                request.expected_previous_sequence,
                request.received_previous_sequence,
                request.received_sequence
            ),
            Self::ResyncRequired(request) => write!(
                f,
                "depth snapshot resync is required after sequence {}",
                request.expected_previous_sequence
            ),
        }
    }
}

impl crate::ChartEngine {
    /// Return the existing stream for `key`, or create its one canonical bounded book.
    pub fn add_depth_stream(
        &mut self,
        key: &str,
        options: DepthOptions,
    ) -> Result<u64, DepthError> {
        if key.is_empty() || key.len() > MAX_DEPTH_STREAM_KEY_BYTES {
            return Err(DepthError::InvalidStreamKey);
        }
        if let Some(&id) = self.depth_stream_keys.get(key) {
            let book = self
                .depth_streams
                .get(&id)
                .ok_or(DepthError::UnknownStream(id))?;
            if book.options() != options {
                return Err(DepthError::InvalidOptions);
            }
            return Ok(id);
        }
        if self.depth_streams.len() >= MAX_DEPTH_STREAMS {
            return Err(DepthError::StreamCapacity);
        }
        let book = DepthBook::new(options)?;
        let id = self.next_depth_stream_id;
        self.next_depth_stream_id = self.next_depth_stream_id.saturating_add(1).max(1);
        self.depth_streams.insert(id, book);
        self.depth_stream_keys.insert(key.to_owned(), id);
        Ok(id)
    }

    pub fn depth_book(&self, stream_id: u64) -> Option<&DepthBook> {
        self.depth_streams.get(&stream_id)
    }

    pub fn depth_stream_id(&self, key: &str) -> Option<u64> {
        self.depth_stream_keys.get(key).copied()
    }

    pub fn remove_depth_stream(&mut self, stream_id: u64) -> bool {
        if self.depth_streams.remove(&stream_id).is_none() {
            return false;
        }
        self.depth_stream_keys.retain(|_, id| *id != stream_id);
        self.depth_heatmaps
            .retain(|_, heatmap| heatmap.stream_id != stream_id);
        self.depth_event_layers
            .retain(|_, layer| layer.stream_id != stream_id);
        self.invalidate_frame_scene();
        true
    }

    pub fn set_depth_snapshot(
        &mut self,
        stream_id: u64,
        snapshot: DepthSnapshot,
    ) -> Result<(), DepthError> {
        self.depth_streams
            .get_mut(&stream_id)
            .ok_or(DepthError::UnknownStream(stream_id))?
            .set_snapshot(snapshot)?;
        self.refresh_depth_heatmaps(stream_id)?;
        self.invalidate_frame_scene();
        Ok(())
    }

    pub fn update_depth(&mut self, stream_id: u64, update: DepthUpdate) -> Result<(), DepthError> {
        self.depth_streams
            .get_mut(&stream_id)
            .ok_or(DepthError::UnknownStream(stream_id))?
            .apply_update(update)?;
        self.refresh_depth_heatmaps(stream_id)?;
        self.invalidate_frame_scene();
        Ok(())
    }

    pub fn update_depth_batch(
        &mut self,
        stream_id: u64,
        updates: &[DepthUpdate],
    ) -> Result<usize, DepthError> {
        let accepted = self
            .depth_streams
            .get_mut(&stream_id)
            .ok_or(DepthError::UnknownStream(stream_id))?
            .apply_updates(updates)?;
        if accepted != 0 {
            self.refresh_depth_heatmaps(stream_id)?;
            self.invalidate_frame_scene();
        }
        Ok(accepted)
    }

    pub fn set_depth_microstructure_events(
        &mut self,
        stream_id: u64,
        events: Vec<DepthMicrostructureEvent>,
    ) -> Result<(), DepthError> {
        self.depth_streams
            .get_mut(&stream_id)
            .ok_or(DepthError::UnknownStream(stream_id))?
            .set_microstructure_events(events)?;
        self.invalidate_frame_scene();
        Ok(())
    }

    pub fn push_depth_microstructure_event(
        &mut self,
        stream_id: u64,
        event: DepthMicrostructureEvent,
    ) -> Result<(), DepthError> {
        self.depth_streams
            .get_mut(&stream_id)
            .ok_or(DepthError::UnknownStream(stream_id))?
            .push_microstructure_event(event)?;
        self.invalidate_frame_scene();
        Ok(())
    }

    pub fn add_depth_heatmap(
        &mut self,
        stream_id: u64,
        options: DepthHeatmapOptions,
    ) -> Result<u64, DepthError> {
        if !self.depth_streams.contains_key(&stream_id)
            || options.pane_index >= self.panes.len()
            || !options.price_min.is_finite()
            || !options.price_max.is_finite()
            || options.price_min >= options.price_max
            || !options.minimum_size.is_finite()
            || !options.maximum_size.is_finite()
            || options.minimum_size < 0.0
            || options.maximum_size <= options.minimum_size
            || !options.opacity.is_finite()
            || !(0.0..=1.0).contains(&options.opacity)
            || self.depth_heatmaps.len() >= MAX_DEPTH_HEATMAPS
        {
            return Err(DepthError::InvalidOptions);
        }
        let id = self.next_depth_heatmap_id;
        self.next_depth_heatmap_id = self.next_depth_heatmap_id.saturating_add(1).max(1);
        self.depth_heatmaps
            .insert(id, DepthHeatmap::new(id, stream_id, options));
        self.refresh_depth_heatmap(id)?;
        self.invalidate_frame_scene();
        Ok(id)
    }

    pub fn remove_depth_heatmap(&mut self, id: u64) -> bool {
        let removed = self.depth_heatmaps.remove(&id).is_some();
        if removed {
            self.invalidate_frame_scene();
        }
        removed
    }

    pub fn add_depth_event_layer(
        &mut self,
        stream_id: u64,
        options: DepthEventLayerOptions,
    ) -> Result<u64, DepthError> {
        if !self.depth_streams.contains_key(&stream_id)
            || options.pane_index >= self.panes.len()
            || options.max_markers == 0
            || options.max_markers > MAX_DEPTH_EVENT_MARKERS
            || self.depth_event_layers.len() >= MAX_DEPTH_EVENT_LAYERS
        {
            return Err(DepthError::InvalidOptions);
        }
        let id = self.next_depth_event_layer_id;
        self.next_depth_event_layer_id = self.next_depth_event_layer_id.saturating_add(1).max(1);
        self.depth_event_layers
            .insert(id, DepthEventLayer { stream_id, options });
        self.invalidate_frame_scene();
        Ok(id)
    }

    pub fn remove_depth_event_layer(&mut self, id: u64) -> bool {
        let removed = self.depth_event_layers.remove(&id).is_some();
        if removed {
            self.invalidate_frame_scene();
        }
        removed
    }

    fn refresh_depth_heatmaps(&mut self, stream_id: u64) -> Result<(), DepthError> {
        let ids = self
            .depth_heatmaps
            .iter()
            .filter_map(|(&id, heatmap)| (heatmap.stream_id == stream_id).then_some(id))
            .collect::<Vec<_>>();
        for id in ids {
            self.refresh_depth_heatmap(id)?;
        }
        Ok(())
    }

    pub(crate) fn refresh_all_depth_heatmaps(&mut self) -> Result<(), DepthError> {
        let ids = self.depth_heatmaps.keys().copied().collect::<Vec<_>>();
        for id in ids {
            self.refresh_depth_heatmap(id)?;
        }
        Ok(())
    }

    fn refresh_depth_heatmap(&mut self, id: u64) -> Result<(), DepthError> {
        let mut heatmap = self
            .depth_heatmaps
            .remove(&id)
            .ok_or(DepthError::InvalidOptions)?;
        let result = (|| {
            let book = self
                .depth_streams
                .get(&heatmap.stream_id)
                .ok_or(DepthError::UnknownStream(heatmap.stream_id))?;
            let cutoff = book.replay_clock_micros;
            let desired = book.history.iter().filter(|bucket| {
                cutoff.is_none_or(|clock| {
                    bucket.start_micros + book.options.history_bucket_micros <= clock
                })
            });
            let signature = {
                let mut count = 0usize;
                let mut first = 0i64;
                let mut last = 0i64;
                for bucket in desired.clone() {
                    if count == 0 {
                        first = bucket.start_micros;
                    }
                    last = bucket.start_micros;
                    count += 1;
                }
                (count != 0).then_some((count, first, last))
            };
            if heatmap.history_signature != signature {
                let interval = book.options.history_bucket_micros;
                let chunk_span = interval.saturating_mul(DEPTH_HEATMAP_CHUNK_COLUMNS as i64);
                let mut groups = BTreeMap::<i64, Vec<&DepthBucket>>::new();
                for bucket in desired {
                    let chunk_start = bucket.start_micros.div_euclid(chunk_span) * chunk_span;
                    groups.entry(chunk_start).or_default().push(bucket);
                }
                let previous = core::mem::take(&mut heatmap.chunks)
                    .into_iter()
                    .map(|chunk| (chunk.start_micros, chunk))
                    .collect::<BTreeMap<_, _>>();
                for (chunk_start, buckets) in groups {
                    let bucket_starts = buckets
                        .iter()
                        .map(|bucket| bucket.start_micros)
                        .collect::<Vec<_>>();
                    let chunk = previous
                        .get(&chunk_start)
                        .filter(|chunk| chunk.bucket_starts == bucket_starts)
                        .cloned()
                        .map_or_else(|| heatmap.chunk(book, chunk_start, &buckets), Ok)?;
                    heatmap.chunks.push_back(chunk);
                }
                heatmap.history_signature = signature;
            }
            let view = book.view();
            heatmap.active = view
                .active_bucket_start
                .map(|start_micros| {
                    let bucket = DepthBucket {
                        start_micros,
                        bids: view.canonical_levels(DepthSide::Bid),
                        asks: view.canonical_levels(DepthSide::Ask),
                    };
                    heatmap.column(book, &bucket)
                })
                .transpose()?;
            Ok(())
        })();
        self.depth_heatmaps.insert(id, heatmap);
        result
    }

    pub(crate) fn depth_capacity_bytes(&self) -> usize {
        let streams = self
            .depth_streams
            .values()
            .map(DepthBook::capacity_bytes)
            .sum::<usize>();
        let keys = self
            .depth_stream_keys
            .keys()
            .map(String::capacity)
            .sum::<usize>();
        let heatmaps = self
            .depth_heatmaps
            .values()
            .map(|heatmap| {
                heatmap.chunks.capacity() * core::mem::size_of::<DepthHeatmapChunk>()
                    + heatmap
                        .chunks
                        .iter()
                        .map(|chunk| {
                            chunk.bucket_starts.capacity() * core::mem::size_of::<i64>()
                                + chunk.image.pixels.len()
                        })
                        .sum::<usize>()
                    + heatmap
                        .active
                        .as_ref()
                        .map_or(0, |column| column.image.pixels.len())
            })
            .sum::<usize>();
        streams + keys + heatmaps
    }
}

impl std::error::Error for DepthError {}

fn depth_bucket_capacity_bytes(bucket: &DepthBucket) -> usize {
    (bucket.bids.capacity() + bucket.asks.capacity()) * core::mem::size_of::<DepthLevel>()
}

fn depth_snapshot_capacity_bytes(snapshot: &DepthSnapshot) -> usize {
    (snapshot.bids.capacity() + snapshot.asks.capacity()) * core::mem::size_of::<DepthLevel>()
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StoredLevel {
    size: f64,
    order_count: Option<u32>,
}

#[derive(Clone, Debug)]
enum DepthTapeEvent {
    Snapshot(DepthSnapshot),
    Update(DepthUpdate),
}

impl DepthTapeEvent {
    fn timestamp_micros(&self) -> i64 {
        match self {
            Self::Snapshot(snapshot) => snapshot.timestamp_micros,
            Self::Update(update) => update.timestamp_micros,
        }
    }
}

#[derive(Clone, Debug)]
struct DepthReplayCheckpoint {
    event_count: usize,
    snapshot: DepthSnapshot,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct DepthReplayStats {
    pub visible_events: usize,
    pub rebuilt_events: usize,
    pub checkpoint_event_count: usize,
}

#[derive(Clone, Debug)]
pub struct DepthBook {
    options: DepthOptions,
    bids: BTreeMap<i64, StoredLevel>,
    asks: BTreeMap<i64, StoredLevel>,
    last_sequence: Option<u64>,
    last_timestamp_micros: Option<i64>,
    active_bucket_start: Option<i64>,
    history: VecDeque<DepthBucket>,
    history_cells: usize,
    events: VecDeque<DepthMicrostructureEvent>,
    tape: Vec<DepthTapeEvent>,
    replay_checkpoints: VecDeque<DepthReplayCheckpoint>,
    replay_clock_micros: Option<i64>,
    replay_projection: Option<Box<DepthBook>>,
    pending_resync: Option<DepthResyncRequest>,
}

impl DepthBook {
    pub fn new(options: DepthOptions) -> Result<Self, DepthError> {
        if !options.tick_size.is_finite()
            || options.tick_size <= 0.0
            || options.max_levels_per_side == 0
            || options.max_levels_per_side > MAX_DEPTH_LEVELS_PER_SIDE
            || options.history_bucket_micros <= 0
            || options.max_history_buckets == 0
            || options.max_history_buckets > MAX_DEPTH_HISTORY_BUCKETS
            || options.max_history_cells == 0
            || options.max_history_cells > MAX_DEPTH_HISTORY_CELLS
            || options.max_event_markers == 0
            || options.max_event_markers > MAX_DEPTH_EVENT_MARKERS
        {
            return Err(DepthError::InvalidOptions);
        }
        Ok(Self {
            options,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_sequence: None,
            last_timestamp_micros: None,
            active_bucket_start: None,
            history: VecDeque::with_capacity(options.max_history_buckets.min(256)),
            history_cells: 0,
            events: VecDeque::with_capacity(options.max_event_markers.min(256)),
            tape: Vec::new(),
            replay_checkpoints: VecDeque::new(),
            replay_clock_micros: None,
            replay_projection: None,
            pending_resync: None,
        })
    }

    pub fn options(&self) -> DepthOptions {
        self.options
    }

    pub fn capacity_bytes(&self) -> usize {
        let levels =
            (self.bids.len() + self.asks.len()) * core::mem::size_of::<(i64, StoredLevel)>();
        let history = self.history.capacity() * core::mem::size_of::<DepthBucket>()
            + self
                .history
                .iter()
                .map(depth_bucket_capacity_bytes)
                .sum::<usize>();
        let events = self.events.capacity() * core::mem::size_of::<DepthMicrostructureEvent>()
            + self
                .events
                .iter()
                .filter_map(|event| event.host_label.as_ref())
                .map(String::capacity)
                .sum::<usize>();
        let tape = self.tape.capacity() * core::mem::size_of::<DepthTapeEvent>()
            + self
                .tape
                .iter()
                .map(|event| match event {
                    DepthTapeEvent::Snapshot(snapshot) => depth_snapshot_capacity_bytes(snapshot),
                    DepthTapeEvent::Update(_) => 0,
                })
                .sum::<usize>();
        let checkpoints = self.replay_checkpoints.capacity()
            * core::mem::size_of::<DepthReplayCheckpoint>()
            + self
                .replay_checkpoints
                .iter()
                .map(|checkpoint| depth_snapshot_capacity_bytes(&checkpoint.snapshot))
                .sum::<usize>();
        levels
            + history
            + events
            + tape
            + checkpoints
            + self
                .replay_projection
                .as_deref()
                .map_or(0, DepthBook::capacity_bytes)
    }

    pub fn last_sequence(&self) -> Option<u64> {
        self.view().last_sequence
    }

    pub fn pending_resync(&self) -> Option<DepthResyncRequest> {
        self.pending_resync
    }

    pub fn replay_clock_micros(&self) -> Option<i64> {
        self.replay_clock_micros
    }

    pub fn set_replay_clock_micros(
        &mut self,
        clock_micros: Option<i64>,
    ) -> Result<DepthReplayStats, DepthError> {
        if let Some(clock) = clock_micros {
            validate_timestamp(clock, 0)?;
        }
        if self.replay_clock_micros == clock_micros {
            return Ok(DepthReplayStats {
                visible_events: self.visible_event_count(clock_micros),
                ..DepthReplayStats::default()
            });
        }
        self.replay_clock_micros = clock_micros;
        self.refresh_replay_projection()
    }

    pub fn history(&self) -> &VecDeque<DepthBucket> {
        &self.view().history
    }

    pub fn set_snapshot(&mut self, snapshot: DepthSnapshot) -> Result<(), DepthError> {
        self.set_snapshot_inner(snapshot, true)
    }

    fn set_snapshot_inner(
        &mut self,
        snapshot: DepthSnapshot,
        record: bool,
    ) -> Result<(), DepthError> {
        validate_timestamp(snapshot.timestamp_micros, 0)?;
        if self
            .last_sequence
            .is_some_and(|sequence| snapshot.sequence <= sequence)
            && self.pending_resync.is_none()
        {
            return Err(DepthError::StaleSequence);
        }
        let bids = self.validate_levels(DepthSide::Bid, &snapshot.bids)?;
        let asks = self.validate_levels(DepthSide::Ask, &snapshot.asks)?;
        if bids
            .keys()
            .next_back()
            .zip(asks.keys().next())
            .is_some_and(|(best_bid, best_ask)| best_bid >= best_ask)
        {
            return Err(DepthError::CrossedSnapshot);
        }
        self.roll_history(snapshot.timestamp_micros);
        let recorded = record.then(|| snapshot.clone());
        self.bids = bids;
        self.asks = asks;
        self.last_sequence = Some(snapshot.sequence);
        self.last_timestamp_micros = Some(snapshot.timestamp_micros);
        self.active_bucket_start = Some(self.bucket_start(snapshot.timestamp_micros));
        self.pending_resync = None;
        if let Some(snapshot) = recorded {
            self.tape.clear();
            self.tape.push(DepthTapeEvent::Snapshot(snapshot));
            self.replay_checkpoints.clear();
            self.refresh_replay_projection()?;
        }
        Ok(())
    }

    pub fn apply_update(&mut self, update: DepthUpdate) -> Result<(), DepthError> {
        self.apply_update_inner(update, true, true)
    }

    fn apply_update_inner(
        &mut self,
        update: DepthUpdate,
        record: bool,
        refresh_replay: bool,
    ) -> Result<(), DepthError> {
        if let Some(request) = self.pending_resync {
            return Err(DepthError::ResyncRequired(request));
        }
        validate_timestamp(update.timestamp_micros, 0)?;
        if self
            .last_timestamp_micros
            .is_some_and(|timestamp| update.timestamp_micros < timestamp)
            || update.sequence <= update.previous_sequence
        {
            return Err(DepthError::StaleSequence);
        }
        let expected = self.last_sequence.ok_or(DepthError::StaleSequence)?;
        if update.previous_sequence != expected {
            let request = DepthResyncRequest {
                expected_previous_sequence: expected,
                received_previous_sequence: update.previous_sequence,
                received_sequence: update.sequence,
            };
            self.pending_resync = Some(request);
            return Err(DepthError::SequenceGap(request));
        }
        let tick = validate_level(self.options.tick_size, update.level, 0, true)?;
        if record && self.tape.len() > MAX_DEPTH_REPLAY_UPDATES {
            let snapshot = self.canonical_snapshot();
            self.tape.clear();
            self.tape.push(DepthTapeEvent::Snapshot(snapshot));
            self.replay_checkpoints.clear();
        }
        self.roll_history(update.timestamp_micros);
        let levels = match update.side {
            DepthSide::Bid => &mut self.bids,
            DepthSide::Ask => &mut self.asks,
        };
        if update.level.size == 0.0 {
            levels.remove(&tick);
        } else {
            levels.insert(
                tick,
                StoredLevel {
                    size: update.level.size,
                    order_count: update.level.order_count,
                },
            );
            while levels.len() > self.options.max_levels_per_side {
                match update.side {
                    DepthSide::Bid => {
                        levels.pop_first();
                    }
                    DepthSide::Ask => {
                        levels.pop_last();
                    }
                }
            }
        }
        self.last_sequence = Some(update.sequence);
        self.last_timestamp_micros = Some(update.timestamp_micros);
        if record {
            self.tape.push(DepthTapeEvent::Update(update));
            if self
                .tape
                .len()
                .is_multiple_of(DEPTH_REPLAY_CHECKPOINT_INTERVAL)
            {
                self.replay_checkpoints.push_back(DepthReplayCheckpoint {
                    event_count: self.tape.len(),
                    snapshot: self.canonical_snapshot(),
                });
                while self.replay_checkpoints.len() > MAX_DEPTH_REPLAY_CHECKPOINTS {
                    self.replay_checkpoints.pop_front();
                }
            }
            if refresh_replay
                && self
                    .replay_clock_micros
                    .is_some_and(|clock| update.timestamp_micros <= clock)
            {
                self.refresh_replay_projection()?;
            }
        }
        Ok(())
    }

    pub fn apply_updates(&mut self, updates: &[DepthUpdate]) -> Result<usize, DepthError> {
        if updates.len() > MAX_DEPTH_BATCH_UPDATES {
            return Err(DepthError::BatchCapacity);
        }
        // Validate against a bounded copy of the live book, not the potentially much larger
        // history ring. History rolling cannot fail once sequence and level validation succeeds.
        let mut candidate = Self {
            options: self.options,
            bids: self.bids.clone(),
            asks: self.asks.clone(),
            last_sequence: self.last_sequence,
            last_timestamp_micros: self.last_timestamp_micros,
            active_bucket_start: self.active_bucket_start,
            history: VecDeque::new(),
            history_cells: 0,
            events: VecDeque::new(),
            tape: Vec::new(),
            replay_checkpoints: VecDeque::new(),
            replay_clock_micros: None,
            replay_projection: None,
            pending_resync: self.pending_resync,
        };
        for update in updates {
            candidate.apply_update_inner(*update, false, false)?;
        }
        for update in updates {
            self.apply_update_inner(*update, true, false)?;
        }
        if updates.last().is_some_and(|update| {
            self.replay_clock_micros
                .is_some_and(|clock| update.timestamp_micros <= clock)
        }) {
            self.refresh_replay_projection()?;
        }
        Ok(updates.len())
    }

    pub fn best_bid(&self) -> Option<DepthLevel> {
        self.view()
            .bids
            .last_key_value()
            .map(|(&tick, &level)| self.public_level(tick, level))
    }

    pub fn best_ask(&self) -> Option<DepthLevel> {
        self.view()
            .asks
            .first_key_value()
            .map(|(&tick, &level)| self.public_level(tick, level))
    }

    pub fn size_at_price(&self, side: DepthSide, price: f64) -> Option<f64> {
        let tick = price_tick(self.options.tick_size, price).ok()?;
        let view = self.view();
        match side {
            DepthSide::Bid => view.bids.get(&tick),
            DepthSide::Ask => view.asks.get(&tick),
        }
        .map(|level| level.size)
    }

    pub fn cumulative_depth(&self, side: DepthSide, levels: usize) -> f64 {
        let view = self.view();
        match side {
            DepthSide::Bid => view
                .bids
                .values()
                .rev()
                .take(levels)
                .map(|level| level.size)
                .sum(),
            DepthSide::Ask => view
                .asks
                .values()
                .take(levels)
                .map(|level| level.size)
                .sum(),
        }
    }

    pub fn imbalance(&self, levels: usize) -> Option<f64> {
        let bid = self.cumulative_depth(DepthSide::Bid, levels);
        let ask = self.cumulative_depth(DepthSide::Ask, levels);
        let total = bid + ask;
        (total > 0.0).then_some((bid - ask) / total)
    }

    pub fn levels(&self, side: DepthSide) -> Vec<DepthLevel> {
        let view = self.view();
        let iter: Box<dyn Iterator<Item = (&i64, &StoredLevel)> + '_> = match side {
            DepthSide::Bid => Box::new(view.bids.iter().rev()),
            DepthSide::Ask => Box::new(view.asks.iter()),
        };
        iter.map(|(&tick, &level)| self.public_level(tick, level))
            .collect()
    }

    pub fn set_microstructure_events(
        &mut self,
        mut events: Vec<DepthMicrostructureEvent>,
    ) -> Result<(), DepthError> {
        if events.len() > self.options.max_event_markers {
            return Err(DepthError::BatchCapacity);
        }
        for (index, event) in events.iter().enumerate() {
            validate_event(self.options.tick_size, event, index)?;
        }
        events.sort_by_key(|event| event.timestamp_micros);
        self.events = events.into();
        Ok(())
    }

    pub fn push_microstructure_event(
        &mut self,
        event: DepthMicrostructureEvent,
    ) -> Result<(), DepthError> {
        validate_event(self.options.tick_size, &event, 0)?;
        if self
            .events
            .back()
            .is_some_and(|last| event.timestamp_micros < last.timestamp_micros)
        {
            return Err(DepthError::StaleSequence);
        }
        self.events.push_back(event);
        while self.events.len() > self.options.max_event_markers {
            self.events.pop_front();
        }
        Ok(())
    }

    pub fn microstructure_events_lod(
        &self,
        from_micros: i64,
        to_micros: i64,
        max_markers: usize,
    ) -> Vec<DepthEventCluster> {
        if max_markers == 0 || from_micros > to_micros {
            return Vec::new();
        }
        let visible = self
            .events
            .iter()
            .filter(|event| (from_micros..=to_micros).contains(&event.timestamp_micros))
            .cloned()
            .collect::<Vec<_>>();
        let chunk = visible.len().div_ceil(max_markers).max(1);
        visible
            .chunks(chunk)
            .map(|events| {
                let first = &events[0];
                let size = events.iter().map(|event| event.size).sum::<f64>();
                let price = events
                    .iter()
                    .map(|event| event.price * event.size)
                    .sum::<f64>()
                    / size;
                let kind = if events.iter().all(|event| event.kind == first.kind) {
                    first.kind
                } else {
                    DepthEventKind::Mixed
                };
                DepthEventCluster {
                    timestamp_micros: events
                        .last()
                        .map_or(first.timestamp_micros, |event| event.timestamp_micros),
                    price,
                    size,
                    side: events
                        .iter()
                        .all(|event| event.side == first.side)
                        .then_some(first.side)
                        .flatten(),
                    kind,
                    event_count: events.len().min(u32::MAX as usize) as u32,
                    host_label: events
                        .iter()
                        .all(|event| event.host_label == first.host_label)
                        .then(|| first.host_label.clone())
                        .flatten(),
                }
            })
            .collect()
    }

    /// A host-neutral DOM ladder view centered on the touch. Filtering happens while traversing
    /// the bounded book; it never creates a second mutable book model.
    pub fn ladder(
        &self,
        levels_per_side: usize,
        minimum_size: f64,
        max_distance_ticks: Option<u32>,
    ) -> Vec<DepthLadderRow> {
        if levels_per_side == 0 || !minimum_size.is_finite() || minimum_size < 0.0 {
            return Vec::new();
        }
        let view = self.view();
        let best_bid = view.bids.last_key_value().map(|(&tick, _)| tick);
        let best_ask = view.asks.first_key_value().map(|(&tick, _)| tick);
        let mut rows = BTreeMap::<i64, DepthLadderRow>::new();
        let mut bids = 0usize;
        for (&tick, level) in view.bids.iter().rev() {
            let Some(touch) = best_bid else { break };
            let distance = touch.saturating_sub(tick).max(0) as u64;
            if max_distance_ticks.is_some_and(|max| distance > u64::from(max)) {
                break;
            }
            if level.size < minimum_size {
                continue;
            }
            rows.insert(
                tick,
                DepthLadderRow {
                    price: tick as f64 * self.options.tick_size,
                    bid_size: Some(level.size),
                    ask_size: None,
                    bid_order_count: level.order_count,
                    ask_order_count: None,
                    distance_from_touch_ticks: distance.min(u64::from(u32::MAX)) as u32,
                },
            );
            bids += 1;
            if bids >= levels_per_side {
                break;
            }
        }
        let mut asks = 0usize;
        for (&tick, level) in &view.asks {
            let Some(touch) = best_ask else { break };
            let distance = tick.saturating_sub(touch).max(0) as u64;
            if max_distance_ticks.is_some_and(|max| distance > u64::from(max)) {
                break;
            }
            if level.size < minimum_size {
                continue;
            }
            rows.entry(tick)
                .and_modify(|row| {
                    row.ask_size = Some(level.size);
                    row.ask_order_count = level.order_count;
                    row.distance_from_touch_ticks = row
                        .distance_from_touch_ticks
                        .min(distance.min(u64::from(u32::MAX)) as u32);
                })
                .or_insert(DepthLadderRow {
                    price: tick as f64 * self.options.tick_size,
                    bid_size: None,
                    ask_size: Some(level.size),
                    bid_order_count: None,
                    ask_order_count: level.order_count,
                    distance_from_touch_ticks: distance.min(u64::from(u32::MAX)) as u32,
                });
            asks += 1;
            if asks >= levels_per_side {
                break;
            }
        }
        rows.into_values().rev().collect()
    }

    /// Book-imbalance and cumulative curves derived directly from the canonical live levels.
    pub fn study_snapshot(
        &self,
        levels_per_side: usize,
        minimum_size: f64,
        max_distance_ticks: Option<u32>,
    ) -> DepthStudySnapshot {
        let rows = self.ladder(levels_per_side, minimum_size, max_distance_ticks);
        let mut bid_total = 0.0;
        let mut ask_total = 0.0;
        let mut bid_cumulative = Vec::new();
        let mut ask_cumulative = Vec::new();
        for row in &rows {
            if let Some(size) = row.bid_size {
                bid_total += size;
                bid_cumulative.push(DepthLevel {
                    price: row.price,
                    size: bid_total,
                    order_count: row.bid_order_count,
                });
            }
        }
        for row in rows.iter().rev() {
            if let Some(size) = row.ask_size {
                ask_total += size;
                ask_cumulative.push(DepthLevel {
                    price: row.price,
                    size: ask_total,
                    order_count: row.ask_order_count,
                });
            }
        }
        let total = bid_total + ask_total;
        DepthStudySnapshot {
            sequence: self.view().last_sequence,
            best_bid: self.best_bid(),
            best_ask: self.best_ask(),
            bid_cumulative,
            ask_cumulative,
            imbalance: (total > 0.0).then_some((bid_total - ask_total) / total),
        }
    }

    fn validate_levels(
        &self,
        side: DepthSide,
        input: &[DepthLevel],
    ) -> Result<BTreeMap<i64, StoredLevel>, DepthError> {
        if input.len() > MAX_DEPTH_LEVELS_PER_SIDE {
            return Err(DepthError::LevelCapacity { side });
        }
        let mut output = BTreeMap::new();
        for (index, &level) in input.iter().enumerate() {
            let tick = validate_level(self.options.tick_size, level, index, false)?;
            if output
                .insert(
                    tick,
                    StoredLevel {
                        size: level.size,
                        order_count: level.order_count,
                    },
                )
                .is_some()
            {
                return Err(DepthError::DuplicateLevel { index });
            }
        }
        while output.len() > self.options.max_levels_per_side {
            match side {
                DepthSide::Bid => {
                    output.pop_first();
                }
                DepthSide::Ask => {
                    output.pop_last();
                }
            }
        }
        Ok(output)
    }

    fn bucket_start(&self, timestamp_micros: i64) -> i64 {
        timestamp_micros.div_euclid(self.options.history_bucket_micros)
            * self.options.history_bucket_micros
    }

    fn roll_history(&mut self, timestamp_micros: i64) {
        let next = self.bucket_start(timestamp_micros);
        let Some(active) = self.active_bucket_start else {
            self.active_bucket_start = Some(next);
            return;
        };
        if next <= active {
            return;
        }
        let bucket = DepthBucket {
            start_micros: active,
            bids: self.canonical_levels(DepthSide::Bid),
            asks: self.canonical_levels(DepthSide::Ask),
        };
        self.history_cells = self
            .history_cells
            .saturating_add(bucket.bids.len().saturating_add(bucket.asks.len()));
        self.history.push_back(bucket);
        while self.history.len() > self.options.max_history_buckets
            || self.history_cells > self.options.max_history_cells
        {
            let Some(evicted) = self.history.pop_front() else {
                break;
            };
            self.history_cells = self
                .history_cells
                .saturating_sub(evicted.bids.len().saturating_add(evicted.asks.len()));
        }
        self.active_bucket_start = Some(next);
    }

    fn public_level(&self, tick: i64, level: StoredLevel) -> DepthLevel {
        DepthLevel {
            price: tick as f64 * self.options.tick_size,
            size: level.size,
            order_count: level.order_count,
        }
    }

    fn view(&self) -> &Self {
        self.replay_projection.as_deref().unwrap_or(self)
    }

    fn canonical_levels(&self, side: DepthSide) -> Vec<DepthLevel> {
        let iter: Box<dyn Iterator<Item = (&i64, &StoredLevel)> + '_> = match side {
            DepthSide::Bid => Box::new(self.bids.iter().rev()),
            DepthSide::Ask => Box::new(self.asks.iter()),
        };
        iter.map(|(&tick, &level)| self.public_level(tick, level))
            .collect()
    }

    fn canonical_snapshot(&self) -> DepthSnapshot {
        DepthSnapshot {
            timestamp_micros: self.last_timestamp_micros.unwrap_or_default(),
            sequence: self.last_sequence.unwrap_or_default(),
            bids: self.canonical_levels(DepthSide::Bid),
            asks: self.canonical_levels(DepthSide::Ask),
        }
    }

    fn visible_event_count(&self, clock_micros: Option<i64>) -> usize {
        clock_micros.map_or(self.tape.len(), |clock| {
            self.tape
                .partition_point(|event| event.timestamp_micros() <= clock)
        })
    }

    fn refresh_replay_projection(&mut self) -> Result<DepthReplayStats, DepthError> {
        let Some(clock) = self.replay_clock_micros else {
            self.replay_projection = None;
            return Ok(DepthReplayStats {
                visible_events: self.tape.len(),
                ..DepthReplayStats::default()
            });
        };
        let visible_events = self.visible_event_count(Some(clock));
        let mut projection = Self::new(self.options)?;
        let checkpoint = self
            .replay_checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.event_count <= visible_events);
        let start = if let Some(checkpoint) = checkpoint {
            projection.set_snapshot_inner(checkpoint.snapshot.clone(), false)?;
            checkpoint.event_count
        } else {
            0
        };
        for event in &self.tape[start..visible_events] {
            match event {
                DepthTapeEvent::Snapshot(snapshot) => {
                    projection.set_snapshot_inner(snapshot.clone(), false)?;
                }
                DepthTapeEvent::Update(update) => {
                    projection.apply_update_inner(*update, false, false)?;
                }
            }
        }
        self.replay_projection = Some(Box::new(projection));
        Ok(DepthReplayStats {
            visible_events,
            rebuilt_events: visible_events.saturating_sub(start),
            checkpoint_event_count: start,
        })
    }
}

fn validate_timestamp(timestamp_micros: i64, index: usize) -> Result<(), DepthError> {
    // Same inclusive calendar boundary used by the chart's canonical timestamp validator.
    const MIN: i64 = -62_167_219_200_000_000;
    const MAX: i64 = 253_402_300_799_999_999;
    if (MIN..=MAX).contains(&timestamp_micros) {
        Ok(())
    } else {
        Err(DepthError::InvalidTimestamp { index })
    }
}

fn validate_level(
    tick_size: f64,
    level: DepthLevel,
    index: usize,
    allow_delete: bool,
) -> Result<i64, DepthError> {
    let tick = price_tick(tick_size, level.price).map_err(|error| match error {
        PriceError::Invalid => DepthError::InvalidPrice { index },
        PriceError::OffGrid => DepthError::OffGridPrice { index },
    })?;
    if !level.size.is_finite() || level.size < 0.0 || (!allow_delete && level.size == 0.0) {
        return Err(DepthError::InvalidSize { index });
    }
    if level.size == 0.0 && level.order_count.is_some_and(|count| count != 0)
        || level.size > 0.0 && level.order_count == Some(0)
    {
        return Err(DepthError::InvalidOrderCount { index });
    }
    Ok(tick)
}

fn validate_event(
    tick_size: f64,
    event: &DepthMicrostructureEvent,
    index: usize,
) -> Result<(), DepthError> {
    validate_timestamp(event.timestamp_micros, index)?;
    price_tick(tick_size, event.price).map_err(|error| match error {
        PriceError::Invalid => DepthError::InvalidPrice { index },
        PriceError::OffGrid => DepthError::OffGridPrice { index },
    })?;
    if !event.size.is_finite() || event.size <= 0.0 {
        return Err(DepthError::InvalidSize { index });
    }
    if event
        .host_label
        .as_ref()
        .is_some_and(|label| label.len() > MAX_DEPTH_EVENT_LABEL_BYTES)
    {
        return Err(DepthError::InvalidOptions);
    }
    Ok(())
}

enum PriceError {
    Invalid,
    OffGrid,
}

fn price_tick(tick_size: f64, price: f64) -> Result<i64, PriceError> {
    if !price.is_finite() {
        return Err(PriceError::Invalid);
    }
    let scaled = price / tick_size;
    if !scaled.is_finite() || scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err(PriceError::Invalid);
    }
    let rounded = scaled.round();
    if (scaled - rounded).abs() > 1e-9_f64.max(scaled.abs() * 1e-12) {
        return Err(PriceError::OffGrid);
    }
    Ok(rounded as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(price: f64, size: f64, order_count: Option<u32>) -> DepthLevel {
        DepthLevel {
            price,
            size,
            order_count,
        }
    }

    fn snapshot(sequence: u64) -> DepthSnapshot {
        DepthSnapshot {
            timestamp_micros: 1_000_000,
            sequence,
            bids: vec![level(99.75, 10.0, Some(2)), level(99.5, 20.0, None)],
            asks: vec![level(100.0, 8.0, Some(1)), level(100.25, 12.0, None)],
        }
    }

    #[test]
    fn snapshot_queries_preserve_fixed_tick_identity_and_optional_counts() {
        let mut book = DepthBook::new(DepthOptions::default()).unwrap();
        book.set_snapshot(snapshot(10)).unwrap();
        assert_eq!(book.best_bid(), Some(level(99.75, 10.0, Some(2))));
        assert_eq!(book.best_ask(), Some(level(100.0, 8.0, Some(1))));
        assert_eq!(book.size_at_price(DepthSide::Bid, 99.5), Some(20.0));
        assert_eq!(book.cumulative_depth(DepthSide::Bid, 2), 30.0);
        assert_eq!(book.imbalance(1), Some(2.0 / 18.0));
    }

    #[test]
    fn gaps_are_typed_and_fence_mutation_until_a_new_snapshot() {
        let mut book = DepthBook::new(DepthOptions::default()).unwrap();
        book.set_snapshot(snapshot(10)).unwrap();
        let gap = DepthUpdate {
            timestamp_micros: 1_100_000,
            sequence: 13,
            previous_sequence: 12,
            side: DepthSide::Bid,
            level: level(99.75, 50.0, Some(5)),
        };
        let request = DepthResyncRequest {
            expected_previous_sequence: 10,
            received_previous_sequence: 12,
            received_sequence: 13,
        };
        assert_eq!(
            book.apply_update(gap),
            Err(DepthError::SequenceGap(request))
        );
        assert_eq!(book.best_bid().unwrap().size, 10.0);
        assert_eq!(
            book.apply_update(gap),
            Err(DepthError::ResyncRequired(request))
        );
        book.set_snapshot(snapshot(20)).unwrap();
        assert_eq!(book.pending_resync(), None);
    }

    #[test]
    fn batches_are_atomic_and_history_is_bounded_by_time_bucket() {
        let options = DepthOptions {
            max_history_buckets: 2,
            ..DepthOptions::default()
        };
        let mut book = DepthBook::new(options).unwrap();
        book.set_snapshot(snapshot(1)).unwrap();
        let updates = [
            DepthUpdate {
                timestamp_micros: 1_100_000,
                sequence: 2,
                previous_sequence: 1,
                side: DepthSide::Bid,
                level: level(99.75, 11.0, Some(2)),
            },
            DepthUpdate {
                timestamp_micros: 1_200_000,
                sequence: 3,
                previous_sequence: 2,
                side: DepthSide::Ask,
                level: level(100.0, 0.0, Some(4)),
            },
        ];
        assert!(book.apply_updates(&updates).is_err());
        assert_eq!(book.last_sequence(), Some(1));
        for sequence in 2..=5 {
            book.apply_update(DepthUpdate {
                timestamp_micros: sequence as i64 * 100_000 + 1_000_000,
                sequence,
                previous_sequence: sequence - 1,
                side: DepthSide::Bid,
                level: level(99.75, sequence as f64, Some(1)),
            })
            .unwrap();
        }
        assert_eq!(book.history().len(), 2);
    }

    #[test]
    fn malformed_or_crossed_snapshots_are_atomic() {
        let mut book = DepthBook::new(DepthOptions::default()).unwrap();
        book.set_snapshot(snapshot(10)).unwrap();
        let before = book.best_bid();
        let mut crossed = snapshot(11);
        crossed.bids[0].price = 100.0;
        assert_eq!(book.set_snapshot(crossed), Err(DepthError::CrossedSnapshot));
        assert_eq!(book.best_bid(), before);
        let mut off_grid = snapshot(11);
        off_grid.asks[0].price = 100.1;
        assert_eq!(
            book.set_snapshot(off_grid),
            Err(DepthError::OffGridPrice { index: 0 })
        );
        assert_eq!(book.best_bid(), before);
    }

    #[test]
    fn chart_owns_one_book_per_key_and_rejects_option_drift() {
        let mut chart = crate::ChartEngine::new(600.0, 400.0, 1.0);
        let id = chart
            .add_depth_stream("CME:CL", DepthOptions::default())
            .unwrap();
        assert_eq!(chart.depth_stream_id("CME:CL"), Some(id));
        assert_eq!(
            chart.add_depth_stream("CME:CL", DepthOptions::default()),
            Ok(id)
        );
        let changed = DepthOptions {
            tick_size: 0.01,
            ..DepthOptions::default()
        };
        assert_eq!(
            chart.add_depth_stream("CME:CL", changed),
            Err(DepthError::InvalidOptions)
        );
        let mut negative = snapshot(1);
        negative.bids = vec![level(-1.25, 10.0, None)];
        negative.asks = vec![level(-1.0, 12.0, None)];
        chart.set_depth_snapshot(id, negative).unwrap();
        assert_eq!(
            chart.depth_book(id).unwrap().best_bid().unwrap().price,
            -1.25
        );
        assert!(chart.remove_depth_stream(id));
        assert_eq!(chart.depth_stream_id("CME:CL"), None);
    }

    #[test]
    fn level_cap_retains_nearest_prices_without_breaking_sequence_continuity() {
        let mut book = DepthBook::new(DepthOptions {
            max_levels_per_side: 2,
            ..DepthOptions::default()
        })
        .unwrap();
        book.set_snapshot(DepthSnapshot {
            timestamp_micros: 1_000_000,
            sequence: 1,
            bids: vec![
                level(99.0, 1.0, None),
                level(99.5, 1.0, None),
                level(99.75, 1.0, None),
            ],
            asks: vec![
                level(100.0, 1.0, None),
                level(100.25, 1.0, None),
                level(101.0, 1.0, None),
            ],
        })
        .unwrap();
        assert_eq!(
            book.levels(DepthSide::Bid)
                .iter()
                .map(|level| level.price)
                .collect::<Vec<_>>(),
            vec![99.75, 99.5]
        );
        assert_eq!(
            book.levels(DepthSide::Ask)
                .iter()
                .map(|level| level.price)
                .collect::<Vec<_>>(),
            vec![100.0, 100.25]
        );
        book.apply_update(DepthUpdate {
            timestamp_micros: 1_100_000,
            sequence: 2,
            previous_sequence: 1,
            side: DepthSide::Bid,
            level: level(100.0, 2.0, None),
        })
        .unwrap();
        assert_eq!(book.last_sequence(), Some(2));
        assert_eq!(book.best_bid().unwrap().price, 100.0);
        assert_eq!(book.levels(DepthSide::Bid).len(), 2);
    }

    #[test]
    fn ladder_and_studies_filter_without_duplicating_book_state() {
        let mut book = DepthBook::new(DepthOptions::default()).unwrap();
        book.set_snapshot(snapshot(10)).unwrap();
        let ladder = book.ladder(2, 9.0, Some(1));
        assert_eq!(
            ladder.iter().map(|row| row.price).collect::<Vec<_>>(),
            vec![100.25, 99.75, 99.5]
        );
        let study = book.study_snapshot(2, 0.0, None);
        assert_eq!(
            study
                .bid_cumulative
                .iter()
                .map(|level| (level.price, level.size))
                .collect::<Vec<_>>(),
            vec![(99.75, 10.0), (99.5, 30.0)]
        );
        assert_eq!(
            study
                .ask_cumulative
                .iter()
                .map(|level| (level.price, level.size))
                .collect::<Vec<_>>(),
            vec![(100.0, 8.0), (100.25, 20.0)]
        );
        assert_eq!(study.imbalance, Some(0.2));
    }

    #[test]
    fn host_detected_microstructure_events_are_capped_and_collapse_for_lod() {
        let options = DepthOptions {
            max_event_markers: 3,
            ..DepthOptions::default()
        };
        let mut book = DepthBook::new(options).unwrap();
        for index in 0..4 {
            book.push_microstructure_event(DepthMicrostructureEvent {
                timestamp_micros: 1_000_000 + index * 1_000,
                price: 100.0 + index as f64 * 0.25,
                size: 10.0,
                side: Some(DepthSide::Ask),
                kind: if index == 3 {
                    DepthEventKind::Sweep
                } else {
                    DepthEventKind::PulledLiquidity
                },
                host_label: Some(format!("event {index}")),
            })
            .unwrap();
        }
        let collapsed = book.microstructure_events_lod(0, 2_000_000, 1);
        assert_eq!(collapsed.len(), 1);
        assert_eq!(collapsed[0].event_count, 3);
        assert_eq!(collapsed[0].kind, DepthEventKind::Mixed);
        assert_eq!(collapsed[0].host_label, None);
        assert_eq!(
            book.push_microstructure_event(DepthMicrostructureEvent {
                timestamp_micros: 2_000_001,
                price: 100.0,
                size: 1.0,
                side: None,
                kind: DepthEventKind::SizeCluster,
                host_label: Some("x".repeat(MAX_DEPTH_EVENT_LABEL_BYTES + 1)),
            }),
            Err(DepthError::InvalidOptions)
        );
    }

    #[test]
    fn replay_masks_future_depth_without_blocking_canonical_ingest() {
        let mut book = DepthBook::new(DepthOptions::default()).unwrap();
        book.set_snapshot(snapshot(1)).unwrap();
        for sequence in 2..=3 {
            book.apply_update(DepthUpdate {
                timestamp_micros: sequence as i64 * 1_000_000,
                sequence,
                previous_sequence: sequence - 1,
                side: DepthSide::Bid,
                level: level(99.75, sequence as f64 * 10.0, Some(2)),
            })
            .unwrap();
        }
        book.set_replay_clock_micros(Some(2_500_000)).unwrap();
        assert_eq!(book.last_sequence(), Some(2));
        assert_eq!(book.best_bid().unwrap().size, 20.0);
        book.apply_update(DepthUpdate {
            timestamp_micros: 4_000_000,
            sequence: 4,
            previous_sequence: 3,
            side: DepthSide::Bid,
            level: level(99.75, 40.0, Some(2)),
        })
        .unwrap();
        assert_eq!(book.last_sequence(), Some(2));
        book.set_replay_clock_micros(Some(4_000_000)).unwrap();
        assert_eq!(book.last_sequence(), Some(4));
        assert_eq!(book.best_bid().unwrap().size, 40.0);
        book.set_replay_clock_micros(None).unwrap();
        assert_eq!(book.last_sequence(), Some(4));
    }

    #[test]
    fn backward_depth_replay_restores_a_bounded_checkpoint_suffix() {
        let mut book = DepthBook::new(DepthOptions::default()).unwrap();
        book.set_snapshot(snapshot(1)).unwrap();
        for sequence in 2..=2_100 {
            book.apply_update(DepthUpdate {
                timestamp_micros: 1_000_000 + sequence as i64 * 1_000,
                sequence,
                previous_sequence: sequence - 1,
                side: DepthSide::Bid,
                level: level(99.75, sequence as f64, Some(1)),
            })
            .unwrap();
        }
        let stats = book.set_replay_clock_micros(Some(3_050_000)).unwrap();
        assert_eq!(stats.visible_events, 2_050);
        assert_eq!(stats.checkpoint_event_count, 2_048);
        assert_eq!(stats.rebuilt_events, 2);
        assert_eq!(book.last_sequence(), Some(2_050));
        assert_eq!(book.best_bid().unwrap().size, 2_050.0);
    }

    #[test]
    fn heatmap_keeps_finalized_chunks_and_replaces_only_the_live_edge_image() {
        let mut chart = crate::ChartEngine::new(600.0, 400.0, 1.0);
        chart
            .set_series_data(
                0,
                &[1.0, 2.0, 3.0],
                &[100.0; 3],
                &[101.0; 3],
                &[99.0; 3],
                &[100.0; 3],
            )
            .unwrap();
        chart.time_scale.set_width(600.0);
        chart.layout_panes(372.0);
        chart.fit_content();
        chart.set_visible_logical_range(-1.0, 3.0);
        chart.autoscale_visible();
        let stream = chart
            .add_depth_stream("heatmap", DepthOptions::default())
            .unwrap();
        chart.set_depth_snapshot(stream, snapshot(1)).unwrap();
        let heatmap = chart
            .add_depth_heatmap(
                stream,
                DepthHeatmapOptions {
                    price_min: 99.0,
                    price_max: 101.0,
                    maximum_size: 100.0,
                    ..DepthHeatmapOptions::default()
                },
            )
            .unwrap();
        chart
            .set_depth_microstructure_events(
                stream,
                vec![DepthMicrostructureEvent {
                    timestamp_micros: 1_125_000,
                    price: 100.0,
                    size: 20.0,
                    side: Some(DepthSide::Ask),
                    kind: DepthEventKind::Sweep,
                    host_label: Some("ask sweep".to_string()),
                }],
            )
            .unwrap();
        let event_layer = chart
            .add_depth_event_layer(stream, DepthEventLayerOptions::default())
            .unwrap();
        assert_eq!(chart.depth_heatmaps[&heatmap].chunks.len(), 0);
        assert!(chart.depth_heatmaps[&heatmap].active.is_some());
        chart
            .update_depth(
                stream,
                DepthUpdate {
                    timestamp_micros: 1_100_000,
                    sequence: 2,
                    previous_sequence: 1,
                    side: DepthSide::Bid,
                    level: level(99.75, 20.0, Some(2)),
                },
            )
            .unwrap();
        assert_eq!(chart.depth_heatmaps[&heatmap].chunks.len(), 1);
        assert!(chart.depth_heatmaps[&heatmap].active.is_some());
        chart.build_frame();
        assert!(chart.depth_time_coordinate(1_000_000).is_some());
        assert!(chart.series_price_to_coordinate(0, 99.0).is_some());
        let mut direct = Vec::new();
        chart.build_depth_heatmap_frame(0, 1.0, 1.0, &mut direct);
        assert_eq!(direct.len(), 2);
        let frame = chart.build_frame();
        let keys = frame.panes[0]
            .under
            .iter()
            .filter_map(|primitive| match primitive {
                aeris_charts_render::draw_list::Prim::Image { image, .. } => Some(image.key),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(keys.len(), 2);
        chart
            .update_depth(
                stream,
                DepthUpdate {
                    timestamp_micros: 1_150_000,
                    sequence: 3,
                    previous_sequence: 2,
                    side: DepthSide::Ask,
                    level: level(100.0, 30.0, Some(3)),
                },
            )
            .unwrap();
        chart.build_frame();
        let frame = chart.build_frame();
        let next_keys = frame.panes[0]
            .under
            .iter()
            .filter_map(|primitive| match primitive {
                aeris_charts_render::draw_list::Prim::Image { image, .. } => Some(image.key),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(next_keys.len(), 2);
        assert_eq!(next_keys[0], keys[0]);
        assert_ne!(next_keys[1], keys[1]);
        assert!(frame.panes[0].top_prims.iter().any(|primitive| matches!(
            primitive,
            aeris_charts_render::draw_list::Prim::Circle { .. }
        )));
        assert!(chart.remove_depth_event_layer(event_layer));
    }
}
