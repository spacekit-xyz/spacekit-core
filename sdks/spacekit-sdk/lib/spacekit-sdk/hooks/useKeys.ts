/**
 * useKeys - Hook for post-quantum encryption keys
 * 
 * @example
 * ```tsx
 * function EncryptionDemo() {
 *   const { hasKeys, generateKeys, encrypt, decrypt } = useKeys();
 *   
 *   const handleEncrypt = async () => {
 *     if (!hasKeys) {
 *       await generateKeys();
 *     }
 *     
 *     const data = new TextEncoder().encode('Secret message');
 *     const encrypted = await encrypt(data);
 *     console.log('Encrypted:', encrypted);
 *     
 *     const decrypted = await decrypt(encrypted);
 *     console.log('Decrypted:', new TextDecoder().decode(decrypted));
 *   };
 *   
 *   return (
 *     <div>
 *       <p>Keys: {hasKeys ? 'Ready' : 'Not generated'}</p>
 *       <button onClick={handleEncrypt}>Encrypt & Decrypt</button>
 *     </div>
 *   );
 * }
 * ```
 */

import { useSpacekit } from '../SpacekitProvider';

export function useKeys() {
  const { keys } = useSpacekit();
  return keys;
}
