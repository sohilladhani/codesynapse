import time
import hashlib
from typing import Any, Optional, Dict


class CacheEntry:
    def __init__(self, value: Any, ttl: int) -> None:
        self.value = value
        self.expires_at = time.time() + ttl

    def is_expired(self) -> bool:
        return time.time() > self.expires_at


class MemoryCache:
    def __init__(self, default_ttl: int = 300) -> None:
        self.default_ttl = default_ttl
        self._store: Dict[str, CacheEntry] = {}

    def get(self, key: str) -> Optional[Any]:
        entry = self._store.get(key)
        if entry is None or entry.is_expired():
            return None
        return entry.value

    def set(self, key: str, value: Any, ttl: Optional[int] = None) -> None:
        self._store[key] = CacheEntry(value, ttl or self.default_ttl)

    def delete(self, key: str) -> None:
        self._store.pop(key, None)

    def flush(self) -> None:
        self._store.clear()

    def make_key(self, *parts: str) -> str:
        combined = ":".join(parts)
        return hashlib.md5(combined.encode()).hexdigest()
