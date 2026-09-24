import type { AllowanceType } from 'types/client/revoke';

export default function formatAllowance(approval: AllowanceType) {
  if (!approval.allowance) return 'N/A';
  if (approval.allowance === 'Unlimited') return 'Unlimited';

  const allowance = parseFloat(approval.allowance);
  return Number(
    allowance >= 1 ? allowance.toFixed(2) : allowance.toPrecision(2),
  ).toString();
}
