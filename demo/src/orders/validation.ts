export type Order = {
  customerId: string;
  items: Array<{ sku: string; quantity: number }>;
};

export type Customer = {
  id: string;
  active: boolean;
};

// Order-specific business rules belong in this module.
export function hasItems(order: Order): boolean {
  return order.items.length > 0;
}
