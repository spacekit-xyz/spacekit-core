// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.20;

import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";

/**
 * @title SpaceKitOperatorRegistry
 * @notice Maps SpaceKit operator DIDs to Ethereum payout addresses.
 *
 *         Only the trusted `registrar` (SpaceKit website-api relayer) may write
 *         mappings after off-chain verification:
 *           - authenticated session matches the DID (username claim)
 *           - wallet EIP-191 signature over the DID + payout address
 *
 *         Prevents arbitrary callers from squatting someone else's DID payout.
 */
contract SpaceKitOperatorRegistry is Ownable2Step {
    mapping(bytes32 => address) public operatorByDidHash;

    /// Hot wallet allowed to call `registerOperator` (website-api relayer).
    address public registrar;

    event OperatorRegistered(
        bytes32 indexed didHash,
        string did,
        address payoutAddress,
        address indexed submittedBy
    );
    event RegistrarChanged(address indexed previousRegistrar, address indexed newRegistrar);

    constructor(address initialOwner, address initialRegistrar) Ownable(initialOwner) {
        require(initialRegistrar != address(0), "SpaceKitRegistry: zero registrar");
        registrar = initialRegistrar;
    }

    modifier onlyRegistrar() {
        require(msg.sender == registrar, "SpaceKitRegistry: not registrar");
        _;
    }

    function setRegistrar(address newRegistrar) external onlyOwner {
        require(newRegistrar != address(0), "SpaceKitRegistry: zero registrar");
        address previous = registrar;
        registrar = newRegistrar;
        emit RegistrarChanged(previous, newRegistrar);
    }

    /**
     * @notice Set payout address for an operator DID. Callable only by `registrar`.
     */
    function registerOperator(string calldata operatorDID, address payoutAddress) external onlyRegistrar {
        require(bytes(operatorDID).length > 0, "SpaceKitRegistry: empty DID");
        require(payoutAddress != address(0), "SpaceKitRegistry: zero payout");
        bytes32 h = keccak256(bytes(operatorDID));
        operatorByDidHash[h] = payoutAddress;
        emit OperatorRegistered(h, operatorDID, payoutAddress, msg.sender);
    }

    function lookupAddress(string calldata operatorDID) external view returns (address) {
        return operatorByDidHash[keccak256(bytes(operatorDID))];
    }
}
