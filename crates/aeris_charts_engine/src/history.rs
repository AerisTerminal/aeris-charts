//! Chart-local undo history shared by every committed chart edit.
//!
//! Drawings, engine-owned indicators (bindings and volume profiles) and host-owned objects record
//! into one ordered stack, so undo and redo always step the newest change whichever owner made
//! it. Entries carry only the semantic state needed to reverse one edit; previews, hover,
//! selection and render caches never enter the stack. The history is runtime-only: persistence
//! stores the current state, never this stack.
//!
//! An indicator brought back by undo or redo gets fresh output identities (a removed identity is
//! never reused). The step remaps every entry still in the history to the new identities and
//! reports the pair through [`ChartHistoryChange::IndicatorRestored`], so hosts that keep
//! per-binding state follow it.

use std::any::Any;
use std::collections::{HashMap, VecDeque};

use crate::drawings::{Drawing, DrawingCommand};
use crate::indicators::{IndicatorPane, indicator_output_style};
use crate::native_primitives::NativeSeriesPrimitiveKind;
use crate::{
    ChartEngine, IndicatorInputSource, IndicatorKind, IndicatorOutputStyle, NativePrimitiveId,
    SeriesId, VolumeProfileIndicatorOptions,
};

/// Committed edits kept per chart; the oldest is forgotten past this bound.
pub(crate) const HISTORY_LIMIT: usize = 100;
/// Undrained history changes kept for the host; the oldest are dropped past this bound.
const MAX_PENDING_HISTORY_CHANGES: usize = 4096;

/// Identity of one host-owned edit recorded through [`ChartEngine::record_host_edit`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HostEditId(u64);

impl HostEditId {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// One effect of an undo or redo step that a host may need to follow, in application order.
/// Drain them through [`ChartEngine::take_history_changes`] after
/// [`crate::ChartInputEvent::HistoryChanged`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChartHistoryChange {
    /// An indicator binding came back. `previous` is the binding identity it had before it was
    /// removed; `binding` is its new identity (its first output).
    IndicatorRestored {
        previous: SeriesId,
        binding: SeriesId,
    },
    /// The history removed an indicator binding (with every output).
    IndicatorRemoved(SeriesId),
    /// A volume-profile indicator came back under a new handle.
    VolumeProfileRestored {
        previous: NativePrimitiveId,
        id: NativePrimitiveId,
    },
    /// The history removed a volume-profile indicator.
    VolumeProfileRemoved(NativePrimitiveId),
    /// The host reverses its own edit, read through [`ChartEngine::host_edit_mut`].
    HostEditUndone(HostEditId),
    /// The host reapplies its own edit, read through [`ChartEngine::host_edit_mut`].
    HostEditRedone(HostEditId),
}

/// Everything needed to recreate one indicator binding as it was.
#[derive(Clone)]
pub(crate) struct IndicatorSnapshot {
    outputs: Vec<SeriesId>,
    /// Position among the bindings, so the binding returns to its place in creation order.
    index: usize,
    kind: IndicatorKind,
    source: SeriesId,
    source_input: IndicatorInputSource,
    volume_source: Option<SeriesId>,
    styles: Vec<IndicatorOutputStyle>,
    pane: IndicatorPane,
    /// Index and stretch of the pane a separate-pane binding drew in, so it returns to the same
    /// slot rather than a fresh pane at the bottom.
    pane_slot: Option<(usize, f64)>,
    /// Custom studies keep a per-output placement instead of one binding pane.
    dedicated_outputs: Vec<bool>,
}

impl IndicatorSnapshot {
    fn binding(&self) -> SeriesId {
        self.outputs[0]
    }

    fn remap(&mut self, ids: &IdRemap) {
        for output in &mut self.outputs {
            *output = ids.series(*output);
        }
        self.source = ids.series(self.source);
        self.volume_source = self.volume_source.map(|source| ids.series(source));
    }
}

/// Everything needed to recreate one volume-profile indicator as it was.
pub(crate) struct VolumeProfileSnapshot {
    id: NativePrimitiveId,
    source: SeriesId,
    volume_source: SeriesId,
    options: VolumeProfileIndicatorOptions,
}

impl VolumeProfileSnapshot {
    fn remap(&mut self, ids: &IdRemap) {
        self.id = ids.profiles.get(&self.id).copied().unwrap_or(self.id);
        self.source = ids.series(self.source);
        self.volume_source = ids.series(self.volume_source);
    }
}

/// Identities a history step replaced: old identity to new identity.
#[derive(Default)]
struct IdRemap {
    series: HashMap<SeriesId, SeriesId>,
    profiles: HashMap<NativePrimitiveId, NativePrimitiveId>,
}

impl IdRemap {
    fn series(&self, id: SeriesId) -> SeriesId {
        self.series.get(&id).copied().unwrap_or(id)
    }

    fn is_empty(&self) -> bool {
        self.series.is_empty() && self.profiles.is_empty()
    }
}

enum HistoryEntry {
    Drawing(Box<DrawingCommand>),
    /// Indicator bindings added or removed together, in creation order.
    Indicators {
        added: bool,
        bindings: Vec<IndicatorSnapshot>,
    },
    VolumeProfile {
        added: bool,
        profile: VolumeProfileSnapshot,
    },
    /// A host-owned edit; the engine keeps its payload and only orders it.
    Host {
        id: HostEditId,
        payload: Box<dyn Any + Send>,
    },
    /// Several edits committed as one step: applied in order, undone in reverse.
    Group(Vec<HistoryEntry>),
}

impl HistoryEntry {
    fn has_drawings(&self) -> bool {
        match self {
            Self::Drawing(_) => true,
            Self::Group(entries) => entries.iter().any(Self::has_drawings),
            Self::Indicators { .. } | Self::VolumeProfile { .. } | Self::Host { .. } => false,
        }
    }

    fn for_each_drawing(&mut self, apply: &mut impl FnMut(&mut DrawingCommand)) {
        match self {
            Self::Drawing(command) => apply(command.as_mut()),
            Self::Group(entries) => {
                for entry in entries {
                    entry.for_each_drawing(apply);
                }
            }
            Self::Indicators { .. } | Self::VolumeProfile { .. } | Self::Host { .. } => {}
        }
    }

    fn remap(&mut self, ids: &IdRemap) {
        match self {
            Self::Indicators { bindings, .. } => {
                for binding in bindings {
                    binding.remap(ids);
                }
            }
            Self::VolumeProfile { profile, .. } => profile.remap(ids),
            Self::Group(entries) => {
                for entry in entries {
                    entry.remap(ids);
                }
            }
            Self::Drawing(_) | Self::Host { .. } => {}
        }
    }

    fn host_payload(&self, id: HostEditId) -> Option<&(dyn Any + Send)> {
        match self {
            Self::Host { id: own, payload } if *own == id => Some(payload.as_ref()),
            Self::Group(entries) => entries.iter().find_map(|entry| entry.host_payload(id)),
            _ => None,
        }
    }

    fn host_payload_mut(&mut self, id: HostEditId) -> Option<&mut (dyn Any + Send)> {
        match self {
            Self::Host { id: own, payload } if *own == id => Some(payload.as_mut()),
            Self::Group(entries) => entries
                .iter_mut()
                .find_map(|entry| entry.host_payload_mut(id)),
            _ => None,
        }
    }
}

/// Runtime-only, chart-local edit history.
#[derive(Default)]
pub(crate) struct ChartHistory {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    /// Snapshots taken when a drawing settings session opened (`begin_drawing_options_edit` or
    /// `begin_selected_drawings_options_edit`). While non-empty, updates to those drawings are
    /// live previews that coalesce into one entry on commit. It lives with the stack so every
    /// history reset also ends a session whose baseline it invalidated.
    pub(crate) options_edit: Vec<Drawing>,
    /// Edits recorded while a host group is open; they commit as one step when it closes.
    group: Vec<HistoryEntry>,
    group_depth: usize,
    /// Set while an undo or redo step applies, so the edits it performs are not recorded again.
    applying: bool,
    next_host_edit: u64,
    changes: VecDeque<ChartHistoryChange>,
}

impl ChartHistory {
    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.options_edit.clear();
        self.group.clear();
    }

    pub(crate) fn in_options_edit(&self, id: crate::DrawingId) -> bool {
        self.options_edit.iter().any(|before| before.id == id)
    }

    /// Whether a new edit would be recorded now.
    pub(crate) const fn recording(&self) -> bool {
        !self.applying
    }

    pub(crate) fn record_drawing(&mut self, command: DrawingCommand) {
        self.push(HistoryEntry::Drawing(Box::new(command)));
    }

    fn push(&mut self, entry: HistoryEntry) {
        if self.applying {
            return;
        }
        if self.group_depth > 0 {
            self.group.push(entry);
        } else {
            self.commit(entry);
        }
    }

    fn commit(&mut self, entry: HistoryEntry) {
        if self.undo.len() == HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(entry);
        self.redo.clear();
    }

    /// Drop every step that carries a drawing edit, after the committed drawings were replaced
    /// wholesale (an import or a sync). Indicator and host steps stay.
    pub(crate) fn forget_drawings(&mut self) {
        self.undo.retain(|entry| !entry.has_drawings());
        self.redo.retain(|entry| !entry.has_drawings());
        self.group.retain(|entry| !entry.has_drawings());
        self.options_edit.clear();
    }

    pub(crate) fn rebase_logical_with<F>(&mut self, map: F)
    where
        F: Fn(f64) -> f64 + Copy,
    {
        self.for_each_drawing(&mut |command| command.rebase_logical_with(map));
    }

    /// Follow a pane removal, swap or move in every recorded drawing.
    pub(crate) fn remap_drawing_panes<F>(&mut self, map: F)
    where
        F: Fn(usize) -> usize + Copy,
    {
        self.for_each_drawing(&mut |command| command.remap_panes_with(map));
        for drawing in &mut self.options_edit {
            drawing.pane_index = map(drawing.pane_index);
        }
    }

    fn for_each_drawing(&mut self, apply: &mut impl FnMut(&mut DrawingCommand)) {
        for entry in self
            .undo
            .iter_mut()
            .chain(&mut self.redo)
            .chain(&mut self.group)
        {
            entry.for_each_drawing(apply);
        }
    }

    fn note(&mut self, change: ChartHistoryChange) {
        if self.changes.len() == MAX_PENDING_HISTORY_CHANGES {
            self.changes.pop_front();
        }
        self.changes.push_back(change);
    }
}

impl ChartEngine {
    /// Undo the newest committed edit on this chart: a drawing, an indicator or a host edit.
    pub fn undo(&mut self) -> bool {
        self.step_history(true)
    }

    /// Redo the newest undone edit on this chart.
    pub fn redo(&mut self) -> bool {
        self.step_history(false)
    }

    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
            || self
                .history
                .options_edit
                .iter()
                .any(|before| self.drawing(before.id) != Some(before))
    }

    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    /// Forget every recorded edit, for example after a host recreated its saved state through
    /// the ordinary add APIs.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Open a group: every edit recorded until the matching [`Self::end_history_group`] becomes
    /// one undo step. Groups nest; only the outermost close commits.
    pub fn begin_history_group(&mut self) {
        self.history.group_depth += 1;
    }

    /// Close the innermost group. Returns whether the outermost group committed a step.
    pub fn end_history_group(&mut self) -> bool {
        let Some(depth) = self.history.group_depth.checked_sub(1) else {
            return false;
        };
        self.history.group_depth = depth;
        if depth > 0 {
            return false;
        }
        let mut entries = std::mem::take(&mut self.history.group);
        let entry = match entries.len() {
            0 => return false,
            1 => entries.pop().expect("one grouped entry"),
            _ => HistoryEntry::Group(entries),
        };
        self.history.commit(entry);
        true
    }

    /// Record an edit to a host-owned object as one step of this chart's history. The engine
    /// keeps `payload` (whatever the host needs to reverse and reapply the edit) until the step
    /// leaves the history, and reports each undo or redo of it as a [`ChartHistoryChange`].
    /// The host applies those through its own code, which must not record them again.
    pub fn record_host_edit(&mut self, payload: Box<dyn Any + Send>) -> HostEditId {
        self.history.next_host_edit = self.history.next_host_edit.wrapping_add(1);
        let id = HostEditId(self.history.next_host_edit);
        self.history.push(HistoryEntry::Host { id, payload });
        id
    }

    /// The payload of a host edit still in the history.
    pub fn host_edit(&self, id: HostEditId) -> Option<&(dyn Any + Send)> {
        self.history
            .undo
            .iter()
            .chain(&self.history.redo)
            .chain(&self.history.group)
            .find_map(|entry| entry.host_payload(id))
    }

    /// The payload of a host edit still in the history, for a host that must update it (for
    /// example with the identity a recreated object received).
    pub fn host_edit_mut(&mut self, id: HostEditId) -> Option<&mut (dyn Any + Send)> {
        let history = &mut self.history;
        history
            .undo
            .iter_mut()
            .chain(&mut history.redo)
            .chain(&mut history.group)
            .find_map(|entry| entry.host_payload_mut(id))
    }

    /// Drain the history changes undo and redo produced since the last drain, in order.
    pub fn take_history_changes(&mut self) -> Vec<ChartHistoryChange> {
        self.history.changes.drain(..).collect()
    }

    fn step_history(&mut self, undo: bool) -> bool {
        self.commit_drawing_options_edit();
        loop {
            let popped = if undo {
                self.history.undo.pop()
            } else {
                self.history.redo.pop()
            };
            let Some(mut entry) = popped else {
                return false;
            };
            let drawings = entry.has_drawings();
            if drawings {
                self.invalidate_frame_drawings();
                self.drawing_controller.pending = None;
                self.drawing_controller.brush = None;
            }
            let noted = self.history.changes.len();
            let mut ids = IdRemap::default();
            self.history.applying = true;
            let applied = self.apply_history_entry(&mut entry, undo, &mut ids);
            self.history.applying = false;
            if !ids.is_empty() {
                entry.remap(&ids);
                for other in self.history.undo.iter_mut().chain(&mut self.history.redo) {
                    other.remap(&ids);
                }
            }
            if drawings {
                self.bump_drawing_sync_revision();
                self.drawing_revision = self.drawing_revision.wrapping_add(1);
            }
            if self.history.changes.len() != noted || !ids.is_empty() {
                self.note_history_changed();
            }
            // A step whose objects are all gone (their source was removed outside the history)
            // has nothing left to reverse; it is dropped and the next one runs instead.
            if applied {
                if undo {
                    self.history.redo.push(entry);
                } else {
                    self.history.undo.push(entry);
                }
                return true;
            }
        }
    }

    fn apply_history_entry(
        &mut self,
        entry: &mut HistoryEntry,
        undo: bool,
        ids: &mut IdRemap,
    ) -> bool {
        match entry {
            HistoryEntry::Drawing(command) => {
                self.apply_drawing_command(command, undo);
                true
            }
            HistoryEntry::Indicators { added, bindings } => {
                for binding in bindings.iter_mut() {
                    binding.remap(ids);
                }
                if *added == undo {
                    self.remove_indicator_snapshots(bindings)
                } else {
                    self.restore_indicator_snapshots(bindings, ids)
                }
            }
            HistoryEntry::VolumeProfile { added, profile } => {
                profile.remap(ids);
                if *added == undo {
                    self.remove_volume_profile_snapshot(profile)
                } else {
                    self.restore_volume_profile_snapshot(profile, ids)
                }
            }
            HistoryEntry::Host { id, .. } => {
                self.history.note(if undo {
                    ChartHistoryChange::HostEditUndone(*id)
                } else {
                    ChartHistoryChange::HostEditRedone(*id)
                });
                true
            }
            HistoryEntry::Group(entries) => {
                let mut applied = false;
                if undo {
                    for entry in entries.iter_mut().rev() {
                        entry.remap(ids);
                        applied |= self.apply_history_entry(entry, true, ids);
                    }
                } else {
                    for entry in entries.iter_mut() {
                        entry.remap(ids);
                        applied |= self.apply_history_entry(entry, false, ids);
                    }
                }
                applied
            }
        }
    }

    // --- indicator bindings ---

    fn indicator_index(&self, binding: SeriesId) -> Option<usize> {
        self.indicators
            .iter()
            .position(|indicator| indicator.outputs.first() == Some(&binding))
    }

    fn indicator_snapshot(&self, index: usize) -> Option<IndicatorSnapshot> {
        let binding = self.indicators.get(index)?;
        let first = *binding.outputs.first()?;
        let styles = binding
            .outputs
            .iter()
            .map(|&id| self.series_entry(id).map(indicator_output_style))
            .collect::<Option<Vec<_>>>()?;
        let dedicated_outputs = if matches!(binding.kind, IndicatorKind::Custom { .. }) {
            let source_pane = self.series_entry(binding.source)?.pane_index;
            binding
                .outputs
                .iter()
                .map(|&id| {
                    self.series_entry(id)
                        .is_some_and(|series| series.pane_index != source_pane)
                })
                .collect()
        } else {
            Vec::new()
        };
        let pane = self.indicator_binding_pane(first)?;
        let pane_slot = (pane == IndicatorPane::Separate && dedicated_outputs.is_empty())
            .then(|| self.series_entry(first).map(|series| series.pane_index))
            .flatten()
            .and_then(|index| Some((index, self.panes.get(index)?.stretch_factor)));
        Some(IndicatorSnapshot {
            outputs: binding.outputs.clone(),
            index,
            kind: binding.kind.clone(),
            source: binding.source,
            source_input: binding.source_input,
            volume_source: binding.volume_source,
            styles,
            pane,
            pane_slot,
            dedicated_outputs,
        })
    }

    /// The binding and every binding derived from it, in creation order: exactly what removing
    /// the binding's first output drops (see `drop_indicators_touching`).
    pub(crate) fn indicator_subtree_snapshots(&self, binding: SeriesId) -> Vec<IndicatorSnapshot> {
        if !self.history.recording() {
            return Vec::new();
        }
        let mut dropped = vec![binding];
        let mut snapshots = Vec::new();
        for (index, indicator) in self.indicators.iter().enumerate() {
            let touches = indicator.outputs.contains(&binding)
                || dropped.contains(&indicator.source)
                || indicator
                    .volume_source
                    .is_some_and(|source| dropped.contains(&source));
            if touches {
                dropped.extend(indicator.outputs.iter().copied());
                if let Some(snapshot) = self.indicator_snapshot(index) {
                    snapshots.push(snapshot);
                }
            }
        }
        snapshots
    }

    pub(crate) fn record_indicator_added(&mut self, binding: SeriesId) {
        if !self.history.recording() {
            return;
        }
        if let Some(snapshot) = self
            .indicator_index(binding)
            .and_then(|index| self.indicator_snapshot(index))
        {
            self.history.push(HistoryEntry::Indicators {
                added: true,
                bindings: vec![snapshot],
            });
        }
    }

    pub(crate) fn record_indicators_removed(&mut self, bindings: Vec<IndicatorSnapshot>) {
        if !bindings.is_empty() {
            self.history.push(HistoryEntry::Indicators {
                added: false,
                bindings,
            });
        }
    }

    fn remove_indicator_snapshots(&mut self, snapshots: &mut [IndicatorSnapshot]) -> bool {
        let mut removed = false;
        // Dependents come later in creation order, so they leave first.
        for snapshot in snapshots.iter_mut().rev() {
            let Some(index) = self.indicator_index(snapshot.binding()) else {
                continue;
            };
            // Redo brings it back as it is now, not as it was first recorded.
            if let Some(current) = self.indicator_snapshot(index) {
                *snapshot = current;
            }
            let live = self
                .indicators
                .iter()
                .filter_map(|indicator| indicator.outputs.first().copied())
                .collect::<Vec<_>>();
            let tombstones = self.remove_series_tracked(snapshot.binding());
            for binding in live.into_iter().filter(|id| tombstones.contains(id)) {
                self.history
                    .note(ChartHistoryChange::IndicatorRemoved(binding));
                removed = true;
            }
        }
        removed
    }

    fn restore_indicator_snapshots(
        &mut self,
        snapshots: &mut [IndicatorSnapshot],
        ids: &mut IdRemap,
    ) -> bool {
        let mut restored = false;
        for snapshot in snapshots.iter_mut() {
            snapshot.remap(ids);
            if self.indicator_index(snapshot.binding()).is_some() {
                continue;
            }
            let Some(outputs) = self.restore_indicator(snapshot) else {
                continue;
            };
            let previous = snapshot.binding();
            for (&old, &new) in snapshot.outputs.iter().zip(&outputs) {
                ids.series.insert(old, new);
            }
            snapshot.outputs = outputs;
            self.history.note(ChartHistoryChange::IndicatorRestored {
                previous,
                binding: snapshot.binding(),
            });
            restored = true;
        }
        restored
    }

    fn restore_indicator(&mut self, snapshot: &IndicatorSnapshot) -> Option<Vec<SeriesId>> {
        self.series_entry(snapshot.source)?;
        if let Some(volume) = snapshot.volume_source {
            self.series_entry(volume)?;
        }
        let custom = matches!(snapshot.kind, IndicatorKind::Custom { .. });
        let outputs = if custom {
            self.restore_custom_study_with_panes(
                snapshot.source,
                snapshot.source_input,
                snapshot.kind.clone(),
                snapshot.volume_source,
                &snapshot.dedicated_outputs,
            )
            .ok()?
        } else {
            self.add_indicator_kind_with_input(
                snapshot.source,
                snapshot.source_input,
                snapshot.kind.clone(),
                snapshot.volume_source,
            )
        };
        if outputs.len() != snapshot.styles.len() {
            if let Some(&binding) = outputs.first() {
                let _ = self.remove_series(binding);
            }
            return None;
        }
        for (&output, style) in outputs.iter().zip(&snapshot.styles) {
            let _ = self.set_indicator_output_style(output, style.clone());
        }
        if !custom && self.indicator_binding_pane(outputs[0]) != Some(snapshot.pane) {
            let _ = self.set_indicator_binding_pane(outputs[0], snapshot.pane);
        }
        if let Some((index, stretch)) = snapshot.pane_slot {
            self.restore_indicator_pane_slot(outputs[0], index, stretch);
        }
        self.reposition_indicator(outputs[0], snapshot.index);
        Some(outputs)
    }

    /// Move the fresh pane a recreated separate-pane binding received back to its former slot.
    fn restore_indicator_pane_slot(&mut self, binding: SeriesId, index: usize, stretch: f64) {
        let Some(current) = self.series_entry(binding).map(|series| series.pane_index) else {
            return;
        };
        if let Some(pane) = self.panes.get_mut(current) {
            pane.stretch_factor = stretch;
        }
        if index != current && index > 0 && index < self.panes.len() {
            let _ = self.move_pane(current, index);
        }
    }

    /// Move a recreated binding back to `index` in creation order, never ahead of a binding it
    /// reads from.
    fn reposition_indicator(&mut self, binding: SeriesId, index: usize) {
        let Some(current) = self.indicator_index(binding) else {
            return;
        };
        let source = self.indicators[current].source;
        let volume_source = self.indicators[current].volume_source;
        let earliest = self.indicators[..current]
            .iter()
            .rposition(|other| {
                other.outputs.contains(&source)
                    || volume_source.is_some_and(|volume| other.outputs.contains(&volume))
            })
            .map_or(0, |position| position + 1);
        let target = index.clamp(earliest, current);
        if target < current {
            let indicator = self.indicators.remove(current);
            self.indicators.insert(target, indicator);
        }
    }

    // --- volume profiles ---

    pub(crate) fn volume_profile_history_snapshot(
        &self,
        id: NativePrimitiveId,
    ) -> Option<VolumeProfileSnapshot> {
        self.series
            .iter()
            .filter(|series| !series.removed)
            .find_map(|series| {
                series.native_primitives.iter().find_map(|primitive| {
                    let NativeSeriesPrimitiveKind::VolumeProfileIndicator(state) = &primitive.kind
                    else {
                        return None;
                    };
                    (primitive.id == id).then(|| VolumeProfileSnapshot {
                        id,
                        source: series.id,
                        volume_source: state.volume_source,
                        options: state.options.clone(),
                    })
                })
            })
    }

    pub(crate) fn record_volume_profile(&mut self, profile: VolumeProfileSnapshot, added: bool) {
        self.history
            .push(HistoryEntry::VolumeProfile { added, profile });
    }

    fn remove_volume_profile_snapshot(&mut self, profile: &mut VolumeProfileSnapshot) -> bool {
        let Some(current) = self.volume_profile_history_snapshot(profile.id) else {
            return false;
        };
        *profile = current;
        if !self.remove_native_primitive(profile.id) {
            return false;
        }
        self.history
            .note(ChartHistoryChange::VolumeProfileRemoved(profile.id));
        true
    }

    fn restore_volume_profile_snapshot(
        &mut self,
        profile: &mut VolumeProfileSnapshot,
        ids: &mut IdRemap,
    ) -> bool {
        let Some(id) = self.add_volume_profile_indicator(
            profile.source,
            profile.volume_source,
            profile.options.clone(),
        ) else {
            return false;
        };
        let previous = profile.id;
        ids.profiles.insert(previous, id);
        profile.id = id;
        self.history
            .note(ChartHistoryChange::VolumeProfileRestored { previous, id });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawings::DrawingKind;
    use crate::{ChartInputEvent, ChartKey, InputModifiers};

    fn chart() -> ChartEngine {
        let mut chart = ChartEngine::new(800.0, 500.0, 1.0);
        let times = (0..40).map(|i| f64::from(i) * 3600.0).collect::<Vec<_>>();
        let close = (0..40)
            .map(|i| 100.0 + (f64::from(i) * 0.7).sin() * 4.0)
            .collect::<Vec<_>>();
        let high = close.iter().map(|value| value + 1.0).collect::<Vec<_>>();
        let low = close.iter().map(|value| value - 1.0).collect::<Vec<_>>();
        chart
            .set_series_data(0, &times, &close, &high, &low, &close)
            .unwrap();
        chart.time_scale.set_width(800.0);
        chart.fit_content();
        chart.build_frame();
        chart
    }

    fn kinds(chart: &ChartEngine) -> Vec<IndicatorKind> {
        chart
            .indicator_bindings()
            .into_iter()
            .map(|binding| binding.kind)
            .collect()
    }

    fn pane_of(chart: &ChartEngine, id: SeriesId) -> usize {
        chart.series_entry(id).unwrap().pane_index
    }

    fn red_line() -> IndicatorOutputStyle {
        IndicatorOutputStyle {
            line_color: Some("#ff0000".into()),
            line_width: Some(3.0),
            ..IndicatorOutputStyle::default()
        }
    }

    #[test]
    fn adding_an_indicator_is_one_undo_step() {
        let mut chart = chart();
        let sma = chart.add_sma(0, 5).unwrap();
        assert!(chart.can_undo());
        assert!(chart.undo());
        assert!(!chart.has_indicator_bindings());
        assert_eq!(
            chart.take_history_changes(),
            vec![ChartHistoryChange::IndicatorRemoved(sma)]
        );
        assert!(chart.redo());
        let restored = chart.indicator_bindings()[0].outputs[0];
        assert_ne!(restored, sma, "a removed identity is never reused");
        assert_eq!(
            chart.take_history_changes(),
            vec![ChartHistoryChange::IndicatorRestored {
                previous: sma,
                binding: restored,
            }]
        );
        assert!(
            chart
                .take_input_events()
                .contains(&ChartInputEvent::HistoryChanged)
        );
        assert!(!chart.can_redo());
    }

    #[test]
    fn deleting_an_indicator_restores_it_and_its_dependents_with_their_styles() {
        let mut chart = chart();
        let rsi = chart.add_rsi(0, 14).unwrap();
        let smoothed = chart.add_sma(rsi, 3).unwrap();
        let bands = chart.add_bollinger(0, 20, 2.0);
        assert!(chart.set_indicator_output_style(rsi, red_line()));
        let before = kinds(&chart);
        let rsi_pane = pane_of(&chart, rsi);
        chart.take_input_events();
        chart.set_selected_series(Some(rsi));
        assert!(chart.input_key_down(ChartKey::Delete, InputModifiers::default(), false, 0.0));
        assert_eq!(
            chart.indicator_bindings().len(),
            1,
            "the smoothing left with its source"
        );
        assert_eq!(
            chart.take_input_events(),
            vec![ChartInputEvent::IndicatorRemoved(rsi)]
        );

        let control = InputModifiers {
            control: true,
            ..InputModifiers::default()
        };
        assert!(chart.input_key_down(ChartKey::Undo, control, false, 0.0));
        assert_eq!(kinds(&chart), before, "creation order is restored");
        let bindings = chart.indicator_bindings();
        let restored_rsi = bindings[0].outputs[0];
        assert_eq!(bindings[1].source, restored_rsi);
        assert_eq!(bindings[0].styles[0], red_line());
        assert_eq!(pane_of(&chart, restored_rsi), rsi_pane);
        assert_eq!(
            bindings[2].outputs, bands,
            "untouched bindings keep their identities"
        );
        assert!(
            chart
                .take_input_events()
                .contains(&ChartInputEvent::HistoryChanged)
        );
        assert_eq!(
            chart.take_history_changes(),
            vec![
                ChartHistoryChange::IndicatorRestored {
                    previous: rsi,
                    binding: restored_rsi,
                },
                ChartHistoryChange::IndicatorRestored {
                    previous: smoothed,
                    binding: bindings[1].outputs[0],
                },
            ]
        );

        assert!(chart.redo());
        assert_eq!(chart.indicator_bindings().len(), 1);
        assert!(chart.undo());
        assert_eq!(
            kinds(&chart),
            before,
            "steps keep following the newest identities"
        );
    }

    #[test]
    fn a_restored_separate_pane_returns_to_its_slot() {
        let mut chart = chart();
        let rsi = chart.add_rsi(0, 14).unwrap();
        let macd = chart.add_indicator_kind(
            0,
            IndicatorKind::Macd {
                fast: 12,
                slow: 26,
                signal: 9,
            },
            None,
        );
        assert_eq!((pane_of(&chart, rsi), pane_of(&chart, macd[0])), (1, 2));
        assert!(chart.remove_indicator_binding(rsi));
        assert_eq!(pane_of(&chart, macd[0]), 1, "the emptied pane collapsed");
        assert!(chart.undo());
        let restored = chart.indicator_bindings()[0].outputs[0];
        assert_eq!(
            (pane_of(&chart, restored), pane_of(&chart, macd[0])),
            (1, 2)
        );
    }

    #[test]
    fn clearing_every_indicator_is_one_undo_step() {
        let mut chart = chart();
        chart.add_sma(0, 5).unwrap();
        chart.add_rsi(0, 14).unwrap();
        let before = kinds(&chart);
        assert!(chart.clear_indicator_bindings());
        assert!(chart.undo());
        assert_eq!(kinds(&chart), before);
    }

    #[test]
    fn removing_a_host_series_does_not_record_its_dependent_indicators() {
        let mut chart = chart();
        let line = chart.add_series(crate::SeriesKind::Line);
        let times = (0..40).map(|i| f64::from(i) * 3600.0).collect::<Vec<_>>();
        let values = vec![10.0; 40];
        chart
            .set_series_data(line, &times, &values, &values, &values, &values)
            .unwrap();
        chart.add_sma(line, 5).unwrap();
        chart.clear_history();
        assert!(chart.remove_series(line));
        assert!(!chart.can_undo());
    }

    #[test]
    fn volume_profiles_are_undoable() {
        let mut chart = chart();
        let volume = chart.add_series(crate::SeriesKind::Histogram);
        let times = (0..40).map(|i| f64::from(i) * 3600.0).collect::<Vec<_>>();
        let values = vec![5.0; 40];
        chart
            .set_series_data(volume, &times, &values, &values, &values, &values)
            .unwrap();
        chart.clear_history();
        let profile = chart
            .add_volume_profile_indicator(0, volume, VolumeProfileIndicatorOptions::default())
            .unwrap();
        assert!(chart.remove_native_primitive(profile));
        assert!(chart.undo());
        let [ChartHistoryChange::VolumeProfileRestored { previous, id }] =
            chart.take_history_changes()[..]
        else {
            panic!("undo must report the recreated profile");
        };
        assert_eq!(previous, profile);
        assert!(chart.volume_profile_indicator_options(id).is_some());
        assert!(chart.undo());
        assert_eq!(
            chart.take_history_changes(),
            vec![ChartHistoryChange::VolumeProfileRemoved(id)]
        );
        assert!(!chart.can_undo());
    }

    #[test]
    fn drawings_indicators_and_host_edits_share_one_ordered_history() {
        let mut chart = chart();
        assert!(chart.set_drawing_tool(Some(DrawingKind::HorizontalLine), None, None));
        chart.drawing_tool_activate(200.0, 200.0, crate::DrawingModifiers::default());
        let drawing = chart.drawings()[0].id;
        chart.add_sma(0, 5).unwrap();
        let edit = chart.record_host_edit(Box::new(7_u32));

        assert!(chart.undo());
        assert_eq!(
            chart.take_history_changes(),
            vec![ChartHistoryChange::HostEditUndone(edit)]
        );
        *chart
            .host_edit_mut(edit)
            .and_then(|payload| payload.downcast_mut::<u32>())
            .unwrap() = 8;
        assert!(chart.undo());
        assert!(!chart.has_indicator_bindings());
        assert!(chart.drawing(drawing).is_some());
        assert!(chart.undo());
        assert!(chart.drawing(drawing).is_none());

        assert!(chart.redo());
        assert!(chart.redo());
        chart.take_history_changes();
        assert!(chart.redo());
        assert_eq!(
            chart.take_history_changes(),
            vec![ChartHistoryChange::HostEditRedone(edit)]
        );
        assert_eq!(
            chart
                .host_edit(edit)
                .and_then(|payload| payload.downcast_ref::<u32>()),
            Some(&8)
        );
    }

    #[test]
    fn a_history_group_commits_one_step() {
        let mut chart = chart();
        chart.begin_history_group();
        chart.add_sma(0, 5).unwrap();
        chart.record_host_edit(Box::new(()));
        chart.add_ema(0, 9).unwrap();
        assert!(chart.end_history_group());
        assert!(chart.undo());
        assert!(!chart.has_indicator_bindings());
        assert!(!chart.can_undo());
        assert!(chart.redo());
        assert_eq!(chart.indicator_bindings().len(), 2);
    }
}
