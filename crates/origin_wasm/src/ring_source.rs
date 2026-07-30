//! Layout math for the `SharedArrayBuffer` ring data source (`series_api.set_ring_source`).
//!
//! The producer (a Web Worker) writes fixed-stride rows into a ring and publishes a **monotonic**
//! row count through an `Int32` cursor. The engine drains once per frame: read the cursor, work out
//! which ring slots are new, and copy those rows in. Nothing here touches JS — it is the arithmetic
//! that decides *what* to read, so it is unit-testable off the browser, which matters because the
//! failure modes (wrap, overrun, cursor overflow) are exactly the ones that are miserable to debug
//! from a rendered chart.
//!
//! ## Contract with the producer
//!
//! 1. Write the row's bytes **first**, then publish the incremented cursor with
//!    `Atomics.store`. The engine reads the cursor with `Atomics.load` and then reads only rows
//!    strictly below it, so a row is never read half-written *provided* that ordering holds.
//! 2. The cursor counts rows ever written, not a ring slot. Slot is `count % capacity`.
//! 3. The cursor may overflow `i32`. Differences are computed with wrapping arithmetic, so a wrap
//!    is transparent as long as fewer than `i32::MAX` rows accumulate between two drains — which
//!    the capacity bound already guarantees.
//!
//! ## Overrun
//!
//! If the producer wrote more than `capacity` rows since the last drain, the oldest of them have
//! already been overwritten and are simply gone. The engine then takes the newest `capacity` rows
//! and reports the shortfall, rather than reading a torn window of mixed-age slots.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

/// Byte layout of one ring, mirroring the public `ring_source_layout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingLayout {
    /// Byte offset of row 0.
    pub data_offset: usize,
    /// Bytes per row.
    pub row_stride: usize,
    /// Rows the ring holds before wrapping.
    pub capacity: usize,
    /// Byte offsets, within a row, of the five `f64` channels.
    pub time_offset: usize,
    pub open_offset: usize,
    pub high_offset: usize,
    pub low_offset: usize,
    pub close_offset: usize,
    /// Byte offset of the `Int32` write cursor.
    pub write_cursor_offset: usize,
}

/// Why a layout was rejected. Bound as a message rather than a code — this is a programming error
/// in the host's ring setup, surfaced once at bind time, never in a hot path.
#[derive(Debug, PartialEq, Eq)]
pub enum LayoutError {
    ZeroCapacity,
    /// A row is too short to hold the channel at this offset.
    ChannelOutsideRow {
        channel: &'static str,
        offset: usize,
        row_stride: usize,
    },
    /// The cursor is not 4-byte aligned, so it cannot be read as an `Int32` element.
    CursorMisaligned {
        offset: usize,
    },
    /// The ring rows, or the cursor, do not fit inside the buffer.
    OutsideBuffer {
        needed: usize,
        buffer_bytes: usize,
    },
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroCapacity => write!(f, "capacity must be at least 1 row"),
            Self::ChannelOutsideRow {
                channel,
                offset,
                row_stride,
            } => write!(
                f,
                "{channel}_offset {offset} + 8 bytes exceeds row_stride {row_stride}"
            ),
            Self::CursorMisaligned { offset } => write!(
                f,
                "write_cursor_offset {offset} is not 4-byte aligned (Int32 cursor)"
            ),
            Self::OutsideBuffer {
                needed,
                buffer_bytes,
            } => write!(
                f,
                "layout needs {needed} bytes but the buffer is {buffer_bytes}"
            ),
        }
    }
}

/// One contiguous run of ring slots to read: `count` rows starting at ring slot `slot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RingSegment {
    pub slot: usize,
    pub count: usize,
}

/// What one drain should read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrainPlan {
    /// Up to two runs (a wrap splits the window at the end of the ring), in **oldest-first** order
    /// so the rows apply in ascending time.
    pub segments: Vec<RingSegment>,
    /// Rows the producer overwrote before this drain could read them.
    pub lost_rows: usize,
    /// The cursor value this plan consumed up to; becomes the new `consumed`.
    pub consumed_to: i32,
}

impl RingLayout {
    /// Total bytes the layout addresses; the buffer must be at least this large.
    fn required_bytes(&self) -> usize {
        let rows_end = self
            .data_offset
            .saturating_add(self.row_stride.saturating_mul(self.capacity));
        rows_end.max(self.write_cursor_offset.saturating_add(4))
    }

    /// Reject a layout that could read outside the buffer or outside a row. Checked once at bind
    /// time so the per-frame drain can index without bounds logic.
    pub fn validate(&self, buffer_bytes: usize) -> Result<(), LayoutError> {
        if self.capacity == 0 {
            return Err(LayoutError::ZeroCapacity);
        }
        for (channel, offset) in [
            ("time", self.time_offset),
            ("open", self.open_offset),
            ("high", self.high_offset),
            ("low", self.low_offset),
            ("close", self.close_offset),
        ] {
            if offset.saturating_add(8) > self.row_stride {
                return Err(LayoutError::ChannelOutsideRow {
                    channel,
                    offset,
                    row_stride: self.row_stride,
                });
            }
        }
        if !self.write_cursor_offset.is_multiple_of(4) {
            return Err(LayoutError::CursorMisaligned {
                offset: self.write_cursor_offset,
            });
        }
        let needed = self.required_bytes();
        if needed > buffer_bytes {
            return Err(LayoutError::OutsideBuffer {
                needed,
                buffer_bytes,
            });
        }
        Ok(())
    }

    /// Byte offset of ring slot `slot`.
    pub fn slot_offset(&self, slot: usize) -> usize {
        self.data_offset + slot * self.row_stride
    }

    /// Element index of the write cursor in an `Int32Array` over the whole buffer.
    pub fn cursor_index(&self) -> u32 {
        (self.write_cursor_offset / 4) as u32
    }

    /// Plan the reads for a drain, given the last-consumed row count and the cursor just loaded.
    ///
    /// `cursor - consumed` is computed with wrapping arithmetic so an `i32` cursor overflow is
    /// transparent. A negative difference means the producer reset (or the host rebound a stale
    /// cursor); that is treated as "nothing new" and resynchronizes on the next drain.
    pub fn plan_drain(&self, consumed: i32, cursor: i32) -> DrainPlan {
        let available = cursor.wrapping_sub(consumed);
        if available <= 0 {
            return DrainPlan {
                segments: Vec::new(),
                lost_rows: 0,
                consumed_to: cursor,
            };
        }
        let available = available as usize;
        // Overrun: the oldest `available - capacity` rows were overwritten in place before we got
        // to them. Take the newest window rather than a torn mix of old and new slots.
        let (take, lost_rows) = if available > self.capacity {
            (self.capacity, available - self.capacity)
        } else {
            (available, 0)
        };
        let start_count = cursor.wrapping_sub(take as i32);
        // `rem_euclid` keeps the slot correct for a cursor that has wrapped into the negatives.
        let start_slot = (start_count as i64).rem_euclid(self.capacity as i64) as usize;
        let mut segments = Vec::with_capacity(2);
        let first = take.min(self.capacity - start_slot);
        segments.push(RingSegment {
            slot: start_slot,
            count: first,
        });
        if take > first {
            segments.push(RingSegment {
                slot: 0,
                count: take - first,
            });
        }
        DrainPlan {
            segments,
            lost_rows,
            consumed_to: cursor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tightly packed 5-channel row: time, open, high, low, close.
    fn layout(capacity: usize) -> RingLayout {
        RingLayout {
            data_offset: 64,
            row_stride: 40,
            capacity,
            time_offset: 0,
            open_offset: 8,
            high_offset: 16,
            low_offset: 24,
            close_offset: 32,
            write_cursor_offset: 0,
        }
    }

    fn buffer_bytes(l: &RingLayout) -> usize {
        l.data_offset + l.row_stride * l.capacity
    }

    #[test]
    fn a_well_formed_layout_validates() {
        let l = layout(1024);
        assert_eq!(l.validate(buffer_bytes(&l)), Ok(()));
        assert_eq!(l.cursor_index(), 0);
        assert_eq!(l.slot_offset(0), 64);
        assert_eq!(l.slot_offset(3), 64 + 120);
    }

    #[test]
    fn zero_capacity_is_rejected() {
        let l = layout(0);
        assert_eq!(l.validate(4096), Err(LayoutError::ZeroCapacity));
    }

    #[test]
    fn a_channel_that_overruns_its_row_is_rejected() {
        let mut l = layout(8);
        l.close_offset = 36; // 36 + 8 > 40
        assert_eq!(
            l.validate(buffer_bytes(&l)),
            Err(LayoutError::ChannelOutsideRow {
                channel: "close",
                offset: 36,
                row_stride: 40,
            })
        );
    }

    #[test]
    fn a_misaligned_cursor_is_rejected() {
        let mut l = layout(8);
        l.write_cursor_offset = 6;
        assert_eq!(
            l.validate(buffer_bytes(&l)),
            Err(LayoutError::CursorMisaligned { offset: 6 })
        );
    }

    #[test]
    fn a_layout_that_leaves_the_buffer_is_rejected() {
        let l = layout(1024);
        let short = buffer_bytes(&l) - 1;
        assert_eq!(
            l.validate(short),
            Err(LayoutError::OutsideBuffer {
                needed: buffer_bytes(&l),
                buffer_bytes: short,
            })
        );
        // A cursor past the rows also has to fit.
        let mut l = layout(4);
        l.write_cursor_offset = 4096;
        assert!(matches!(
            l.validate(2048),
            Err(LayoutError::OutsideBuffer { .. })
        ));
    }

    #[test]
    fn an_idle_producer_yields_no_reads() {
        let l = layout(16);
        let plan = l.plan_drain(7, 7);
        assert!(plan.segments.is_empty());
        assert_eq!(plan.lost_rows, 0);
        assert_eq!(plan.consumed_to, 7);
    }

    #[test]
    fn a_contiguous_window_is_one_segment() {
        let l = layout(16);
        let plan = l.plan_drain(2, 6);
        assert_eq!(plan.segments, vec![RingSegment { slot: 2, count: 4 }]);
        assert_eq!(plan.lost_rows, 0);
        assert_eq!(plan.consumed_to, 6);
    }

    #[test]
    fn a_window_that_wraps_splits_oldest_first() {
        let l = layout(16);
        // Rows 14,15 then 0,1 — the older pair must come first so times stay ascending.
        let plan = l.plan_drain(14, 18);
        assert_eq!(
            plan.segments,
            vec![
                RingSegment { slot: 14, count: 2 },
                RingSegment { slot: 0, count: 2 },
            ]
        );
        assert_eq!(plan.consumed_to, 18);
    }

    #[test]
    fn a_window_ending_exactly_at_the_wrap_stays_one_segment() {
        let l = layout(16);
        let plan = l.plan_drain(12, 16);
        assert_eq!(plan.segments, vec![RingSegment { slot: 12, count: 4 }]);
    }

    #[test]
    fn an_overrun_takes_the_newest_capacity_rows_and_reports_the_loss() {
        let l = layout(16);
        // The producer wrote 40 rows since the last drain into a 16-row ring: 24 are gone.
        let plan = l.plan_drain(0, 40);
        assert_eq!(plan.lost_rows, 24);
        let taken: usize = plan.segments.iter().map(|s| s.count).sum();
        assert_eq!(taken, 16, "exactly the newest capacity rows");
        // Newest 16 rows are counts 24..40, i.e. slots 8..16 then 0..8.
        assert_eq!(
            plan.segments,
            vec![
                RingSegment { slot: 8, count: 8 },
                RingSegment { slot: 0, count: 8 },
            ]
        );
        assert_eq!(plan.consumed_to, 40);
    }

    #[test]
    fn a_full_ring_exactly_at_capacity_is_not_an_overrun() {
        let l = layout(16);
        let plan = l.plan_drain(0, 16);
        assert_eq!(plan.lost_rows, 0);
        assert_eq!(plan.segments, vec![RingSegment { slot: 0, count: 16 }]);
    }

    #[test]
    fn an_i32_cursor_overflow_is_transparent() {
        let l = layout(16);
        // The producer's cursor wrapped from i32::MAX to i32::MIN mid-window.
        let consumed = i32::MAX - 1;
        let cursor = i32::MAX.wrapping_add(2); // == i32::MIN + 1
        let plan = l.plan_drain(consumed, cursor);
        let taken: usize = plan.segments.iter().map(|s| s.count).sum();
        assert_eq!(taken, 3, "three rows spanning the overflow");
        assert_eq!(plan.lost_rows, 0);
        assert_eq!(plan.consumed_to, cursor);
        // Slots stay in range across the negative cursor values.
        for segment in &plan.segments {
            assert!(segment.slot < l.capacity);
            assert!(segment.slot + segment.count <= l.capacity);
        }
    }

    #[test]
    fn a_producer_reset_is_treated_as_nothing_new() {
        let l = layout(16);
        // A cursor that went backwards (worker restarted with a fresh buffer view).
        let plan = l.plan_drain(100, 3);
        assert!(plan.segments.is_empty());
        // Resynchronize on the reset value rather than stalling forever.
        assert_eq!(plan.consumed_to, 3);
    }

    #[test]
    fn every_plan_stays_inside_the_ring() {
        let l = layout(7); // deliberately not a power of two
        for consumed in -20i32..40 {
            for advance in 0i32..30 {
                let plan = l.plan_drain(consumed, consumed.wrapping_add(advance));
                let taken: usize = plan.segments.iter().map(|s| s.count).sum();
                assert!(taken <= l.capacity);
                assert_eq!(taken + plan.lost_rows, advance.max(0) as usize);
                for segment in &plan.segments {
                    assert!(
                        segment.slot + segment.count <= l.capacity,
                        "segment {segment:?} leaves a {}-row ring",
                        l.capacity
                    );
                }
            }
        }
    }
}
