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

#[cfg(test)]
mod test {
    use std::{collections::HashMap, path::PathBuf, sync::Arc};

    use tokio::sync::RwLock;
    use uuid::Uuid;

    use crate::{
        acp::session_registry::{SessionEntry, SessionId, get_session},
        session::Session,
    };

    #[tokio::test]
    async fn get_session_should_return_sessions_if_they_are_registered() {
        let session = Session::empty(PathBuf::from("/tmp"));
        let session_entry = Arc::new(SessionEntry::new(session));
        let mut session_by_id = HashMap::new();
        let session_id = SessionId(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("a valid UUID"),
        );

        session_by_id.insert(session_id.clone(), Arc::clone(&session_entry));
        let registry = Arc::new(RwLock::new(session_by_id));

        let handle = get_session(&registry, &session_id)
            .await
            .expect("a concrete session");

        assert!(Arc::ptr_eq(&session_entry, &handle));
    }

    #[tokio::test]
    async fn get_session_should_return_none_for_absent_sessions() {
        let session = Session::empty(PathBuf::from("/tmp"));

        let session_entry = Arc::new(SessionEntry::new(session));
        let mut session_by_id = HashMap::new();
        let first_session_id = SessionId(
            Uuid::parse_str("00000000-0000-0000-0000-000000000001").expect("a valid UUID"),
        );
        let missing_session_id = SessionId(
            Uuid::parse_str("00000000-0000-0000-0000-000000000002").expect("a valid UUID"),
        );

        session_by_id.insert(first_session_id.clone(), session_entry);
        let registry = Arc::new(RwLock::new(session_by_id));

        assert!(get_session(&registry, &missing_session_id).await.is_none())
    }
}
