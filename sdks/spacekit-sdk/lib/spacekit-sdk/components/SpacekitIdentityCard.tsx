/**
 * SpacekitIdentityCard - Pre-built identity display component
 * 
 * Shows current identity with optional switch functionality.
 * 
 * @example
 * ```tsx
 * import { SpacekitIdentityCard } from './lib/spacekit-sdk';
 * 
 * function App() {
 *   return <SpacekitIdentityCard allowSwitch />;
 * }
 * ```
 */

import React, { useState } from 'react';
import { useSpacekit } from '../SpacekitProvider';

export interface SpacekitIdentityCardProps {
  /** Allow identity switching */
  allowSwitch?: boolean;
  /** Show full DID */
  showFullDid?: boolean;
  /** Custom class name */
  className?: string;
  /** Compact mode */
  compact?: boolean;
}

export function SpacekitIdentityCard({
  allowSwitch = false,
  showFullDid = false,
  className = '',
  compact = false,
}: SpacekitIdentityCardProps) {
  const { identity, balance } = useSpacekit();
  const [isEditing, setIsEditing] = useState(false);
  const [newName, setNewName] = useState('');

  const handleSwitch = () => {
    if (newName.trim()) {
      identity.setIdentity(newName.trim());
      setNewName('');
      setIsEditing(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleSwitch();
    } else if (e.key === 'Escape') {
      setIsEditing(false);
      setNewName('');
    }
  };

  const shortDid = identity.did.split(':').pop() || identity.did;

  if (compact) {
    return (
      <div className={`spacekit-identity-card spacekit-identity-card-compact ${className}`}>
        <span className="spacekit-identity-avatar">
          {identity.name.charAt(0).toUpperCase()}
        </span>
        <span className="spacekit-identity-name">{identity.name}</span>
      </div>
    );
  }

  return (
    <div className={`spacekit-identity-card ${className}`}>
      <div className="spacekit-identity-card-header">
        <div className="spacekit-identity-avatar-large">
          {identity.name.charAt(0).toUpperCase()}
        </div>
        <div className="spacekit-identity-info">
          {isEditing ? (
            <input
              type="text"
              className="spacekit-input"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Enter new name"
              autoFocus
            />
          ) : (
            <h3 className="spacekit-identity-name-large">{identity.name}</h3>
          )}
          <p className="spacekit-identity-did" title={identity.did}>
            {showFullDid ? identity.did : shortDid}
          </p>
        </div>
      </div>

      <div className="spacekit-identity-card-body">
        <div className="spacekit-identity-stat">
          <label>Balance</label>
          <strong>{balance.formatted} ASTRA</strong>
        </div>
        <div className="spacekit-identity-stat">
          <label>Network</label>
          <strong>Local</strong>
        </div>
      </div>

      {allowSwitch && (
        <div className="spacekit-identity-card-footer">
          {isEditing ? (
            <>
              <button
                className="spacekit-btn spacekit-btn-primary spacekit-btn-sm"
                onClick={handleSwitch}
                disabled={!newName.trim()}
              >
                Confirm
              </button>
              <button
                className="spacekit-btn spacekit-btn-secondary spacekit-btn-sm"
                onClick={() => {
                  setIsEditing(false);
                  setNewName('');
                }}
              >
                Cancel
              </button>
            </>
          ) : (
            <button
              className="spacekit-btn spacekit-btn-secondary spacekit-btn-sm"
              onClick={() => setIsEditing(true)}
            >
              Switch Identity
            </button>
          )}
        </div>
      )}
    </div>
  );
}
