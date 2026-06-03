use crate::db::Database;
use crate::auth::AuthManager;
use crate::models::{User, Product, Order, OrderStatus};

pub struct UserService {
    db: Database,
    auth: AuthManager,
}

impl UserService {
    pub fn new(db: Database, auth: AuthManager) -> Self {
        Self { db, auth }
    }

    pub fn register(&self, username: &str, email: &str) -> User {
        let user = User::new(0, username, email);
        self.db.save_user(&user);
        user
    }

    pub fn get_profile(&self, token: &str) -> Option<User> {
        self.auth.get_user(token)
    }

    pub fn deactivate(&self, token: &str) -> bool {
        let mut user = self.auth.get_user(token)?;
        user.deactivate();
        self.db.save_user(&user);
        Some(()).is_some()
    }
}

pub struct ProductService {
    db: Database,
}

impl ProductService {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn get_by_id(&self, id: u64) -> Option<Product> {
        self.db.find_product(id)
    }

    pub fn restock(&self, id: u64, qty: u32) -> bool {
        let mut product = self.db.find_product(id)?;
        product.stock += qty;
        self.db.save_product(&product);
        Some(()).is_some()
    }
}

pub struct OrderService {
    db: Database,
    product_service: ProductService,
}

impl OrderService {
    pub fn new(db: Database, product_service: ProductService) -> Self {
        Self { db, product_service }
    }

    pub fn create(&self, user_id: u64) -> Order {
        let order = Order { id: 0, user_id, items: vec![], total: 0.0, status: OrderStatus::Pending };
        self.db.save_order(&order);
        order
    }
}
