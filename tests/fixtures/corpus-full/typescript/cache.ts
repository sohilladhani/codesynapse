export class CacheEntry<T> {
  readonly expiresAt: number;

  constructor(readonly value: T, ttl: number) {
    this.expiresAt = Date.now() + ttl * 1000;
  }

  isExpired(): boolean {
    return Date.now() > this.expiresAt;
  }
}

export class MemoryCache {
  private store = new Map<string, CacheEntry<unknown>>();

  constructor(private defaultTtl = 300) {}

  get<T>(key: string): T | undefined {
    const entry = this.store.get(key) as CacheEntry<T> | undefined;
    if (!entry || entry.isExpired()) return undefined;
    return entry.value;
  }

  set<T>(key: string, value: T, ttl?: number): void {
    this.store.set(key, new CacheEntry(value, ttl ?? this.defaultTtl));
  }

  delete(key: string): void {
    this.store.delete(key);
  }

  flush(): void {
    this.store.clear();
  }
}
