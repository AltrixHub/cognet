//! Elementwise list broadcast shared by the binary arithmetic operators.
//!
//! Each operator carries two forms of every operand: the scalar port and a
//! typed `List(Number)` alternative. Slot types are compared exactly, so a
//! list cannot be an overload of the scalar port — it has to be its own
//! slot, the same alternative-source shape used wherever a node accepts
//! one payload in two shapes. A wired `A[]` decides `A`; otherwise the
//! scalar `A` does.
//!
//! When at least one operand arrives as a list the operation is applied
//! once per element — scalars broadcast, and every list operand must share
//! the same length. The per-element results go to the list output; the
//! scalar output stays unwritten, and vice versa, so a consumer always
//! reads the shape it asked for.
//!
//! Iteration lives inside the node. The engine never auto-loops: one
//! execution per node, one value per wire.

use crate::{Data, DataType, ExecutionContext, ListElem, SlotDef};

/// Slot index of the per-element form of operand `A`; `B[]` follows it.
const LIST_SLOT_BASE: usize = 2;

/// The scalar form of one operand.
pub(crate) const fn number_operand(label: &'static str) -> SlotDef {
    SlotDef {
        label,
        data_type: DataType::Number,
        max_connections: Some(1),
        inspector_visible: true,
    }
}

/// The per-element form of one operand.
pub(crate) const fn list_operand(label: &'static str) -> SlotDef {
    SlotDef {
        label,
        data_type: DataType::List(ListElem::Number),
        max_connections: Some(1),
        inspector_visible: false,
    }
}

/// The scalar result.
pub(crate) const fn number_result(label: &'static str) -> SlotDef {
    SlotDef {
        label,
        data_type: DataType::Number,
        max_connections: None,
        inspector_visible: true,
    }
}

/// The per-element result.
pub(crate) const fn list_result(label: &'static str) -> SlotDef {
    SlotDef {
        label,
        data_type: DataType::List(ListElem::Number),
        max_connections: None,
        inspector_visible: true,
    }
}

/// One operand as connected: a scalar that broadcasts, or a list that maps.
enum Operand {
    Scalar(f64),
    List(Vec<f64>),
}

/// Reads operand `index`, preferring its per-element slot over its scalar
/// one. An unwired scalar reads as `0.0`, matching the operators' own
/// long-standing default.
fn read_operand(ctx: &ExecutionContext, index: usize) -> Operand {
    ctx.input_values
        .get(LIST_SLOT_BASE + index)
        .and_then(|values| values.first())
        .and_then(|data| data.value::<Vec<f64>>().ok().cloned())
        .map_or_else(
            || {
                Operand::Scalar(
                    ctx.input_values
                        .get(index)
                        .and_then(|v| v.first())
                        .and_then(|d| d.value::<f64>().ok().copied())
                        .unwrap_or(0.0),
                )
            },
            Operand::List,
        )
}

/// Applies `op` over the node's two operands, writing the scalar result to
/// slot 0 or the per-element results to slot 1 depending on how the
/// operands arrived.
pub(crate) fn apply(
    ctx: &ExecutionContext,
    node: &str,
    op: fn(f64, f64) -> f64,
) -> Result<(), String> {
    let operands = [read_operand(ctx, 0), read_operand(ctx, 1)];

    let length = operands.iter().find_map(|operand| match operand {
        Operand::List(values) => Some(values.len()),
        Operand::Scalar(_) => None,
    });
    let Some(length) = length else {
        let scalars = [&operands[0], &operands[1]].map(|operand| match operand {
            Operand::Scalar(value) => *value,
            Operand::List(_) => unreachable!("no list operand in scalar mode"),
        });
        return ctx
            .output_writer
            .set(0, Data::new(op(scalars[0], scalars[1]))?);
    };

    for (index, operand) in operands.iter().enumerate() {
        if let Operand::List(values) = operand {
            if values.len() != length {
                let label = if index == 0 { "A[]" } else { "B[]" };
                return Err(format!(
                    "{node}: list inputs must share one length; '{label}' has {} elements, \
                     expected {length}",
                    values.len()
                ));
            }
        }
    }

    let results = (0..length)
        .map(|k| {
            let scalars = [&operands[0], &operands[1]].map(|operand| match operand {
                Operand::Scalar(value) => *value,
                Operand::List(values) => values[k],
            });
            op(scalars[0], scalars[1])
        })
        .collect::<Vec<f64>>();
    ctx.output_writer.set(1, Data::from_list(results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AddNode, DivideNode, ExecutionCache, MultiplyNode, NodeId, NodeImpl, NodeMeta, NodePath,
        OutputSlotId, OutputWriter, SharedExecutionCache, SubtractNode,
    };

    /// Runs a node over raw slot vectors and returns the writer's cache
    /// alongside the scalar and list output slot ids, so a test can assert
    /// which of the two the node actually wrote.
    fn run(
        node: &dyn NodeImpl,
        inputs: Vec<Vec<Data>>,
    ) -> Result<(SharedExecutionCache, OutputSlotId, OutputSlotId), String> {
        let cache = SharedExecutionCache::new(ExecutionCache::default());
        let scalar = OutputSlotId::new();
        let list = OutputSlotId::new();
        let writer = OutputWriter::new(
            cache.share(),
            vec![
                (scalar, DataType::Number),
                (list, DataType::List(ListElem::Number)),
            ],
        );
        let ctx = ExecutionContext {
            path: NodePath::root().child(NodeId::new()),
            node_data: None,
            input_values: inputs,
            output_writer: writer,
        };
        node.execute_sync(ctx)?;
        Ok((cache, scalar, list))
    }

    fn num(value: f64) -> Vec<Data> {
        vec![Data::new(value).expect("number")]
    }

    fn list(values: &[f64]) -> Vec<Data> {
        vec![Data::from_list(values.to_vec())]
    }

    fn scalar_out(cache: &SharedExecutionCache, slot: &OutputSlotId) -> Option<f64> {
        cache
            .read()
            .expect("read")
            .get_output(slot)
            .and_then(|data| data.value::<f64>().ok().copied())
    }

    fn list_out(cache: &SharedExecutionCache, slot: &OutputSlotId) -> Option<Vec<f64>> {
        cache
            .read()
            .expect("read")
            .get_output(slot)
            .and_then(|data| data.value::<Vec<f64>>().ok().cloned())
    }

    /// With no list wired, the operators behave exactly as they always
    /// have: scalar slot written, list slot untouched.
    #[test]
    fn scalar_mode_writes_only_the_scalar_output() {
        let (cache, scalar, list_slot) =
            run(&AddNode, vec![num(2.0), num(3.0)]).expect("add executes");
        assert_eq!(scalar_out(&cache, &scalar), Some(5.0));
        assert_eq!(
            list_out(&cache, &list_slot),
            None,
            "the list output must stay unwritten in scalar mode",
        );
    }

    /// A wired per-element slot decides the operand, and the results leave
    /// on the list output — the scalar one stays unwritten so a consumer
    /// never silently reads one element of a mapped operation.
    #[test]
    fn a_wired_list_operand_maps_elementwise() {
        let (cache, scalar, list_slot) = run(
            &AddNode,
            vec![Vec::new(), num(10.0), list(&[1.0, 2.0, 3.0]), Vec::new()],
        )
        .expect("add executes");
        assert_eq!(list_out(&cache, &list_slot), Some(vec![11.0, 12.0, 13.0]));
        assert_eq!(
            scalar_out(&cache, &scalar),
            None,
            "the scalar output must stay unwritten in list mode",
        );
    }

    #[test]
    fn two_lists_of_equal_length_pair_up() {
        let (cache, _scalar, list_slot) = run(
            &SubtractNode,
            vec![
                Vec::new(),
                Vec::new(),
                list(&[10.0, 20.0]),
                list(&[1.0, 2.0]),
            ],
        )
        .expect("subtract executes");
        assert_eq!(list_out(&cache, &list_slot), Some(vec![9.0, 18.0]));
    }

    #[test]
    fn a_length_mismatch_is_a_typed_error() {
        let Err(err) = run(
            &MultiplyNode,
            vec![
                Vec::new(),
                Vec::new(),
                list(&[1.0, 2.0]),
                list(&[3.0, 4.0, 5.0]),
            ],
        ) else {
            panic!("mismatched lengths must error");
        };
        assert!(err.contains("must share one length"), "got: {err}");
        assert!(err.contains("'B[]' has 3 elements"), "got: {err}");
    }

    /// The scalar port is the fallback, not an override: wiring both leaves
    /// the list in charge, the same precedence every alternative-source
    /// slot uses.
    #[test]
    fn the_list_slot_wins_over_the_scalar_slot() {
        let (cache, _scalar, list_slot) = run(
            &AddNode,
            vec![num(100.0), num(1.0), list(&[1.0, 2.0]), Vec::new()],
        )
        .expect("add executes");
        assert_eq!(list_out(&cache, &list_slot), Some(vec![2.0, 3.0]));
    }

    /// An empty list maps to an empty list — no elements, no error.
    #[test]
    fn an_empty_list_maps_to_an_empty_list() {
        let (cache, _scalar, list_slot) =
            run(&AddNode, vec![Vec::new(), num(1.0), list(&[]), Vec::new()]).expect("add executes");
        assert_eq!(list_out(&cache, &list_slot), Some(Vec::new()));
    }

    /// `Divide`'s zero-divisor substitution has to apply per element too,
    /// or the scalar and list forms of one node would disagree.
    #[test]
    fn divide_substitutes_zero_per_element_as_it_does_for_a_scalar() {
        let (cache, scalar, _list) =
            run(&DivideNode, vec![num(6.0), num(0.0)]).expect("divide executes");
        assert_eq!(scalar_out(&cache, &scalar), Some(0.0));

        let (cache, _scalar, list_slot) = run(
            &DivideNode,
            vec![Vec::new(), Vec::new(), list(&[6.0, 8.0]), list(&[0.0, 2.0])],
        )
        .expect("divide executes");
        assert_eq!(list_out(&cache, &list_slot), Some(vec![0.0, 4.0]));
    }

    /// Every operator declares both forms of both operands, so the slot
    /// layout the helper assumes is the layout each node publishes.
    #[test]
    fn every_operator_declares_both_forms_of_both_operands() {
        for (inputs, outputs, name) in [
            (AddNode::INPUTS, AddNode::OUTPUTS, AddNode::NAME),
            (
                SubtractNode::INPUTS,
                SubtractNode::OUTPUTS,
                SubtractNode::NAME,
            ),
            (
                MultiplyNode::INPUTS,
                MultiplyNode::OUTPUTS,
                MultiplyNode::NAME,
            ),
            (DivideNode::INPUTS, DivideNode::OUTPUTS, DivideNode::NAME),
        ] {
            assert_eq!(inputs.len(), 4, "{name} inputs");
            assert_eq!(inputs[0].data_type, DataType::Number, "{name} A");
            assert_eq!(inputs[1].data_type, DataType::Number, "{name} B");
            assert_eq!(
                inputs[LIST_SLOT_BASE].data_type,
                DataType::List(ListElem::Number),
                "{name} A[]",
            );
            assert_eq!(
                inputs[LIST_SLOT_BASE + 1].data_type,
                DataType::List(ListElem::Number),
                "{name} B[]",
            );
            assert_eq!(outputs.len(), 2, "{name} outputs");
            assert_eq!(outputs[0].data_type, DataType::Number, "{name} scalar out");
            assert_eq!(
                outputs[1].data_type,
                DataType::List(ListElem::Number),
                "{name} list out",
            );
        }
    }
}
