from config import AppConfig
from db import Database
from auth import AuthManager
from services import UserService, ProductService, OrderService
from api import ApiRouter
from middleware import RateLimiter, RequestLogger, CorsMiddleware


def create_app(config: AppConfig) -> ApiRouter:
    db = Database(config.db.url)
    db.connect()
    auth = AuthManager(db, config.auth.secret_key)
    user_svc = UserService(db, auth)
    product_svc = ProductService(db)
    order_svc = OrderService(db, product_svc)
    return ApiRouter(user_svc, product_svc, order_svc, auth)


def main() -> None:
    config = AppConfig.from_env()
    app = create_app(config)
    rate_limiter = RateLimiter(max_requests=100)
    logger = RequestLogger()
    cors = CorsMiddleware(allowed_origins=["*"])
    print(f"Server starting on {config.server.host}:{config.server.port}")


if __name__ == "__main__":
    main()
