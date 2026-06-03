mod models;
mod db;
mod auth;
mod services;

use db::Database;
use auth::AuthManager;
use services::{UserService, ProductService, OrderService};

fn main() {
    let db = Database::new();
    let auth = AuthManager::new(db.clone());
    let user_svc = UserService::new(db.clone(), auth.clone());
    let product_svc = ProductService::new(db.clone());
    let order_svc = OrderService::new(db.clone(), product_svc);

    let user = user_svc.register("alice", "alice@example.com");
    println!("Registered: {}", user.display_name());
}
