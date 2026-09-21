use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::num::NonZeroU32;

use crate::{ChartError, ErrorCode, MAX_GENERAL_TEMPORAL_MILLISECONDS};

pub const MAX_GENERAL_DATASETS: usize = 1_024;
pub const MAX_GENERAL_DATASET_ROWS: usize = 16_777_216;
pub const MAX_GENERAL_DATASET_CATEGORIES: usize = 65_536;
pub const MAX_GENERAL_DATASET_CATEGORY_BYTES: usize = 1_048_576;
pub const MAX_GENERAL_ROW_ID_BYTES: usize = 4_096;
pub const MAX_GENERAL_ROW_ID_BYTES_TOTAL: usize = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeneralXKind {
    Numeric,
    Temporal,
    Category,
}

/// Explicit row identity accepted by the general-data boundary.
///
/// Numeric IDs normalize `-0` and `0` to the same identity. Non-finite values are rejected before
/// installation.
#[derive(Clone, Debug)]
pub enum GeneralRowId {
    Number(f64),
    Text(String),
}

impl PartialEq for GeneralRowId {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => {
                normalized_number_bits(*a) == normalized_number_bits(*b)
            }
            (Self::Text(a), Self::Text(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for GeneralRowId {}

impl Hash for GeneralRowId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Number(value) => {
                0u8.hash(state);
                normalized_number_bits(*value).hash(state);
            }
            Self::Text(value) => {
                1u8.hash(state);
                value.hash(state);
            }
        }
    }
}

fn normalized_number_bits(value: f64) -> u64 {
    if value == 0.0 {
        0
    } else {
        value.to_bits()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GeneralRowIdentity {
    Generated(u64),
    Explicit(GeneralRowId),
}

/// Owned typed-column input for the first general Cartesian slices.
///
/// Object-shaped host rows are converted into one of these variants once at the boundary. Missing
/// Y values use `y_valid`; NaN and infinity are never missing sentinels.
#[derive(Clone, Debug, PartialEq)]
pub enum GeneralXyInput {
    Numeric {
        ids: Option<Vec<GeneralRowId>>,
        x: Vec<f64>,
        y: Vec<f64>,
        y_valid: Option<Vec<u8>>,
    },
    Temporal {
        ids: Option<Vec<GeneralRowId>>,
        x_epoch_ms: Vec<i64>,
        y: Vec<f64>,
        y_valid: Option<Vec<u8>>,
    },
    Category {
        ids: Option<Vec<GeneralRowId>>,
        categories: Vec<String>,
        category_indices: Vec<u32>,
        y: Vec<f64>,
        y_valid: Option<Vec<u8>>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GeneralDatasetId(NonZeroU32);

impl GeneralDatasetId {
    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Debug, PartialEq)]
enum GeneralXColumn {
    Numeric(Vec<f64>),
    Temporal(Vec<i64>),
    Category {
        categories: Vec<String>,
        indices: Vec<u32>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeneralDataset {
    id: GeneralDatasetId,
    identities: Vec<GeneralRowIdentity>,
    x: GeneralXColumn,
    y: Vec<f64>,
    y_valid: Option<Vec<u8>>,
}

impl GeneralDataset {
    pub fn id(&self) -> GeneralDatasetId {
        self.id
    }

    pub fn len(&self) -> usize {
        self.y.len()
    }

    pub fn is_empty(&self) -> bool {
        self.y.is_empty()
    }

    pub fn x_kind(&self) -> GeneralXKind {
        match self.x {
            GeneralXColumn::Numeric(_) => GeneralXKind::Numeric,
            GeneralXColumn::Temporal(_) => GeneralXKind::Temporal,
            GeneralXColumn::Category { .. } => GeneralXKind::Category,
        }
    }

    pub fn row_identity(&self, index: usize) -> Option<&GeneralRowIdentity> {
        self.identities.get(index)
    }

    pub fn y(&self) -> &[f64] {
        &self.y
    }

    pub fn y_is_valid(&self, index: usize) -> bool {
        index < self.y.len()
            && self
                .y_valid
                .as_ref()
                .is_none_or(|validity| validity[index] != 0)
    }

    pub fn numeric_x(&self) -> Option<&[f64]> {
        match &self.x {
            GeneralXColumn::Numeric(values) => Some(values),
            _ => None,
        }
    }

    pub fn temporal_x_epoch_ms(&self) -> Option<&[i64]> {
        match &self.x {
            GeneralXColumn::Temporal(values) => Some(values),
            _ => None,
        }
    }

    pub fn categories(&self) -> Option<&[String]> {
        match &self.x {
            GeneralXColumn::Category { categories, .. } => Some(categories),
            _ => None,
        }
    }

    pub fn category_indices(&self) -> Option<&[u32]> {
        match &self.x {
            GeneralXColumn::Category { indices, .. } => Some(indices),
            _ => None,
        }
    }

    fn estimated_bytes(&self) -> usize {
        let identity_text = self
            .identities
            .iter()
            .map(|identity| match identity {
                GeneralRowIdentity::Explicit(GeneralRowId::Text(value)) => value.capacity(),
                _ => 0,
            })
            .sum::<usize>();
        let x_bytes = match &self.x {
            GeneralXColumn::Numeric(values) => values.capacity() * std::mem::size_of::<f64>(),
            GeneralXColumn::Temporal(values) => values.capacity() * std::mem::size_of::<i64>(),
            GeneralXColumn::Category {
                categories,
                indices,
            } => {
                categories.capacity() * std::mem::size_of::<String>()
                    + categories.iter().map(String::capacity).sum::<usize>()
                    + indices.capacity() * std::mem::size_of::<u32>()
            }
        };
        self.identities.capacity() * std::mem::size_of::<GeneralRowIdentity>()
            + identity_text
            + x_bytes
            + self.y.capacity() * std::mem::size_of::<f64>()
            + self.y_valid.as_ref().map_or(0, Vec::capacity)
    }
}

struct ValidatedGeneralXy {
    ids: Option<Vec<GeneralRowId>>,
    x: GeneralXColumn,
    y: Vec<f64>,
    y_valid: Option<Vec<u8>>,
}

impl GeneralXyInput {
    fn validate(self) -> Result<ValidatedGeneralXy, ChartError> {
        match self {
            Self::Numeric { ids, x, y, y_valid } => {
                validate_row_count(x.len())?;
                validate_common(x.len(), ids.as_deref(), &y, y_valid.as_deref())?;
                if x.iter().any(|value| !value.is_finite()) {
                    return Err(invalid_data("general numeric X values must be finite"));
                }
                Ok(ValidatedGeneralXy {
                    ids,
                    x: GeneralXColumn::Numeric(x),
                    y,
                    y_valid: normalize_validity(y_valid),
                })
            }
            Self::Temporal {
                ids,
                x_epoch_ms,
                y,
                y_valid,
            } => {
                validate_row_count(x_epoch_ms.len())?;
                validate_common(x_epoch_ms.len(), ids.as_deref(), &y, y_valid.as_deref())?;
                if x_epoch_ms
                    .iter()
                    .any(|value| value.unsigned_abs() > MAX_GENERAL_TEMPORAL_MILLISECONDS as u64)
                {
                    return Err(invalid_data(format!(
                        "general temporal X values must stay within +/-{MAX_GENERAL_TEMPORAL_MILLISECONDS} epoch milliseconds"
                    )));
                }
                Ok(ValidatedGeneralXy {
                    ids,
                    x: GeneralXColumn::Temporal(x_epoch_ms),
                    y,
                    y_valid: normalize_validity(y_valid),
                })
            }
            Self::Category {
                ids,
                categories,
                category_indices,
                y,
                y_valid,
            } => {
                validate_row_count(category_indices.len())?;
                validate_common(
                    category_indices.len(),
                    ids.as_deref(),
                    &y,
                    y_valid.as_deref(),
                )?;
                validate_categories(&categories, &category_indices)?;
                Ok(ValidatedGeneralXy {
                    ids,
                    x: GeneralXColumn::Category {
                        categories,
                        indices: category_indices,
                    },
                    y,
                    y_valid: normalize_validity(y_valid),
                })
            }
        }
    }
}

fn validate_row_count(row_count: usize) -> Result<(), ChartError> {
    if row_count > MAX_GENERAL_DATASET_ROWS {
        return Err(resource(format!(
            "general dataset exceeds {MAX_GENERAL_DATASET_ROWS} rows"
        )));
    }
    Ok(())
}

fn validate_common(
    row_count: usize,
    ids: Option<&[GeneralRowId]>,
    y: &[f64],
    y_valid: Option<&[u8]>,
) -> Result<(), ChartError> {
    if y.len() != row_count {
        return Err(invalid_data(
            "general X and Y columns must have equal lengths",
        ));
    }
    if let Some(validity) = y_valid {
        if validity.len() != row_count {
            return Err(invalid_data(
                "general Y validity and value columns must have equal lengths",
            ));
        }
        if validity.iter().any(|value| !matches!(*value, 0 | 1)) {
            return Err(invalid_data("general Y validity values must be 0 or 1"));
        }
    }
    if y.iter().any(|value| !value.is_finite()) {
        return Err(invalid_data(
            "general Y values must be finite; use the validity column for missing values",
        ));
    }
    if let Some(ids) = ids {
        validate_ids(ids, row_count)?;
    }
    Ok(())
}

fn validate_ids(ids: &[GeneralRowId], row_count: usize) -> Result<(), ChartError> {
    if ids.len() != row_count {
        return Err(invalid_data(
            "general row IDs and value columns must have equal lengths",
        ));
    }
    let mut total_text_bytes = 0usize;
    let mut unique = HashSet::with_capacity(ids.len());
    for id in ids {
        match id {
            GeneralRowId::Number(value) if !value.is_finite() => {
                return Err(invalid_data("general numeric row IDs must be finite"));
            }
            GeneralRowId::Text(value) => {
                if value.len() > MAX_GENERAL_ROW_ID_BYTES {
                    return Err(resource(format!(
                        "general row ID exceeds {MAX_GENERAL_ROW_ID_BYTES} UTF-8 bytes"
                    )));
                }
                total_text_bytes = total_text_bytes
                    .checked_add(value.len())
                    .ok_or_else(|| resource("general row ID byte count overflow"))?;
                if total_text_bytes > MAX_GENERAL_ROW_ID_BYTES_TOTAL {
                    return Err(resource(format!(
                        "general row IDs exceed {MAX_GENERAL_ROW_ID_BYTES_TOTAL} UTF-8 bytes"
                    )));
                }
            }
            GeneralRowId::Number(_) => {}
        }
        if !unique.insert(id) {
            return Err(invalid_data("general explicit row IDs must be unique"));
        }
    }
    Ok(())
}

fn validate_categories(categories: &[String], indices: &[u32]) -> Result<(), ChartError> {
    if categories.len() > MAX_GENERAL_DATASET_CATEGORIES {
        return Err(resource(format!(
            "general dataset exceeds {MAX_GENERAL_DATASET_CATEGORIES} category labels"
        )));
    }
    let category_bytes = categories.iter().try_fold(0usize, |total, category| {
        total
            .checked_add(category.len())
            .ok_or_else(|| resource("general category byte count overflow"))
    })?;
    if category_bytes > MAX_GENERAL_DATASET_CATEGORY_BYTES {
        return Err(resource(format!(
            "general dataset categories exceed {MAX_GENERAL_DATASET_CATEGORY_BYTES} UTF-8 bytes"
        )));
    }
    let mut unique = HashSet::with_capacity(categories.len());
    if categories.iter().any(|category| !unique.insert(category)) {
        return Err(invalid_data("general category labels must be unique"));
    }
    if indices
        .iter()
        .any(|index| usize::try_from(*index).map_or(true, |index| index >= categories.len()))
    {
        return Err(invalid_data("general category index is out of range"));
    }
    Ok(())
}

fn normalize_validity(validity: Option<Vec<u8>>) -> Option<Vec<u8>> {
    validity.and_then(|values| values.contains(&0).then_some(values))
}

pub(crate) struct GeneralDataStore {
    datasets: Vec<GeneralDataset>,
    next_dataset_id: u64,
    next_generated_row_id: u64,
}

impl GeneralDataStore {
    pub(crate) fn new() -> Self {
        Self {
            datasets: Vec::new(),
            next_dataset_id: 1,
            next_generated_row_id: 1,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.datasets.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.datasets.is_empty()
    }

    pub(crate) fn get(&self, id: GeneralDatasetId) -> Option<&GeneralDataset> {
        self.datasets.iter().find(|dataset| dataset.id == id)
    }

    pub(crate) fn insert(&mut self, input: GeneralXyInput) -> Result<GeneralDatasetId, ChartError> {
        if self.datasets.len() >= MAX_GENERAL_DATASETS {
            return Err(resource(format!(
                "a chart supports at most {MAX_GENERAL_DATASETS} general datasets"
            )));
        }
        let validated = input.validate()?;
        let raw_id = u32::try_from(self.next_dataset_id)
            .ok()
            .and_then(NonZeroU32::new)
            .ok_or_else(|| resource("general dataset identity space is exhausted"))?;
        let (identities, next_generated_row_id) =
            identities_for(validated.ids, validated.y.len(), self.next_generated_row_id)?;
        let next_dataset_id = self
            .next_dataset_id
            .checked_add(1)
            .ok_or_else(|| resource("general dataset identity space is exhausted"))?;
        let id = GeneralDatasetId(raw_id);
        self.datasets.push(GeneralDataset {
            id,
            identities,
            x: validated.x,
            y: validated.y,
            y_valid: validated.y_valid,
        });
        self.next_dataset_id = next_dataset_id;
        self.next_generated_row_id = next_generated_row_id;
        Ok(id)
    }

    pub(crate) fn replace(
        &mut self,
        id: GeneralDatasetId,
        input: GeneralXyInput,
    ) -> Result<(), ChartError> {
        let Some(slot) = self.datasets.iter().position(|dataset| dataset.id == id) else {
            return Err(ChartError::new(
                ErrorCode::InvalidHandle,
                "general dataset handle is stale",
            ));
        };
        let validated = input.validate()?;
        let (identities, next_generated_row_id) =
            identities_for(validated.ids, validated.y.len(), self.next_generated_row_id)?;
        self.datasets[slot] = GeneralDataset {
            id,
            identities,
            x: validated.x,
            y: validated.y,
            y_valid: validated.y_valid,
        };
        self.next_generated_row_id = next_generated_row_id;
        Ok(())
    }

    pub(crate) fn remove(&mut self, id: GeneralDatasetId) -> bool {
        let Some(index) = self.datasets.iter().position(|dataset| dataset.id == id) else {
            return false;
        };
        self.datasets.remove(index);
        true
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.datasets.capacity() * std::mem::size_of::<GeneralDataset>()
            + self
                .datasets
                .iter()
                .map(GeneralDataset::estimated_bytes)
                .sum::<usize>()
    }
}

impl Default for GeneralDataStore {
    fn default() -> Self {
        Self::new()
    }
}

fn identities_for(
    ids: Option<Vec<GeneralRowId>>,
    row_count: usize,
    next_generated_row_id: u64,
) -> Result<(Vec<GeneralRowIdentity>, u64), ChartError> {
    if let Some(ids) = ids {
        return Ok((
            ids.into_iter().map(GeneralRowIdentity::Explicit).collect(),
            next_generated_row_id,
        ));
    }
    let count = u64::try_from(row_count)
        .map_err(|_| resource("general generated row identity count overflow"))?;
    let end = next_generated_row_id
        .checked_add(count)
        .ok_or_else(|| resource("general generated row identity space is exhausted"))?;
    Ok((
        (next_generated_row_id..end)
            .map(GeneralRowIdentity::Generated)
            .collect(),
        end,
    ))
}

fn invalid_data(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::InvalidData, message)
}

fn resource(message: impl Into<String>) -> ChartError {
    ChartError::new(ErrorCode::ResourceLimit, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric_input(ids: Option<Vec<GeneralRowId>>) -> GeneralXyInput {
        GeneralXyInput::Numeric {
            ids,
            x: vec![1.0, 2.0, 3.0],
            y: vec![10.0, 20.0, 30.0],
            y_valid: Some(vec![1, 0, 1]),
        }
    }

    #[test]
    fn typed_xy_storage_preserves_missing_rows_and_stable_identity() {
        let mut store = GeneralDataStore::new();
        let id = store.insert(numeric_input(None)).unwrap();
        let dataset = store.get(id).unwrap();
        assert_eq!(dataset.x_kind(), GeneralXKind::Numeric);
        assert_eq!(dataset.numeric_x(), Some(&[1.0, 2.0, 3.0][..]));
        assert_eq!(dataset.y(), &[10.0, 20.0, 30.0]);
        assert!(dataset.y_is_valid(0));
        assert!(!dataset.y_is_valid(1));
        assert_eq!(
            dataset.row_identity(0),
            Some(&GeneralRowIdentity::Generated(1))
        );

        store
            .replace(
                id,
                GeneralXyInput::Numeric {
                    ids: None,
                    x: vec![4.0],
                    y: vec![40.0],
                    y_valid: None,
                },
            )
            .unwrap();
        assert_eq!(
            store.get(id).unwrap().row_identity(0),
            Some(&GeneralRowIdentity::Generated(4))
        );
    }

    #[test]
    fn explicit_ids_are_unique_and_zero_is_normalized() {
        let mut store = GeneralDataStore::new();
        let err = store
            .insert(numeric_input(Some(vec![
                GeneralRowId::Number(0.0),
                GeneralRowId::Number(-0.0),
                GeneralRowId::Text("third".into()),
            ])))
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidData);
        assert!(store.is_empty());
    }

    #[test]
    fn category_and_temporal_inputs_validate_before_mutation() {
        let mut store = GeneralDataStore::new();
        let category = store
            .insert(GeneralXyInput::Category {
                ids: None,
                categories: vec!["Jan".into(), "Feb".into()],
                category_indices: vec![0, 1, 0],
                y: vec![1.0, 2.0, 3.0],
                y_valid: None,
            })
            .unwrap();
        let dataset = store.get(category).unwrap();
        assert_eq!(dataset.categories().unwrap(), &["Jan", "Feb"]);
        assert_eq!(dataset.category_indices().unwrap(), &[0, 1, 0]);

        let before = store.len();
        let err = store
            .insert(GeneralXyInput::Category {
                ids: None,
                categories: vec!["Jan".into()],
                category_indices: vec![1],
                y: vec![1.0],
                y_valid: None,
            })
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidData);
        assert_eq!(store.len(), before);

        let err = store
            .insert(GeneralXyInput::Temporal {
                ids: None,
                x_epoch_ms: vec![MAX_GENERAL_TEMPORAL_MILLISECONDS + 1],
                y: vec![1.0],
                y_valid: None,
            })
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidData);
        assert_eq!(store.len(), before);
    }

    #[test]
    fn failed_replace_is_atomic_and_all_validity_is_compacted() {
        let mut store = GeneralDataStore::new();
        let id = store
            .insert(GeneralXyInput::Numeric {
                ids: Some(vec![GeneralRowId::Text("a".into())]),
                x: vec![1.0],
                y: vec![2.0],
                y_valid: Some(vec![1]),
            })
            .unwrap();
        let before = store.get(id).unwrap().clone();
        assert!(before.y_valid.is_none());

        let err = store
            .replace(
                id,
                GeneralXyInput::Numeric {
                    ids: None,
                    x: vec![f64::NAN],
                    y: vec![3.0],
                    y_valid: None,
                },
            )
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidData);
        assert_eq!(store.get(id), Some(&before));
    }
}
