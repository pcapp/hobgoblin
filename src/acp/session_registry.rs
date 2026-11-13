use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock, oneshot};
use uuid::Uuid;

use crate::session::Session;

pub type SessionHandle = Arc<SessionEntry>;
pub type SessionRegistry = Arc<RwLock<HashMap<SessionId, SessionHandle>>>;

pub async fn get_session(
    registry: &SessionRegistry,
    session_id: &SessionId,
) -> Option<SessionHandle> {
    let guard = registry.read().await;
    guard.get(session_id).cloned()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

pub struct SessionEntry {
    pub session: Mutex<Session>,
    pub active_turn_stop: Mutex<Option<oneshot::Sender<()>>>,
}

impl SessionEntry {
    pub fn new(session: Session) -> Self {
        Self {
            session: Mutex::new(session),
            active_turn_stop: Mutex::new(None),
        }
    }
}
