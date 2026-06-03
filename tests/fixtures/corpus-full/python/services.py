from typing import List, Optional
from models import User, Product, Order
from db import Database
from auth import AuthManager


class UserService:
    def __init__(self, db: Database, auth: AuthManager) -> None:
        self.db = db
        self.auth = auth

    def register(self, username: str, email: str, password: str) -> User:
        user = User(id=hash(username), username=username, email=email)
        self.db.save_user(user)
        return user

    def get_profile(self, token: str) -> Optional[User]:
        return self.auth.get_user(token)

    def deactivate_account(self, token: str) -> bool:
        user = self.auth.get_user(token)
        if user is None:
            return False
        user.deactivate()
        self.db.save_user(user)
        return True


class ProductService:
    def __init__(self, db: Database) -> None:
        self.db = db

    def list_available(self) -> List[Product]:
        return []

    def get_by_id(self, product_id: int) -> Optional[Product]:
        return self.db.find_product(product_id)

    def restock(self, product_id: int, qty: int) -> bool:
        product = self.db.find_product(product_id)
        if product is None:
            return False
        product.stock += qty
        self.db.save_product(product)
        return True


class OrderService:
    def __init__(self, db: Database, product_svc: ProductService) -> None:
        self.db = db
        self.product_svc = product_svc

    def create_order(self, user: User) -> Order:
        order = Order(id=hash(user.username), user=user)
        self.db.save_order(order)
        return order

    def add_item(self, order_id: int, product_id: int, qty: int = 1) -> bool:
        order = self.db.find_order(order_id)
        product = self.product_svc.get_by_id(product_id)
        if order is None or product is None:
            return False
        order.add_item(product, qty)
        self.db.save_order(order)
        return True
