use std::path::Path;

use loro::LoroDoc;

const STATE_FILE: &str = "meshlet-server.state";

pub struct ServerDoc {
    doc: LoroDoc,
}

impl ServerDoc {
    pub fn load_or_create(dir: &Path) -> Self {
        let state_path = dir.join(STATE_FILE);

        if let Ok(data) = std::fs::read(&state_path)
            && let Ok(doc) = LoroDoc::from_snapshot(&data)
        {
            return Self { doc };
        }

        let doc = LoroDoc::new();
        doc.set_record_timestamp(true);
        doc.set_peer_id(0).ok();
        Self { doc }
    }

    pub fn save(&self, dir: &Path) -> anyhow::Result<()> {
        let state_path = dir.join(STATE_FILE);
        let snapshot = self
            .doc
            .export(loro::ExportMode::Snapshot)
            .map_err(|e| anyhow::anyhow!("export failed: {}", e))?;

        std::fs::create_dir_all(dir)?;
        std::fs::write(&state_path, snapshot)?;
        tracing::info!("saved server state to {:?}", state_path);
        Ok(())
    }

    pub fn import(&self, data: &[u8]) -> anyhow::Result<()> {
        self.doc
            .import(data)
            .map_err(|e| anyhow::anyhow!("import failed: {}", e))?;
        Ok(())
    }

    pub fn export_updates_since(
        &self,
        vv: &loro::VersionVector,
    ) -> anyhow::Result<Vec<u8>> {
        self.doc
            .export(loro::ExportMode::Updates {
                from: std::borrow::Cow::Borrowed(vv),
            })
            .map_err(|e| anyhow::anyhow!("export failed: {}", e))
    }

    pub fn oplog_vv(&self) -> loro::VersionVector {
        self.doc.oplog_vv().clone()
    }
}