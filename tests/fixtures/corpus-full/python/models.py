from dataclasses import dataclass, field
from typing import Optional, List
from datetime import datetime


@dataclass
class User:
    id: int
    username: str
    email: str
    created_at: datetime = field(default_factory=datetime.now)
    is_active: bool = True

    def display_name(self) -> str:
        return self.username

    def deactivate(self) -> None:
        self.is_active = False


@dataclass
class Product:
    id: int
    name: str
    price: float
    stock: int = 0
    category: Optional[str] = None

    def is_available(self) -> bool:
        return self.stock > 0

    def apply_discount(self, pct: float) -> float:
        return self.price * (1 - pct / 100)


@dataclass
class Order:
    id: int
    user: User
    items: List[Product] = field(default_factory=list)
    total: float = 0.0
    status: str = "pending"

    def add_item(self, product: Product, qty: int = 1) -> None:
        self.items.append(product)
        self.total += product.price * qty

    def complete(self) -> None:
        self.status = "completed"
