use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ResourceRecord {
    pub id: String,
    pub r#type: String,
    pub label: String,
    pub vram_gb: u32,
    pub device_hint: String,
    pub max_concurrent_tasks: u32,
    pub owner: Option<String>,
    pub status: String,
}
