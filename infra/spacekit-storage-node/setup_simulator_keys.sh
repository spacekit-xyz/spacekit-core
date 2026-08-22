#!/bin/bash

# Setup script for SpaceKit Simulator Database Encryption Keys
# This script helps set up encryption keys for spacekit-storage-node

set -e

echo "🔐 SpaceKit Storage Node - Key Setup"
echo "====================================="
echo ""

# Check if AWS Secrets Manager should be used
USE_AWS=${USE_AWS:-"false"}
SECRET_NAME=${DATABASE_KEM_SECRET_NAME:-"spacekit/simulator-database-keys"}
AWS_REGION=${AWS_DEFAULT_REGION:-"us-east-1"}

if [ "$USE_AWS" = "true" ]; then
    echo "📦 Setting up AWS Secrets Manager..."
    echo ""
    
    # Check if AWS CLI is installed
    if ! command -v aws &> /dev/null; then
        echo "❌ AWS CLI is not installed"
        echo "   Install it from: https://aws.amazon.com/cli/"
        exit 1
    fi
    
    # Check AWS credentials
    if ! aws sts get-caller-identity &> /dev/null; then
        echo "❌ AWS credentials not configured"
        echo "   Run: aws configure"
        exit 1
    fi
    
    echo "✅ AWS CLI configured"
    echo ""
    echo "🔑 Checking for existing keys in AWS Secrets Manager..."
    echo "   Secret name: $SECRET_NAME"
    echo "   Region: $AWS_REGION"
    echo ""
    
    # Check if secret exists
    if aws secretsmanager describe-secret --secret-id "$SECRET_NAME" --region "$AWS_REGION" &> /dev/null; then
        echo "✅ Keys already exist in AWS Secrets Manager"
        echo ""
        echo "📋 To use these keys, set:"
        echo "   export DATABASE_KEM_SECRET_NAME=\"$SECRET_NAME\""
        echo "   export AWS_DEFAULT_REGION=\"$AWS_REGION\""
        echo ""
        echo "💡 Keys will be automatically loaded when you run the simulator"
    else
        echo "⚠️  Keys not found in AWS Secrets Manager"
        echo ""
        echo "💡 Keys will be automatically generated and stored on first run"
        echo ""
        echo "📋 To enable AWS Secrets Manager, set:"
        echo "   export DATABASE_KEM_SECRET_NAME=\"$SECRET_NAME\""
        echo "   export AWS_DEFAULT_REGION=\"$AWS_REGION\""
        echo ""
        echo "   Then run your simulator - keys will be created automatically"
    fi
else
    echo "📁 Using Local File Storage (Development)"
    echo ""
    echo "💡 Keys will be automatically generated on first run"
    echo "   Location: ./your_storage_path/db.key"
    echo ""
    echo "✅ No setup needed - just run your simulator!"
    echo ""
    echo "💡 To use AWS Secrets Manager instead, run:"
    echo "   USE_AWS=true ./setup_simulator_keys.sh"
fi

echo ""
echo "🚀 Next Steps:"
echo "   1. Run your simulator: cargo run --example ai_companion_demo"
echo "   2. Keys will be created automatically if they don't exist"
echo "   3. Database encryption will work transparently"
echo ""
echo "📚 For more details, see: SIMULATOR_INTEGRATION_GUIDE.md"
echo ""

