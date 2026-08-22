# SpaceKit Developer Guide - Complete Development Reference (NEEDS REVIEW FOR UPDATES)

**🚀 Welcome to the Future of Identity-Native Computing**

This comprehensive guide will get you building revolutionary applications with SpaceKit's quantum-resistant, identity-native computational blockchain. From your first "Hello World" to advanced enterprise applications, this guide covers everything you need.

---

## 🎯 **Quick Start Overview**

### **What You'll Build**
- **Identity-Aware Smart Contracts** - Contracts that know who's calling them
- **Reputation-Based Applications** - Apps that adapt based on user reputation
- **Cross-Platform DID Integration** - Same identity across mobile, web, desktop
- **Quantum-Safe Applications** - Future-proof with post-quantum cryptography
- **GPU-Accelerated Compute** - Harness the power of distributed GPU networks

### **Prerequisites**
- **Rust** 1.70+ (primary development language)
- **Node.js** 18+ (for TypeScript SDK and tools)
- **Basic blockchain knowledge** (helpful but not required)
- **Understanding of identity concepts** (DIDs, signatures, reputation)

---

## 🚀 **Quick Start (5 Minutes)**

### **1. Install SWTCH Development Tools**

```bash
# Install Rust if you haven't already
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install SpaceKit CLI and SDK
cargo install spacekit
npm install -g @spacekit/sdk

# Verify installation
spacekit --version
```

### **2. Create Your First Project**

```bash
# Create new SWTCH project
spacekit new my-first-app --template=identity-contract
cd my-first-app

# Initialize with default configuration
spacekit init
```

### **3. Your First Identity-Aware Contract**

```rust
// src/contracts/hello_identity.rs
use spacekit_sdk::prelude::*;

#[spacekit_contract]
#[did_enabled]
pub struct HelloIdentity {
    greetings: HashMap<DID, String>,
    reputation_tracker: ReputationTracker,
}

#[spacekit_impl]
impl HelloIdentity {
    #[spacekit_function("greet")]
    pub fn greet(&mut self, user_did: DID, message: String) -> GreetingResult {
        // Verify the caller's identity
        let verified_user = spacekit_verify_did(user_did)?;
        require!(verified_user.is_verified(), "Identity verification failed");
        
        // Get user's reputation (new users start at 0.5)
        let reputation = self.reputation_tracker.get_reputation(user_did)
            .unwrap_or(0.5);
        
        // Personalized greeting based on reputation
        let greeting = match reputation {
            r if r > 0.8 => format!("👑 Welcome back, esteemed {}!", message),
            r if r > 0.6 => format!("⭐ Hello there, {}!", message),
            r if r > 0.4 => format!("👋 Hi {}!", message),
            _ => format!("🆕 Welcome newcomer {}!", message),
        };
        
        // Store the greeting
        self.greetings.insert(user_did, greeting.clone());
        
        // Update reputation (+0.01 for each interaction)
        self.reputation_tracker.update_reputation(user_did, 0.01);
        
        GreetingResult {
            greeting,
            user_did,
            reputation,
            total_interactions: self.greetings.len(),
        }
    }
    
    #[spacekit_function("get_my_reputation")]
    #[spacekit_view]
    pub fn get_reputation(&self, user_did: DID) -> ReputationInfo {
        let reputation = self.reputation_tracker.get_reputation(user_did)
            .unwrap_or(0.5);
        
        let tier = match reputation {
            r if r > 0.9 => "Platinum 💎",
            r if r > 0.7 => "Gold 🥇",
            r if r > 0.5 => "Silver 🥈",
            r if r > 0.3 => "Bronze 🥉",
            _ => "Newcomer 🆕",
        };
        
        ReputationInfo {
            user_did,
            score: reputation,
            tier: tier.to_string(),
            total_interactions: self.greetings.len(),
            benefits: self.calculate_benefits(reputation),
        }
    }
}
```

### **4. Deploy and Test**

```bash
# Compile your contract
spacekit build

# Deploy to local testnet
spacekit deploy --network=local

# Interact with your contract
spacekit call greet --did="did:spacekit:alice" --message="Alice"
```

**🎉 Congratulations!** You just created your first identity-aware smart contract!

---

## 🆔 **DID Integration Development**

### **Creating and Managing DIDs**

#### **Create a Quantum-Safe DID**

```rust
use swtch_sdk::{DID, QuantumSafeDID, SecurityLevel};

// Create a new quantum-safe DID
let did = QuantumSafeDID::create_with_security(SecurityLevel::High).await?;

println!("Created DID: {}", did.identifier);
println!("Public Key (Kyber768): {}", did.kyber_public_key());
println!("Verify Key (Dilithium2): {}", did.dilithium_verify_key());
```

#### **DID Verification in Contracts**

```rust
#[spacekit_function("verify_user")]
pub fn verify_user_identity(&self, user_did: DID, challenge: Vec<u8>) -> VerificationResult {
    // Verify the DID with quantum-safe cryptography
    let verification = spacekit_verify_did_with_challenge(user_did, &challenge)?;
    
    match verification.is_verified() {
        true => VerificationResult {
            verified: true,
            confidence: verification.confidence_score(),
            quantum_safe: true,
            verification_time: verification.duration(),
        },
        false => VerificationResult {
            verified: false,
            error: verification.error_message(),
            quantum_safe: true,
            verification_time: verification.duration(),
        }
    }
}
```

#### **Cross-Platform DID Usage**

**TypeScript/JavaScript (Web & Node.js):**
```typescript
import { SpaceKitDID, VerificationLevel } from '@spacekit/sdk';

// Create DID in web browser
const did = await SpaceKitDID.createInBrowser({
    algorithm: 'Kyber768',
    signatureAlgorithm: 'Dilithium2',
    storageType: 'indexeddb', // or 'memory', 'localstorage'
    verificationLevel: VerificationLevel.High
});

// Sign a message
const message = "Hello SpaceKit!";
const signature = await did.sign(message);

// Verify signature
const isValid = await SpaceKitDID.verify(message, signature, did.publicKey);
console.log(`Signature valid: ${isValid}`);
```

**React Native (Mobile):**
```typescript
import { SpaceKitMobileDID, BiometricType } from '@spacekit/mobile-sdk';

// Create DID with biometric authentication
const did = await SpaceKitMobileDID.createWithBiometrics({
    biometricType: BiometricType.Fingerprint,
    algorithm: 'Kyber768',
    signatureAlgorithm: 'Dilithium2',
    secureEnclaveStorage: true
});

// Authenticate and sign
const authenticated = await did.authenticateWithBiometrics();
if (authenticated) {
    const signature = await did.signWithBiometrics("Transaction data");
}
```

**Flutter (Cross-Platform Mobile):**
```dart
import 'package:spacekit_flutter/spacekit_flutter.dart';

// Initialize SWTCH DID
final did = await SpaceKitDID.createWithBiometrics(
  algorithm: Algorithm.kyber768,
  signatureAlgorithm: SignatureAlgorithm.dilithium2,
  biometricType: BiometricType.fingerprint,
);

// Sign and verify
final signature = await did.signMessage("Hello from Flutter!");
final isValid = await did.verifySignature("Hello from SpaceKit!", signature);
```

### **Identity-Aware Contract Patterns**

#### **Reputation-Based Access Control**

```rust
#[spacekit_contract]
pub struct ReputationGatedService {
    user_reputations: HashMap<DID, f64>,
    access_logs: Vec<AccessLog>,
    service_tiers: Vec<ServiceTier>,
}

#[spacekit_impl]
impl ReputationGatedService {
    #[spacekit_function("request_premium_service")]
    pub fn request_premium_access(&mut self, user_did: DID, service_type: ServiceType) -> AccessResult {
        // Verify user identity
        let verified_user = spacekit_verify_did(user_did)?;
        require!(verified_user.is_verified(), "Identity not verified");
        
        // Check reputation requirement
        let user_reputation = self.user_reputations.get(&user_did).unwrap_or(&0.0);
        let required_reputation = service_type.minimum_reputation();
        
        if *user_reputation >= required_reputation {
            // Grant access
            let access_token = self.generate_access_token(user_did, service_type);
            
            // Log access
            self.access_logs.push(AccessLog {
                user_did,
                service_type,
                reputation: *user_reputation,
                granted: true,
                timestamp: spacekit_now(),
            });
            
            AccessResult::Granted {
                access_token,
                expires_at: spacekit_now() + service_type.access_duration(),
                tier: self.calculate_tier(*user_reputation),
            }
        } else {
            AccessResult::Denied {
                current_reputation: *user_reputation,
                required_reputation,
                suggested_actions: self.suggest_reputation_improvement(user_did),
            }
        }
    }
    
    #[spacekit_function("improve_reputation")]
    pub fn complete_reputation_task(&mut self, user_did: DID, task: ReputationTask) -> ReputationUpdate {
        let verified_user = spacekit_verify_did(user_did)?;
        require!(verified_user.is_verified(), "Identity not verified");
        
        // Validate task completion
        let task_verification = self.verify_task_completion(user_did, &task)?;
        require!(task_verification.is_valid(), "Task not properly completed");
        
        // Update reputation
        let current_reputation = self.user_reputations.entry(user_did).or_insert(0.5);
        let reputation_boost = task.reputation_value() * task_verification.quality_multiplier();
        *current_reputation += reputation_boost;
        
        // Cap at 1.0
        if *current_reputation > 1.0 {
            *current_reputation = 1.0;
        }
        
        ReputationUpdate {
            user_did,
            previous_reputation: *current_reputation - reputation_boost,
            new_reputation: *current_reputation,
            reputation_boost,
            new_tier: self.calculate_tier(*current_reputation),
            unlocked_services: self.get_newly_unlocked_services(user_did),
        }
    }
}
```

#### **Multi-Party Consensus with Identity**

```rust
#[spacekit_contract]
pub struct MultiPartyAgreement {
    agreements: HashMap<AgreementID, Agreement>,
    signatures: HashMap<AgreementID, Vec<PartySignature>>,
    identity_requirements: HashMap<AgreementID, IdentityRequirements>,
}

#[spacekit_impl]
impl MultiPartyAgreement {
    #[spacekit_function("create_agreement")]
    pub fn create_multi_party_agreement(
        &mut self,
        creator_did: DID,
        parties: Vec<DID>,
        terms: AgreementTerms,
        consensus_threshold: f64,
    ) -> AgreementCreation {
        // Verify creator identity
        let verified_creator = spacekit_verify_did(creator_did)?;
        require!(verified_creator.is_verified(), "Creator identity not verified");
        
        // Verify all party identities
        let mut verified_parties = Vec::new();
        for party_did in &parties {
            let verified_party = spacekit_verify_did(*party_did)?;
            require!(verified_party.is_verified(), "Party {} identity not verified", party_did);
            verified_parties.push(verified_party);
        }
        
        let agreement_id = self.generate_agreement_id(&parties, &terms);
        
        // Create the agreement
        let agreement = Agreement {
            id: agreement_id.clone(),
            creator: creator_did,
            parties: parties.clone(),
            terms,
            consensus_threshold,
            created_at: spacekit_now(),
            status: AgreementStatus::Pending,
        };
        
        // Set identity requirements
        let identity_requirements = IdentityRequirements {
            minimum_reputation: 0.3,
            require_kyc: false,
            require_biometric: false,
            quantum_signature_required: true,
        };
        
        self.agreements.insert(agreement_id.clone(), agreement);
        self.signatures.insert(agreement_id.clone(), Vec::new());
        self.identity_requirements.insert(agreement_id.clone(), identity_requirements);
        
        // Notify parties
        self.notify_parties(&parties, &agreement_id);
        
        AgreementCreation {
            agreement_id,
            parties,
            consensus_threshold,
            estimated_completion_time: self.estimate_completion_time(&parties),
        }
    }
    
    #[spacekit_function("sign_agreement")]
    pub fn sign_agreement(&mut self, signer_did: DID, agreement_id: AgreementID) -> SignatureResult {
        // Verify signer identity with high security
        let verified_signer = spacekit_verify_did_high_security(signer_did)?;
        require!(verified_signer.is_verified(), "Signer identity not verified");
        
        // Get agreement
        let agreement = self.agreements.get(&agreement_id)
            .ok_or("Agreement not found")?;
        
        // Check if signer is a party to the agreement
        require!(agreement.parties.contains(&signer_did), "Not a party to this agreement");
        
        // Check identity requirements
        let requirements = self.identity_requirements.get(&agreement_id).unwrap();
        require!(verified_signer.meets_requirements(requirements), "Identity requirements not met");
        
        // Create quantum-safe signature
        let signature_data = self.prepare_signature_data(&agreement);
        let quantum_signature = spacekit_sign_quantum_safe(signer_did, &signature_data)?;
        
        // Store signature
        let party_signature = PartySignature {
            signer: signer_did,
            signature: quantum_signature,
            signed_at: spacekit_now(),
            verification_level: verified_signer.verification_level(),
        };
        
        self.signatures.entry(agreement_id.clone()).or_default().push(party_signature);
        
        // Check if consensus is reached
        let signatures = self.signatures.get(&agreement_id).unwrap();
        let signature_percentage = signatures.len() as f64 / agreement.parties.len() as f64;
        
        if signature_percentage >= agreement.consensus_threshold {
            // Execute agreement
            self.execute_agreement(agreement_id.clone())?;
            
            SignatureResult::ConsensusReached {
                agreement_id,
                final_signatures: signatures.len(),
                execution_time: spacekit_now(),
                quantum_verified: true,
            }
        } else {
            SignatureResult::SignatureAdded {
                agreement_id,
                signatures_collected: signatures.len(),
                signatures_needed: (agreement.parties.len() as f64 * agreement.consensus_threshold).ceil() as usize,
                estimated_completion: self.estimate_completion_time(&agreement.parties),
            }
        }
    }
}
```

---

## 💻 **Smart Contract Development**

### **SpaceKit Contract Structure**

#### **Basic Contract Template**

```rust
use swtch_sdk::prelude::*;

#[spacekit_contract]
#[did_enabled] // Enable DID verification
#[gpu_enabled] // Enable GPU computation
pub struct MyContract {
    // State variables
    owner: DID,
    users: HashMap<DID, UserProfile>,
    settings: ContractSettings,
    
    // Events
    #[swtch_event]
    user_registered: Event<UserRegistered>,
    
    #[swtch_event]
    computation_completed: Event<ComputationCompleted>,
}

#[spacekit_impl]
impl MyContract {
    // Constructor
    #[spacekit_constructor]
    pub fn new(owner_did: DID) -> Self {
        let verified_owner = spacekit_verify_did(owner_did)
            .expect("Owner DID verification failed");
        
        Self {
            owner: owner_did,
            users: HashMap::new(),
            settings: ContractSettings::default(),
            user_registered: Event::new(),
            computation_completed: Event::new(),
        }
    }
    
    // Public functions
    #[spacekit_function("register_user")]
    pub fn register_user(&mut self, user_did: DID, profile: UserProfile) -> RegistrationResult {
        // Implementation here
    }
    
    // View functions (read-only)
    #[spacekit_function("get_user_profile")]
    #[spacekit_view]
    pub fn get_user_profile(&self, user_did: DID) -> Option<UserProfile> {
        self.users.get(&user_did).cloned()
    }
    
    // GPU-accelerated functions
    #[spacekit_function("process_data")]
    #[spacekit_gpu_compute]
    pub fn process_large_dataset(&mut self, data: Vec<f32>) -> ProcessingResult {
        // GPU computation implementation
    }
    
    // Owner-only functions
    #[spacekit_function("update_settings")]
    #[spacekit_modifier(only_owner)]
    pub fn update_settings(&mut self, new_settings: ContractSettings) -> bool {
        self.settings = new_settings;
        true
    }
}

// Modifiers
impl MyContract {
    fn only_owner(&self) -> bool {
        swtch_caller() == self.owner
    }
}
```

### **Advanced Contract Features**

#### **GPU-Accelerated Computation**

```rust
#[spacekit_contract]
pub struct GPUComputeService {
    compute_queue: Vec<ComputeJob>,
    gpu_resources: HashMap<GPUID, GPUResource>,
    job_results: HashMap<JobID, ComputeResult>,
}

#[spacekit_impl]
impl GPUComputeService {
    #[spacekit_function("submit_compute_job")]
    #[swtch_payable] // Requires payment
    pub fn submit_job(&mut self, user_did: DID, job_data: ComputeJobData) -> JobSubmission {
        let verified_user = spacekit_verify_did(user_did)?;
        require!(verified_user.is_verified(), "User not verified");
        
        // Calculate cost based on complexity
        let estimated_cost = self.calculate_compute_cost(&job_data);
        require!(spacekit_value() >= estimated_cost, "Insufficient payment");
        
        // Create job
        let job_id = self.generate_job_id();
        let compute_job = ComputeJob {
            id: job_id.clone(),
            submitter: user_did,
            data: job_data,
            submitted_at: spacekit_now(),
            status: JobStatus::Queued,
            estimated_duration: self.estimate_duration(&job_data),
        };
        
        self.compute_queue.push(compute_job);
        
        JobSubmission {
            job_id,
            estimated_cost,
            queue_position: self.compute_queue.len(),
            estimated_completion: spacekit_now() + self.estimate_total_wait_time(),
        }
    }
    
    #[spacekit_function("execute_gpu_computation")]
    #[spacekit_gpu_compute]
    #[spacekit_deterministic] // Ensures reproducible results
    pub fn execute_computation(&mut self, job_id: JobID) -> ComputeResult {
        let job = self.get_job_mut(&job_id)?;
        require!(job.status == JobStatus::Queued, "Job not ready for execution");
        
        job.status = JobStatus::Running;
        
        // Execute on GPU
        let result = match &job.data {
            ComputeJobData::MatrixMultiplication { a, b } => {
                self.gpu_matrix_multiply(a, b)
            },
            ComputeJobData::AIInference { model, input } => {
                self.gpu_ai_inference(model, input)
            },
            ComputeJobData::CryptographicOperation { operation, data } => {
                self.gpu_crypto_operation(operation, data)
            },
            ComputeJobData::CustomWASM { code, input } => {
                self.gpu_wasm_execution(code, input)
            },
        }?;
        
        job.status = JobStatus::Completed;
        
        let compute_result = ComputeResult {
            job_id: job_id.clone(),
            result_data: result,
            execution_time: spacekit_now() - job.submitted_at,
            gpu_used: spacekit_get_gpu_info(),
            gas_used: spacekit_gas_used(),
            verified: true,
        };
        
        self.job_results.insert(job_id.clone(), compute_result.clone());
        
        // Emit event
        self.computation_completed.emit(ComputationCompleted {
            job_id,
            submitter: job.submitter,
            execution_time: compute_result.execution_time,
        });
        
        compute_result
    }
    
    // GPU computation implementations
    fn gpu_matrix_multiply(&self, a: &Matrix, b: &Matrix) -> Result<Vec<f32>> {
        // Offload to GPU for parallel computation
        spacekit_gpu_execute(r#"
            // GPU kernel for matrix multiplication
            __global__ void matrix_multiply(float* a, float* b, float* c, int n) {
                int row = blockIdx.y * blockDim.y + threadIdx.y;
                int col = blockIdx.x * blockDim.x + threadIdx.x;
                
                if (row < n && col < n) {
                    float sum = 0.0f;
                    for (int k = 0; k < n; k++) {
                        sum += a[row * n + k] * b[k * n + col];
                    }
                    c[row * n + col] = sum;
                }
            }
        "#, &[a.data(), b.data()])
    }
    
    fn gpu_ai_inference(&self, model: &AIModel, input: &Vec<f32>) -> Result<Vec<f32>> {
        // GPU-accelerated neural network inference
        spacekit_gpu_neural_network_execute(model, input)
    }
}
```

#### **Cross-Chain Integration**

```rust
#[spacekit_contract]
pub struct CrossChainBridge {
    supported_chains: HashMap<ChainID, ChainConfig>,
    pending_transfers: HashMap<TransferID, CrossChainTransfer>,
    bridge_operators: HashMap<DID, OperatorConfig>,
}

#[spacekit_impl]
impl CrossChainBridge {
    #[spacekit_function("initiate_cross_chain_transfer")]
    #[swtch_payable]
    pub fn transfer_to_chain(
        &mut self,
        sender_did: DID,
        target_chain: ChainID,
        target_address: String,
        amount: u128,
    ) -> TransferInitiation {
        let verified_sender = spacekit_verify_did(sender_did)?;
        require!(verified_sender.is_verified(), "Sender not verified");
        
        // Check if target chain is supported
        let chain_config = self.supported_chains.get(&target_chain)
            .ok_or("Target chain not supported")?;
        
        // Calculate bridge fees
        let bridge_fee = self.calculate_bridge_fee(target_chain, amount);
        require!(spacekit_value() >= amount + bridge_fee, "Insufficient payment for transfer + fees");
        
        // Create transfer record
        let transfer_id = self.generate_transfer_id();
        let transfer = CrossChainTransfer {
            id: transfer_id.clone(),
            sender: sender_did,
            source_chain: ChainID::SWTCH,
            target_chain,
            target_address,
            amount,
            bridge_fee,
            initiated_at: spacekit_now(),
            status: TransferStatus::Pending,
            confirmations: 0,
            required_confirmations: chain_config.required_confirmations,
        };
        
        self.pending_transfers.insert(transfer_id.clone(), transfer);
        
        // Initiate cross-chain communication
        self.initiate_layerzero_transfer(&transfer)?;
        
        TransferInitiation {
            transfer_id,
            estimated_completion: spacekit_now() + chain_config.average_completion_time,
            required_confirmations: chain_config.required_confirmations,
            bridge_fee,
        }
    }
    
    #[spacekit_function("confirm_cross_chain_transfer")]
    #[spacekit_modifier(only_bridge_operator)]
    pub fn confirm_transfer(&mut self, operator_did: DID, transfer_id: TransferID, confirmation: Confirmation) -> TransferStatus {
        let verified_operator = spacekit_verify_did(operator_did)?;
        require!(verified_operator.is_verified(), "Operator not verified");
        
        let transfer = self.pending_transfers.get_mut(&transfer_id)
            .ok_or("Transfer not found")?;
        
        // Verify confirmation signature
        require!(confirmation.verify_signature(), "Invalid confirmation signature");
        
        transfer.confirmations += 1;
        
        if transfer.confirmations >= transfer.required_confirmations {
            // Complete the transfer
            transfer.status = TransferStatus::Completed;
            self.complete_cross_chain_transfer(transfer_id.clone())?;
            TransferStatus::Completed
        } else {
            TransferStatus::PendingConfirmations {
                current: transfer.confirmations,
                required: transfer.required_confirmations,
            }
        }
    }
}
```

---

## 🌐 **Cross-Platform Development**

### **Web Development**

#### **React Integration**

```typescript
// components/SpaceKitProvider.tsx
import React, { createContext, useContext, useEffect, useState } from 'react';
import { SpaceKitWebSDK, DID } from '@spacekit/web-sdk';

interface SpaceKitContextType {
    sdk: SpaceKitWebSDK | null;
    did: DID | null;
    isConnected: boolean;
    connect: () => Promise<void>;
    disconnect: () => void;
}

const SpaceKitContext = createContext<SpaceKitContextType | null>(null);

export const SpaceKitProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [sdk, setSdk] = useState<SpaceKitWebSDK | null>(null);
    const [did, setDid] = useState<DID | null>(null);
    const [isConnected, setIsConnected] = useState(false);
    
    useEffect(() => {
        // Initialize SDK
        const initSDK = async () => {
            const spacekitSDK = new SpaceKitWebSDK({
                network: 'mainnet', // or 'testnet'
                apiKey: process.env.REACT_APP_SPACEKIT_API_KEY,
            });
            
            await spacekitSDK.initialize();
            setSdk(spacekitSDK);
            
            // Check for existing session
            const existingDID = await spacekitSDK.getStoredDID();
            if (existingDID) {
                setDid(existingDID);
                setIsConnected(true);
            }
        };
        
        initSDK();
    }, []);
    
    const connect = async () => {
        if (!sdk) return;
        
        try {
            // Create or restore DID
            const userDID = await spacekitSDK.createOrRestoreDID({
                authMethod: 'passkey', // or 'metamask', 'wallet'
                algorithm: 'Kyber768',
            });
            
            setDid(userDID);
            setIsConnected(true);
            
            // Store for future sessions
            await sdk.storeDID(userDID);
        } catch (error) {
            console.error('Failed to connect:', error);
        }
    };
    
    const disconnect = () => {
        setDid(null);
        setIsConnected(false);
        sdk?.clearStoredDID();
    };
    
    return (
        <SpaceKitContext.Provider value={{ sdk, did, isConnected, connect, disconnect }}>
            {children}
        </SpaceKitContext.Provider>
    );
};

export const useSpaceKit = () => {
    const context = useContext(SpaceKitContext);
    if (!context) {
        throw new Error('useSpaceKit must be used within a SpaceKitProvider');
    }
    return context;
};
```

#### **React Component Example**

```tsx
// components/ReputationCard.tsx
import React, { useEffect, useState } from 'react';
import { useSpaceKit } from './SpaceKitProvider';

interface ReputationData {
    score: number;
    tier: string;
    benefits: string[];
    totalInteractions: number;
}

export const ReputationCard: React.FC = () => {
    const { sdk, did, isConnected } = useSpaceKit();
    const [reputation, setReputation] = useState<ReputationData | null>(null);
    const [loading, setLoading] = useState(false);
    
    useEffect(() => {
        if (isConnected && did) {
            loadReputation();
        }
    }, [isConnected, did]);
    
    const loadReputation = async () => {
        if (!sdk || !did) return;
        
        setLoading(true);
        try {
            // Call our reputation contract
            const result = await sdk.callContract({
                contractAddress: '0x...', // Your deployed contract
                function: 'get_my_reputation',
                params: { user_did: did.identifier },
            });
            
            setReputation(result);
        } catch (error) {
            console.error('Failed to load reputation:', error);
        } finally {
            setLoading(false);
        }
    };
    
    const improveReputation = async () => {
        if (!sdk || !did) return;
        
        try {
            const result = await sdk.callContract({
                contractAddress: '0x...',
                function: 'complete_reputation_task',
                params: {
                    user_did: did.identifier,
                    task: {
                        task_type: 'daily_check_in',
                        completion_proof: 'proof_data',
                    }
                },
                gasLimit: 100000,
            });
            
            // Refresh reputation after update
            await loadReputation();
            
            console.log('Reputation updated:', result);
        } catch (error) {
            console.error('Failed to improve reputation:', error);
        }
    };
    
    if (!isConnected) {
        return (
            <div className="reputation-card">
                <p>Connect your SWTCH identity to view reputation</p>
            </div>
        );
    }
    
    if (loading) {
        return <div className="reputation-card">Loading reputation...</div>;
    }
    
    return (
        <div className="reputation-card">
            <h3>Your Reputation</h3>
            {reputation && (
                <>
                    <div className="score">
                        <span className="value">{reputation.score.toFixed(2)}</span>
                        <span className="tier">{reputation.tier}</span>
                    </div>
                    
                    <div className="benefits">
                        <h4>Current Benefits:</h4>
                        <ul>
                            {reputation.benefits.map((benefit, index) => (
                                <li key={index}>{benefit}</li>
                            ))}
                        </ul>
                    </div>
                    
                    <div className="stats">
                        <p>Total Interactions: {reputation.totalInteractions}</p>
                    </div>
                    
                    <button onClick={improveReputation}>
                        Complete Daily Task (+0.01)
                    </button>
                </>
            )}
        </div>
    );
};
```

### **Mobile Development**

#### **React Native Example**

```typescript
// SpaceKitMobileProvider.tsx
import React, { createContext, useContext, useState } from 'react';
import { SpaceKitMobileSDK, BiometricType } from '@spacekit/react-native';

export const SpaceKitMobileProvider: React.FC<{ children: React.ReactNode }> = ({ children }) => {
    const [sdk] = useState(() => new SpaceKitMobileSDK({
        network: 'mainnet',
        biometricAuth: true,
    }));
    
    const [did, setDid] = useState(null);
    const [isAuthenticated, setIsAuthenticated] = useState(false);
    
    const authenticateWithBiometrics = async () => {
        try {
            // Check if biometrics are available
            const biometricsAvailable = await sdk.isBiometricsAvailable();
            if (!biometricsAvailable) {
                throw new Error('Biometric authentication not available');
            }
            
            // Create or restore DID with biometric authentication
            const userDID = await sdk.createOrRestoreDIDWithBiometrics({
                biometricType: BiometricType.FingerprintOrFace,
                algorithm: 'Kyber768',
                signatureAlgorithm: 'Dilithium2',
                promptMessage: 'Authenticate to access your SpaceKit identity',
            });
            
            setDid(userDID);
            setIsAuthenticated(true);
            
            return userDID;
        } catch (error) {
            console.error('Biometric authentication failed:', error);
            throw error;
        }
    };
    
    const signWithBiometrics = async (data: string) => {
        if (!did) throw new Error('Not authenticated');
        
        return await sdk.signWithBiometrics(data, {
            promptMessage: 'Sign transaction with your biometric',
        });
    };
    
    return (
        <SpaceKitMobileContext.Provider value={{
            sdk,
            did,
            isAuthenticated,
            authenticateWithBiometrics,
            signWithBiometrics,
        }}>
            {children}
        </SpaceKitMobileContext.Provider>
    );
};
```

#### **Flutter Example**

```dart
// lib/services/spacekit_service.dart
import 'package:spacekit_flutter/spacekit_flutter.dart';

class SpaceKitService {
    static final SpaceKitService _instance = SpaceKitService._internal();
    factory SpaceKitService() => _instance;
    SpaceKitService._internal();
    
    late SpaceKitSDK _sdk;
    SpaceKitDID? _did;
    bool _isInitialized = false;
    
    Future<void> initialize() async {
        _sdk = SpaceKitSDK(
            network: Network.mainnet,
            config: SpaceKitConfig(
                biometricAuth: true,
                quantumSafe: true,
            ),
        );
        
        await _sdk.initialize();
        _isInitialized = true;
        
        // Try to restore existing DID
        _did = await _sdk.getStoredDID();
    }
    
    Future<SpaceKitDID> authenticateWithBiometrics() async {
        if (!_isInitialized) await initialize();
        
        try {
            _did = await _sdk.createOrRestoreDIDWithBiometrics(
                biometricType: BiometricType.fingerprint,
                algorithm: Algorithm.kyber768,
                signatureAlgorithm: SignatureAlgorithm.dilithium2,
                localizedFallbackTitle: 'Use Password',
                localizedFallbackDescription: 'Use password to access your identity',
            );
            
            return _did!;
        } catch (e) {
            throw SpaceKitAuthenticationException('Biometric authentication failed: $e');
        }
    }
    
    Future<ReputationInfo> getMyReputation() async {
        if (_did == null) throw SpaceKitException('Not authenticated');
        
        final result = await _sdk.callContract(
            contractAddress: '0x...', // Your contract
            function: 'get_my_reputation',
            params: {'user_did': _did!.identifier},
        );
        
        return ReputationInfo.fromJson(result);
    }
    
    Future<ComputeResult> submitComputeTask({
        required String code,
        required List<int> data,
        ComputeType type = ComputeType.cpu,
    }) async {
        if (_did == null) throw SpaceKitException('Not authenticated');
        
        // Sign the task with biometrics
        final signature = await _sdk.signWithBiometrics(
            data: '$code${data.join(',')}',
            reason: 'Sign compute task submission',
        );
        
        final result = await _sdk.submitComputeTask(
            userDID: _did!.identifier,
            code: code,
            data: data,
            signature: signature,
            computeType: type,
        );
        
        return result;
    }
}

// Usage in a Flutter widget
class ReputationWidget extends StatefulWidget {
    @override
    _ReputationWidgetState createState() => _ReputationWidgetState();
}

class _ReputationWidgetState extends State<ReputationWidget> {
    final SpaceKitService _spacekitService = SpaceKitService();
    ReputationInfo? _reputation;
    bool _loading = false;
    
    @override
    void initState() {
        super.initState();
        _loadReputation();
    }
    
    Future<void> _loadReputation() async {
        setState(() => _loading = true);
        
        try {
            // Authenticate if needed
            if (_spacekitService._did == null) {
                await _spacekitService.authenticateWithBiometrics();
            }
            
            final reputation = await _spacekitService.getMyReputation();
            setState(() => _reputation = reputation);
        } catch (e) {
            ScaffoldMessenger.of(context).showSnackBar(
                SnackBar(content: Text('Failed to load reputation: $e')),
            );
        } finally {
            setState(() => _loading = false);
        }
    }
    
    @override
    Widget build(BuildContext context) {
        if (_loading) {
            return Center(child: CircularProgressIndicator());
        }
        
        if (_reputation == null) {
            return Center(
                child: ElevatedButton(
                    onPressed: _loadReputation,
                    child: Text('Load Reputation'),
                ),
            );
        }
        
        return Card(
            child: Padding(
                padding: EdgeInsets.all(16),
                child: Column(
                    children: [
                        Text(
                            'Reputation Score',
                            style: Theme.of(context).textTheme.headline6,
                        ),
                        SizedBox(height: 8),
                        Text(
                            _reputation!.score.toStringAsFixed(2),
                            style: Theme.of(context).textTheme.headline4,
                        ),
                        Text(_reputation!.tier),
                        SizedBox(height: 16),
                        ...(_reputation!.benefits.map((benefit) => 
                            ListTile(
                                leading: Icon(Icons.check_circle, color: Colors.green),
                                title: Text(benefit),
                            )
                        )),
                    ],
                ),
            ),
        );
    }
}
```

---

## 🧪 **Testing & Debugging**

### **Unit Testing Contracts**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use spacekit_sdk::testing::*;
    
    #[spacekit_test]
    async fn test_reputation_based_access() {
        // Setup test environment
        let mut contract = HelloIdentity::new();
        let test_user = TestDID::create("alice");
        
        // Test initial reputation (should be 0.5 for new users)
        let initial_reputation = contract.get_reputation(test_user.did()).await;
        assert_eq!(initial_reputation.score, 0.5);
        assert_eq!(initial_reputation.tier, "Silver 🥈");
        
        // Test greeting functionality
        let greeting_result = contract.greet(test_user.did(), "Alice".to_string()).await;
        assert!(greeting_result.greeting.contains("Hi Alice!"));
        assert_eq!(greeting_result.reputation, 0.51); // +0.01 bonus
        
        // Test multiple interactions
        for i in 0..30 {
            contract.greet(test_user.did(), format!("Interaction {}", i)).await;
        }
        
        let final_reputation = contract.get_reputation(test_user.did()).await;
        assert!(final_reputation.score > 0.8); // Should be Gold tier now
        assert_eq!(final_reputation.tier, "Gold 🥇");
    }
    
    #[spacekit_test]
    async fn test_multi_user_interactions() {
        let mut contract = HelloIdentity::new();
        let alice = TestDID::create("alice");
        let bob = TestDID::create("bob");
        let charlie = TestDID::create("charlie");
        
        // All users interact
        contract.greet(alice.did(), "Alice".to_string()).await;
        contract.greet(bob.did(), "Bob".to_string()).await;
        contract.greet(charlie.did(), "Charlie".to_string()).await;
        
        // Verify independent reputation tracking
        let alice_rep = contract.get_reputation(alice.did()).await;
        let bob_rep = contract.get_reputation(bob.did()).await;
        let charlie_rep = contract.get_reputation(charlie.did()).await;
        
        assert_eq!(alice_rep.score, 0.51);
        assert_eq!(bob_rep.score, 0.51);
        assert_eq!(charlie_rep.score, 0.51);
        
        // Test reputation affects greeting
        // Give Alice high reputation
        for _ in 0..50 {
            contract.greet(alice.did(), "Alice".to_string()).await;
        }
        
        let alice_final = contract.greet(alice.did(), "Alice VIP".to_string()).await;
        assert!(alice_final.greeting.contains("👑 Welcome back, esteemed"));
    }
    
    #[spacekit_test]
    async fn test_gpu_computation() {
        let mut contract = GPUComputeService::new();
        let user = TestDID::create("gpu_user");
        
        // Submit a matrix multiplication job
        let job_data = ComputeJobData::MatrixMultiplication {
            a: Matrix::random(100, 100),
            b: Matrix::random(100, 100),
        };
        
        // Fund the user account for payment
        TestEnvironment::fund_account(user.address(), 1000000);
        
        let submission = contract.submit_job(user.did(), job_data).await;
        assert!(submission.estimated_cost > 0);
        
        // Execute the job
        let result = contract.execute_computation(submission.job_id).await;
        assert!(result.execution_time > 0);
        assert_eq!(result.result_data.len(), 10000); // 100x100 matrix
        assert!(result.verified);
    }
}
```

### **Integration Testing**

```rust
// tests/integration_tests.rs
use spacekit_sdk::testing::*;

#[spacekit_integration_test]
async fn test_full_reputation_workflow() {
    // Deploy contracts to test network
    let hello_contract = deploy_contract::<HelloIdentity>().await;
    let reputation_service = deploy_contract::<ReputationGatedService>().await;
    
    // Create test user
    let user = TestUser::create_with_funds("test_user", 1000000).await;
    
    // Step 1: Build reputation in hello contract
    for i in 0..20 {
        hello_contract.greet(user.did(), format!("Interaction {}", i)).await;
    }
    
    let reputation = hello_contract.get_reputation(user.did()).await;
    assert!(reputation.score > 0.7); // Should be Gold tier
    
    // Step 2: Try to access premium service
    let access_result = reputation_service.request_premium_access(
        user.did(),
        ServiceType::Premium
    ).await;
    
    match access_result {
        AccessResult::Granted { access_token, .. } => {
            assert!(!access_token.is_empty());
        },
        AccessResult::Denied { .. } => {
            panic!("Should have been granted access with high reputation");
        }
    }
}

#[spacekit_integration_test]
async fn test_cross_chain_integration() {
    // Test cross-chain DID verification
    let user = TestUser::create("cross_chain_user").await;
    
    // Register DID on multiple test chains
    let ethereum_registration = user.register_on_chain(ChainID::EthereumTestnet).await;
    let polygon_registration = user.register_on_chain(ChainID::PolygonTestnet).await;
    
    assert!(ethereum_registration.success);
    assert!(polygon_registration.success);
    
    // Verify cross-chain identity consistency
    let ethereum_did = TestChain::ethereum().get_did(user.address()).await;
    let polygon_did = TestChain::polygon().get_did(user.address()).await;
    
    assert_eq!(ethereum_did.identifier, polygon_did.identifier);
    assert_eq!(ethereum_did.public_key(), polygon_did.public_key());
}
```

### **Frontend Testing**

```typescript
// tests/SpaceKitProvider.test.tsx
import { render, screen, waitFor } from '@testing-library/react';
import { SpaceKitProvider, useSpaceKit } from '../components/SpaceKitProvider';
import { SpaceKitTestingProvider } from '@spacekit/testing';

// Mock component to test the provider
const TestComponent = () => {
    const { sdk, did, isConnected, connect } = useSpaceKit();
    
    return (
        <div>
            <div data-testid="connection-status">
                {isConnected ? 'Connected' : 'Not Connected'}
            </div>
            <div data-testid="did-identifier">
                {did?.identifier || 'No DID'}
            </div>
            <button onClick={connect} data-testid="connect-button">
                Connect
            </button>
        </div>
    );
};

describe('SpaceKitProvider', () => {
    it('should initialize SDK and handle connection', async () => {
        render(
            <SpaceKitTestingProvider>
                <SpaceKitProvider>
                    <TestComponent />
                </SpaceKitProvider>
            </SpaceKitTestingProvider>
        );
        
        // Initially not connected
        expect(screen.getByTestId('connection-status')).toHaveTextContent('Not Connected');
        expect(screen.getByTestId('did-identifier')).toHaveTextContent('No DID');
        
        // Click connect
        const connectButton = screen.getByTestId('connect-button');
        connectButton.click();
        
        // Wait for connection
        await waitFor(() => {
            expect(screen.getByTestId('connection-status')).toHaveTextContent('Connected');
        });
        
        // Should have DID
        const didElement = screen.getByTestId('did-identifier');
        expect(didElement.textContent).toMatch(/^did:swtch:quantum:/);
    });
    
    it('should handle contract calls', async () => {
        const { result } = renderHook(() => useSpaceKit(), {
            wrapper: ({ children }) => (
                <SpaceKitTestingProvider>
                    <SpaceKitProvider>
                        {children}
                    </SpaceKitProvider>
                </SpaceKitTestingProvider>
            ),
        });
        
        // Connect first
        await act(async () => {
            await result.current.connect();
        });
        
        // Call contract
        const contractResult = await result.current.sdk!.callContract({
            contractAddress: '0x123...', // Test contract
            function: 'get_my_reputation',
            params: { user_did: result.current.did!.identifier },
        });
        
        expect(contractResult.score).toBeGreaterThanOrEqual(0);
        expect(contractResult.tier).toBeDefined();
    });
});
```

---

## 📚 **API Reference**

### **Core SDK Functions**

#### **DID Management**

```typescript
interface SpaceKitSDK {
    // DID Creation
    createDID(options: DIDCreationOptions): Promise<DID>;
    restoreDID(backup: DIDBackup): Promise<DID>;
    
    // Verification
    verifyDID(did: string): Promise<VerificationResult>;
    verifySignature(message: string, signature: string, publicKey: string): Promise<boolean>;
    
    // Storage
    storeDID(did: DID): Promise<void>;
    getStoredDID(): Promise<DID | null>;
    clearStoredDID(): Promise<void>;
    
    // Contract Interaction
    callContract(options: ContractCallOptions): Promise<any>;
    deployContract(bytecode: string, constructor_args: any[]): Promise<DeploymentResult>;
    
    // Compute Tasks
    submitComputeTask(options: ComputeTaskOptions): Promise<TaskSubmission>;
    getTaskResult(taskId: string): Promise<ComputeResult>;
    
    // Cross-Chain
    bridgeToChain(options: BridgeOptions): Promise<BridgeResult>;
    getChainStatus(chainId: ChainID): Promise<ChainStatus>;
}
```

#### **Contract Annotations**

```rust
// Contract-level annotations
#[spacekit_contract]           // Mark as SpaceKit smart contract
#[did_enabled]              // Enable DID verification functions
#[gpu_enabled]              // Enable GPU computation functions
#[cross_chain_enabled]      // Enable cross-chain functionality

// Function-level annotations
#[spacekit_function("name")]   // Public contract function
#[spacekit_view]              // Read-only function (no state changes)
#[swtch_payable]           // Function can receive payments
#[spacekit_gpu_compute]       // Function uses GPU computation
#[spacekit_deterministic]     // Function guarantees deterministic results
#[spacekit_modifier(name)]    // Apply modifier for access control

// Event annotations
#[spacekit_event]             // Define contract event
```

### **Built-in Functions**

```rust
// Identity Functions
fn spacekit_verify_did(did: DID) -> Result<VerificationResult>;
fn swtch_verify_did_high_security(did: DID) -> Result<VerificationResult>;
fn spacekit_sign_quantum_safe(did: DID, data: &[u8]) -> Result<Signature>;

// Context Functions
fn spacekit_caller() -> DID;           // Get caller's DID
fn spacekit_value() -> u128;           // Get payment amount
fn spacekit_now() -> Timestamp;        // Get current timestamp
fn spacekit_gas_used() -> u64;         // Get gas consumed

// GPU Functions
fn spacekit_gpu_execute(kernel: &str, args: &[&[u8]]) -> Result<Vec<u8>>;
fn spacekit_gpu_neural_network_execute(model: &AIModel, input: &[f32]) -> Result<Vec<f32>>;
fn spacekit_get_gpu_info() -> GPUInfo;

// Cross-Chain Functions
fn spacekit_bridge_to_chain(chain: ChainID, data: &[u8]) -> Result<BridgeResult>;
fn spacekit_get_chain_state(chain: ChainID) -> Result<ChainState>;
```

---

## 🎯 **Best Practices**

### **Security Best Practices**

#### **DID Security**
```rust
// ✅ Always verify DID before sensitive operations
let verified_user = spacekit_verify_did(user_did)?;
require!(verified_user.is_verified(), "DID verification failed");
require!(verified_user.confidence_score() > 0.8, "Low confidence verification");

// ✅ Use high security verification for critical operations
let high_sec_verification = spacekit_verify_did_high_security(user_did)?;
require!(high_sec_verification.quantum_safe(), "Quantum-safe verification required");

// ❌ Don't skip verification
// Bad: directly using user_did without verification
```

#### **Reputation Management**
```rust
// ✅ Implement gradual reputation changes
fn update_reputation(&mut self, user_did: DID, change: f64) {
    let current = self.get_reputation(user_did);
    let max_change = 0.1; // Limit to prevent gaming
    let bounded_change = change.clamp(-max_change, max_change);
    self.set_reputation(user_did, current + bounded_change);
}

// ✅ Require verification for reputation-sensitive operations
require!(user_reputation > required_threshold, "Insufficient reputation");

// ❌ Don't allow unlimited reputation changes
// Bad: self.reputation += user_provided_value;
```

#### **Access Control**
```rust
// ✅ Use modifiers for consistent access control
#[spacekit_modifier(only_owner)]
fn only_owner(&self) -> bool {
    spacekit_caller() == self.owner
}

// ✅ Multi-factor authorization for critical functions
#[spacekit_function("critical_operation")]
#[swtch_modifier(only_owner)]
#[spacekit_modifier(high_security_verification)]
pub fn critical_function(&mut self) -> Result<()> {
    // Critical operation logic
}
```

### **Performance Best Practices**

#### **GPU Optimization**
```rust
// ✅ Batch operations for GPU efficiency
#[spacekit_function("batch_process")]
#[spacekit_gpu_compute]
pub fn process_batch(&mut self, data_batch: Vec<Vec<f32>>) -> Vec<ProcessResult> {
    // Process all data in a single GPU call
    spacekit_gpu_batch_process(&data_batch)
}

// ✅ Check data size before GPU operations
require!(data.len() > GPU_THRESHOLD, "Use CPU for small datasets");

// ❌ Don't use GPU for small operations
// Bad: GPU call for single float multiplication
```

#### **Gas Optimization**
```rust
// ✅ Use view functions for read-only operations
#[spacekit_function("get_user_data")]
#[spacekit_view]
pub fn get_user_data(&self, user_did: DID) -> UserData {
    // No gas cost for reading
}

// ✅ Batch multiple operations
pub fn batch_update(&mut self, updates: Vec<Update>) -> BatchResult {
    // Single transaction for multiple updates
}

// ✅ Cache expensive computations
fn get_expensive_computation(&self, input: &[u8]) -> Result<ComputeResult> {
    if let Some(cached) = self.cache.get(input) {
        return Ok(cached.clone());
    }
    
    let result = self.perform_expensive_computation(input)?;
    self.cache.insert(input.to_vec(), result.clone());
    Ok(result)
}
```

### **Cross-Platform Best Practices**

#### **Consistent Error Handling**
```typescript
// ✅ Consistent error types across platforms
interface SpaceKitError {
    code: string;
    message: string;
    details?: any;
}

// ✅ Graceful fallbacks
async function callWithFallback<T>(
    primary: () => Promise<T>,
    fallback: () => Promise<T>
): Promise<T> {
    try {
        return await primary();
    } catch (error) {
        console.warn('Primary method failed, using fallback:', error);
        return await fallback();
    }
}
```

#### **State Management**
```typescript
// ✅ Keep DID state synchronized across components
const SpaceKitContext = createContext<{
    did: DID | null;
    reputation: ReputationInfo | null;
    updateReputation: () => Promise<void>;
}>();

// ✅ Persist important state
useEffect(() => {
    if (did) {
        localStorage.setItem('spacekit_did_backup', JSON.stringify(did.backup()));
    }
}, [did]);
```

---

## 🚀 **Deployment Guide**

### **Local Development**

```bash
# Start local SpaceKit node
spacekit node start --network=local

# Deploy your contracts
spacekit deploy --network=local --contract=./src/contracts/

# Start development server with hot reload
spacekit dev --port=3000
```

### **Testnet Deployment**

```bash
# Configure testnet
spacekit config set-network testnet
spacekit config set-api-key YOUR_API_KEY

# Deploy to testnet
spacekit deploy --network=testnet --verify

# Verify deployment
spacekit verify-contract 0x... --network=testnet
```

### **Mainnet Deployment**

```toml
    # spacekit.toml
[network.mainnet]
rpc_url = "https://rpc.spacekit.xyz"
api_key = "${SPACEKIT_API_KEY}"
gas_price = "auto"
confirmations = 3

[contracts]
verify_on_deploy = true
optimization = true
quantum_safe = true
```

```bash
# Deploy to mainnet (requires verification)
spacekit deploy --network=mainnet --confirm-production
```

---

## 📖 **Learning Resources**

### **Example Projects**
- [Identity-Based Marketplace](./examples/marketplace/) - Full marketplace with reputation system
- [Cross-Chain DID Registry](./examples/cross-chain-registry/) - Multi-chain identity management
- [GPU Compute Service](./examples/gpu-service/) - Distributed GPU computation
- [Medical Records System](./examples/medical-records/) - HIPAA-compliant patient data

### **Advanced Tutorials**
- [Building Your First DID-Enabled DApp](./tutorials/first-dapp.md)
- [Cross-Platform Identity Integration](./tutorials/cross-platform.md)
- [GPU-Accelerated Smart Contracts](./tutorials/gpu-contracts.md)
- [Reputation System Design](./tutorials/reputation-systems.md)

### **Community Resources**
- [SpaceKit Developer Discord](https://discord.gg/spacekit-network)
- [Developer Forum](https://forum.spacekit.xyz)
- [GitHub Discussions](https://github.com/spacekit-network/discussions)
- [Stack Overflow Tag: spacekit](https://stackoverflow.com/questions/tagged/spacekit-network)

---

## 🎯 **Next Steps**

### **Beginner Path** 
1. ✅ Complete the Quick Start guide
2. 📖 Read [DID Integration Tutorial](./tutorials/did-integration.md)
3. 🏗️ Build the Hello Identity contract
4. 🌐 Deploy to testnet
5. 📱 Integrate with a frontend

### **Intermediate Path**
1. 🔧 Build a reputation-based service
2. 🖥️ Add GPU computation features
3. 🌉 Implement cross-chain functionality
4. 📊 Add comprehensive monitoring
5. 🧪 Write extensive tests

### **Advanced Path**
1. 🏢 Design enterprise-grade applications
2. 🔒 Implement custom consensus mechanisms
3. 🌍 Build cross-platform SDKs
4. 🤖 Integrate AI/ML capabilities
5. 🌟 Contribute to the SWTCH ecosystem

---

## 🆘 **Support & Troubleshooting**

### **Common Issues**

**DID Verification Failing:**
```bash
# Check network connection
spacekit network status

# Verify DID format
spacekit validate-did did:spacekit:quantum:abc123...

# Test with different verification level
spacekit verify-did --level=basic did:spacekit:quantum:abc123...
```

**Contract Deployment Issues:**
```bash
# Check gas estimation
spacekit estimate-gas --contract=./contract.rs

# Verify contract syntax
spacekit compile --check-only

# Deploy with verbose logging
spacekit deploy --verbose --network=testnet
```

**Cross-Platform SDK Issues:**
```typescript
// Enable debug mode
const sdk = new SpaceKitSDK({
    debug: true,
    logLevel: 'verbose'
});

// Check SDK version compatibility
console.log('SDK Version:', sdk.version);
console.log('Network Version:', await sdk.getNetworkVersion());
```

### **Getting Help**
- 📧 Email: [dev-support@spacekit.xyz](mailto:dev-support@spacekit.xyz)
- 💬 Discord: [SpaceKit Developer Community](https://discord.gg/spacekit-network)
- 📝 GitHub Issues: [spacekit-network/spacekit-compute-node](https://github.com/spacekit-network/spacekit-compute-node/issues)
- 📖 Documentation: [docs.spacekit.xyz](https://docs.spacekit.xyz)

---

**🌟 Welcome to the Future of Identity-Native Computing!** 

You're now equipped with everything you need to build revolutionary applications with SpaceKit. The combination of quantum-safe DIDs, reputation-based resource allocation, and cross-platform compatibility opens up possibilities that have never existed before in blockchain development.

*Happy Building! 🚀* 