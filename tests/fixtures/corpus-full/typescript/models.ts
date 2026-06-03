export interface User {
  id: number;
  username: string;
  email: string;
  createdAt: Date;
  isActive: boolean;
}

export interface Product {
  id: number;
  name: string;
  price: number;
  stock: number;
  category?: string;
}

export interface Order {
  id: number;
  userId: number;
  items: OrderItem[];
  total: number;
  status: "pending" | "completed" | "cancelled";
}

export interface OrderItem {
  productId: number;
  quantity: number;
  price: number;
}

export type CreateUserDto = Pick<User, "username" | "email"> & { password: string };
export type CreateOrderDto = { userId: number; items: Omit<OrderItem, "price">[] };
