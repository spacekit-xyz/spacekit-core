/**
 * SpacekitExplorer - Pre-built block explorer component
 * 
 * Displays blocks, transactions, and receipts.
 * 
 * @example
 * ```tsx
 * import { SpacekitExplorer } from './lib/spacekit-sdk';
 * 
 * function App() {
 *   return <SpacekitExplorer maxBlocks={10} />;
 * }
 * ```
 */

import { useMemo, useState } from 'react';
import { useSpacekit } from '../SpacekitProvider';
import type { Transaction } from '../types';

export interface SpacekitExplorerProps {
  /** Maximum blocks to show */
  maxBlocks?: number;
  /** Maximum transactions to show */
  maxTransactions?: number;
  /** Show receipts section */
  showReceipts?: boolean;
  /** Custom class name */
  className?: string;
  /** Compact mode */
  compact?: boolean;
}

function formatTime(timestamp: number): string {
  return new Date(timestamp).toLocaleTimeString();
}

function truncateHash(hash: string, length: number = 8): string {
  if (hash.length <= length * 2) return hash;
  return `${hash.slice(0, length)}...${hash.slice(-length)}`;
}

export function SpacekitExplorer({
  maxBlocks = 10,
  maxTransactions = 10,
  showReceipts: _showReceipts = true,
  className = '',
  compact = false,
}: SpacekitExplorerProps) {
  const { explorer } = useSpacekit();
  const [selectedTx, setSelectedTx] = useState<Transaction | null>(null);

  const displayBlocks = useMemo(() => {
    return explorer.blocks.slice(0, maxBlocks);
  }, [explorer.blocks, maxBlocks]);

  const displayTxs = useMemo(() => {
    return explorer.transactions.slice(0, maxTransactions);
  }, [explorer.transactions, maxTransactions]);

  const selectedReceipt = useMemo(() => {
    if (!selectedTx) return null;
    return explorer.receipts.find((r) => r.txId === selectedTx.id) || null;
  }, [selectedTx, explorer.receipts]);

  if (compact) {
    return (
      <div className={`spacekit-explorer spacekit-explorer-compact ${className}`}>
        <span>Height: {explorer.chainHeight}</span>
        <span>Blocks: {explorer.blocks.length}</span>
        <span>Txs: {explorer.txCount}</span>
      </div>
    );
  }

  return (
    <div className={`spacekit-explorer ${className}`}>
      {/* Stats Header */}
      <div className="spacekit-explorer-stats">
        <div className="spacekit-explorer-stat">
          <label>Chain Height</label>
          <strong>{explorer.chainHeight}</strong>
        </div>
        <div className="spacekit-explorer-stat">
          <label>Blocks</label>
          <strong>{explorer.blocks.length}</strong>
        </div>
        <div className="spacekit-explorer-stat">
          <label>Transactions</label>
          <strong>{explorer.txCount}</strong>
        </div>
        <button
          className="spacekit-btn spacekit-btn-secondary spacekit-btn-sm"
          onClick={explorer.refresh}
          disabled={explorer.isLoading}
        >
          Refresh
        </button>
      </div>

      {/* Blocks Table */}
      <div className="spacekit-explorer-section">
        <h4>Recent Blocks</h4>
        <div className="spacekit-table">
          <div className="spacekit-table-header">
            <div>Height</div>
            <div>Txs</div>
            <div>Time</div>
            <div>Hash</div>
          </div>
          {displayBlocks.length === 0 ? (
            <div className="spacekit-table-empty">No blocks yet</div>
          ) : (
            displayBlocks.map((block) => (
              <div key={block.blockHash} className="spacekit-table-row">
                <div>{block.height}</div>
                <div>{block.transactions.length}</div>
                <div>{formatTime(block.timestamp)}</div>
                <div className="spacekit-hash" title={block.blockHash}>
                  {truncateHash(block.blockHash)}
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      {/* Transactions Table */}
      <div className="spacekit-explorer-section">
        <h4>Recent Transactions</h4>
        <div className="spacekit-table">
          <div className="spacekit-table-header">
            <div>Contract</div>
            <div>Caller</div>
            <div>Size</div>
            <div>Time</div>
          </div>
          {displayTxs.length === 0 ? (
            <div className="spacekit-table-empty">No transactions yet</div>
          ) : (
            displayTxs.map((tx) => (
              <button
                key={tx.id}
                type="button"
                className="spacekit-table-row spacekit-table-row-clickable"
                onClick={() => setSelectedTx(tx)}
              >
                <div>{tx.contractId}</div>
                <div className="spacekit-hash" title={tx.callerDid}>
                  {truncateHash(tx.callerDid, 12)}
                </div>
                <div>{tx.input.length} bytes</div>
                <div>{formatTime(tx.timestamp)}</div>
              </button>
            ))
          )}
        </div>
      </div>

      {/* Transaction Detail */}
      {selectedTx && (
        <div className="spacekit-explorer-section">
          <h4>Transaction Detail</h4>
          <div className="spacekit-detail-grid">
            <div>
              <label>Tx ID</label>
              <span className="spacekit-hash">{selectedTx.id}</span>
            </div>
            <div>
              <label>Contract</label>
              <span>{selectedTx.contractId}</span>
            </div>
            <div>
              <label>Caller</label>
              <span className="spacekit-hash">{selectedTx.callerDid}</span>
            </div>
            <div>
              <label>Value</label>
              <span>{selectedTx.value.toString()} ASTRA</span>
            </div>
            <div>
              <label>Input Size</label>
              <span>{selectedTx.input.length} bytes</span>
            </div>
            {selectedReceipt && (
              <>
                <div>
                  <label>Status</label>
                  <span>{selectedReceipt.status === 1 ? '✓ Success' : '✗ Failed'}</span>
                </div>
                <div>
                  <label>Gas Used</label>
                  <span>{selectedReceipt.gasUsed ?? 'N/A'}</span>
                </div>
              </>
            )}
          </div>
          <button
            className="spacekit-btn spacekit-btn-secondary spacekit-btn-sm"
            onClick={() => setSelectedTx(null)}
          >
            Close
          </button>
        </div>
      )}
    </div>
  );
}
