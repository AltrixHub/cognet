use cognet::{AddListNode, Data, NodeGraph, NodeGraphAPI, NodeImpl, NodePrimitive, NumberNode};

fn main() {
    let mut node_graph = NodeGraph::new();

    let node1 = NumberNode::initialize();
    node1
        .set_default_value(&mut node_graph.context, Data::Number(10.))
        .unwrap();
    let node2 = NumberNode::initialize();
    node2
        .set_default_value(&mut node_graph.context, Data::Number(20.))
        .unwrap();
    let node3 = AddListNode::initialize();

    let node1_id = node_graph.add_node(node1);
    let node2_id = node_graph.add_node(node2);
    let node3_id = node_graph.add_node(node3);

    node_graph.connect_nodes(node1_id, 0, node3_id, 0).unwrap();
    node_graph.connect_nodes(node2_id, 0, node3_id, 0).unwrap();

    node_graph.execute().unwrap();

    let res = node_graph.get_output_value(node3_id, 0);
    assert_eq!(res.unwrap(), &Data::Number(30.));
}
