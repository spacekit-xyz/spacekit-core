// SPDX-License-Identifier: MIT
pragma solidity ^0.8.28;

/**
 * @title KeymasterPayments
 * @dev Minimal Shield subscription ledger for SKKM-1 coverage SLA.
 *
 * Off-chain coordinator issues ML-DSA signed quotes; this contract records
 * USDC payments and exposes `isActive(subject)` for federation / indexing.
 *
 * Production deployments should wire USDC via Safe + multisig admin.
 */
interface IERC20 {
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
}

contract KeymasterPayments {
    enum Tier {
        ShieldMonthly,
        ShieldAnnual
    }

    struct Entitlement {
        uint256 paidUntil;
        Tier tier;
        bool active;
    }

    IERC20 public immutable usdc;
    address public admin;
    address public treasury;

    uint256 public constant MONTHLY_USDC = 10 * 1e6;
    uint256 public constant ANNUAL_USDC = 96 * 1e6;

    mapping(bytes32 => Entitlement) public entitlements;

    event ShieldPaid(bytes32 indexed subject, Tier tier, uint256 paidUntil, address payer);
    event TreasuryUpdated(address indexed treasury);
    event AdminUpdated(address indexed admin);

    error NotAdmin();
    error InsufficientAllowance();
    error TransferFailed();

    modifier onlyAdmin() {
        if (msg.sender != admin) revert NotAdmin();
        _;
    }

    constructor(address usdcToken, address treasury_) {
        admin = msg.sender;
        usdc = IERC20(usdcToken);
        treasury = treasury_;
    }

    function payShield(bytes32 subject, Tier tier) external {
        uint256 amount = tier == Tier.ShieldAnnual ? ANNUAL_USDC : MONTHLY_USDC;
        uint256 duration = tier == Tier.ShieldAnnual ? 365 days : 30 days;

        if (!usdc.transferFrom(msg.sender, treasury, amount)) revert TransferFailed();

        Entitlement storage e = entitlements[subject];
        uint256 base = e.paidUntil > block.timestamp ? e.paidUntil : block.timestamp;
        e.paidUntil = base + duration;
        e.tier = tier;
        e.active = true;

        emit ShieldPaid(subject, tier, e.paidUntil, msg.sender);
    }

    function isActive(bytes32 subject) external view returns (bool) {
        Entitlement storage e = entitlements[subject];
        return e.active && e.paidUntil > block.timestamp;
    }

    function setTreasury(address treasury_) external onlyAdmin {
        treasury = treasury_;
        emit TreasuryUpdated(treasury_);
    }

    function setAdmin(address admin_) external onlyAdmin {
        admin = admin_;
        emit AdminUpdated(admin_);
    }
}
