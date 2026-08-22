/**
 * useBalance - Hook for balance management
 * 
 * @example
 * ```tsx
 * function BalanceDisplay() {
 *   const { formatted, microAstra, refresh, deductFee } = useBalance();
 *   
 *   return (
 *     <div>
 *       <p>{formatted} ASTRA</p>
 *       <p>{microAstra} µASTRA</p>
 *       <button onClick={refresh}>Refresh</button>
 *       <button onClick={() => deductFee(1000n)}>Spend 1000</button>
 *     </div>
 *   );
 * }
 * ```
 */

import { useSpacekit } from '../SpacekitProvider';

export function useBalance() {
  const { balance } = useSpacekit();
  return balance;
}
