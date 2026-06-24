use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncRequest {
    pub client_vv: serde_json::Value,
    pub updates_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResponse {
    pub server_vv: serde_json::Value,
    pub updates_base64: String,
}

impl SyncRequest {
    pub fn new(client_vv: &loro::VersionVector, updates: &[u8]) -> Self {
        Self {
            client_vv: serde_json::to_value(client_vv).unwrap_or_default(),
            updates_base64: base64::engine::general_purpose::STANDARD.encode(updates),
        }
    }

    pub fn updates(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.updates_base64)
    }

    pub fn client_vv(&self) -> Option<loro::VersionVector> {
        serde_json::from_value(self.client_vv.clone()).ok()
    }
}

impl SyncResponse {
    pub fn new(server_vv: &loro::VersionVector, updates: &[u8]) -> Self {
        Self {
            server_vv: serde_json::to_value(server_vv).unwrap_or_default(),
            updates_base64: base64::engine::general_purpose::STANDARD.encode(updates),
        }
    }

    pub fn updates(&self) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(&self.updates_base64)
    }

    pub fn server_vv(&self) -> Option<loro::VersionVector> {
        serde_json::from_value(self.server_vv.clone()).ok()
    }
}