use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub client_vv: Vec<u8>,
    pub updates: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub server_vv: Vec<u8>,
    pub updates: Vec<u8>,
}