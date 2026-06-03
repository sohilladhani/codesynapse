import os
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class DatabaseConfig:
    url: str = "sqlite:///app.db"
    pool_size: int = 5
    timeout: int = 30


@dataclass
class AuthConfig:
    secret_key: str = field(default_factory=lambda: os.environ.get("SECRET_KEY", "dev-secret"))
    token_ttl: int = 3600
    bcrypt_rounds: int = 12


@dataclass
class ServerConfig:
    host: str = "0.0.0.0"
    port: int = 8000
    debug: bool = False
    workers: int = 4


@dataclass
class AppConfig:
    db: DatabaseConfig = field(default_factory=DatabaseConfig)
    auth: AuthConfig = field(default_factory=AuthConfig)
    server: ServerConfig = field(default_factory=ServerConfig)
    environment: str = "development"

    @classmethod
    def from_env(cls) -> "AppConfig":
        return cls(
            environment=os.environ.get("APP_ENV", "development"),
        )

    def is_production(self) -> bool:
        return self.environment == "production"
