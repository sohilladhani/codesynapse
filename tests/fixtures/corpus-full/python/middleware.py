import time
import logging
from typing import Callable, Any
from auth import AuthManager

logger = logging.getLogger(__name__)


class RateLimiter:
    def __init__(self, max_requests: int = 100, window_secs: int = 60) -> None:
        self.max_requests = max_requests
        self.window_secs = window_secs
        self._counts: dict = {}

    def is_allowed(self, client_id: str) -> bool:
        now = time.time()
        bucket = int(now / self.window_secs)
        key = f"{client_id}:{bucket}"
        count = self._counts.get(key, 0)
        if count >= self.max_requests:
            return False
        self._counts[key] = count + 1
        return True


class RequestLogger:
    def __init__(self, level: int = logging.INFO) -> None:
        self.level = level

    def log(self, method: str, path: str, status: int, duration_ms: float) -> None:
        logger.log(self.level, "%s %s -> %d (%.1fms)", method, path, status, duration_ms)


class CorsMiddleware:
    def __init__(self, allowed_origins: list) -> None:
        self.allowed_origins = allowed_origins

    def is_allowed(self, origin: str) -> bool:
        return origin in self.allowed_origins or "*" in self.allowed_origins

    def get_headers(self, origin: str) -> dict:
        if not self.is_allowed(origin):
            return {}
        return {
            "Access-Control-Allow-Origin": origin,
            "Access-Control-Allow-Methods": "GET, POST, PUT, DELETE, OPTIONS",
            "Access-Control-Allow-Headers": "Content-Type, Authorization",
        }
