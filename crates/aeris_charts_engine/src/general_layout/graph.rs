//! Directed relationship graphs: nodes with unique IDs and directed, weighted edges keyed by
//! their ordered `(source, target)` pair. Cycles are allowed. A self-link (`source == target`)
//! is allowed and stays one item; the chord layout draws it as a ribbon that leaves and
//! re-enters its own group.

use std::collections::{HashMap, HashSet};

use super::{
    GeneralLayoutAccessibilityItem, GeneralLayoutElement, GeneralLayoutHit, GraphEdge, GraphNode,
    MAX_FLOW_LINKS, add_text_bytes, invalid_data, resource_limit, validate_id, validate_rows,
    validate_value,
};
use crate::{ChartEngine, ChartError, GeneralLayoutSeriesId};

pub(super) fn validate(nodes: &[GraphNode], edges: &[GraphEdge]) -> Result<(), ChartError> {
    validate_rows(nodes.len())?;
    if edges.len() > MAX_FLOW_LINKS {
        return Err(resource_limit("too many graph edges"));
    }
    let mut ids = HashSet::with_capacity(nodes.len());
    let mut bytes = 0usize;
    for node in nodes {
        validate_id(&node.id, &mut ids)?;
        add_text_bytes(&mut bytes, &node.id)?;
        add_text_bytes(&mut bytes, &node.label)?;
        if let Some(category) = &node.category {
            add_text_bytes(&mut bytes, category)?;
        }
        if let Some(value) = node.value {
            validate_value(value)?;
        }
        match (node.x, node.y) {
            (None, None) => {}
            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => {}
            _ => {
                return Err(invalid_data(
                    "graph node x and y must both be finite or both be absent",
                ));
            }
        }
    }
    let mut pairs = HashSet::with_capacity(edges.len());
    for edge in edges {
        add_text_bytes(&mut bytes, &edge.source)?;
        add_text_bytes(&mut bytes, &edge.target)?;
        validate_value(edge.value)?;
        if !ids.contains(edge.source.as_str()) {
            return Err(invalid_data("graph edge source does not exist"));
        }
        if !ids.contains(edge.target.as_str()) {
            return Err(invalid_data("graph edge target does not exist"));
        }
        if !pairs.insert((edge.source.as_str(), edge.target.as_str())) {
            return Err(invalid_data("duplicate graph edge"));
        }
    }
    Ok(())
}

/// Input index of every node ID. Only called on validated graphs, whose IDs are unique.
pub(super) fn node_index(nodes: &[GraphNode]) -> HashMap<&str, usize> {
    nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect()
}

/// Incoming plus outgoing edge value of every node; a self-link counts once in each direction.
pub(super) fn flow_totals(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<f64> {
    let index = node_index(nodes);
    let mut totals = vec![0.0; nodes.len()];
    for edge in edges {
        if let (Some(&source), Some(&target)) = (
            index.get(edge.source.as_str()),
            index.get(edge.target.as_str()),
        ) {
            totals[source] += edge.value;
            totals[target] += edge.value;
        }
    }
    totals
}

/// Node values (the larger of the explicit `value` and the node's incoming plus outgoing flow,
/// which is the weight its chord group spans) followed by edge values.
pub(super) fn semantic_values(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<f64> {
    let totals = flow_totals(nodes, edges);
    let mut values = Vec::with_capacity(nodes.len() + edges.len());
    values.extend(
        nodes
            .iter()
            .zip(totals)
            .map(|(node, total)| node.value.unwrap_or(0.0).max(total)),
    );
    values.extend(edges.iter().map(|edge| edge.value));
    values
}

/// Color slot of every node: nodes sharing a category share the slot of its first node, and a
/// node without a category takes a slot of its own, all in input order.
pub(super) fn color_slots(nodes: &[GraphNode]) -> Vec<usize> {
    let mut categories = HashMap::new();
    let mut next = 0usize;
    nodes
        .iter()
        .map(|node| {
            let slot = match &node.category {
                Some(category) => *categories.entry(category.as_str()).or_insert(next),
                None => next,
            };
            if slot == next {
                next += 1;
            }
            slot
        })
        .collect()
}

fn label_of<'a>(nodes: &'a [GraphNode], id: &'a str) -> &'a str {
    nodes
        .iter()
        .find(|node| node.id == id)
        .map_or(id, |node| node.label.as_str())
}

impl ChartEngine {
    /// Node items are identified by their ID; edge items by `source→target`.
    pub(super) fn graph_item_identity(
        &self,
        nodes: &[GraphNode],
        edges: &[GraphEdge],
        element: GeneralLayoutElement,
        row: usize,
    ) -> Option<(String, String)> {
        match element {
            GeneralLayoutElement::Node => {
                let node = nodes.get(row)?;
                Some((node.id.clone(), node.label.clone()))
            }
            GeneralLayoutElement::Link => {
                let edge = edges.get(row)?;
                Some((
                    format!("{}→{}", edge.source, edge.target),
                    self.graph_link_label(nodes, edge),
                ))
            }
            _ => None,
        }
    }

    /// Readable name of an edge in the chart language.
    pub(super) fn graph_link_label(&self, nodes: &[GraphNode], edge: &GraphEdge) -> String {
        crate::text::complete::GRAPH_LINK.format(
            self.language,
            &[
                ("source", &label_of(nodes, &edge.source)),
                ("target", &label_of(nodes, &edge.target)),
            ],
        )
    }

    /// Nodes in input order followed by edges in input order.
    pub(super) fn graph_accessibility_items(
        &self,
        series: GeneralLayoutSeriesId,
        (nodes, edges): (&[GraphNode], &[GraphEdge]),
        values: &[f64],
        selected: Option<GeneralLayoutHit>,
        offset: usize,
        limit: usize,
    ) -> Vec<GeneralLayoutAccessibilityItem> {
        let total = nodes.len() + edges.len();
        (offset..offset.saturating_add(limit).min(total))
            .map(|index| {
                let (element, row, id, label) = if index < nodes.len() {
                    let node = &nodes[index];
                    (
                        GeneralLayoutElement::Node,
                        index,
                        node.id.clone(),
                        node.label.clone(),
                    )
                } else {
                    let row = index - nodes.len();
                    let edge = &edges[row];
                    (
                        GeneralLayoutElement::Link,
                        row,
                        format!("{}→{}", edge.source, edge.target),
                        self.graph_link_label(nodes, edge),
                    )
                };
                GeneralLayoutAccessibilityItem {
                    element,
                    row,
                    id,
                    label,
                    value: values.get(index).copied(),
                    selected: selected
                        == Some(GeneralLayoutHit {
                            series,
                            element,
                            row,
                        }),
                    expanded: None,
                    placed: None,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, category: Option<&str>) -> GraphNode {
        GraphNode {
            id: id.into(),
            label: id.to_uppercase(),
            category: category.map(Into::into),
            value: None,
            x: None,
            y: None,
        }
    }

    fn edge(source: &str, target: &str, value: f64) -> GraphEdge {
        GraphEdge {
            source: source.into(),
            target: target.into(),
            value,
        }
    }

    #[test]
    fn validation_allows_cycles_and_self_links_but_rejects_repeated_pairs() {
        let nodes = [node("a", None), node("b", None)];
        let cyclic = [
            edge("a", "b", 1.0),
            edge("b", "a", 2.0),
            edge("a", "a", 1.0),
        ];
        assert!(validate(&nodes, &cyclic).is_ok());
        assert!(validate(&nodes, &[edge("a", "b", 1.0), edge("a", "b", 3.0)]).is_err());
        assert!(validate(&nodes, &[edge("a", "c", 1.0)]).is_err());
        assert!(validate(&nodes, &[edge("a", "b", -1.0)]).is_err());
        assert!(validate(&nodes, &[edge("a", "b", f64::INFINITY)]).is_err());
        let mut half = node("c", None);
        half.x = Some(1.0);
        assert!(validate(&[half], &[]).is_err());
        assert!(validate(&[node("a", None), node("a", None)], &[]).is_err());
    }

    #[test]
    fn semantic_node_values_are_the_larger_of_value_and_flow() {
        let mut nodes = vec![node("a", None), node("b", None), node("c", None)];
        nodes[2].value = Some(9.0);
        nodes[1].value = Some(1.0);
        let edges = [
            edge("a", "b", 2.0),
            edge("b", "a", 3.0),
            edge("a", "a", 1.0),
        ];
        assert_eq!(
            semantic_values(&nodes, &edges),
            vec![7.0, 5.0, 9.0, 2.0, 3.0, 1.0]
        );
    }

    #[test]
    fn color_slots_group_categories_in_first_appearance_order() {
        let nodes = [
            node("a", Some("x")),
            node("b", None),
            node("c", Some("x")),
            node("d", Some("y")),
        ];
        assert_eq!(color_slots(&nodes), vec![0, 1, 0, 2]);
    }
}
