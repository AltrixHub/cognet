use cognet::{AddNode, Data, NodeGraph, NodeGraphRead, NodeGraphWrite, NumberNode};

fn main() -> Result<(), String> {
    let mut graph = NodeGraph::new()?;

    let node1_id = graph.create_node::<NumberNode>()?;
    graph.update_node_data(&node1_id, Data::new(10.)?)?;
    let node2_id = graph.create_node::<NumberNode>()?;
    graph.update_node_data(&node2_id, Data::new(20.)?)?;
    let node3_id = graph.create_node::<AddNode>()?;

    graph.connect_nodes(&node1_id, 0, &node3_id, 0)?;
    graph.connect_nodes(&node2_id, 0, &node3_id, 0)?;

    graph.execute_sync()?;

    let data = graph.get_output_value(&node3_id, 0).unwrap();
    let output = data.value::<f64>()?;
    assert_eq!(*output, 30.);

    Ok(())
}
