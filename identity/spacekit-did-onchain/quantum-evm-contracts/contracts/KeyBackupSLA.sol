// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

/**
 * @title KeyBackupSLA
 * @dev Service Level Agreement contract for encrypted PQ key backup storage.
 *
 * Architecture:
 *   - Encrypted key blobs live on SpaceKit Storage Node (off-chain)
 *   - This contract stores payment + metadata (hash, expiry, storage URI)
 *   - Users pay ASTRA/ETH to store; SLA guarantees retrieval
 *   - If storage node fails to serve the blob, user can claim refund after dispute window
 *
 * Flow:
 *   1. User encrypts keys with password (PBKDF2+AES-GCM, client-side)
 *   2. User uploads encrypted blob to storage node, gets a URI
 *   3. User calls `storeBackup(blobHash, storageUri)` with payment
 *   4. To retrieve: user authenticates via wallet → storage node returns blob
 *   5. User verifies `keccak256(blob) == blobHash` from contract
 *   6. If storage node can't produce blob, user files dispute → refund after timeout
 */
contract KeyBackupSLA {

    struct BackupRecord {
        address owner;
        bytes32 blobHash;          // keccak256 of the encrypted blob
        string storageUri;         // Storage node path (e.g. "documents/keybackup/{id}")
        uint256 paidAmount;        // Wei paid for storage
        uint256 expiresAt;         // Unix timestamp when SLA expires (0 = indefinite)
        uint256 createdAt;
        bool isActive;
        bool disputeFiled;
        uint256 disputeFiledAt;
    }

    uint256 public constant MIN_BACKUP_FEE = 0.001 ether;
    uint256 public constant DISPUTE_WINDOW = 7 days;
    uint256 public constant DEFAULT_SLA_DURATION = 365 days;

    mapping(address => BackupRecord) public backups;
    mapping(address => uint256) public backupCount;

    address public admin;
    uint256 public totalFeesCollected;

    event BackupStored(address indexed owner, bytes32 blobHash, string storageUri, uint256 paidAmount, uint256 expiresAt);
    event BackupUpdated(address indexed owner, bytes32 newBlobHash, string newStorageUri);
    event BackupDeleted(address indexed owner);
    event DisputeFiled(address indexed owner, uint256 filedAt);
    event DisputeResolved(address indexed owner, bool refunded);
    event FundsWithdrawn(address indexed admin, uint256 amount);

    error InsufficientPayment();
    error NoActiveBackup();
    error BackupAlreadyExists();
    error DisputeAlreadyFiled();
    error DisputeWindowNotPassed();
    error NotAdmin();
    error TransferFailed();

    modifier onlyAdmin() {
        if (msg.sender != admin) revert NotAdmin();
        _;
    }

    constructor() {
        admin = msg.sender;
    }

    /**
     * @dev Store a new backup record with payment
     * @param blobHash keccak256 hash of the encrypted blob (for integrity verification)
     * @param storageUri Path on the storage node where blob is stored
     */
    function storeBackup(bytes32 blobHash, string calldata storageUri) external payable {
        if (msg.value < MIN_BACKUP_FEE) revert InsufficientPayment();
        if (backups[msg.sender].isActive) revert BackupAlreadyExists();

        uint256 expiry = block.timestamp + DEFAULT_SLA_DURATION;

        backups[msg.sender] = BackupRecord({
            owner: msg.sender,
            blobHash: blobHash,
            storageUri: storageUri,
            paidAmount: msg.value,
            expiresAt: expiry,
            createdAt: block.timestamp,
            isActive: true,
            disputeFiled: false,
            disputeFiledAt: 0
        });

        backupCount[msg.sender]++;
        totalFeesCollected += msg.value;

        emit BackupStored(msg.sender, blobHash, storageUri, msg.value, expiry);
    }

    /**
     * @dev Update an existing backup (key rotation). Requires active backup.
     * @param newBlobHash New hash after re-encryption
     * @param newStorageUri New storage path
     */
    function updateBackup(bytes32 newBlobHash, string calldata newStorageUri) external {
        BackupRecord storage record = backups[msg.sender];
        if (!record.isActive) revert NoActiveBackup();

        record.blobHash = newBlobHash;
        record.storageUri = newStorageUri;
        record.disputeFiled = false;
        record.disputeFiledAt = 0;

        emit BackupUpdated(msg.sender, newBlobHash, newStorageUri);
    }

    /**
     * @dev Delete backup record (user wants to remove their data)
     */
    function deleteBackup() external {
        BackupRecord storage record = backups[msg.sender];
        if (!record.isActive) revert NoActiveBackup();

        record.isActive = false;
        emit BackupDeleted(msg.sender);
    }

    /**
     * @dev File a dispute when storage node cannot produce the blob
     */
    function fileDispute() external {
        BackupRecord storage record = backups[msg.sender];
        if (!record.isActive) revert NoActiveBackup();
        if (record.disputeFiled) revert DisputeAlreadyFiled();

        record.disputeFiled = true;
        record.disputeFiledAt = block.timestamp;

        emit DisputeFiled(msg.sender, block.timestamp);
    }

    /**
     * @dev Claim refund after dispute window passes without resolution
     */
    function claimRefund() external {
        BackupRecord storage record = backups[msg.sender];
        if (!record.isActive) revert NoActiveBackup();
        if (!record.disputeFiled) revert NoActiveBackup();
        if (block.timestamp < record.disputeFiledAt + DISPUTE_WINDOW) {
            revert DisputeWindowNotPassed();
        }

        uint256 refundAmount = record.paidAmount;
        record.isActive = false;
        record.paidAmount = 0;

        (bool success, ) = payable(msg.sender).call{value: refundAmount}("");
        if (!success) revert TransferFailed();

        emit DisputeResolved(msg.sender, true);
    }

    /**
     * @dev Admin resolves dispute (e.g., storage node proved availability)
     */
    function resolveDispute(address user, bool refund) external onlyAdmin {
        BackupRecord storage record = backups[user];
        if (!record.disputeFiled) revert NoActiveBackup();

        record.disputeFiled = false;
        record.disputeFiledAt = 0;

        if (refund) {
            uint256 refundAmount = record.paidAmount;
            record.isActive = false;
            record.paidAmount = 0;
            (bool success, ) = payable(user).call{value: refundAmount}("");
            if (!success) revert TransferFailed();
        }

        emit DisputeResolved(user, refund);
    }

    /**
     * @dev Get backup info for a user
     */
    function getBackup(address user) external view returns (
        bytes32 blobHash,
        string memory storageUri,
        uint256 paidAmount,
        uint256 expiresAt,
        bool isActive,
        bool disputeFiled
    ) {
        BackupRecord storage record = backups[user];
        return (
            record.blobHash,
            record.storageUri,
            record.paidAmount,
            record.expiresAt,
            record.isActive,
            record.disputeFiled
        );
    }

    /**
     * @dev Verify blob integrity — client calls this to confirm hash matches
     */
    function verifyBlobHash(address user, bytes32 computedHash) external view returns (bool) {
        return backups[user].isActive && backups[user].blobHash == computedHash;
    }

    /**
     * @dev Admin withdraws collected fees (minus reserved for active disputes)
     */
    function withdrawFees(uint256 amount) external onlyAdmin {
        (bool success, ) = payable(admin).call{value: amount}("");
        if (!success) revert TransferFailed();
        emit FundsWithdrawn(admin, amount);
    }

    /**
     * @dev Transfer admin role
     */
    function transferAdmin(address newAdmin) external onlyAdmin {
        admin = newAdmin;
    }

    receive() external payable {}
}
