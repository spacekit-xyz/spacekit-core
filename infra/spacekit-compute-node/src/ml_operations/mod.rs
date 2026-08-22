// ML Operations - Concrete implementations of MLOperation trait

pub mod distilbert_sentiment;
pub mod embeddings;
pub mod sentence_transformers;
pub mod text_generation;
pub mod trm_recursive_reasoning;

pub use distilbert_sentiment::DistilBertSentimentOperation;
pub use embeddings::EmbeddingsOperation;
pub use sentence_transformers::SentenceTransformersOperation;
pub use text_generation::TextGenerationOperation;
pub use trm_recursive_reasoning::{TRMOperationFactory, TRMRecursiveReasoningOperation};
