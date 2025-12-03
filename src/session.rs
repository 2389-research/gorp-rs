// ABOUTME: Persistent session storage for Matrix room conversations using sled database.
// ABOUTME: Each room has a unique Claude session ID that survives bot restarts.
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub started: bool,
}

impl Session {
    pub fn cli_args(&self) -> Vec<&str> {
        if self.started {
            vec!["--resume", &self.session_id]
        } else {
            vec!["--session-id", &self.session_id]
        }
    }
}

#[derive(Clone)]
pub struct SessionStore {
    db: sled::Db,
}

impl SessionStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path)?;
        Ok(SessionStore { db })
    }

    pub fn get_or_create(&self, room_id: &str) -> Result<Session> {
        if let Some(data) = self.db.get(room_id)? {
            let session: Session = serde_json::from_slice(&data)?;
            Ok(session)
        } else {
            let session = Session {
                session_id: uuid::Uuid::new_v4().to_string(),
                started: false,
            };
            self.save(room_id, &session)?;
            Ok(session)
        }
    }

    pub fn mark_started(&self, room_id: &str) -> Result<()> {
        let mut session = self.get_or_create(room_id)?;
        session.started = true;
        self.save(room_id, &session)?;
        Ok(())
    }

    fn save(&self, room_id: &str, session: &Session) -> Result<()> {
        let data = serde_json::to_vec(session)?;
        self.db.insert(room_id, data)?;
        Ok(())
    }
}
