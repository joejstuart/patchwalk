import { customers, payments, orders } from '../dependencies';

// Application service: coordinates every order submission path.
// HTTP routes and background jobs both call this function.
export async function submitOrder(order: Order) {
  const customer = await customers.find(order.customerId);

  const payment = await payments.charge(order);
  const saved = await orders.save({ ...order, paymentId: payment.id });
  return { accepted: true, order: saved };
}

export async function getOrder(id: string) {
  return orders.find(id);
}
