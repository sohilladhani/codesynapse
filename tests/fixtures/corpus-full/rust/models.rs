use std::time::SystemTime;

#[derive(Debug, Clone)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub email: String,
    pub created_at: SystemTime,
    pub is_active: bool,
}

impl User {
    pub fn new(id: u64, username: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            id,
            username: username.into(),
            email: email.into(),
            created_at: SystemTime::now(),
            is_active: true,
        }
    }

    pub fn display_name(&self) -> &str {
        &self.username
    }

    pub fn deactivate(&mut self) {
        self.is_active = false;
    }
}

#[derive(Debug, Clone)]
pub struct Product {
    pub id: u64,
    pub name: String,
    pub price: f64,
    pub stock: u32,
    pub category: Option<String>,
}

impl Product {
    pub fn is_available(&self) -> bool {
        self.stock > 0
    }

    pub fn apply_discount(&self, pct: f64) -> f64 {
        self.price * (1.0 - pct / 100.0)
    }
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: u64,
    pub user_id: u64,
    pub items: Vec<OrderItem>,
    pub total: f64,
    pub status: OrderStatus,
}

#[derive(Debug, Clone)]
pub struct OrderItem {
    pub product_id: u64,
    pub quantity: u32,
    pub price: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OrderStatus {
    Pending,
    Completed,
    Cancelled,
}
