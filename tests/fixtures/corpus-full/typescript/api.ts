import { UserService, ProductService, OrderService } from "./services";
import { AuthManager } from "./auth";
import { parseJsonSafe } from "./utils";

export class ApiRouter {
  constructor(
    private userService: UserService,
    private productService: ProductService,
    private orderService: OrderService,
    private auth: AuthManager,
  ) {}

  async handle(method: string, path: string, body?: string, token?: string): Promise<{ status: number; data: unknown }> {
    const parts = path.replace(/^\//, "").split("/");
    switch (parts[0]) {
      case "users":
        return this.handleUsers(method, parts.slice(1), body, token);
      case "products":
        return this.handleProducts(method, parts.slice(1), body, token);
      case "orders":
        return this.handleOrders(method, parts.slice(1), body, token);
      default:
        return { status: 404, data: { error: "not found" } };
    }
  }

  private async handleUsers(method: string, parts: string[], body?: string, token?: string) {
    if (method === "POST" && parts.length === 0) {
      const dto = parseJsonSafe<{ username: string; email: string; password: string }>(body ?? "");
      if (!dto) return { status: 400, data: { error: "bad request" } };
      const user = await this.userService.register(dto);
      return { status: 201, data: { id: user.id, username: user.username } };
    }
    return { status: 404, data: { error: "not found" } };
  }

  private async handleProducts(method: string, parts: string[], body?: string, token?: string) {
    return { status: 501, data: { error: "not implemented" } };
  }

  private async handleOrders(method: string, parts: string[], body?: string, token?: string) {
    return { status: 501, data: { error: "not implemented" } };
  }
}
