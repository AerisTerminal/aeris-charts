use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::num::NonZeroU32;

use crate::{ChartError, ErrorCode, MAX_GENERAL_TEMPORAL_MILLISECONDS};

pub const MAX_GENERAL_DATASETS: usize = 1_024;
pub const MAX_GENERAL_DATASET_ROWS: usize = 16_777_216;
pub const MAX_GENERAL_DATASET_CATEGORIES: usize = 65_536;
pub const MAX_GENERAL_DATASET_CATEGORY_BYTES: usize = 1_048_576;
pub const MAX_GENERAL_ROW_ID_BYTES: usize = 4_096;
pub const MAX_GENERAL_ROW_ID_BYTES_TOTAL: usize = 1_048_576;
pub const MAX_GENERAL_ROW_LABEL_BYTES: usize = 4_096;
pub const MAX_GENERAL_ROW_LABEL_BYTES_TOTAL: usize = 1_048_576;
pub const MAX_GENERAL_ROW_LABELS: usize = 65_536;

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
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum GeneralRowId {
    Number(f64),
    Text(String),
    /// Boundary marker for an omitted object-row ID. The store replaces it with a monotonic
    /// generated identity during the same validated transaction.
    Generated,
}

impl PartialEq for GeneralRowId {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => {
                normalized_number_bits(*a) == normalized_number_bits(*b)
            }
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Generated, Self::Generated) => true,
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
            Self::Generated => 2u8.hash(state),
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
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GeneralXyInput {
    Numeric {
        ids: Option<Vec<GeneralRowId>>,
        x: Vec<f64>,
        y: Vec<f64>,
        y_valid: Option<Vec<u8>>,
    },
    Bubble {
        ids: Option<Vec<GeneralRowId>>,
        x: Vec<f64>,
        y: Vec<f64>,
        y_valid: Option<Vec<u8>>,
        size: Vec<f64>,
        size_valid: Option<Vec<u8>>,
    },
    RangeNumeric {
        ids: Option<Vec<GeneralRowId>>,
        x: Vec<f64>,
        low: Vec<f64>,
        low_valid: Option<Vec<u8>>,
        high: Vec<f64>,
        high_valid: Option<Vec<u8>>,
    },
    Temporal {
        ids: Option<Vec<GeneralRowId>>,
        x_epoch_ms: Vec<i64>,
        y: Vec<f64>,
        y_valid: Option<Vec<u8>>,
    },
    RangeTemporal {
        ids: Option<Vec<GeneralRowId>>,
        x_epoch_ms: Vec<i64>,
        low: Vec<f64>,
        low_valid: Option<Vec<u8>>,
        high: Vec<f64>,
        high_valid: Option<Vec<u8>>,
    },
    Category {
        ids: Option<Vec<GeneralRowId>>,
        categories: Vec<String>,
        category_indices: Vec<u32>,
        y: Vec<f64>,
        y_valid: Option<Vec<u8>>,
    },
    RangeCategory {
        ids: Option<Vec<GeneralRowId>>,
        categories: Vec<String>,
        category_indices: Vec<u32>,
        low: Vec<f64>,
        low_valid: Option<Vec<u8>>,
        high: Vec<f64>,
        high_valid: Option<Vec<u8>>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GeneralDatasetId(NonZeroU32);

impl GeneralDatasetId {
    pub fn get(self) -> u32 {
        self.0.get()
    }

    #[doc(hidden)]
    pub fn from_raw(value: u32) -> Option<Self> {
        NonZeroU32::new(value).map(Self)
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
    generation: u64,
    identities: Vec<GeneralRowIdentity>,
    x: GeneralXColumn,
    y: Vec<f64>,
    y_valid: Option<Vec<u8>>,
    low: Option<Vec<f64>>,
    low_valid: Option<Vec<u8>>,
    size: Option<Vec<f64>>,
    size_valid: Option<Vec<u8>>,
    labels: HashMap<usize, String>,
}

impl GeneralDataset {
    pub fn id(&self) -> GeneralDatasetId {
        self.id
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
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

    pub fn size(&self) -> Option<&[f64]> {
        self.size.as_deref()
    }

    pub fn size_is_valid(&self, index: usize) -> bool {
        self.size
            .as_ref()
            .is_some_and(|values| index < values.len())
            && self
                .size_valid
                .as_ref()
                .is_none_or(|validity| validity[index] != 0)
    }

    pub fn low(&self) -> Option<&[f64]> {
        self.low.as_deref()
    }

    pub fn low_is_valid(&self, index: usize) -> bool {
        self.low.as_ref().is_some_and(|values| index < values.len())
            && self
                .low_valid
                .as_ref()
                .is_none_or(|validity| validity[index] != 0)
    }

    pub fn row_label(&self, index: usize) -> Option<&str> {
        self.labels.get(&index).map(String::as_str)
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
            + self
                .low
                .as_ref()
                .map_or(0, |values| values.capacity() * std::mem::size_of::<f64>())
            + self.low_valid.as_ref().map_or(0, Vec::capacity)
            + self
                .size
                .as_ref()
                .map_or(0, |values| values.capacity() * std::mem::size_of::<f64>())
            + self.size_valid.as_ref().map_or(0, Vec::capacity)
            + self.labels.capacity()
                * (std::mem::size_of::<usize>()
                    + std::mem::size_of::<String>()
                    + std::mem::size_of::<usize>())
            + self.labels.values().map(String::capacity).sum::<usize>()
    }
}

struct ValidatedGeneralXy {
    ids: Option<Vec<GeneralRowId>>,
    x: GeneralXColumn,
    y: Vec<f64>,
    y_valid: Option<Vec<u8>>,
    low: Option<Vec<f64>>,
    low_valid: Option<Vec<u8>>,
    size: Option<Vec<f64>>,
    size_valid: Option<Vec<u8>>,
}

impl GeneralXyInput {
    pub(crate) fn x_kind(&self) -> GeneralXKind {
        match self {
            Self::Numeric { .. } | Self::Bubble { .. } | Self::RangeNumeric { .. } => {
                GeneralXKind::Numeric
            }
            Self::Temporal { .. } | Self::RangeTemporal { .. } => GeneralXKind::Temporal,
            Self::Category { .. } | Self::RangeCategory { .. } => GeneralXKind::Category,
        }
    }

    pub(crate) fn numeric_x_values(&self) -> Option<&[f64]> {
        match self {
            Self::Numeric { x, .. } | Self::Bubble { x, .. } | Self::RangeNumeric { x, .. } => {
                Some(x)
            }
            _ => None,
        }
    }

    pub(crate) fn y_values(&self) -> &[f64] {
        match self {
            Self::Numeric { y, .. }
            | Self::Bubble { y, .. }
            | Self::Temporal { y, .. }
            | Self::Category { y, .. } => y,
            Self::RangeNumeric { high, .. }
            | Self::RangeTemporal { high, .. }
            | Self::RangeCategory { high, .. } => high,
        }
    }

    pub(crate) fn y_valid_values(&self) -> Option<&[u8]> {
        match self {
            Self::Numeric { y_valid, .. }
            | Self::Bubble { y_valid, .. }
            | Self::Temporal { y_valid, .. }
            | Self::Category { y_valid, .. } => y_valid.as_deref(),
            Self::RangeNumeric { high_valid, .. }
            | Self::RangeTemporal { high_valid, .. }
            | Self::RangeCategory { high_valid, .. } => high_valid.as_deref(),
        }
    }

    pub(crate) fn size_values(&self) -> Option<&[f64]> {
        match self {
            Self::Bubble { size, .. } => Some(size),
            _ => None,
        }
    }

    pub(crate) fn low_values(&self) -> Option<&[f64]> {
        match self {
            Self::RangeNumeric { low, .. }
            | Self::RangeTemporal { low, .. }
            | Self::RangeCategory { low, .. } => Some(low),
            _ => None,
        }
    }

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
                    low: None,
                    low_valid: None,
                    size: None,
                    size_valid: None,
                })
            }
            Self::Bubble {
                ids,
                x,
                y,
                y_valid,
                size,
                size_valid,
            } => {
                validate_row_count(x.len())?;
                validate_common(x.len(), ids.as_deref(), &y, y_valid.as_deref())?;
                if x.iter().any(|value| !value.is_finite()) {
                    return Err(invalid_data("general numeric X values must be finite"));
                }
                validate_size_channel(x.len(), &size, size_valid.as_deref())?;
                Ok(ValidatedGeneralXy {
                    ids,
                    x: GeneralXColumn::Numeric(x),
                    y,
                    y_valid: normalize_validity(y_valid),
                    low: None,
                    low_valid: None,
                    size: Some(size),
                    size_valid: normalize_validity(size_valid),
                })
            }
            Self::RangeNumeric {
                ids,
                x,
                low,
                low_valid,
                high,
                high_valid,
            } => {
                validate_row_count(x.len())?;
                validate_common(x.len(), ids.as_deref(), &high, high_valid.as_deref())?;
                if x.iter().any(|value| !value.is_finite()) {
                    return Err(invalid_data("general numeric X values must be finite"));
                }
                validate_range_channel(
                    x.len(),
                    &low,
                    low_valid.as_deref(),
                    &high,
                    high_valid.as_deref(),
                )?;
                Ok(ValidatedGeneralXy {
                    ids,
                    x: GeneralXColumn::Numeric(x),
                    y: high,
                    y_valid: normalize_validity(high_valid),
                    low: Some(low),
                    low_valid: normalize_validity(low_valid),
                    size: None,
                    size_valid: None,
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
                    low: None,
                    low_valid: None,
                    size: None,
                    size_valid: None,
                })
            }
            Self::RangeTemporal {
                ids,
                x_epoch_ms,
                low,
                low_valid,
                high,
                high_valid,
            } => {
                validate_row_count(x_epoch_ms.len())?;
                validate_common(
                    x_epoch_ms.len(),
                    ids.as_deref(),
                    &high,
                    high_valid.as_deref(),
                )?;
                if x_epoch_ms
                    .iter()
                    .any(|value| value.unsigned_abs() > MAX_GENERAL_TEMPORAL_MILLISECONDS as u64)
                {
                    return Err(invalid_data(format!(
                        "general temporal X values must stay within +/-{MAX_GENERAL_TEMPORAL_MILLISECONDS} epoch milliseconds"
                    )));
                }
                validate_range_channel(
                    x_epoch_ms.len(),
                    &low,
                    low_valid.as_deref(),
                    &high,
                    high_valid.as_deref(),
                )?;
                Ok(ValidatedGeneralXy {
                    ids,
                    x: GeneralXColumn::Temporal(x_epoch_ms),
                    y: high,
                    y_valid: normalize_validity(high_valid),
                    low: Some(low),
                    low_valid: normalize_validity(low_valid),
                    size: None,
                    size_valid: None,
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
                    low: None,
                    low_valid: None,
                    size: None,
                    size_valid: None,
                })
            }
            Self::RangeCategory {
                ids,
                categories,
                category_indices,
                low,
                low_valid,
                high,
                high_valid,
            } => {
                validate_row_count(category_indices.len())?;
                validate_common(
                    category_indices.len(),
                    ids.as_deref(),
                    &high,
                    high_valid.as_deref(),
                )?;
                validate_categories(&categories, &category_indices)?;
                validate_range_channel(
                    category_indices.len(),
                    &low,
                    low_valid.as_deref(),
                    &high,
                    high_valid.as_deref(),
                )?;
                Ok(ValidatedGeneralXy {
                    ids,
                    x: GeneralXColumn::Category {
                        categories,
                        indices: category_indices,
                    },
                    y: high,
                    y_valid: normalize_validity(high_valid),
                    low: Some(low),
                    low_valid: normalize_validity(low_valid),
                    size: None,
                    size_valid: None,
                })
            }
        }
    }
}

fn validate_range_channel(
    row_count: usize,
    low: &[f64],
    low_valid: Option<&[u8]>,
    high: &[f64],
    high_valid: Option<&[u8]>,
) -> Result<(), ChartError> {
    if low.len() != row_count {
        return Err(invalid_data(
            "general range low and high columns must have equal lengths",
        ));
    }
    if low.iter().any(|value| !value.is_finite()) {
        return Err(invalid_data(
            "general range low values must be finite; use the validity column for missing values",
        ));
    }
    if let Some(validity) = low_valid {
        if validity.len() != row_count {
            return Err(invalid_data(
                "general range low validity and value columns must have equal lengths",
            ));
        }
        if validity.iter().any(|value| !matches!(*value, 0 | 1)) {
            return Err(invalid_data(
                "general range low validity values must be 0 or 1",
            ));
        }
    }
    for row in 0..row_count {
        let low_present = low_valid.is_none_or(|validity| validity[row] != 0);
        let high_present = high_valid.is_none_or(|validity| validity[row] != 0);
        if low_present && high_present && low[row] > high[row] {
            return Err(invalid_data(
                "general range rows require low to be less than or equal to high",
            ));
        }
    }
    Ok(())
}

fn validate_size_channel(
    row_count: usize,
    size: &[f64],
    size_valid: Option<&[u8]>,
) -> Result<(), ChartError> {
    if size.len() != row_count {
        return Err(invalid_data(
            "general bubble size and value columns must have equal lengths",
        ));
    }
    if let Some(validity) = size_valid {
        if validity.len() != row_count {
            return Err(invalid_data(
                "general bubble size validity and value columns must have equal lengths",
            ));
        }
        if validity.iter().any(|value| !matches!(*value, 0 | 1)) {
            return Err(invalid_data(
                "general bubble size validity values must be 0 or 1",
            ));
        }
    }
    if size.iter().any(|value| !value.is_finite() || *value < 0.0) {
        return Err(invalid_data(
            "general bubble sizes must be finite and non-negative; use the validity column for missing values",
        ));
    }
    Ok(())
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
            GeneralRowId::Generated => continue,
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

fn validate_label_input(
    labels: Option<&[Option<String>]>,
    row_count: usize,
) -> Result<(), ChartError> {
    let Some(labels) = labels else {
        return Ok(());
    };
    if labels.len() != row_count {
        return Err(invalid_data(
            "general row labels and value columns must have equal lengths",
        ));
    }
    let mut count = 0usize;
    let mut bytes = 0usize;
    for label in labels.iter().flatten() {
        if label.len() > MAX_GENERAL_ROW_LABEL_BYTES {
            return Err(resource(format!(
                "general row label exceeds {MAX_GENERAL_ROW_LABEL_BYTES} UTF-8 bytes"
            )));
        }
        count += 1;
        bytes = bytes
            .checked_add(label.len())
            .ok_or_else(|| resource("general row label byte count overflow"))?;
    }
    if count > MAX_GENERAL_ROW_LABELS || bytes > MAX_GENERAL_ROW_LABEL_BYTES_TOTAL {
        return Err(resource(
            "general row labels exceed their count or UTF-8 byte limit",
        ));
    }
    Ok(())
}

fn validate_label_map(labels: &HashMap<usize, String>) -> Result<(), ChartError> {
    if labels.len() > MAX_GENERAL_ROW_LABELS
        || labels.values().map(String::len).sum::<usize>() > MAX_GENERAL_ROW_LABEL_BYTES_TOTAL
    {
        return Err(resource(
            "general row labels exceed their count or UTF-8 byte limit",
        ));
    }
    Ok(())
}

fn labels_for_rows(
    labels: Option<Vec<Option<String>>>,
    row_count: usize,
) -> Result<HashMap<usize, String>, ChartError> {
    validate_label_input(labels.as_deref(), row_count)?;
    Ok(labels
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(row, label)| label.map(|label| (row, label)))
        .collect())
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

    pub(crate) fn iter(&self) -> impl Iterator<Item = &GeneralDataset> {
        self.datasets.iter()
    }

    pub(crate) fn insert(&mut self, input: GeneralXyInput) -> Result<GeneralDatasetId, ChartError> {
        self.insert_labeled(input, None)
    }

    pub(crate) fn insert_labeled(
        &mut self,
        input: GeneralXyInput,
        labels: Option<Vec<Option<String>>>,
    ) -> Result<GeneralDatasetId, ChartError> {
        if self.datasets.len() >= MAX_GENERAL_DATASETS {
            return Err(resource(format!(
                "a chart supports at most {MAX_GENERAL_DATASETS} general datasets"
            )));
        }
        let validated = input.validate()?;
        let labels = labels_for_rows(labels, validated.y.len())?;
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
            generation: 1,
            identities,
            x: validated.x,
            y: validated.y,
            y_valid: validated.y_valid,
            low: validated.low,
            low_valid: validated.low_valid,
            size: validated.size,
            size_valid: validated.size_valid,
            labels,
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
        self.replace_labeled(id, input, None)
    }

    pub(crate) fn replace_labeled(
        &mut self,
        id: GeneralDatasetId,
        input: GeneralXyInput,
        labels: Option<Vec<Option<String>>>,
    ) -> Result<(), ChartError> {
        let Some(slot) = self.datasets.iter().position(|dataset| dataset.id == id) else {
            return Err(ChartError::new(
                ErrorCode::InvalidHandle,
                "general dataset handle is stale",
            ));
        };
        let validated = input.validate()?;
        let labels = labels_for_rows(labels, validated.y.len())?;
        let generation = self.datasets[slot]
            .generation
            .checked_add(1)
            .ok_or_else(|| resource("general dataset generation is exhausted"))?;
        let (identities, next_generated_row_id) =
            identities_for(validated.ids, validated.y.len(), self.next_generated_row_id)?;
        self.datasets[slot] = GeneralDataset {
            id,
            generation,
            identities,
            x: validated.x,
            y: validated.y,
            y_valid: validated.y_valid,
            low: validated.low,
            low_valid: validated.low_valid,
            size: validated.size,
            size_valid: validated.size_valid,
            labels,
        };
        self.next_generated_row_id = next_generated_row_id;
        Ok(())
    }

    pub(crate) fn upsert(
        &mut self,
        id: GeneralDatasetId,
        input: GeneralXyInput,
        max_rows: Option<usize>,
    ) -> Result<usize, ChartError> {
        self.upsert_labeled(id, input, None, max_rows)
    }

    pub(crate) fn upsert_labeled(
        &mut self,
        id: GeneralDatasetId,
        input: GeneralXyInput,
        labels: Option<Vec<Option<String>>>,
        max_rows: Option<usize>,
    ) -> Result<usize, ChartError> {
        let Some(slot) = self.datasets.iter().position(|dataset| dataset.id == id) else {
            return Err(ChartError::new(
                ErrorCode::InvalidHandle,
                "general dataset handle is stale",
            ));
        };
        if max_rows == Some(0) || max_rows.is_some_and(|limit| limit > MAX_GENERAL_DATASET_ROWS) {
            return Err(invalid_data(format!(
                "general retention must be between 1 and {MAX_GENERAL_DATASET_ROWS} rows"
            )));
        }
        let validated = input.validate()?;
        validate_label_input(labels.as_deref(), validated.y.len())?;
        let Some(ids) = validated.ids.as_ref() else {
            return Err(invalid_data(
                "general incremental updates require explicit row IDs",
            ));
        };
        if ids.iter().any(|id| matches!(id, GeneralRowId::Generated)) {
            return Err(invalid_data(
                "general incremental updates require explicit row IDs",
            ));
        }
        let dataset = &self.datasets[slot];
        if dataset.x_kind()
            != match &validated.x {
                GeneralXColumn::Numeric(_) => GeneralXKind::Numeric,
                GeneralXColumn::Temporal(_) => GeneralXKind::Temporal,
                GeneralXColumn::Category { .. } => GeneralXKind::Category,
            }
        {
            return Err(invalid_data(
                "general incremental X columns must match the dataset kind",
            ));
        }
        if dataset.size.is_some() != validated.size.is_some() {
            return Err(invalid_data(
                "general incremental size columns must match the dataset channel shape",
            ));
        }
        if dataset.low.is_some() != validated.low.is_some() {
            return Err(invalid_data(
                "general incremental range columns must match the dataset channel shape",
            ));
        }
        let existing: HashMap<GeneralRowId, usize> = dataset
            .identities
            .iter()
            .enumerate()
            .filter_map(|(row, identity)| match identity {
                GeneralRowIdentity::Explicit(id) => Some((id.clone(), row)),
                GeneralRowIdentity::Generated(_) => None,
            })
            .collect();
        let appended = ids.iter().filter(|id| !existing.contains_key(*id)).count();
        let untrimmed_len = dataset
            .len()
            .checked_add(appended)
            .ok_or_else(|| resource("general dataset row count overflow"))?;
        if max_rows.is_none() && untrimmed_len > MAX_GENERAL_DATASET_ROWS {
            return Err(resource(format!(
                "general dataset exceeds {MAX_GENERAL_DATASET_ROWS} rows"
            )));
        }
        let trim_count = max_rows.map_or(0, |limit| untrimmed_len.saturating_sub(limit));

        let mut next_labels = dataset.labels.clone();
        let mut next_new_row = dataset.len();
        for (source_row, id) in ids.iter().enumerate() {
            let target_row = if let Some(&row) = existing.get(id) {
                row
            } else {
                let row = next_new_row;
                next_new_row += 1;
                row
            };
            next_labels.remove(&target_row);
            if let Some(label) = labels.as_ref().and_then(|items| items[source_row].as_ref()) {
                next_labels.insert(target_row, label.clone());
            }
        }
        if trim_count > 0 {
            next_labels = next_labels
                .into_iter()
                .filter_map(|(row, label)| row.checked_sub(trim_count).map(|row| (row, label)))
                .collect();
        }
        validate_label_map(&next_labels)?;
        let bounded_label_capacity = next_labels.len().saturating_mul(2).max(64);
        if next_labels.capacity() > bounded_label_capacity {
            next_labels.shrink_to(bounded_label_capacity);
        }

        let (category_registry, category_remap, old_category_remap) =
            match (&dataset.x, &validated.x) {
                (
                    GeneralXColumn::Category {
                        categories: current,
                        indices: current_indices,
                    },
                    GeneralXColumn::Category {
                        categories: incoming,
                        indices: incoming_indices,
                    },
                ) => {
                    let mut registry = current.clone();
                    let mut lookup: HashMap<String, u32> = registry
                        .iter()
                        .enumerate()
                        .map(|(index, category)| (category.clone(), index as u32))
                        .collect();
                    let mut remap = Vec::with_capacity(incoming.len());
                    for category in incoming {
                        let index =
                            if let Some(&index) = lookup.get(category) {
                                index
                            } else {
                                let index = u32::try_from(registry.len()).map_err(|_| {
                            resource("general category registry exceeds its index representation")
                        })?;
                                lookup.insert(category.clone(), index);
                                registry.push(category.clone());
                                index
                            };
                        remap.push(index);
                    }
                    let mut old_remap = Vec::new();
                    if max_rows.is_some() {
                        let mut projected = current_indices.clone();
                        for (source_row, id) in ids.iter().enumerate() {
                            let category = remap[incoming_indices[source_row] as usize];
                            if let Some(&row) = existing.get(id) {
                                projected[row] = category;
                            } else {
                                projected.push(category);
                            }
                        }
                        let mut used = vec![false; registry.len()];
                        for &index in projected.iter().skip(trim_count) {
                            used[index as usize] = true;
                        }
                        let mut final_remap = vec![0u32; registry.len()];
                        let mut retained =
                            Vec::with_capacity(used.iter().filter(|&&value| value).count());
                        for (index, category) in registry.drain(..).enumerate() {
                            if used[index] {
                                final_remap[index] = retained.len() as u32;
                                retained.push(category);
                            }
                        }
                        old_remap = final_remap[..current.len()].to_vec();
                        for index in &mut remap {
                            *index = final_remap[*index as usize];
                        }
                        registry = retained;
                    }
                    validate_categories(&registry, &[])?;
                    (Some(registry), remap, old_remap)
                }
                _ => (None, Vec::new(), Vec::new()),
            };
        let generation = dataset
            .generation
            .checked_add(1)
            .ok_or_else(|| resource("general dataset generation is exhausted"))?;
        let dataset = &mut self.datasets[slot];
        if let (
            GeneralXColumn::Category {
                categories,
                indices,
            },
            Some(category_registry),
        ) = (&mut dataset.x, category_registry)
        {
            for index in indices {
                *index = old_category_remap
                    .get(*index as usize)
                    .copied()
                    .unwrap_or(*index);
            }
            *categories = category_registry;
        }

        for (source_row, id) in ids.iter().enumerate() {
            let target_row = if let Some(&row) = existing.get(id) {
                row
            } else {
                dataset
                    .identities
                    .push(GeneralRowIdentity::Explicit(id.clone()));
                match &mut dataset.x {
                    GeneralXColumn::Numeric(values) => values.push(0.0),
                    GeneralXColumn::Temporal(values) => values.push(0),
                    GeneralXColumn::Category { indices, .. } => indices.push(0),
                }
                dataset.y.push(0.0);
                if let Some(validity) = dataset.y_valid.as_mut() {
                    validity.push(1);
                }
                if let Some(size) = dataset.size.as_mut() {
                    size.push(0.0);
                }
                if let Some(validity) = dataset.size_valid.as_mut() {
                    validity.push(1);
                }
                if let Some(low) = dataset.low.as_mut() {
                    low.push(0.0);
                }
                if let Some(validity) = dataset.low_valid.as_mut() {
                    validity.push(1);
                }
                dataset.len() - 1
            };
            match (&mut dataset.x, &validated.x) {
                (GeneralXColumn::Numeric(target), GeneralXColumn::Numeric(source)) => {
                    target[target_row] = source[source_row];
                }
                (GeneralXColumn::Temporal(target), GeneralXColumn::Temporal(source)) => {
                    target[target_row] = source[source_row];
                }
                (
                    GeneralXColumn::Category {
                        indices: target, ..
                    },
                    GeneralXColumn::Category {
                        indices: source, ..
                    },
                ) => {
                    target[target_row] = category_remap[source[source_row] as usize];
                }
                _ => unreachable!("general X kinds were validated before mutation"),
            }
            dataset.y[target_row] = validated.y[source_row];
            let valid = validated
                .y_valid
                .as_ref()
                .is_none_or(|values| values[source_row] != 0);
            if !valid && dataset.y_valid.is_none() {
                dataset.y_valid = Some(vec![1; dataset.len()]);
            }
            if let Some(validity) = dataset.y_valid.as_mut() {
                validity[target_row] = u8::from(valid);
            }
            if let (Some(target), Some(source)) = (dataset.size.as_mut(), validated.size.as_ref()) {
                target[target_row] = source[source_row];
                let valid = validated
                    .size_valid
                    .as_ref()
                    .is_none_or(|values| values[source_row] != 0);
                if !valid && dataset.size_valid.is_none() {
                    dataset.size_valid = Some(vec![1; dataset.len()]);
                }
                if let Some(validity) = dataset.size_valid.as_mut() {
                    validity[target_row] = u8::from(valid);
                }
            }
            if let (Some(target), Some(source)) = (dataset.low.as_mut(), validated.low.as_ref()) {
                target[target_row] = source[source_row];
                let valid = validated
                    .low_valid
                    .as_ref()
                    .is_none_or(|values| values[source_row] != 0);
                if !valid && dataset.low_valid.is_none() {
                    dataset.low_valid = Some(vec![1; dataset.len()]);
                }
                if let Some(validity) = dataset.low_valid.as_mut() {
                    validity[target_row] = u8::from(valid);
                }
            }
        }
        let mut removed_front = 0;
        if let Some(limit) = max_rows {
            let trim = dataset.len().saturating_sub(limit);
            if trim > 0 {
                removed_front = trim;
                dataset.identities.drain(..trim);
                match &mut dataset.x {
                    GeneralXColumn::Numeric(values) => {
                        values.drain(..trim);
                    }
                    GeneralXColumn::Temporal(values) => {
                        values.drain(..trim);
                    }
                    GeneralXColumn::Category { indices, .. } => {
                        indices.drain(..trim);
                    }
                }
                dataset.y.drain(..trim);
                if let Some(validity) = dataset.y_valid.as_mut() {
                    validity.drain(..trim);
                }
                if let Some(size) = dataset.size.as_mut() {
                    size.drain(..trim);
                }
                if let Some(validity) = dataset.size_valid.as_mut() {
                    validity.drain(..trim);
                }
                if let Some(low) = dataset.low.as_mut() {
                    low.drain(..trim);
                }
                if let Some(validity) = dataset.low_valid.as_mut() {
                    validity.drain(..trim);
                }
                let bounded_capacity = dataset.len().saturating_mul(2).max(1024);
                if dataset.identities.capacity() > bounded_capacity {
                    dataset.identities.shrink_to(bounded_capacity);
                    dataset.y.shrink_to(bounded_capacity);
                    if let Some(validity) = dataset.y_valid.as_mut() {
                        validity.shrink_to(bounded_capacity);
                    }
                    if let Some(size) = dataset.size.as_mut() {
                        size.shrink_to(bounded_capacity);
                    }
                    if let Some(validity) = dataset.size_valid.as_mut() {
                        validity.shrink_to(bounded_capacity);
                    }
                    if let Some(low) = dataset.low.as_mut() {
                        low.shrink_to(bounded_capacity);
                    }
                    if let Some(validity) = dataset.low_valid.as_mut() {
                        validity.shrink_to(bounded_capacity);
                    }
                    match &mut dataset.x {
                        GeneralXColumn::Numeric(values) => values.shrink_to(bounded_capacity),
                        GeneralXColumn::Temporal(values) => values.shrink_to(bounded_capacity),
                        GeneralXColumn::Category { indices, .. } => {
                            indices.shrink_to(bounded_capacity)
                        }
                    }
                }
            }
        }
        if dataset
            .y_valid
            .as_ref()
            .is_some_and(|validity| !validity.contains(&0))
        {
            dataset.y_valid = None;
        }
        if dataset
            .size_valid
            .as_ref()
            .is_some_and(|validity| !validity.contains(&0))
        {
            dataset.size_valid = None;
        }
        if dataset
            .low_valid
            .as_ref()
            .is_some_and(|validity| !validity.contains(&0))
        {
            dataset.low_valid = None;
        }
        dataset.labels = next_labels;
        dataset.generation = generation;
        Ok(removed_front)
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
        let generated_count = ids
            .iter()
            .filter(|id| matches!(id, GeneralRowId::Generated))
            .count();
        let generated_count = u64::try_from(generated_count)
            .map_err(|_| resource("general generated row identity count overflow"))?;
        let end = next_generated_row_id
            .checked_add(generated_count)
            .ok_or_else(|| resource("general generated row identity space is exhausted"))?;
        let mut next = next_generated_row_id;
        let identities = ids
            .into_iter()
            .map(|id| match id {
                GeneralRowId::Generated => {
                    let identity = GeneralRowIdentity::Generated(next);
                    next += 1;
                    identity
                }
                explicit => GeneralRowIdentity::Explicit(explicit),
            })
            .collect();
        return Ok((identities, end));
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
    fn mixed_explicit_and_omitted_ids_generate_only_the_missing_rows() {
        let mut store = GeneralDataStore::new();
        let id = store
            .insert(GeneralXyInput::Numeric {
                ids: Some(vec![
                    GeneralRowId::Text("first".into()),
                    GeneralRowId::Generated,
                    GeneralRowId::Number(7.0),
                    GeneralRowId::Generated,
                ]),
                x: vec![1.0, 2.0, 3.0, 4.0],
                y: vec![10.0, 20.0, 30.0, 40.0],
                y_valid: None,
            })
            .unwrap();
        let dataset = store.get(id).unwrap();
        assert_eq!(
            dataset.row_identity(0),
            Some(&GeneralRowIdentity::Explicit(GeneralRowId::Text(
                "first".into()
            )))
        );
        assert_eq!(
            dataset.row_identity(1),
            Some(&GeneralRowIdentity::Generated(1))
        );
        assert_eq!(
            dataset.row_identity(2),
            Some(&GeneralRowIdentity::Explicit(GeneralRowId::Number(7.0)))
        );
        assert_eq!(
            dataset.row_identity(3),
            Some(&GeneralRowIdentity::Generated(2))
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

    #[test]
    fn explicit_id_upsert_updates_appends_and_trims_atomically() {
        let mut store = GeneralDataStore::new();
        let id = store
            .insert(GeneralXyInput::Numeric {
                ids: Some(vec![
                    GeneralRowId::Text("a".into()),
                    GeneralRowId::Text("b".into()),
                ]),
                x: vec![1.0, 2.0],
                y: vec![10.0, 20.0],
                y_valid: None,
            })
            .unwrap();
        store
            .upsert(
                id,
                GeneralXyInput::Numeric {
                    ids: Some(vec![
                        GeneralRowId::Text("b".into()),
                        GeneralRowId::Text("c".into()),
                    ]),
                    x: vec![22.0, 3.0],
                    y: vec![220.0, 30.0],
                    y_valid: Some(vec![0, 1]),
                },
                Some(2),
            )
            .unwrap();
        let dataset = store.get(id).unwrap();
        assert_eq!(dataset.numeric_x(), Some(&[22.0, 3.0][..]));
        assert_eq!(dataset.y(), &[220.0, 30.0]);
        assert!(!dataset.y_is_valid(0));
        assert!(dataset.y_is_valid(1));
        assert_eq!(
            dataset.row_identity(0),
            Some(&GeneralRowIdentity::Explicit(GeneralRowId::Text(
                "b".into()
            )))
        );
        assert_eq!(dataset.generation(), 2);

        let before = dataset.clone();
        let error = store
            .upsert(
                id,
                GeneralXyInput::Numeric {
                    ids: None,
                    x: vec![4.0],
                    y: vec![40.0],
                    y_valid: None,
                },
                None,
            )
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::InvalidData);
        assert_eq!(store.get(id), Some(&before));
    }

    #[test]
    fn category_upsert_merges_the_registry_and_remaps_rows() {
        let mut store = GeneralDataStore::new();
        let id = store
            .insert(GeneralXyInput::Category {
                ids: Some(vec![GeneralRowId::Number(1.0)]),
                categories: vec!["Jan".into()],
                category_indices: vec![0],
                y: vec![10.0],
                y_valid: None,
            })
            .unwrap();
        store
            .upsert(
                id,
                GeneralXyInput::Category {
                    ids: Some(vec![GeneralRowId::Number(1.0), GeneralRowId::Number(2.0)]),
                    categories: vec!["Feb".into(), "Jan".into()],
                    category_indices: vec![0, 1],
                    y: vec![11.0, 20.0],
                    y_valid: None,
                },
                None,
            )
            .unwrap();
        let dataset = store.get(id).unwrap();
        assert_eq!(dataset.categories().unwrap(), &["Jan", "Feb"]);
        assert_eq!(dataset.category_indices().unwrap(), &[1, 0]);
        assert_eq!(dataset.y(), &[11.0, 20.0]);
        assert_eq!(
            store
                .upsert(
                    id,
                    GeneralXyInput::Category {
                        ids: Some(vec![GeneralRowId::Number(3.0)]),
                        categories: vec!["Mar".into()],
                        category_indices: vec![0],
                        y: vec![30.0],
                        y_valid: None,
                    },
                    Some(2),
                )
                .unwrap(),
            1
        );
        let dataset = store.get(id).unwrap();
        assert_eq!(dataset.categories().unwrap(), &["Jan", "Mar"]);
        assert_eq!(dataset.category_indices().unwrap(), &[0, 1]);
        assert_eq!(dataset.y(), &[20.0, 30.0]);
    }

    #[test]
    fn category_retention_validates_the_final_registry() {
        let mut store = GeneralDataStore::new();
        let id = store
            .insert(GeneralXyInput::Category {
                ids: Some(vec![GeneralRowId::Number(1.0)]),
                categories: vec!["a".repeat(600_000)],
                category_indices: vec![0],
                y: vec![1.0],
                y_valid: None,
            })
            .unwrap();
        let update = GeneralXyInput::Category {
            ids: Some(vec![GeneralRowId::Number(2.0)]),
            categories: vec!["b".repeat(600_000)],
            category_indices: vec![0],
            y: vec![2.0],
            y_valid: None,
        };
        let before = store.get(id).unwrap().clone();
        assert_eq!(
            store.upsert(id, update.clone(), None).unwrap_err().code(),
            ErrorCode::ResourceLimit
        );
        assert_eq!(store.get(id), Some(&before));
        assert_eq!(store.upsert(id, update, Some(1)).unwrap(), 1);
        let retained = store.get(id).unwrap();
        assert_eq!(retained.categories().unwrap().len(), 1);
        assert_eq!(retained.categories().unwrap()[0].len(), 600_000);
        assert_eq!(retained.y(), &[2.0]);
        store
            .upsert(
                id,
                GeneralXyInput::Category {
                    ids: Some(vec![GeneralRowId::Number(2.0)]),
                    categories: vec!["c".repeat(600_000)],
                    category_indices: vec![0],
                    y: vec![3.0],
                    y_valid: None,
                },
                Some(1),
            )
            .unwrap();
        let retained = store.get(id).unwrap();
        assert_eq!(retained.categories().unwrap().len(), 1);
        assert_eq!(retained.categories().unwrap()[0].as_bytes()[0], b'c');
        assert_eq!(retained.y(), &[3.0]);
    }

    #[test]
    fn custom_labels_follow_upsert_and_retention_atomically() {
        let mut store = GeneralDataStore::new();
        let id = store
            .insert_labeled(
                GeneralXyInput::Numeric {
                    ids: Some(vec![
                        GeneralRowId::Text("a".into()),
                        GeneralRowId::Text("b".into()),
                    ]),
                    x: vec![1.0, 2.0],
                    y: vec![10.0, 20.0],
                    y_valid: None,
                },
                Some(vec![Some("Alpha".into()), Some("Beta".into())]),
            )
            .unwrap();
        assert_eq!(store.get(id).unwrap().row_label(0), Some("Alpha"));
        store
            .upsert_labeled(
                id,
                GeneralXyInput::Numeric {
                    ids: Some(vec![
                        GeneralRowId::Text("b".into()),
                        GeneralRowId::Text("c".into()),
                    ]),
                    x: vec![2.0, 3.0],
                    y: vec![21.0, 30.0],
                    y_valid: None,
                },
                Some(vec![Some("Bravo".into()), Some("Charlie".into())]),
                Some(2),
            )
            .unwrap();
        let dataset = store.get(id).unwrap();
        assert_eq!(dataset.row_label(0), Some("Bravo"));
        assert_eq!(dataset.row_label(1), Some("Charlie"));
        let before = dataset.clone();
        let error = store
            .upsert_labeled(
                id,
                GeneralXyInput::Numeric {
                    ids: Some(vec![GeneralRowId::Text("c".into())]),
                    x: vec![4.0],
                    y: vec![40.0],
                    y_valid: None,
                },
                Some(vec![Some("x".repeat(MAX_GENERAL_ROW_LABEL_BYTES + 1))]),
                Some(2),
            )
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::ResourceLimit);
        assert_eq!(store.get(id), Some(&before));
    }

    #[test]
    fn combined_custom_label_budget_is_checked_before_upsert() {
        let mut store = GeneralDataStore::new();
        let id = store
            .insert_labeled(
                GeneralXyInput::Numeric {
                    ids: None,
                    x: (0..256).map(f64::from).collect(),
                    y: vec![1.0; 256],
                    y_valid: None,
                },
                Some(vec![Some("x".repeat(MAX_GENERAL_ROW_LABEL_BYTES)); 256]),
            )
            .unwrap();
        let before = store.get(id).unwrap().clone();
        let error = store
            .upsert_labeled(
                id,
                GeneralXyInput::Numeric {
                    ids: Some(vec![GeneralRowId::Number(1.0)]),
                    x: vec![256.0],
                    y: vec![2.0],
                    y_valid: None,
                },
                Some(vec![Some("y".into())]),
                None,
            )
            .unwrap_err();
        assert_eq!(error.code(), ErrorCode::ResourceLimit);
        assert_eq!(store.get(id), Some(&before));
    }
}
