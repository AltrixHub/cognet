use crate::{Edge, EdgeId, OutputSlotId, SharedData};
use std::collections::HashMap;

#[derive(Default, Debug)]
pub struct EvaluationContext {
    pub(crate) edges: HashMap<EdgeId, Edge>,
    pub(crate) outputs: HashMap<OutputSlotId, SharedData>,
}
