// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/utils/cryptography/ECDSA.sol";
import "@openzeppelin/contracts/utils/cryptography/MessageHashUtils.sol";
import "@openzeppelin/contracts/utils/math/Math.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

/**
 * @title BatchCallAndSponsor
 * @notice EIP-7702 account batches with optional Sidiora sponsorship.
 * @dev Sponsored execution consumes both nonces before calls and repays the
 * sponsor after all calls. Any failed call or repayment reverts the whole batch.
 * Digests use abi.encode and the Ethereum signed-message prefix for 32 bytes.
 */
contract BatchCallAndSponsor {
    using ECDSA for bytes32;

    /// @notice A nonce used for replay protection.
    uint256 public nonce;

    /// @notice Represents a single call within a batch.
    struct Call {
        address to;
        uint256 value;
        bytes data;
    }

    struct Quote {
        address sponsor;
        address token;
        uint256 maxTokenAmount;
        uint256 tokenAmount;
        uint256 deadline;
        uint256 quoteNonce;
        uint256 gasCost;
    }

    address public constant SIDIORA = 0x21f7b20a555199fa73A238B1a91FD0f549068fEe;
    uint256 public constant PAX_BASE_UNITS = 1e18;
    address public immutable rateSource;
    uint256 public immutable maxRateAge;
    uint256 public constant MAX_SPREAD_BPS = 500;
    bytes32 public constant QUOTE_TYPEHASH = keccak256(
        "Quote(uint256 chainId,address account,address sponsor,address token,uint256 maxTokenAmount,uint256 tokenAmount,uint256 deadline,uint256 quoteNonce,uint256 gasCost)"
    );
    bytes32 public constant BATCH_TYPEHASH =
        keccak256("SponsoredBatch(uint256 nonce,bytes32 callsHash,bytes32 quoteDigest)");

    mapping(address => mapping(uint256 => bool)) public usedQuoteNonces;
    uint256 public rate;
    uint256 public rateUpdatedAt;
    address public owner;
    address public pendingOwner;

    error InvalidAccountSignature();
    error InvalidRelayerSignature();
    error InvalidQuote();
    error QuoteExpired();
    error QuoteAboveMaximum();
    error QuoteAlreadyUsed();
    error UnauthorizedOwner();
    error InvalidOwner();
    error InvalidRate();
    error InvalidMaxRateAge();
    error InvalidRateContext();
    error StaleRate();
    error QuoteOutsideSpread();
    error TokenTransferFailed();

    event Sponsored(address indexed sponsor, address indexed token, uint256 tokenAmount, uint256 quoteNonce);

    event RateUpdated(uint256 rate, uint256 updatedAt);
    event OwnershipTransferStarted(address indexed previousOwner, address indexed newOwner);
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);

    constructor(address initialOwner, uint256 initialRate, uint256 maximumAge) {
        if (initialOwner == address(0)) revert InvalidOwner();
        if (initialRate == 0) revert InvalidRate();
        if (maximumAge == 0) revert InvalidMaxRateAge();
        owner = initialOwner;
        rateSource = address(this);
        maxRateAge = maximumAge;
        rate = initialRate;
        rateUpdatedAt = block.timestamp;
    }

    function setRate(uint256 newRate) external {
        if (address(this) != rateSource) revert InvalidRateContext();
        if (msg.sender != owner) revert UnauthorizedOwner();
        if (newRate == 0) revert InvalidRate();
        rate = newRate;
        rateUpdatedAt = block.timestamp;
        emit RateUpdated(newRate, block.timestamp);
    }

    function transferOwnership(address newOwner) external {
        if (address(this) != rateSource) revert InvalidRateContext();
        if (msg.sender != owner) revert UnauthorizedOwner();
        if (newOwner == address(0)) revert InvalidOwner();
        pendingOwner = newOwner;
        emit OwnershipTransferStarted(owner, newOwner);
    }

    function acceptOwnership() external {
        if (address(this) != rateSource) revert InvalidRateContext();
        if (msg.sender != pendingOwner) revert UnauthorizedOwner();
        emit OwnershipTransferred(owner, msg.sender);
        owner = msg.sender;
        pendingOwner = address(0);
    }

    function currentRate() public view returns (uint256) {
        if (address(this) != rateSource) {
            return BatchCallAndSponsor(payable(rateSource)).currentRate();
        }
        if (rate == 0) revert InvalidRate();
        if (block.timestamp - rateUpdatedAt > maxRateAge) revert StaleRate();
        return rate;
    }

    /// @notice gasCost is the declared fee in PAX wei; token amounts are SID base units.
    function quoteDigest(Quote calldata quote) public view returns (bytes32) {
        return MessageHashUtils.toEthSignedMessageHash(
            keccak256(abi.encode(QUOTE_TYPEHASH, block.chainid, address(this), quote))
        );
    }

    function sponsoredBatchDigest(Call[] calldata calls, Quote calldata quote) public view returns (bytes32) {
        return MessageHashUtils.toEthSignedMessageHash(
            keccak256(abi.encode(BATCH_TYPEHASH, nonce, keccak256(abi.encode(calls)), quoteDigest(quote)))
        );
    }

    function executeSponsored(
        Call[] calldata calls,
        Quote calldata quote,
        bytes calldata accountSignature,
        bytes calldata relayerSignature
    ) external payable {
        if (
            quote.sponsor == address(0) || quote.sponsor == address(this) || quote.token != SIDIORA
                || quote.gasCost == 0 || quote.tokenAmount == 0
        ) revert InvalidQuote();
        if (block.timestamp > quote.deadline) revert QuoteExpired();
        if (quote.tokenAmount > quote.maxTokenAmount) revert QuoteAboveMaximum();
        if (usedQuoteNonces[quote.sponsor][quote.quoteNonce]) revert QuoteAlreadyUsed();

        (address account, ECDSA.RecoverError accountError,) =
            ECDSA.tryRecover(sponsoredBatchDigest(calls, quote), accountSignature);
        if (accountError != ECDSA.RecoverError.NoError || account != address(this)) {
            revert InvalidAccountSignature();
        }
        (address sponsor, ECDSA.RecoverError sponsorError,) = ECDSA.tryRecover(quoteDigest(quote), relayerSignature);
        if (sponsorError != ECDSA.RecoverError.NoError || sponsor != quote.sponsor) {
            revert InvalidRelayerSignature();
        }

        _checkPrice(quote);
        usedQuoteNonces[quote.sponsor][quote.quoteNonce] = true;
        _executeBatch(calls);

        (bool success, bytes memory result) =
            quote.token.call(abi.encodeCall(IERC20.transfer, (quote.sponsor, quote.tokenAmount)));
        if (!success || result.length != 32 || abi.decode(result, (uint256)) != 1) revert TokenTransferFailed();
        emit Sponsored(quote.sponsor, quote.token, quote.tokenAmount, quote.quoteNonce);
    }

    function _checkPrice(Quote calldata quote) internal view {
        uint256 expected = Math.mulDiv(quote.gasCost, currentRate(), PAX_BASE_UNITS, Math.Rounding.Ceil);
        uint256 lower = Math.mulDiv(expected, 10_000 - MAX_SPREAD_BPS, 10_000, Math.Rounding.Ceil);
        uint256 upper = Math.mulDiv(expected, 10_000 + MAX_SPREAD_BPS, 10_000);
        if (quote.tokenAmount < lower || quote.tokenAmount > upper) revert QuoteOutsideSpread();
    }

    /// @notice Emitted for every individual call executed.
    event CallExecuted(address indexed sender, address indexed to, uint256 value, bytes data);
    /// @notice Emitted when a full batch is executed.
    event BatchExecuted(uint256 indexed nonce, Call[] calls);

    /**
     * @notice Executes a batch of calls directly.
     * @dev This function is intended for use when the smart account itself (i.e. address(this))
     * calls the contract. It checks that msg.sender is the contract itself.
     * @param calls An array of Call structs containing destination, ETH value, and calldata.
     */
    function execute(Call[] calldata calls) external payable {
        require(msg.sender == address(this), "Invalid authority");
        _executeBatch(calls);
    }

    /**
     * @dev Internal function that handles batch execution and nonce incrementation.
     * @param calls An array of Call structs.
     */
    function _executeBatch(Call[] calldata calls) internal {
        uint256 currentNonce = nonce;
        nonce++; // Increment nonce to protect against replay attacks

        for (uint256 i = 0; i < calls.length; i++) {
            _executeCall(calls[i]);
        }

        emit BatchExecuted(currentNonce, calls);
    }

    /**
     * @dev Internal function to execute a single call.
     * @param callItem The Call struct containing destination, value, and calldata.
     */
    function _executeCall(Call calldata callItem) internal {
        (bool success,) = callItem.to.call{value: callItem.value}(callItem.data);
        require(success, "Call reverted");
        emit CallExecuted(msg.sender, callItem.to, callItem.value, callItem.data);
    }

    // Allow the contract to receive ETH (e.g. from DEX swaps or other transfers).
    fallback() external payable {}
    receive() external payable {}
}
