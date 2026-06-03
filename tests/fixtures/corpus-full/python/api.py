from typing import Any, Dict, Optional
from services import UserService, ProductService, OrderService
from auth import AuthManager, require_auth
from utils import paginate, parse_json_safe


class ApiRouter:
    def __init__(
        self,
        user_svc: UserService,
        product_svc: ProductService,
        order_svc: OrderService,
        auth: AuthManager,
    ) -> None:
        self.user_svc = user_svc
        self.product_svc = product_svc
        self.order_svc = order_svc
        self.auth = auth

    def handle(self, method: str, path: str, body: Optional[str], token: Optional[str]) -> Dict[str, Any]:
        parts = path.strip("/").split("/")
        if parts[0] == "users":
            return self._handle_users(method, parts[1:], body, token)
        if parts[0] == "products":
            return self._handle_products(method, parts[1:], body, token)
        if parts[0] == "orders":
            return self._handle_orders(method, parts[1:], body, token)
        return {"error": "not found", "status": 404}

    def _handle_users(self, method, parts, body, token):
        if method == "POST" and not parts:
            data = parse_json_safe(body or "")
            if data is None:
                return {"error": "bad request", "status": 400}
            user = self.user_svc.register(data["username"], data["email"], data["password"])
            return {"id": user.id, "username": user.username, "status": 201}
        return {"error": "not found", "status": 404}

    def _handle_products(self, method, parts, body, token):
        if method == "GET" and not parts:
            products = self.product_svc.list_available()
            return {"products": [{"id": p.id, "name": p.name} for p in products], "status": 200}
        return {"error": "not found", "status": 404}

    def _handle_orders(self, method, parts, body, token):
        return {"error": "not implemented", "status": 501}
