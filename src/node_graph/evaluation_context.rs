use crate::{Data, Edge, EdgeId, OutputSlotId};
use std::collections::HashMap;

#[derive(Default, Debug)]
pub struct EvaluationContext {
    pub(crate) edges: HashMap<EdgeId, Edge>,
    pub(crate) outputs: HashMap<OutputSlotId, Data>,
}
