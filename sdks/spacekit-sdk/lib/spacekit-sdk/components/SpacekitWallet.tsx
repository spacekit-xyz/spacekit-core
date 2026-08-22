/**
 * SpacekitWallet - Pre-built wallet component
 * 
 * Displays balance, identity, and provides basic wallet actions.
 * 
 * @example
 * ```tsx
 * import { SpacekitWallet } from './lib/spacekit-sdk';
 * 
 * function App() {
 *   return <SpacekitWallet showActions />;
 * }
 * ```
 */

// React 18+ uses the new JSX transform, no explicit import needed
import { useSpacekit } from '../SpacekitProvider';

export interface SpacekitWalletProps {
  /** Show action buttons */
  showActions?: boolean;
  /** Custom class name */
  className?: string;
  /** Compact mode */
  compact?: boolean;
}

export function SpacekitWallet({
  showActions = false,
  className = '',
  compact = false,
}: SpacekitWalletProps) {
  const { identity, balance } = useSpacekit();

  if (compact) {
    return (
      <div className={`spacekit-wallet spacekit-wallet-compact ${className}`}>
        <span className="spacekit-wallet-balance">{balance.formatted} ASTRA</span>
        <span className="spacekit-wallet-identity">{identity.name}</span>
      </div>
    );
  }

  return (
    <div className={`spacekit-wallet ${className}`}>
      <div className="spacekit-wallet-header">
        <h3>Wallet</h3>
      </div>
      
      <div className="spacekit-wallet-content">
        <div className="spacekit-wallet-section">
          <label>Native ASTRA Balance</label>
          <div className="spacekit-wallet-balance-display">
            <span className="spacekit-wallet-balance-value">{balance.formatted}</span>
            <span className="spacekit-wallet-balance-unit">ASTRA</span>
          </div>
          <div className="spacekit-wallet-balance-micro">
            {balance.microAstra} µASTRA
          </div>
        </div>

        <div className="spacekit-wallet-section">
          <label>Identity</label>
          <div className="spacekit-wallet-identity-display">
            <span className="spacekit-wallet-identity-name">{identity.name}</span>
            <span className="spacekit-wallet-identity-did" title={identity.did}>
              {identity.did}
            </span>
          </div>
        </div>

        {showActions && (
          <div className="spacekit-wallet-actions">
            <button
              className="spacekit-btn spacekit-btn-secondary"
              onClick={balance.refresh}
              disabled={balance.isLoading}
            >
              {balance.isLoading ? 'Loading...' : 'Refresh'}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
