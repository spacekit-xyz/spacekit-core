// SWTCH Network Primitives: Node Types and Structures
// Foundational node definitions for the entire SWTCH network ecosystem

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeType {
    /// General network validator nodes
    Validator,
    /// Storage network provider nodes
    StorageProvider,
    /// Compute network provider nodes  
    ComputeProvider,
    /// AI analysis and machine learning nodes
    AIAnalyst,
    /// Economic behavior validation nodes
    EconomicValidator,
    /// General service provider nodes
    ServiceProvider,
    /// Temporal pattern analysis nodes
    TemporalProvider,
    /// Messaging network nodes
    MessagingNode,
    /// Cortex AI orchestration nodes
    CortexNode,
    /// Identity registry and DID management nodes
    IdentityProvider,
    /// Cross-chain bridge and validation nodes
    BridgeValidator,
}

impl NodeType {
    /// Get the node type as a string identifier
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Validator => "validator",
            NodeType::StorageProvider => "storage_provider",
            NodeType::ComputeProvider => "compute_provider",
            NodeType::AIAnalyst => "ai_analyst",
            NodeType::EconomicValidator => "economic_validator",
            NodeType::ServiceProvider => "service_provider",
            NodeType::TemporalProvider => "temporal_provider",
            NodeType::MessagingNode => "messaging_node",
            NodeType::CortexNode => "cortex_node",
            NodeType::IdentityProvider => "identity_provider",
            NodeType::BridgeValidator => "bridge_validator",
        }
    }

    /// Get the node type from a string identifier
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "validator" => Some(NodeType::Validator),
            "storage_provider" => Some(NodeType::StorageProvider),
            "compute_provider" => Some(NodeType::ComputeProvider),
            "ai_analyst" => Some(NodeType::AIAnalyst),
            "economic_validator" => Some(NodeType::EconomicValidator),
            "service_provider" => Some(NodeType::ServiceProvider),
            "temporal_provider" => Some(NodeType::TemporalProvider),
            "messaging_node" => Some(NodeType::MessagingNode),
            "cortex_node" => Some(NodeType::CortexNode),
            "identity_provider" => Some(NodeType::IdentityProvider),
            "bridge_validator" => Some(NodeType::BridgeValidator),
            _ => None,
        }
    }

    /// Get all available node types
    pub fn all_types() -> Vec<NodeType> {
        vec![
            NodeType::Validator,
            NodeType::StorageProvider,
            NodeType::ComputeProvider,
            NodeType::AIAnalyst,
            NodeType::EconomicValidator,
            NodeType::ServiceProvider,
            NodeType::TemporalProvider,
            NodeType::MessagingNode,
            NodeType::CortexNode,
            NodeType::IdentityProvider,
            NodeType::BridgeValidator,
        ]
    }

    /// Check if this node type can participate in recovery verification
    pub fn can_verify_recovery(&self) -> bool {
        matches!(
            self,
            NodeType::Validator
                | NodeType::StorageProvider
                | NodeType::ComputeProvider
                | NodeType::AIAnalyst
                | NodeType::EconomicValidator
        )
    }

    /// Check if this node type can participate in consensus
    pub fn can_participate_in_consensus(&self) -> bool {
        matches!(
            self,
            NodeType::Validator
                | NodeType::StorageProvider
                | NodeType::ComputeProvider
                | NodeType::EconomicValidator
                | NodeType::BridgeValidator
        )
    }

    /// Get the domain of expertise for this node type
    pub fn expertise_domain(&self) -> &'static str {
        match self {
            NodeType::Validator => "general_validation",
            NodeType::StorageProvider => "storage_operations",
            NodeType::ComputeProvider => "compute_operations",
            NodeType::AIAnalyst => "behavioral_analysis",
            NodeType::EconomicValidator => "economic_validation",
            NodeType::ServiceProvider => "service_provision",
            NodeType::TemporalProvider => "temporal_analysis",
            NodeType::MessagingNode => "message_routing",
            NodeType::CortexNode => "ai_orchestration",
            NodeType::IdentityProvider => "identity_management",
            NodeType::BridgeValidator => "cross_chain_validation",
        }
    }
}

/// Network node structure with comprehensive metadata
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Unique node identifier
    pub node_id: String,
    /// Type of network node
    pub node_type: NodeType,
    /// Network endpoint for communication
    pub endpoint: Option<String>,
    /// Node reputation score (0.0 to 1.0)
    pub reputation: f64,
    /// Stake amount for consensus participation
    pub stake_amount: u64,
    /// Whether the node is currently active
    pub is_active: bool,
    /// Node version for compatibility checking
    pub version: String,
}

impl Node {
    /// Create a new node with default values
    pub fn new(node_id: String, node_type: NodeType) -> Self {
        Self {
            node_id,
            node_type,
            endpoint: None,
            reputation: 0.5, // Neutral starting reputation
            stake_amount: 0,
            is_active: false,
            version: "1.0.0".to_string(),
        }
    }

    /// Create a new node with full configuration
    pub fn with_config(
        node_id: String,
        node_type: NodeType,
        endpoint: Option<String>,
        reputation: f64,
        stake_amount: u64,
    ) -> Self {
        Self {
            node_id,
            node_type,
            endpoint,
            reputation: reputation.clamp(0.0, 1.0),
            stake_amount,
            is_active: true,
            version: "1.0.0".to_string(),
        }
    }

    /// Check if the node can participate in a specific operation
    pub fn can_participate(&self, operation: &str) -> bool {
        if !self.is_active {
            return false;
        }

        match operation {
            "recovery_verification" => self.node_type.can_verify_recovery(),
            "consensus" => self.node_type.can_participate_in_consensus(),
            "storage" => matches!(self.node_type, NodeType::StorageProvider),
            "compute" => matches!(self.node_type, NodeType::ComputeProvider),
            "ai_analysis" => matches!(self.node_type, NodeType::AIAnalyst | NodeType::CortexNode),
            "messaging" => matches!(self.node_type, NodeType::MessagingNode),
            "bridge" => matches!(self.node_type, NodeType::BridgeValidator),
            _ => false,
        }
    }

    /// Get node's voting weight based on stake and reputation
    pub fn voting_weight(&self) -> f64 {
        (self.stake_amount as f64 / 10000.0) * self.reputation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_type_string_conversion() {
        let node_type = NodeType::AIAnalyst;
        assert_eq!(node_type.as_str(), "ai_analyst");
        assert_eq!(NodeType::from_str("ai_analyst"), Some(NodeType::AIAnalyst));
    }

    #[test]
    fn test_node_creation() {
        let node = Node::new("test_node".to_string(), NodeType::Validator);
        assert_eq!(node.node_id, "test_node");
        assert_eq!(node.node_type, NodeType::Validator);
        assert_eq!(node.reputation, 0.5);
    }

    #[test]
    fn test_node_participation() {
        let validator = Node::with_config(
            "validator_1".to_string(),
            NodeType::Validator,
            None,
            0.8,
            1000,
        );

        assert!(validator.can_participate("recovery_verification"));
        assert!(validator.can_participate("consensus"));
        assert!(!validator.can_participate("storage"));
    }

    #[test]
    fn test_voting_weight_calculation() {
        let node = Node::with_config("test".to_string(), NodeType::Validator, None, 0.8, 5000);

        assert_eq!(node.voting_weight(), 0.4); // (5000 / 10000) * 0.8 = 0.4
    }
}
