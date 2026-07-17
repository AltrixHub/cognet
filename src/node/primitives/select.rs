use crate::{
    register_nodes, DataType, ExecutionContext, NodeCategory, NodeImpl, NodeMeta, SlotDef,
};

/// String multiplexer that forwards one of its connected `options` by index.
///
/// `options` is a **multi-connection** `String` port: each connected `String`
/// node is one option, ordered by cognet's stable delivery order (the order
/// the edges were authored). `index` is a single-connection `Number` port
/// giving the 0-based position to select. The output `value` is a `String`
/// and emits `options[index]` **verbatim** — the selected payload flows out
/// unchanged, never coerced.
///
/// The node emits **no output** (so the downstream input slot falls back to
/// its own default) and logs a warning when the selection is invalid:
///
/// * `options` is empty, or `index` is unwired,
/// * `index` is negative, non-integral, or out of range.
///
/// Selection is String-only: type-generic selection is deferred until a real
/// non-String consumer exists. Because both ports are `String`, cognet's
/// connection validation already guarantees every option is a `String`; the
/// node never needs to check for mixed types, clamp, coerce, or partially
/// emit.
#[derive(Debug)]
pub struct SelectNode;

impl NodeMeta for SelectNode {
    const NAME: &'static str = "Select";
    const CATEGORY: NodeCategory = NodeCategory::Control;
    const INPUTS: &'static [SlotDef] = &[
        // Multi-connection collector: one String option per connected node,
        // stable authoring order.
        SlotDef::new("options", DataType::String, None),
        // Single-connection selected index (0-based).
        SlotDef::new("index", DataType::Number, Some(1)),
    ];
    // Verbatim pass-through of the selected option's String payload.
    const OUTPUTS: &'static [SlotDef] = &[SlotDef::new("value", DataType::String, None)];
}

impl NodeImpl for SelectNode {
    fn execute_sync(&self, ctx: ExecutionContext) -> Result<(), String> {
        let options = ctx.input_values.first().cloned().unwrap_or_default();

        if options.is_empty() {
            tracing::warn!(
                target: "graph",
                path = %ctx.path,
                "[cognet] Select: options is empty; emitting no value",
            );
            return Ok(());
        }

        // Index port is single-connection; take its first (only) value.
        let Some(index_data) = ctx.input_values.get(1).and_then(|values| values.first()) else {
            tracing::warn!(
                target: "graph",
                path = %ctx.path,
                "[cognet] Select: index port is unwired; emitting no value",
            );
            return Ok(());
        };
        let index_value = *index_data.value::<f64>()?;

        let rounded = index_value.round();
        if rounded < 0.0 || (index_value - rounded).abs() > 1e-9 {
            tracing::warn!(
                target: "graph",
                path = %ctx.path,
                index = index_value,
                "[cognet] Select: index is negative or non-integral; emitting no value",
            );
            return Ok(());
        }

        let idx = rounded as usize;
        let Some(selected) = options.get(idx) else {
            tracing::warn!(
                target: "graph",
                path = %ctx.path,
                index = idx,
                option_count = options.len(),
                "[cognet] Select: index out of range; emitting no value",
            );
            return Ok(());
        };

        // Forward the selected payload verbatim; `share` clones the `Arc`
        // and preserves the option's concrete `DataType`.
        ctx.output_writer.set(0, selected.share())?;
        Ok(())
    }
}

register_nodes!(SelectNode);

#[cfg(test)]
mod tests {
    use crate::{Data, NodeGraph, NodeGraphWrite, NodePath};

    /// Build a graph with `labels.len()` String option nodes wired (in order)
    /// into a Select node's `options` port, plus a Number node feeding
    /// `index`. Returns the graph and the Select node's path so the caller
    /// can execute and read `value`.
    fn build_select(labels: &[&str], index: f64) -> (NodeGraph, NodePath) {
        let mut graph = NodeGraph::new().expect("create graph");

        let select_id = graph.create_node_by_name("Select").expect("create Select");

        for label in labels {
            let str_id = graph.create_node_by_name("String").expect("create String");
            graph
                .update_node_data(
                    &str_id,
                    Data::new((*label).to_string()).expect("string data"),
                )
                .expect("set string data");
            // options is input slot 0 (multi-connection, stable order).
            graph
                .connect_nodes(&str_id, 0, &select_id, 0)
                .expect("connect String -> Select.options");
        }

        let num_id = graph.create_node_by_name("Number").expect("create Number");
        graph
            .update_node_data(&num_id, Data::new(index).expect("number data"))
            .expect("set number data");
        // index is input slot 1 (single-connection).
        graph
            .connect_nodes(&num_id, 0, &select_id, 1)
            .expect("connect Number -> Select.index");

        let select_path = NodePath::root().child(select_id);
        (graph, select_path)
    }

    fn selected_string(graph: &mut NodeGraph, select_path: &NodePath) -> Option<String> {
        let result = graph.execute_sync().expect("execute_sync");
        result
            .node_outputs
            .get(select_path)
            .and_then(|outs| outs.first())
            .and_then(|o| o.as_ref())
            .map(|data| data.value::<String>().expect("String downcast").clone())
    }

    #[test]
    fn selects_each_option_by_index() {
        for (i, expected) in ["center", "inside", "outside"].iter().enumerate() {
            let (mut graph, path) = build_select(&["center", "inside", "outside"], i as f64);
            assert_eq!(
                selected_string(&mut graph, &path).as_deref(),
                Some(*expected),
                "index {i} must select {expected}",
            );
        }
    }

    #[test]
    fn out_of_range_index_emits_no_value() {
        let (mut graph, path) = build_select(&["center", "inside", "outside"], 3.0);
        assert_eq!(
            selected_string(&mut graph, &path),
            None,
            "an out-of-range index must produce no output (downstream default applies)",
        );
    }

    #[test]
    fn negative_index_emits_no_value() {
        let (mut graph, path) = build_select(&["center", "inside", "outside"], -1.0);
        assert_eq!(selected_string(&mut graph, &path), None);
    }

    #[test]
    fn non_integral_index_emits_no_value() {
        let (mut graph, path) = build_select(&["center", "inside", "outside"], 1.5);
        assert_eq!(selected_string(&mut graph, &path), None);
    }

    #[test]
    fn empty_options_emits_no_value() {
        let (mut graph, path) = build_select(&[], 0.0);
        assert_eq!(selected_string(&mut graph, &path), None);
    }

    #[test]
    fn unwired_index_emits_no_value() {
        let mut graph = NodeGraph::new().expect("create graph");
        let select_id = graph.create_node_by_name("Select").expect("create Select");
        let str_id = graph.create_node_by_name("String").expect("create String");
        graph
            .update_node_data(&str_id, Data::new("center".to_string()).expect("data"))
            .expect("set data");
        graph
            .connect_nodes(&str_id, 0, &select_id, 0)
            .expect("connect option");
        // index port left unwired.
        let path = NodePath::root().child(select_id);
        assert_eq!(
            selected_string(&mut graph, &path),
            None,
            "an unwired index port must produce no output",
        );
    }

    #[test]
    fn delivery_order_matches_edge_authoring_order() {
        // Options are delivered in the order their edges were authored, so
        // each index resolves to the correspondingly-positioned option — a
        // deterministic, stable mapping (not sorted or hash-ordered).
        let labels = ["alpha", "beta", "gamma"];
        for (i, expected) in labels.iter().enumerate() {
            let (mut graph, path) = build_select(&labels, i as f64);
            assert_eq!(
                selected_string(&mut graph, &path).as_deref(),
                Some(*expected),
                "index {i} must resolve to the {i}-th authored option",
            );
        }
    }
}
