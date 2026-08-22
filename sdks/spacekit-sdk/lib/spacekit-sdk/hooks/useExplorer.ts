/**
 * useExplorer - Hook for block explorer data
 * 
 * @example
 * ```tsx
 * function ExplorerDisplay() {
 *   const { blocks, transactions, chainHeight, refresh } = useExplorer();
 *   
 *   return (
 *     <div>
 *       <p>Chain Height: {chainHeight}</p>
 *       <p>Blocks: {blocks.length}</p>
 *       <p>Transactions: {transactions.length}</p>
 *       <button onClick={refresh}>Refresh</button>
 *       
 *       {blocks.map(block => (
 *         <div key={block.blockHash}>
 *           Block #{block.height} - {block.transactions.length} txs
 *         </div>
 *       ))}
 *     </div>
 *   );
 * }
 * ```
 */

import { useSpacekit } from '../SpacekitProvider';

export function useExplorer() {
  const { explorer } = useSpacekit();
  return explorer;
}
