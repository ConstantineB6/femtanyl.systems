use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Packet {
    pub id: String,
    pub color: String,
    pub x: f32,
    pub y: f32,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}
