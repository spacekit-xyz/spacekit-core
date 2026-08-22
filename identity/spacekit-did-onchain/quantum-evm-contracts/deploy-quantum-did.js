const { ethers, network } = require("hardhat");

async function main() {
    console.log(`\n🚀 Deploying QuantumDIDRegistry to ${network.name}...`);
    
    const [deployer] = await ethers.getSigners();
    console.log(`📝 Deployer address: ${deployer.address}`);
    console.log(`💰 Deployer balance: ${ethers.utils.formatEther(await deployer.getBalance())} ETH`);

    // Deploy QuantumDIDRegistry
    console.log("\n📦 Deploying QuantumDIDRegistry...");
    const QuantumDIDRegistry = await ethers.getContractFactory("QuantumDIDRegistry");
    const quantumDIDRegistry = await QuantumDIDRegistry.deploy();
    
    await quantumDIDRegistry.deployed();
    console.log(`✅ QuantumDIDRegistry deployed to: ${quantumDIDRegistry.address}`);

    // Deploy QuantumCredentialManager
    console.log("\n📦 Deploying QuantumCredentialManager...");
    const QuantumCredentialManager = await ethers.getContractFactory("QuantumCredentialManager");
    const quantumCredentialManager = await QuantumCredentialManager.deploy(quantumDIDRegistry.address);
    
    await quantumCredentialManager.deployed();
    console.log(`✅ QuantumCredentialManager deployed to: ${quantumCredentialManager.address}`);

    // Save deployment addresses
    const deploymentInfo = {
        network: network.name,
        chainId: await deployer.getChainId(),
        contracts: {
            QuantumDIDRegistry: {
                address: quantumDIDRegistry.address,
                deployer: deployer.address
            },
            QuantumCredentialManager: {
                address: quantumCredentialManager.address,
                deployer: deployer.address
            }
        },
        deploymentTime: new Date().toISOString(),
        gasUsed: {
            QuantumDIDRegistry: await quantumDIDRegistry.deployTransaction.gasUsed,
            QuantumCredentialManager: await quantumCredentialManager.deployTransaction.gasUsed
        }
    };

    const fs = require('fs');
    const deploymentPath = `./deployments/${network.name}.json`;
    
    // Create deployments directory if it doesn't exist
    if (!fs.existsSync('./deployments')) {
        fs.mkdirSync('./deployments');
    }
    
    fs.writeFileSync(deploymentPath, JSON.stringify(deploymentInfo, null, 2));
    console.log(`\n📋 Deployment info saved to: ${deploymentPath}`);

    // Output SDK configuration
    console.log(`\n🔧 SDK Configuration for ${network.name.toUpperCase()}:`);
    console.log(`QUANTUM_DID_REGISTRY_ADDRESS=${quantumDIDRegistry.address}`);
    console.log(`QUANTUM_CREDENTIAL_MANAGER_ADDRESS=${quantumCredentialManager.address}`);
    
    if (network.name === "localhost" || network.name === "hardhat") {
        console.log(`ETHEREUM_RPC_URL=http://localhost:8545`);
    } else if (network.name === "mainnet") {
        console.log(`ETHEREUM_RPC_URL=https://mainnet.infura.io/v3/YOUR_INFURA_PROJECT_ID`);
    }

    console.log(`\n🎯 Next Steps:`);
    console.log(`1. Update your .env file with the contract addresses above`);
    console.log(`2. Register your service DID using the registerQuantumDID function`);
    console.log(`3. Update the API configuration to verify against this contract`);
    
    if (network.name === "mainnet") {
        console.log(`4. ⚠️  MAINNET DEPLOYMENT - Verify contracts on Etherscan!`);
    }

    console.log(`\n✅ Deployment Complete!`);
}

main()
    .then(() => process.exit(0))
    .catch((error) => {
        console.error(error);
        process.exit(1);
    }); 