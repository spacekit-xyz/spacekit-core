# Production Model Integration Examples

This document shows how to replace the simulated inference with real ML models.

## Option 1: Using Candle (Hugging Face Models)

### Install Dependencies

Add to `Cargo.toml`:
```toml
[dependencies]
candle-core = "0.8"
candle-nn = "0.8"
candle-transformers = "0.8"
tokenizers = "0.15"
hf-hub = "0.3"
```

### Example: Llama Model

```rust
use candle_core::{Device, Tensor};
use candle_transformers::models::llama::{Llama, LlamaConfig};
use tokenizers::Tokenizer;

struct ModelInference {
    model: Llama,
    tokenizer: Tokenizer,
    device: Device,
}

impl ModelInference {
    async fn new(model_path: &str) -> anyhow::Result<Self> {
        // Select device (GPU if available, otherwise CPU)
        let device = Device::cuda_if_available(0)?;
        
        // Load tokenizer
        let tokenizer = Tokenizer::from_file(
            format!("{}/tokenizer.json", model_path)
        )?;
        
        // Load model config
        let config: LlamaConfig = serde_json::from_reader(
            std::fs::File::open(format!("{}/config.json", model_path))?
        )?;
        
        // Load model weights
        let weights = candle_core::safetensors::load(
            format!("{}/model.safetensors", model_path),
            &device
        )?;
        
        let model = Llama::load(weights, config, &device)?;
        
        Ok(Self { model, tokenizer, device })
    }
    
    async fn generate(&self, prompt: &str, max_tokens: usize) -> anyhow::Result<String> {
        // Tokenize input
        let encoding = self.tokenizer.encode(prompt, true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;
        
        let tokens = encoding.get_ids();
        let input_ids = Tensor::new(tokens, &self.device)?
            .unsqueeze(0)?;
        
        // Generate
        let mut output_tokens = tokens.to_vec();
        
        for _ in 0..max_tokens {
            let logits = self.model.forward(&input_ids)?;
            let next_token = logits
                .argmax(candle_core::D::Minus1)?
                .to_scalar::<u32>()?;
            
            output_tokens.push(next_token);
            
            // Stop on EOS token
            if next_token == self.tokenizer.get_vocab(true).get("<|endoftext|>").copied().unwrap_or(0) {
                break;
            }
            
            // Update input for next iteration
            let input_ids = Tensor::new(&output_tokens, &self.device)?
                .unsqueeze(0)?;
        }
        
        // Decode
        let output = self.tokenizer.decode(&output_tokens, true)
            .map_err(|e| anyhow::anyhow!("Decoding failed: {}", e))?;
        
        Ok(output)
    }
}
```

### Updated Worker with Real Model

```rust
use std::sync::Arc;
use tokio::sync::Mutex;

struct WorkerState {
    worker_id: String,
    status: RwLock<WorkerStatus>,
    device_type: DeviceType,
    model: Arc<Mutex<ModelInference>>, // Add model
}

impl WorkerState {
    async fn new(
        worker_id: String,
        device_type: DeviceType,
        model_path: &str,
    ) -> anyhow::Result<Self> {
        let model = ModelInference::new(model_path).await?;
        
        Ok(Self {
            worker_id,
            status: RwLock::new(WorkerStatus::Ready),
            device_type,
            model: Arc::new(Mutex::new(model)),
        })
    }
}

async fn infer(
    State(state): State<Arc<WorkerState>>,
    Json(request): Json<InferenceRequest>,
) -> Json<InferenceResponse> {
    *state.status.write().await = WorkerStatus::Busy;
    
    let start = std::time::Instant::now();
    
    // Use real model
    let model = state.model.lock().await;
    let result = model.generate(
        &request.prompt,
        request.max_tokens.unwrap_or(100)
    ).await.unwrap_or_else(|e| format!("Error: {}", e));
    
    drop(model); // Release lock
    
    let processing_time_ms = start.elapsed().as_millis() as u64;
    *state.status.write().await = WorkerStatus::Ready;
    
    Json(InferenceResponse {
        request_id: Uuid::new_v4().to_string(),
        result,
        worker_id: state.worker_id.clone(),
        processing_time_ms,
    })
}
```

## Option 2: Using ONNX Runtime

### Install Dependencies

```toml
[dependencies]
ort = "2.0"
ndarray = "0.15"
```

### Example: BERT Model

```rust
use ort::{Environment, SessionBuilder, Value, GraphOptimizationLevel};
use ndarray::{Array, IxDyn};

struct OnnxInference {
    session: ort::Session,
}

impl OnnxInference {
    fn new(model_path: &str) -> anyhow::Result<Self> {
        let environment = Environment::builder()
            .with_name("inference")
            .build()?;
        
        let session = SessionBuilder::new(&environment)?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_model_from_file(model_path)?;
        
        Ok(Self { session })
    }
    
    fn run(&self, input_ids: Vec<i64>) -> anyhow::Result<Vec<f32>> {
        let array = Array::from_shape_vec(
            IxDyn(&[1, input_ids.len()]),
            input_ids
        )?;
        
        let input = Value::from_array(self.session.allocator(), &array)?;
        let outputs = self.session.run(vec![input])?;
        
        let output: ort::Value = outputs[0].try_extract()?;
        let output_array = output.view();
        
        Ok(output_array.iter().copied().collect())
    }
}
```

## Option 3: Using PyTorch via tch-rs

### Install Dependencies

```toml
[dependencies]
tch = "0.15"
```

### Example: Loading PyTorch Model

```rust
use tch::{nn, Device, Tensor};

struct TorchInference {
    model: nn::Sequential,
    device: Device,
}

impl TorchInference {
    fn new(model_path: &str) -> anyhow::Result<Self> {
        let device = Device::cuda_if_available();
        let vs = nn::VarStore::new(device);
        
        // Load model
        vs.load(model_path)?;
        
        let model = nn::seq()
            .add(nn::linear(&vs.root(), 768, 512, Default::default()))
            .add_fn(|x| x.relu())
            .add(nn::linear(&vs.root(), 512, 256, Default::default()));
        
        Ok(Self { model, device })
    }
    
    fn forward(&self, input: &[f32]) -> anyhow::Result<Vec<f32>> {
        let tensor = Tensor::of_slice(input).to(self.device);
        let output = self.model.forward(&tensor);
        
        Ok(Vec::from(output))
    }
}
```

## Model Downloading

### Using Hugging Face Hub

```rust
use hf_hub::api::sync::Api;

async fn download_model(model_id: &str, local_dir: &str) -> anyhow::Result<String> {
    let api = Api::new()?;
    let repo = api.model(model_id.to_string());
    
    // Download model files
    let model_file = repo.get("model.safetensors")?;
    let config_file = repo.get("config.json")?;
    let tokenizer_file = repo.get("tokenizer.json")?;
    
    Ok(local_dir.to_string())
}

// Usage:
// download_model("meta-llama/Llama-2-7b-hf", "./models/llama").await?;
```

## Worker Initialization with Model

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    
    // Download or load model
    let model_path = match args.model_path {
        Some(path) => path,
        None => {
            info!("Downloading model...");
            download_model(&args.model_id, "./models").await?
        }
    };
    
    // Initialize worker with model
    let state = Arc::new(
        WorkerState::new(
            worker_id,
            device_type,
            &model_path
        ).await?
    );
    
    // ... rest of server setup
}
```

## Performance Optimization

### 1. Model Caching
```rust
lazy_static! {
    static ref MODEL: Arc<Mutex<ModelInference>> = {
        Arc::new(Mutex::new(
            ModelInference::new("./models/llama").await.unwrap()
        ))
    };
}
```

### 2. Request Batching
```rust
struct BatchProcessor {
    batch_size: usize,
    queue: VecDeque<InferenceRequest>,
}

impl BatchProcessor {
    async fn process_batch(&mut self) -> Vec<InferenceResponse> {
        // Collect requests
        let batch: Vec<_> = self.queue.drain(..self.batch_size).collect();
        
        // Process in parallel on GPU
        // ... batched inference
    }
}
```

### 3. Model Quantization
```rust
use candle_core::quantized::QTensor;

// Load quantized model for faster inference
let model = load_quantized_model("model.gguf")?;
```

## GPU Support

### CUDA Setup
```bash
# Install CUDA toolkit
# Set environment variables
export LIBTORCH_USE_PYTORCH=1
export LD_LIBRARY_PATH=/usr/local/cuda/lib64:$LD_LIBRARY_PATH
```

### Multi-GPU Support
```rust
let devices = Device::all_cuda_devices()?;

// Distribute models across GPUs
for (i, device) in devices.iter().enumerate() {
    let model = load_model_on_device(device)?;
    workers.push(Worker::new(model, i));
}
```

## Testing with Real Models

```bash
# Download a small model for testing
./download_model.sh "gpt2"

# Start worker with model
cargo run --bin worker -- \
    --port 8080 \
    --model-path ./models/gpt2 \
    --device gpu

# Test inference
cargo run --bin test_client -- \
    --prompt "Once upon a time" \
    --max-tokens 50
```

## Production Checklist

- [ ] Model loading on startup
- [ ] Error handling for model failures
- [ ] Model versioning
- [ ] Health checks include model status
- [ ] GPU memory monitoring
- [ ] Request batching
- [ ] Model quantization for speed
- [ ] Automatic model updates
- [ ] Fallback to CPU on GPU failure
- [ ] Model warmup on startup
