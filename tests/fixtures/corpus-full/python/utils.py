import re
import json
from typing import Any, Dict, List, Optional
from datetime import datetime


def slugify(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


def truncate(text: str, max_len: int = 100) -> str:
    return text[:max_len] + "..." if len(text) > max_len else text


def paginate(items: List[Any], page: int, per_page: int = 20) -> List[Any]:
    start = (page - 1) * per_page
    return items[start : start + per_page]


def parse_json_safe(raw: str) -> Optional[Dict]:
    try:
        return json.loads(raw)
    except (json.JSONDecodeError, TypeError):
        return None


def format_date(dt: datetime, fmt: str = "%Y-%m-%d") -> str:
    return dt.strftime(fmt)


def flatten(nested: List[List[Any]]) -> List[Any]:
    return [item for sublist in nested for item in sublist]


def chunk(items: List[Any], size: int) -> List[List[Any]]:
    return [items[i : i + size] for i in range(0, len(items), size)]


def deep_merge(base: Dict, override: Dict) -> Dict:
    result = dict(base)
    for k, v in override.items():
        if k in result and isinstance(result[k], dict) and isinstance(v, dict):
            result[k] = deep_merge(result[k], v)
        else:
            result[k] = v
    return result
