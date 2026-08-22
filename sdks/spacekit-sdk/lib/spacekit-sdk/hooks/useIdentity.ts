/**
 * useIdentity - Hook for identity management
 * 
 * @example
 * ```tsx
 * function IdentityDisplay() {
 *   const { did, name, setIdentity } = useIdentity();
 *   
 *   return (
 *     <div>
 *       <p>Hello, {name}!</p>
 *       <p>DID: {did}</p>
 *       <button onClick={() => setIdentity('Bob')}>Switch to Bob</button>
 *     </div>
 *   );
 * }
 * ```
 */

import { useSpacekit } from '../SpacekitProvider';

export function useIdentity() {
  const { identity } = useSpacekit();
  return identity;
}
