#!/bin/bash

# SpaceKit Model Downloader
# Downloads LLM models in GGUF format for CPU inference

set -e

MODEL_DIR="./models"
mkdir -p "$MODEL_DIR"

echo "🤖 SpaceKit Model Downloader"
echo "========================="
echo "Downloads LLM models in GGUF format"
echo ""

# Function to download model
download_model() {
    local model_name=$1
    local url=$2
    local filename=$3
    
    if [ -f "$MODEL_DIR/$filename" ]; then
        echo "✅ $model_name already downloaded"
        return 0
    fi
    
    echo "📥 Downloading $model_name..."
    echo "   URL: $url"
    echo "   Size: This may take a while..."
    
    wget -q --show-progress "$url" -O "$MODEL_DIR/$filename"
    
    echo "✅ $model_name downloaded: $MODEL_DIR/$filename"
    echo ""
}

# BitNet b1.58-2B (Q8 quantized)
echo "1️⃣  BitNet b1.58-2B"
download_model "BitNet b1.58-2B (Q8)" \
    "https://huggingface.co/TheBloke/bitnet-b1.58-2B-GGUF/resolve/main/bitnet-b1.58-2b.Q8_0.gguf" \
    "bitnet-b1.58-2b-q8.gguf"

# Phi-2 (Q8 quantized)
echo "2️⃣  Phi-2"
download_model "Phi-2 (Q8)" \
    "https://huggingface.co/TheBloke/phi-2-GGUF/resolve/main/phi-2.Q8_0.gguf" \
    "phi-2-q8.gguf"

# Qwen 1.5 1.8B (Q8 quantized)
echo "3️⃣  Qwen 1.5 1.8B"
download_model "Qwen 1.5 1.8B (Q8)" \
    "https://huggingface.co/Qwen/Qwen1.5-1.8B-Chat-GGUF/resolve/main/qwen1_5-1_8b-chat-q8_0.gguf" \
    "qwen-1.5-1.8b-q8.gguf"

# Mistral 7B (Q4 quantized - smaller)
echo "4️⃣  Mistral 7B (Optional)"
read -p "Download Mistral 7B? (4GB, y/n): " download_mistral
if [ "$download_mistral" = "y" ]; then
    download_model "Mistral 7B (Q4)" \
        "https://huggingface.co/TheBloke/Mistral-7B-Instruct-v0.2-GGUF/resolve/main/mistral-7b-instruct-v0.2.Q4_K_M.gguf" \
        "mistral-7b-q4.gguf"
fi

# TinyLlama 1.1B (Q8 quantized)
echo "5️⃣  TinyLlama 1.1B"
download_model "TinyLlama 1.1B (Q8)" \
    "https://huggingface.co/TheBloke/TinyLlama-1.1B-Chat-v1.0-GGUF/resolve/main/tinyllama-1.1b-chat-v1.0.Q8_0.gguf" \
    "tinyllama-1.1b-q8.gguf"

echo ""
echo "🎉 Model Download Complete!"
echo ""
echo "📊 Downloaded Models:"
ls -lh "$MODEL_DIR"/*.gguf 2>/dev/null || echo "No models downloaded yet"
echo ""
echo "📋 Next Steps:"
echo "1. Enable models in model_config.yaml"
echo "2. Build with LLM support: cargo build --features llm"
echo "3. Run examples: cargo run --example phi2_integration_demo --features llm"
echo ""
echo "💾 Total Storage Used:"
du -sh "$MODEL_DIR"
