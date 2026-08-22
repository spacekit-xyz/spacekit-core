/**
 * useVm - Hook for VM operations
 * 
 * @example
 * ```tsx
 * function ContractDemo() {
 *   const { isReady, deployContract, submitAndMine, contracts } = useVm();
 *   
 *   const handleDeploy = async () => {
 *     const response = await fetch('/wasm/my_contract.wasm');
 *     const contractId = await deployContract(response, 'my-contract');
 *     console.log('Deployed:', contractId);
 *   };
 *   
 *   const handleExecute = async () => {
 *     const input = new TextEncoder().encode('hello');
 *     const result = await submitAndMine(contracts[0].id, input, 'Say Hello');
 *     console.log('Result:', result);
 *   };
 *   
 *   return (
 *     <div>
 *       <p>VM Ready: {isReady ? 'Yes' : 'No'}</p>
 *       <button onClick={handleDeploy}>Deploy Contract</button>
 *       <button onClick={handleExecute} disabled={contracts.length === 0}>
 *         Execute
 *       </button>
 *     </div>
 *   );
 * }
 * ```
 */

import { useSpacekit } from '../SpacekitProvider';

export function useVm() {
  const { vm } = useSpacekit();
  return vm;
}
