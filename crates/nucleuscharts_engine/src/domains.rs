use std::num::NonZeroU32;

use crate::{ChartError, ErrorCode};

/// General horizontal domains retained by one chart. Financial-time panes use the chart's
/// established shared time scale and consume no registry entry.
pub const MAX_GENERAL_HORIZONTAL_DOMAINS: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContinuousScaleType {
    #[default]
    Linear,
    Logarithmic,
    SymmetricLog,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CategoryScaleType {
    #[default]
    Band,
    Point,
}

/// Engine-owned horizontal coordinate semantics for one pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HorizontalDomain {
    /// Existing logical-index spacing over the chart-wide timestamp union.
    #[default]
    FinancialTime,
    Continuous {
        scale: ContinuousScaleType,
    },
    Temporal,
    Category {
        scale: CategoryScaleType,
    },
    Polar,
}

impl HorizontalDomain {
    pub fn is_financial_time(self) -> bool {
        self == Self::FinancialTime
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GeneralHorizontalDomainId(NonZeroU32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GeneralHorizontalDomainEntry {
    id: GeneralHorizontalDomainId,
    domain: HorizontalDomain,
}

/// Sparse, lazily allocated owner for non-financial horizontal-domain state.
///
/// IDs are chart-local and monotonic so moving panes never changes a binding and removing one
/// cannot retarget a stale internal reference. The live entry count has an explicit hard bound.
pub(crate) struct HorizontalDomainRegistry {
    entries: Vec<GeneralHorizontalDomainEntry>,
    next_id: u32,
}

impl Default for HorizontalDomainRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HorizontalDomainRegistry {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
        }
    }

    pub(crate) fn register(
        &mut self,
        domain: HorizontalDomain,
    ) -> Result<Option<GeneralHorizontalDomainId>, ChartError> {
        if domain.is_financial_time() {
            return Ok(None);
        }
        if self.entries.len() >= MAX_GENERAL_HORIZONTAL_DOMAINS {
            return Err(resource_limit());
        }
        let id = NonZeroU32::new(self.next_id)
            .map(GeneralHorizontalDomainId)
            .ok_or_else(resource_limit)?;
        self.next_id = self.next_id.checked_add(1).ok_or_else(resource_limit)?;
        self.entries
            .push(GeneralHorizontalDomainEntry { id, domain });
        Ok(Some(id))
    }

    pub(crate) fn resolve(
        &self,
        id: Option<GeneralHorizontalDomainId>,
    ) -> Option<HorizontalDomain> {
        let Some(id) = id else {
            return Some(HorizontalDomain::FinancialTime);
        };
        self.entries
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.domain)
    }

    pub(crate) fn remove(&mut self, id: Option<GeneralHorizontalDomainId>) {
        let Some(id) = id else {
            return;
        };
        if let Some(index) = self.entries.iter().position(|entry| entry.id == id) {
            self.entries.remove(index);
        }
    }

    pub(crate) fn capacity_bytes(&self) -> usize {
        self.entries.capacity() * std::mem::size_of::<GeneralHorizontalDomainEntry>()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

fn resource_limit() -> ChartError {
    ChartError::new(
        ErrorCode::ResourceLimit,
        format!("a chart supports at most {MAX_GENERAL_HORIZONTAL_DOMAINS} general domains"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn financial_time_uses_no_registry_entry() {
        let mut registry = HorizontalDomainRegistry::new();
        assert_eq!(registry.capacity_bytes(), 0);
        assert_eq!(registry.register(HorizontalDomain::FinancialTime), Ok(None));
        assert_eq!(registry.len(), 0);
        assert_eq!(registry.capacity_bytes(), 0);
    }

    #[test]
    fn general_entries_are_bounded_and_removed_ids_do_not_retarget() {
        let mut registry = HorizontalDomainRegistry::new();
        let first = registry
            .register(HorizontalDomain::Continuous {
                scale: ContinuousScaleType::Linear,
            })
            .unwrap();
        registry.remove(first);
        let second = registry
            .register(HorizontalDomain::Category {
                scale: CategoryScaleType::Band,
            })
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(registry.resolve(first), None);
        assert_eq!(
            registry.resolve(second),
            Some(HorizontalDomain::Category {
                scale: CategoryScaleType::Band
            })
        );

        while registry.len() < MAX_GENERAL_HORIZONTAL_DOMAINS {
            registry.register(HorizontalDomain::Temporal).unwrap();
        }
        assert_eq!(
            registry
                .register(HorizontalDomain::Polar)
                .unwrap_err()
                .code(),
            ErrorCode::ResourceLimit
        );
        assert_eq!(registry.len(), MAX_GENERAL_HORIZONTAL_DOMAINS);
    }
}
