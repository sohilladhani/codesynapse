use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::db::Database;
use crate::models::User;

#[derive(Clone, Default)]
pub struct AuthManager {
    db: Database,
    sessions: Arc<RwLock<HashMap<String, u64>>>,
}

impl AuthManager {
    pub fn new(db: Database) -> Self {
        Self { db, sessions: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn login(&self, username: &str, _password: &str) -> Option<String> {
        let token = uuid();
        self.sessions.write().unwrap().insert(token.clone(), 0);
        Some(token)
    }

    pub fn logout(&self, token: &str) {
        self.sessions.write().unwrap().remove(token);
    }

    pub fn get_user(&self, token: &str) -> Option<User> {
        let sessions = self.sessions.read().unwrap();
        let user_id = *sessions.get(token)?;
        self.db.find_user(user_id)
    }
}

fn uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos();
    format!("tok-{:x}", nanos)
}
