import { defaultConfig, isProduction } from "./config";
import { Database } from "./db";
import { AuthManager } from "./auth";
import { UserService, ProductService, OrderService } from "./services";
import { ApiRouter } from "./api";
import { RateLimiter, RequestLogger, CorsMiddleware } from "./middleware";

async function main(): Promise<void> {
  const config = defaultConfig();
  const db = new Database();
  await db.connect();

  const auth = new AuthManager(db, config.auth.secretKey);
  const userService = new UserService(db, auth);
  const productService = new ProductService(db);
  const orderService = new OrderService(db, productService);
  const router = new ApiRouter(userService, productService, orderService, auth);

  const rateLimiter = new RateLimiter(100);
  const logger = new RequestLogger();
  const cors = new CorsMiddleware(["*"]);

  console.log(`Server running on ${config.server.host}:${config.server.port}`);
  if (isProduction(config)) {
    console.log("Running in production mode");
  }
}

main().catch(console.error);
