from typing import Optional, List, Dict, Any
from models import User, Product, Order


class Database:
    def __init__(self, url: str) -> None:
        self.url = url
        self._store: Dict[str, Any] = {}

    def connect(self) -> bool:
        return True

    def disconnect(self) -> None:
        pass

    def find_user(self, user_id: int) -> Optional[User]:
        return self._store.get(f"user:{user_id}")

    def save_user(self, user: User) -> None:
        self._store[f"user:{user.id}"] = user

    def find_product(self, product_id: int) -> Optional[Product]:
        return self._store.get(f"product:{product_id}")

    def save_product(self, product: Product) -> None:
        self._store[f"product:{product.id}"] = product

    def find_order(self, order_id: int) -> Optional[Order]:
        return self._store.get(f"order:{order_id}")

    def save_order(self, order: Order) -> None:
        self._store[f"order:{order.id}"] = order

    def list_users(self) -> List[User]:
        return [v for k, v in self._store.items() if k.startswith("user:")]
