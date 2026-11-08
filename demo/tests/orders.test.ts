import { submitOrder } from '../src/orders/service';

describe('submitOrder', () => {
  it('charges and saves an eligible order', async () => {
    const result = await submitOrder(activeCustomerOrder);
    expect(result.accepted).toBe(true);
    expect(payments.charge).toHaveBeenCalledOnce();
    expect(orders.save).toHaveBeenCalledOnce();
  });
});
