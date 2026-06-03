import { User, Product, Order } from "./models";

export class Database {
  private store = new Map<string, unknown>();

  async connect(): Promise<void> {}

  async disconnect(): Promise<void> {}

  async findUser(id: number): Promise<User | undefined> {
    return this.store.get(`user:${id}`) as User | undefined;
  }

  async saveUser(user: User): Promise<void> {
    this.store.set(`user:${user.id}`, user);
  }

  async findProduct(id: number): Promise<Product | undefined> {
    return this.store.get(`product:${id}`) as Product | undefined;
  }

  async saveProduct(product: Product): Promise<void> {
    this.store.set(`product:${product.id}`, product);
  }

  async findOrder(id: number): Promise<Order | undefined> {
    return this.store.get(`order:${id}`) as Order | undefined;
  }

  async saveOrder(order: Order): Promise<void> {
    this.store.set(`order:${order.id}`, order);
  }

  async listUsers(): Promise<User[]> {
    return Array.from(this.store.entries())
      .filter(([k]) => k.startsWith("user:"))
      .map(([, v]) => v as User);
  }
}
