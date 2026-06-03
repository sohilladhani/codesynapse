use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::models::{User, Product, Order};

#[derive(Clone, Default)]
pub struct Database {
    store: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl Database {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn save_user(&self, user: &User) {
        let key = format!("user:{}", user.id);
        let mut store = self.store.write().unwrap();
        store.insert(key, user.username.as_bytes().to_vec());
    }

    pub fn find_user(&self, id: u64) -> Option<User> {
        let key = format!("user:{}", id);
        let store = self.store.read().unwrap();
        store.get(&key).map(|_| User::new(id, "cached", "cached@example.com"))
    }

    pub fn save_product(&self, product: &Product) {
        let key = format!("product:{}", product.id);
        let mut store = self.store.write().unwrap();
        store.insert(key, product.name.as_bytes().to_vec());
    }

    pub fn find_product(&self, id: u64) -> Option<Product> {
        let key = format!("product:{}", id);
        let store = self.store.read().unwrap();
        store.get(&key).map(|_| Product {
            id,
            name: "cached".into(),
            price: 0.0,
            stock: 0,
            category: None,
        })
    }

    pub fn save_order(&self, order: &Order) {
        let key = format!("order:{}", order.id);
        let mut store = self.store.write().unwrap();
        store.insert(key, vec![]);
    }
}
