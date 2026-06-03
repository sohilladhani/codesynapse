import { User, Product, Order, CreateUserDto } from "./models";
import { Database } from "./db";
import { AuthManager } from "./auth";

export class UserService {
  constructor(private db: Database, private auth: AuthManager) {}

  async register(dto: CreateUserDto): Promise<User> {
    const user: User = {
      id: Date.now(),
      username: dto.username,
      email: dto.email,
      createdAt: new Date(),
      isActive: true,
    };
    await this.db.saveUser(user);
    return user;
  }

  async getProfile(token: string): Promise<User | undefined> {
    return this.auth.getUser(token);
  }

  async deactivate(token: string): Promise<boolean> {
    const user = await this.auth.getUser(token);
    if (!user) return false;
    user.isActive = false;
    await this.db.saveUser(user);
    return true;
  }
}

export class ProductService {
  constructor(private db: Database) {}

  async getById(id: number): Promise<Product | undefined> {
    return this.db.findProduct(id);
  }

  async restock(id: number, qty: number): Promise<boolean> {
    const p = await this.db.findProduct(id);
    if (!p) return false;
    p.stock += qty;
    await this.db.saveProduct(p);
    return true;
  }
}

export class OrderService {
  constructor(private db: Database, private productService: ProductService) {}

  async create(userId: number): Promise<Order> {
    const order: Order = { id: Date.now(), userId, items: [], total: 0, status: "pending" };
    await this.db.saveOrder(order);
    return order;
  }

  async addItem(orderId: number, productId: number, qty: number): Promise<boolean> {
    const [order, product] = await Promise.all([
      this.db.findOrder(orderId),
      this.productService.getById(productId),
    ]);
    if (!order || !product) return false;
    order.items.push({ productId, quantity: qty, price: product.price });
    order.total += product.price * qty;
    await this.db.saveOrder(order);
    return true;
  }
}
