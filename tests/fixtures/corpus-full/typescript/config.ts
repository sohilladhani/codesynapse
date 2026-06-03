export interface DatabaseConfig {
  url: string;
  poolSize: number;
  timeout: number;
}

export interface AuthConfig {
  secretKey: string;
  tokenTtl: number;
}

export interface ServerConfig {
  host: string;
  port: number;
  debug: boolean;
}

export interface AppConfig {
  db: DatabaseConfig;
  auth: AuthConfig;
  server: ServerConfig;
  environment: "development" | "production" | "test";
}

export function defaultConfig(): AppConfig {
  return {
    db: { url: "sqlite://app.db", poolSize: 5, timeout: 30 },
    auth: { secretKey: process.env.SECRET_KEY ?? "dev-secret", tokenTtl: 3600 },
    server: { host: "0.0.0.0", port: 8000, debug: false },
    environment: (process.env.APP_ENV as AppConfig["environment"]) ?? "development",
  };
}

export function isProduction(config: AppConfig): boolean {
  return config.environment === "production";
}
