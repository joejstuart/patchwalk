import { submitOrder } from '../orders/service';

// Transport adapter: parses requests and translates service results.
export async function createOrder(request, response) {
  const order = request.body;
  const result = await submitOrder(order);
  return response.status(201).json(result.order);
}
