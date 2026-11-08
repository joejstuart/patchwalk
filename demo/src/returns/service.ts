import { customers, returns } from '../dependencies';

export async function requestReturn(request) {
  const customer = await customers.find(request.customerId);
  const eligibility = validateReturn(request, customer);
  if (!eligibility.allowed) {
    return { accepted: false, reason: eligibility.reason };
  }
  return { accepted: true, return: await returns.save(request) };
}

function validateReturn(request, customer) {
  if (!customer.active) {
    return { allowed: false, reason: 'Customer is inactive' };
  }
  return { allowed: true };
}
