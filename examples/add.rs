use cognet::{AddNode, Data, NodeGraph, NodeGraphAPI, NumberNode};

#[tokio::main]
async fn main() -> Result<(), String> {
    let mut graph = NodeGraph::new()?;

    let node1_id = graph.create_node::<NumberNode>().await?;
    graph.update_node_data(&node1_id, Data::new(10.)?).await?;
    let node2_id = graph.create_node::<NumberNode>().await?;
    graph.update_node_data(&node2_id, Data::new(20.)?).await?;
    let node3_id = graph.create_node::<AddNode>().await?;

    graph.connect_nodes(&node1_id, 0, &node3_id, 0).await?;
    graph.connect_nodes(&node2_id, 0, &node3_id, 0).await?;

    graph.execute().await?;

    let data = graph.get_output_value(&node3_id, 0).await.unwrap();
    let output = data.value::<f64>()?;
    assert_eq!(*output, 30.);

    Ok(())
}
