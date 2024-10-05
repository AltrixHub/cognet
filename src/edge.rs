pub mod slot;
pub use slot::*;

#[derive(Debug)]
pub struct Edge {
    pub node_id: String,
    pub output_slots: Vec<OutputSlot>,
    pub input_slot: Vec<InputSlot>,
}
