// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {AssetRegistry} from "../contracts/custody/AssetRegistry.sol";
import {LayerXVault} from "../contracts/custody/LayerXVault.sol";
import {UUPSNotUpgradeable} from "../contracts/security/UUPSNotUpgradeable.sol";

contract InvariantToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address recipient, uint256 amount) external {
        balanceOf[recipient] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address recipient, uint256 amount) external returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[recipient] += amount;
        return true;
    }

    function transferFrom(address sender, address recipient, uint256 amount) external returns (bool) {
        allowance[sender][msg.sender] -= amount;
        balanceOf[sender] -= amount;
        balanceOf[recipient] += amount;
        return true;
    }
}

contract VaultInvariantHandler {
    LayerXVault public immutable vault;
    InvariantToken public immutable token;
    bytes32 public immutable assetId;
    uint256 public grossDeposited;
    uint256 public grossReleased;
    uint256 public claimNonce;
    bytes32 public lastClaim;
    bool public replayAccepted;
    bool private initialized;

    constructor(LayerXVault custodyVault, InvariantToken custodyToken, bytes32 custodyAssetId) {
        vault = custodyVault;
        token = custodyToken;
        assetId = custodyAssetId;
    }

    function initialize() external {
        if (initialized) return;
        initialized = true;
        token.approve(address(vault), type(uint256).max);
    }

    function deposit(uint128 entropy, bytes32 beneficiary) external {
        uint256 available = token.balanceOf(address(this));
        if (available == 0) return;
        uint256 amount = uint256(entropy) % available + 1;
        if (beneficiary == bytes32(0)) beneficiary = bytes32(uint256(1));
        vault.deposit(assetId, amount, beneficiary);
        grossDeposited += amount;
    }

    function release(uint128 entropy) external {
        uint256 available = token.balanceOf(address(vault));
        if (available == 0) return;
        uint256 amount = uint256(entropy) % available + 1;
        bytes32 claimId = keccak256(abi.encode("invariant-release", claimNonce++, amount));
        vault.release(claimId, assetId, address(this), amount);
        grossReleased += amount;
        lastClaim = claimId;
    }

    function replayLastRelease() external {
        if (lastClaim == bytes32(0)) return;
        try vault.release(lastClaim, assetId, address(this), 1) {
            replayAccepted = true;
        } catch {}
    }
}

contract ContractInvariantsTest {
    struct FuzzSelector {
        address addr;
        bytes4[] selectors;
    }

    struct FuzzArtifactSelector {
        string artifact;
        bytes4[] selectors;
    }

    struct FuzzInterface {
        address addr;
        string[] artifacts;
    }

    uint128 private constant INITIAL_SUPPLY = 1_000_000_000_000_000_000_000_000;
    bytes32 private constant CONFIG = keccak256("invariant-config");
    bytes32 private constant ASSET = keccak256("INVARIANT_ASSET");
    uint192 private constant RELEASE = uint192(1) << 128;

    InvariantToken private token;
    LayerXVault private vault;
    VaultInvariantHandler private handler;
    address[] private invariantTargets;

    function setUp() public {
        token = new InvariantToken();
        AssetRegistry registry = new AssetRegistry(address(this), address(0xEC01), CONFIG, RELEASE);
        registry.registerAsset(ASSET, address(token), 18, 1, INITIAL_SUPPLY);
        vault = new LayerXVault(registry, address(this), address(0xEC01), CONFIG, RELEASE);
        handler = new VaultInvariantHandler(vault, token, ASSET);
        vault.setSettlementModule(address(handler), true);
        token.mint(address(handler), INITIAL_SUPPLY);
        handler.initialize();
        invariantTargets.push(address(handler));
    }

    function targetContracts() public view returns (address[] memory) {
        return invariantTargets;
    }

    function targetArtifactSelectors() public pure returns (FuzzArtifactSelector[] memory targets) {}

    function targetArtifacts() public pure returns (string[] memory targets) {}

    function excludeArtifacts() public pure returns (string[] memory exclusions) {}

    function targetSenders() public pure returns (address[] memory targets) {}

    function excludeSenders() public pure returns (address[] memory exclusions) {}

    function excludeContracts() public pure returns (address[] memory exclusions) {}

    function targetInterfaces() public pure returns (FuzzInterface[] memory targets) {}

    function targetSelectors() public pure returns (FuzzSelector[] memory targets) {}

    function excludeSelectors() public pure returns (FuzzSelector[] memory exclusions) {}

    function invariant_CustodyMirrorMatchesActualTokenBalance() public view {
        require(vault.totalCustodied(ASSET) == token.balanceOf(address(vault)), "custody mirror drift");
    }

    function invariant_AllDepositedValueIsHeldOrReleased() public view {
        require(
            vault.totalCustodied(ASSET) + vault.totalReleased(ASSET) == handler.grossDeposited(), "custody conservation"
        );
        require(vault.totalReleased(ASSET) == handler.grossReleased(), "release accounting");
    }

    function invariant_TokenSupplyNeverLeavesHandlerAndVault() public view {
        require(
            token.balanceOf(address(handler)) + token.balanceOf(address(vault)) == INITIAL_SUPPLY, "token conservation"
        );
    }

    function invariant_ReleaseClaimCanNeverReplay() public view {
        require(!handler.replayAccepted(), "claim replay accepted");
    }

    function invariant_VaultCannotBecomeUUPSUpgradeable() public {
        (bool success, bytes memory reason) =
            address(vault).call(abi.encodeCall(UUPSNotUpgradeable.upgradeTo, (address(this))));
        require(!success && reason.length >= 4, "upgrade accepted");
    }
}
