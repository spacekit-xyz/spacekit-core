//! Growformer-backed consensus tuning (parameter ratification).
//!
//! Wire format and ratification logic live in [`growformer_ratification`].
//! Host-side agent loading (storage node, `.spacekit` manifest, periodic
//! inference) is implemented in `spacekit-compute-node::consensus_growformer_agent`.

pub mod growformer_ratification;

pub use growformer_ratification::{
    evaluate_ratification, validator_should_ratify, ActivatedParameterChange, GrowformerInference,
    GrowformerIntent, MalignRatificationEvidence, ParameterChangeProposal, ParameterChangeVote,
    PolicyRegime, RatificationConfig, RatificationError,
};
