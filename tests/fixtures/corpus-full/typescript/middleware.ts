import { AuthManager } from "./auth";

export class RateLimiter {
  private counts = new Map<string, number>();

  constructor(private maxRequests = 100, private windowSecs = 60) {}

  isAllowed(clientId: string): boolean {
    const bucket = Math.floor(Date.now() / 1000 / this.windowSecs);
    const key = `${clientId}:${bucket}`;
    const count = this.counts.get(key) ?? 0;
    if (count >= this.maxRequests) return false;
    this.counts.set(key, count + 1);
    return true;
  }
}

export class RequestLogger {
  log(method: string, path: string, status: number, durationMs: number): void {
    console.log(`${method} ${path} -> ${status} (${durationMs.toFixed(1)}ms)`);
  }
}

export class CorsMiddleware {
  constructor(private allowedOrigins: string[]) {}

  isAllowed(origin: string): boolean {
    return this.allowedOrigins.includes("*") || this.allowedOrigins.includes(origin);
  }

  getHeaders(origin: string): Record<string, string> {
    if (!this.isAllowed(origin)) return {};
    return {
      "Access-Control-Allow-Origin": origin,
      "Access-Control-Allow-Methods": "GET, POST, PUT, DELETE, OPTIONS",
      "Access-Control-Allow-Headers": "Content-Type, Authorization",
    };
  }
}
