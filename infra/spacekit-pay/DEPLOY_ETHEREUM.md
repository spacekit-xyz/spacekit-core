# Deploy SpaceKit Pay on Ethereum (quick reference)

Full guide for operators and publishers: **[MARKETPLACE_PAY_SETUP.md](./MARKETPLACE_PAY_SETUP.md)**

Non-custodial USDC: **95% publisher / 5% treasury**. aUSD deprecated for marketplace.

## Deploy

```bash
# 🔑 Private Key (Hex): "cd8d149db1f058b6263388629ba746785b0ab8635b9fcdde306a5603164b8dc5"
# 🔑 Public Key (Hex): "04ab16bacdce970352dfa8aebf2f4545252596f3d1e3a036acb4212b591c08aaa99d6d986c34eb87df0ee2678b44dd56fccdf229324d8680806ddd11235385e81e"
# 📍 Ethereum Address: 0x2e6aD219eDff34f1a04Ad1687a38ADB9A39E3EA2
cd spacekit.xyz-contracts
export RPC_URL=https://mainnet.infura.io/v3/a7ff5194ff604a059120620729a5b47f
export PRIVATE_KEY=0xcd8d149db1f058b6263388629ba746785b0ab8635b9fcdde306a5603164b8dc5
export TREASURY_ADDRESS=0x...
export REGISTRAR_ADDRESS=0x...    # admin wallet (manual) or hot wallet (API relayer)
export USDC_ADDRESS=0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48

forge script script/DeploySpaceKitPayEthereum.s.sol:DeploySpaceKitPayEthereum \
  --rpc-url "$RPC_URL" --broadcast
```

## Env — website-api

```bash
ETH_RPC_URL=...
SPACEKIT_PAY_ROUTER_ADDRESS=0x...
OPERATOR_REGISTRY_ADDRESS=0x...
USDC_ADDRESS=0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48
ETH_CHAIN_ID=1
MARKETPLACE_VERIFY_PAYMENTS=true

# Optional — only for Account self-service payout (NOT required at launch)
# PAY_REGISTRAR_PRIVATE_KEY=0x...
```

## Env — website

```bash
VITE_SPACEKIT_PAY_ROUTER_ADDRESS=0x...
VITE_SPACEKIT_OPERATOR_REGISTRY_ADDRESS=0x...
```

## Early stage (0 customers)

Skip `PAY_REGISTRAR_PRIVATE_KEY`. Register payouts manually:

```bash
cast send $REGISTRY "registerOperator(string,address)" \
  "did:spacekit:user:USERNAME" 0xPAYOUT_WALLET \
  --rpc-url $RPC_URL --private-key $REGISTRAR_KEY
```

See [MARKETPLACE_PAY_SETUP.md](./MARKETPLACE_PAY_SETUP.md) for architecture, security, and publisher steps.
