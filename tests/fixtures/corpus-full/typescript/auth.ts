import { User } from "./models";
import { Database } from "./db";

export class AuthManager {
  private sessions = new Map<string, number>();

  constructor(private db: Database, private secretKey: string) {}

  async login(username: string, password: string): Promise<string | null> {
    const users = await this.db.listUsers();
    const user = users.find((u) => u.username === username);
    if (!user) return null;
    const token = crypto.randomUUID();
    this.sessions.set(token, user.id);
    return token;
  }

  async logout(token: string): Promise<void> {
    this.sessions.delete(token);
  }

  async getUser(token: string): Promise<User | undefined> {
    const userId = this.sessions.get(token);
    if (userId === undefined) return undefined;
    return this.db.findUser(userId);
  }

  isValidToken(token: string): boolean {
    return this.sessions.has(token);
  }
}

export function withAuth<T>(fn: (user: User) => Promise<T>): (token: string, auth: AuthManager) => Promise<T | null> {
  return async (token, auth) => {
    const user = await auth.getUser(token);
    if (!user) return null;
    return fn(user);
  };
}
