use cognet::{AddListNode, Data, NodeGraph, NodeGraphAPI, NumberNode};

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut node_graph = NodeGraph::new();

    let node1_id = node_graph
        .node_manager_mut()
        .create_node::<NumberNode>()
        .await?;
    node_graph
        .set_default_value::<NumberNode>(&node1_id, Data::Number(10.))
        .await?;
    let node2_id = node_graph
        .node_manager_mut()
        .create_node::<NumberNode>()
        .await?;
    node_graph
        .set_default_value::<NumberNode>(&node2_id, Data::Number(20.))
        .await?;
    let node3_id = node_graph
        .node_manager_mut()
        .create_node::<AddListNode>()
        .await?;

    node_graph.connect_nodes(&node1_id, 0, &node3_id, 0).await?;
    node_graph.connect_nodes(&node2_id, 0, &node3_id, 0).await?;

    node_graph.execute().await?;

    let output = node_graph
        .get_output_value(&node3_id, 0)
        .await
        .unwrap()
        .value::<f32>()
        .unwrap();
    assert_eq!(output, 30.);

    Ok(())
}
