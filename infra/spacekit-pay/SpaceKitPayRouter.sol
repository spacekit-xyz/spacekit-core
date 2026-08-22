// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

/**
 * @title SpaceKitPayRouter
 * @notice Atomic, non-custodial payment routing for the AI economy.
 *
 *         A buyer pays an operator for an AI service. The router pulls the
 *         payment from the buyer (via prior approve), splits it 95/5 between
 *         operator and treasury, and forwards both atomically in the same
 *         transaction. The contract holds zero balance at the end of every
 *         successful call.
 *
 * @dev Design property: non-custodial. All three transfers happen in the
 *      same transaction (transferFrom from buyer, transfer to operator,
 *      transfer to treasury). If any step fails, the entire transaction
 *      reverts. The contract's balance after any successful payForService()
 *      call is exactly the same as before the call (modulo any tokens sent
 *      to the contract via direct transfer outside the protocol, which can
 *      be swept by the admin).
 *
 *      The OperatorRegistry contract is the source of truth for operator
 *      payout addresses on this network. The router looks up the operator's
 *      address by DID on every payment.
 *
 *      Sister deployments: this contract is deployed on multiple EVM chains
 *      (Ethereum, Base, Polygon, Arbitrum, Optimism) and on SpaceKit (as
 *      SKCL contracts). Each deployment routes same-network payments only.
 *      No cross-network bridging in v1.
 */
contract SpaceKitPayRouter is Ownable2Step, ReentrancyGuard {
    using SafeERC20 for IERC20;

    // ========================================================================
    // Events
    // ========================================================================

    event PaymentRouted(
        address indexed payer,
        bytes32 indexed operatorDID, // keccak256 of operator DID string
        address indexed operator,
        address token,
        uint256 amount,
        uint256 operatorCut,
        uint256 treasuryCut
    );

    event TreasuryAddressChanged(address indexed newTreasury);
    event TokenAllowlistUpdated(address indexed token, bool allowed);
    event OperatorRegistryUpdated(address indexed newRegistry);
    event StuckTokensSwept(address indexed token, uint256 amount, address indexed to);

    // ========================================================================
    // Storage
    // ========================================================================

    /// The treasury address that receives the 5% fee.
    address public treasury;

    /// The OperatorRegistry contract for looking up operator addresses.
    IOperatorRegistry public operatorRegistry;

    /// Allowlisted tokens that this router accepts. Only stablecoins.
    mapping(address => bool) public allowedTokens;

    /// Fee structure: 5% flat (500 basis points).
    uint16 public constant TREASURY_RATE_BPS = 500;
    uint16 public constant BPS_DENOMINATOR = 10_000;

    // ========================================================================
    // Constructor
    // ========================================================================

    constructor(
        address initialOwner,
        address initialTreasury,
        address initialRegistry
    ) Ownable(initialOwner) {
        require(initialTreasury != address(0), "SpaceKitPay: zero treasury");
        require(initialRegistry != address(0), "SpaceKitPay: zero registry");
        treasury = initialTreasury;
        operatorRegistry = IOperatorRegistry(initialRegistry);
    }

    // ========================================================================
    // Core operation: payForService (atomic split and route)
    // ========================================================================

    /**
     * @notice Pay an operator for an AI service. The payment is split 95/5
     *         between the operator and the treasury. All three transfers
     *         happen atomically in this single transaction.
     *
     * @param token The ERC-20 stablecoin being used for payment (must be allowlisted).
     * @param operatorDID The operator's DID string (will be hashed for the event topic).
     * @param amount The total payment amount (in token's smallest unit).
     *
     * @dev Requires the caller to have called `token.approve(this, amount)`
     *      before this call. The router pulls the payment via transferFrom,
     *      then immediately forwards it to the operator and treasury.
     */
    function payForService(
        IERC20 token,
        string calldata operatorDID,
        uint256 amount
    ) external nonReentrant returns (uint256 operatorCut, uint256 treasuryCut) {
        require(amount > 0, "SpaceKitPay: zero amount");
        require(bytes(operatorDID).length > 0, "SpaceKitPay: empty operator DID");
        require(allowedTokens[address(token)], "SpaceKitPay: token not allowed");

        // Look up operator address from the registry
        address operatorAddress = operatorRegistry.lookupAddress(operatorDID);
        require(operatorAddress != address(0), "SpaceKitPay: operator not registered");

        // Compute the split (5% flat)
        treasuryCut = (amount * uint256(TREASURY_RATE_BPS)) / uint256(BPS_DENOMINATOR);
        operatorCut = amount - treasuryCut;

        // ATOMIC ROUTING: pull from buyer, push to operator, push to treasury.
        // All within this single transaction. If any transfer fails, the entire
        // call reverts and no funds move.

        // Step 1: Pull from buyer (requires prior approve)
        token.safeTransferFrom(msg.sender, address(this), amount);

        // Step 2: Push to operator (95% of payment)
        if (operatorCut > 0) {
            token.safeTransfer(operatorAddress, operatorCut);
        }

        // Step 3: Push to treasury (5% of payment)
        if (treasuryCut > 0) {
            token.safeTransfer(treasury, treasuryCut);
        }

        emit PaymentRouted(
            msg.sender,
            keccak256(bytes(operatorDID)),
            operatorAddress,
            address(token),
            amount,
            operatorCut,
            treasuryCut
        );

        return (operatorCut, treasuryCut);
    }

    // ========================================================================
    // Admin functions
    // ========================================================================

    function setTreasury(address newTreasury) external onlyOwner {
        require(newTreasury != address(0), "SpaceKitPay: zero treasury");
        treasury = newTreasury;
        emit TreasuryAddressChanged(newTreasury);
    }

    function setTokenAllowlist(address token, bool allowed) external onlyOwner {
        require(token != address(0), "SpaceKitPay: zero token");
        allowedTokens[token] = allowed;
        emit TokenAllowlistUpdated(token, allowed);
    }

    function setOperatorRegistry(address newRegistry) external onlyOwner {
        require(newRegistry != address(0), "SpaceKitPay: zero registry");
        operatorRegistry = IOperatorRegistry(newRegistry);
        emit OperatorRegistryUpdated(newRegistry);
    }

    /**
     * @notice Sweep tokens that ended up in this contract outside normal flow.
     *
     *         In normal operation, this contract holds zero balance after
     *         every payForService call. But if someone sends tokens directly
     *         to this contract address (bypassing payForService), those
     *         tokens would be stuck. This function lets the admin recover
     *         them and forward to the intended recipient or back to the
     *         sender.
     *
     *         This is NOT an emergency withdraw of user funds in transit -
     *         that's structurally impossible because nothing is in transit
     *         after a call completes. This is only for accidental sends.
     */
    function sweepStuckTokens(
        IERC20 token,
        uint256 amount,
        address to
    ) external onlyOwner nonReentrant {
        require(to != address(0), "SpaceKitPay: zero recipient");
        token.safeTransfer(to, amount);
        emit StuckTokensSwept(address(token), amount, to);
    }

    // ========================================================================
    // View functions
    // ========================================================================

    function getTreasuryRate() external pure returns (uint16) {
        return TREASURY_RATE_BPS;
    }

    function computeSplit(uint256 amount) external pure returns (uint256 operatorCut, uint256 treasuryCut) {
        treasuryCut = (amount * uint256(TREASURY_RATE_BPS)) / uint256(BPS_DENOMINATOR);
        operatorCut = amount - treasuryCut;
    }
}

/**
 * @notice Minimal interface to the OperatorRegistry for address lookups.
 */
interface IOperatorRegistry {
    function lookupAddress(string calldata operatorDID) external view returns (address);
}
