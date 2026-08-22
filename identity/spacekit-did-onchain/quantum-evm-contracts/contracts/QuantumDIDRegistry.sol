// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

/**
 * @title QuantumDIDRegistry
 * @dev Registry for quantum-resistant decentralized identities on EVM chains
 * @notice This contract manages quantum DIDs while using traditional ECDSA for transactions
 */
contract QuantumDIDRegistry {
    
    struct QuantumIdentity {
        bytes publicKey;           // SPHINCS+ public key
        string didDocument;        // JSON DID document
        uint256 keyRotationCount;  // Track key rotations
        bool isActive;
        uint256 lastUpdated;
        mapping(bytes32 => bool) revokedCredentials;  // Track revoked credentials
    }

    struct VerifiableCredential {
        bytes32 credentialHash;
        address issuer;
        address subject;
        string credentialType;
        uint256 issuedAt;
        uint256 expiresAt;
        bool isRevoked;
    }

    // Storage
    mapping(address => QuantumIdentity) public identities;
    mapping(string => address) public didToAddress;
    mapping(bytes32 => VerifiableCredential) public credentials;
    mapping(address => bytes32[]) public addressToCredentials;

    // Events
    event IdentityRegistered(address indexed ethAddress, string indexed did, bytes publicKey);
    event IdentityUpdated(address indexed ethAddress, uint256 keyRotationCount);
    event CredentialIssued(
        bytes32 indexed credentialHash,
        address indexed issuer,
        address indexed subject,
        string credentialType
    );
    event CredentialRevoked(bytes32 indexed credentialHash, address indexed issuer);
    event CredentialVerified(
        bytes32 indexed credentialHash,
        address indexed verifier,
        bool isValid
    );

    // Errors
    error DIDAlreadyRegistered();
    error DIDNotRegistered();
    error InvalidQuantumSignature();
    error UnauthorizedOperation();
    error CredentialExpired();
    // error CredentialRevoked();

    /**
     * @dev Register a quantum DID linked to an Ethereum address
     * @param did The quantum DID string
     * @param quantumPublicKey The SPHINCS+ public key
     * @param didDocument JSON DID document
     * @param quantumSignature Quantum signature proving control of quantum keys
     */
    function registerQuantumDID(
        string calldata did,
        bytes calldata quantumPublicKey,
        string calldata didDocument,
        bytes calldata quantumSignature
    ) external {
        if (identities[msg.sender].isActive) revert DIDAlreadyRegistered();
        
        // Verify quantum signature proves control of quantum keys
        bytes memory message = abi.encodePacked(did, quantumPublicKey, msg.sender);
        if (!_verifyQuantumSignature(message, quantumSignature, quantumPublicKey)) {
            revert InvalidQuantumSignature();
        }

        // Store identity
        QuantumIdentity storage identity = identities[msg.sender];
        identity.publicKey = quantumPublicKey;
        identity.didDocument = didDocument;
        identity.keyRotationCount = 0;
        identity.isActive = true;
        identity.lastUpdated = block.timestamp;

        didToAddress[did] = msg.sender;
        
        emit IdentityRegistered(msg.sender, did, quantumPublicKey);
    }

    /**
     * @dev Rotate quantum keys for enhanced security
     * @param newPublicKey New SPHINCS+ public key
     * @param newDidDocument Updated DID document
     * @param quantumSignature Signature with old key authorizing rotation
     */
    function rotateQuantumKeys(
        bytes calldata newPublicKey,
        string calldata newDidDocument,
        bytes calldata quantumSignature
    ) external {
        if (!identities[msg.sender].isActive) revert DIDNotRegistered();

        QuantumIdentity storage identity = identities[msg.sender];
        
        // Create message for signature verification
        bytes memory message = abi.encodePacked(
            newPublicKey,
            identity.keyRotationCount + 1,
            msg.sender,
            block.timestamp
        );

        // Verify signature with current (old) key
        if (!_verifyQuantumSignature(message, quantumSignature, identity.publicKey)) {
            revert InvalidQuantumSignature();
        }

        // Update to new key
        identity.publicKey = newPublicKey;
        identity.didDocument = newDidDocument;
        identity.keyRotationCount++;
        identity.lastUpdated = block.timestamp;

        emit IdentityUpdated(msg.sender, identity.keyRotationCount);
    }

    /**
     * @dev Issue a verifiable credential on-chain
     * @param credentialHash Hash of the credential content
     * @param subject Ethereum address of credential subject
     * @param credentialType Type of credential
     * @param expiresAt Expiration timestamp (0 for no expiry)
     * @param quantumSignature Quantum signature from issuer
     */
    function issueCredential(
        bytes32 credentialHash,
        address subject,
        string calldata credentialType,
        uint256 expiresAt,
        bytes calldata quantumSignature
    ) external {
        if (!identities[msg.sender].isActive) revert DIDNotRegistered();
        
        // Verify issuer's quantum signature
        bytes memory message = abi.encodePacked(
            credentialHash,
            subject,
            credentialType,
            expiresAt,
            msg.sender
        );
        
        if (!_verifyQuantumSignature(message, quantumSignature, identities[msg.sender].publicKey)) {
            revert InvalidQuantumSignature();
        }

        // Store credential
        credentials[credentialHash] = VerifiableCredential({
            credentialHash: credentialHash,
            issuer: msg.sender,
            subject: subject,
            credentialType: credentialType,
            issuedAt: block.timestamp,
            expiresAt: expiresAt,
            isRevoked: false
        });

        // Add to subject's credential list
        addressToCredentials[subject].push(credentialHash);

        emit CredentialIssued(credentialHash, msg.sender, subject, credentialType);
    }

    /**
     * @dev Verify a quantum-signed credential proof
     * @param credentialHash Hash of the credential to verify
     * @param quantumSignature Quantum signature for verification
     * @param verificationMessage Message that was signed
     * @return isValid Whether the credential proof is valid
     */
    function verifyCredentialProof(
        bytes32 credentialHash,
        bytes calldata quantumSignature,
        string calldata verificationMessage
    ) external returns (bool isValid) {
        VerifiableCredential storage cred = credentials[credentialHash];
        
        // Check if credential exists and is not revoked
        if (cred.issuer == address(0)) return false;
        if (cred.isRevoked) return false;
        
        // Check expiration
        if (cred.expiresAt > 0 && block.timestamp > cred.expiresAt) {
            return false;
        }

        // Verify quantum signature from issuer
        bytes memory expectedMessage = abi.encodePacked(verificationMessage);
        isValid = _verifyQuantumSignature(
            expectedMessage,
            quantumSignature,
            identities[cred.issuer].publicKey
        );

        emit CredentialVerified(credentialHash, msg.sender, isValid);
        return isValid;
    }

    /**
     * @dev Revoke a credential (only by issuer)
     * @param credentialHash Hash of credential to revoke
     * @param quantumSignature Quantum signature authorizing revocation
     */
    function revokeCredential(
        bytes32 credentialHash,
        bytes calldata quantumSignature
    ) external {
        VerifiableCredential storage cred = credentials[credentialHash];
        
        if (cred.issuer != msg.sender) revert UnauthorizedOperation();
        if (cred.isRevoked) return; // Already revoked

        // Verify quantum signature for revocation
        bytes memory message = abi.encodePacked("REVOKE", credentialHash, block.timestamp);
        if (!_verifyQuantumSignature(message, quantumSignature, identities[msg.sender].publicKey)) {
            revert InvalidQuantumSignature();
        }

        cred.isRevoked = true;
        identities[msg.sender].revokedCredentials[credentialHash] = true;

        emit CredentialRevoked(credentialHash, msg.sender);
    }

    /**
     * @dev Get credentials for an address
     * @param addr Ethereum address to query
     * @return Array of credential hashes
     */
    function getCredentialsForAddress(address addr) external view returns (bytes32[] memory) {
        return addressToCredentials[addr];
    }

    /**
     * @dev Check if a DID is registered
     * @param did DID string to check
     * @return Whether the DID is registered
     */
    function isDIDRegistered(string calldata did) external view returns (bool) {
        address addr = didToAddress[did];
        return addr != address(0) && identities[addr].isActive;
    }

    /**
     * @dev Get quantum public key for an address
     * @param addr Ethereum address
     * @return Quantum public key bytes
     */
    function getQuantumPublicKey(address addr) external view returns (bytes memory) {
        if (!identities[addr].isActive) revert DIDNotRegistered();
        return identities[addr].publicKey;
    }

    /**
     * @dev Get DID document for an address
     * @param addr Ethereum address
     * @return DID document JSON string
     */
    function getDIDDocument(address addr) external view returns (string memory) {
        if (!identities[addr].isActive) revert DIDNotRegistered();
        return identities[addr].didDocument;
    }

    /**
     * @dev Internal function to verify quantum signatures
     * @notice In production, this should use a precompiled contract or oracle
     * @param message Message that was signed
     * @param signature Quantum signature
     * @param publicKey Quantum public key
     * @return Whether signature is valid
     */
    function _verifyQuantumSignature(
        bytes memory message,
        bytes memory signature,
        bytes memory publicKey
    ) internal pure returns (bool) {
        // SPHINCS+ verification is not implemented on-chain. Verify off-chain with
        // the spacekit-did Rust library. See quantum-evm-contracts/EXPERIMENTAL.md.
        require(signature.length > 0, "Empty signature");
        require(publicKey.length > 0, "Empty public key");
        require(message.length > 0, "Empty message");
        revert OnChainVerificationUnsupported();
    }

    error OnChainVerificationUnsupported();

    /**
     * @dev Check if an address has an active DID registration
     * @param addr Address to check
     * @return Whether the address has an active DID
     */
    function isAddressRegistered(address addr) external view returns (bool) {
        return identities[addr].isActive;
    }

    /**
     * @dev Get contract version
     */
    function version() external pure returns (string memory) {
        return "1.0.0";
    }
}

/**
 * @title QuantumCredentialManager
 * @dev Helper contract for managing credential schemas and validation
 */
contract QuantumCredentialManager {
    
    struct CredentialSchema {
        string name;
        string[] requiredFields;
        mapping(string => bool) fieldExists;
        bool isActive;
    }

    mapping(string => CredentialSchema) public schemas;
    mapping(address => bool) public schemaManagers;
    
    QuantumDIDRegistry public immutable didRegistry;

    constructor(address _didRegistry) {
        didRegistry = QuantumDIDRegistry(_didRegistry);
        schemaManagers[msg.sender] = true;
    }

    modifier onlySchemaManager() {
        require(schemaManagers[msg.sender], "Not a schema manager");
        _;
    }

    modifier onlyRegisteredDID() {
        require(didRegistry.isAddressRegistered(msg.sender), "DID not registered");
        _;
    }

    /**
     * @dev Create a new credential schema
     */
    function createSchema(
        string calldata schemaName,
        string[] calldata requiredFields
    ) external onlySchemaManager {
        CredentialSchema storage schema = schemas[schemaName];
        schema.name = schemaName;
        schema.requiredFields = requiredFields;
        schema.isActive = true;

        for (uint i = 0; i < requiredFields.length; i++) {
            schema.fieldExists[requiredFields[i]] = true;
        }
    }

    /**
     * @dev Validate credential against schema
     */
    function validateCredentialSchema(
        string calldata schemaName,
        string[] calldata providedFields
    ) external view returns (bool) {
        CredentialSchema storage schema = schemas[schemaName];
        if (!schema.isActive) return false;

        // Check all required fields are provided
        for (uint i = 0; i < schema.requiredFields.length; i++) {
            bool found = false;
            for (uint j = 0; j < providedFields.length; j++) {
                if (keccak256(bytes(schema.requiredFields[i])) == keccak256(bytes(providedFields[j]))) {
                    found = true;
                    break;
                }
            }
            if (!found) return false;
        }

        return true;
    }
} 