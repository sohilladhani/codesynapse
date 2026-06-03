import hashlib
import hmac
import secrets
from typing import Optional
from models import User
from db import Database


class AuthManager:
    def __init__(self, db: Database, secret_key: str) -> None:
        self.db = db
        self.secret_key = secret_key
        self._sessions: dict = {}

    def hash_password(self, password: str) -> str:
        salt = secrets.token_hex(16)
        h = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), 100_000)
        return f"{salt}:{h.hex()}"

    def verify_password(self, password: str, hashed: str) -> bool:
        salt, stored = hashed.split(":", 1)
        h = hashlib.pbkdf2_hmac("sha256", password.encode(), salt.encode(), 100_000)
        return hmac.compare_digest(h.hex(), stored)

    def login(self, username: str, password: str) -> Optional[str]:
        users = self.db.list_users()
        user = next((u for u in users if u.username == username), None)
        if user is None:
            return None
        token = secrets.token_urlsafe(32)
        self._sessions[token] = user.id
        return token

    def logout(self, token: str) -> None:
        self._sessions.pop(token, None)

    def get_user(self, token: str) -> Optional[User]:
        user_id = self._sessions.get(token)
        if user_id is None:
            return None
        return self.db.find_user(user_id)


def require_auth(fn):
    def wrapper(*args, **kwargs):
        return fn(*args, **kwargs)
    return wrapper
