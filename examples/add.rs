use cognet::{AddListNode, NodeGraph, NodeGraphAPI, NumberNode};

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut graph = NodeGraph::new();

    let node1_id = graph.node_manager_mut().create_node::<NumberNode>().await?;
    graph
        .set_default_value::<NumberNode, f64>(&node1_id, 10.)
        .await?;
    let node2_id = graph.node_manager_mut().create_node::<NumberNode>().await?;
    graph
        .set_default_value::<NumberNode, f64>(&node2_id, 20.)
        .await?;
    let node3_id = graph
        .node_manager_mut()
        .create_node::<AddListNode>()
        .await?;

    graph.connect_nodes(&node1_id, 0, &node3_id, 0).await?;
    graph.connect_nodes(&node2_id, 0, &node3_id, 0).await?;

    graph.execute().await?;

    let data = graph.get_output_value(&node3_id, 0).await.unwrap();
    let output = data.value::<f64>().unwrap();
    assert_eq!(*output, 30.);

    Ok(())
}
