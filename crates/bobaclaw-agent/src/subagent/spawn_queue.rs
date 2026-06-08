#[derive(Debug, Clone)]
pub struct SpawnTaskRecord {
    pub id: String,
    pub label: Option<String>,
    pub status: String,
    pub result: Option<String>,
}
