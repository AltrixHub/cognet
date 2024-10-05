#[derive(Debug)]
pub struct InputSlot {
    pub id: String,
    pub index: usize,
    pub label: &'static str,
    pub value: String,
}

#[derive(Debug)]
pub struct OutputSlot {
    pub id: String,
    pub index: usize,
    pub label: &'static str,
    pub value: String,
}
