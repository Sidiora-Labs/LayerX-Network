// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {BatchCallAndSponsor} from "../src/BatchCallAndSponsor.sol";

contract SidioraFixture is ERC20 {
    constructor() ERC20("Sidiora", "SID") {}

    function decimals() public pure override returns (uint8) {
        return 6;
    }

    function mint(address account, uint256 amount) external {
        _mint(account, amount);
    }
}

contract BatchCallAndSponsorTest is Test {
    event RateUpdated(uint256 rate, uint256 updatedAt);
    event OwnershipTransferStarted(address indexed previousOwner, address indexed newOwner);
    event OwnershipTransferred(address indexed previousOwner, address indexed newOwner);
    event Sponsored(address indexed sponsor, address indexed token, uint256 tokenAmount, uint256 quoteNonce);
    event CallExecuted(address indexed sender, address indexed to, uint256 value, bytes data);
    event BatchExecuted(uint256 indexed nonce, BatchCallAndSponsor.Call[] calls);

    BatchCallAndSponsor internal implementation;
    BatchCallAndSponsor internal account;
    SidioraFixture internal token;
    uint256 internal accountKey;
    uint256 internal sponsorKey;
    address internal sponsor;
    address internal recipient;
    uint256 internal constant SID_BASE_UNITS = 10 ** 6;
    uint256 internal constant PAX_BASE_UNITS = 10 ** 18;
    uint256 internal constant INITIAL_RATE = 3114 * SID_BASE_UNITS / 1000;
    uint256 internal constant MAX_RATE_AGE = 300;

    function setUp() public {
        (address accountAddress, uint256 key) = makeAddrAndKey("account");
        accountKey = key;
        (sponsor, sponsorKey) = makeAddrAndKey("sponsor");
        recipient = makeAddr("recipient");
        vm.warp(1000);
        implementation = new BatchCallAndSponsor(address(this), INITIAL_RATE, MAX_RATE_AGE);
        vm.signAndAttachDelegation(address(implementation), accountKey);
        account = BatchCallAndSponsor(payable(accountAddress));
        SidioraFixture tokenImplementation = new SidioraFixture();
        vm.etch(account.SIDIORA(), address(tokenImplementation).code);
        token = SidioraFixture(account.SIDIORA());
        token.mint(address(account), 10_000_000);
    }

    function _quote() internal view returns (BatchCallAndSponsor.Quote memory) {
        return
            BatchCallAndSponsor.Quote(sponsor, address(token), INITIAL_RATE * 105 / 100, INITIAL_RATE, 1100, 7, 1 ether);
    }

    function _calls() internal view returns (BatchCallAndSponsor.Call[] memory calls) {
        calls = new BatchCallAndSponsor.Call[](1);
        calls[0] = BatchCallAndSponsor.Call(address(token), 0, abi.encodeCall(IERC20.transfer, (recipient, 123)));
    }

    function _sign(uint256 key, bytes32 digest) internal pure returns (bytes memory) {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(key, digest);
        return abi.encodePacked(r, s, v);
    }

    function _submit(BatchCallAndSponsor.Call[] memory calls, BatchCallAndSponsor.Quote memory quote) internal {
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.prank(sponsor);
        account.executeSponsored(calls, quote, auth, relayer);
    }

    function _refused(BatchCallAndSponsor.Quote memory quote, bytes4 reason) internal {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.expectRevert(reason);
        account.executeSponsored(calls, quote, auth, relayer);
        assertEq(account.nonce(), 0);
        assertFalse(account.usedQuoteNonces(sponsor, quote.quoteNonce));
        assertEq(token.balanceOf(recipient), 0);
        assertEq(token.balanceOf(sponsor), 0);
        assertEq(token.balanceOf(address(account)), 10_000_000);
    }

    function testSponsoredBatchRepaysSponsorAndEmitsEvents() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.expectEmit(true, true, true, true, address(account));
        emit CallExecuted(sponsor, address(token), 0, calls[0].data);
        vm.expectEmit(true, true, true, true, address(account));
        emit BatchExecuted(0, calls);
        vm.expectEmit(true, true, true, true, address(account));
        emit Sponsored(sponsor, address(token), quote.tokenAmount, 7);
        vm.prank(sponsor);
        account.executeSponsored(calls, quote, auth, relayer);
        assertEq(token.balanceOf(sponsor), quote.tokenAmount);
        assertEq(token.balanceOf(recipient), 123);
        assertEq(token.balanceOf(address(account)), 10_000_000 - quote.tokenAmount - 123);
        assertEq(address(account).balance, 0);
        assertEq(account.nonce(), 1);
        assertTrue(account.usedQuoteNonces(sponsor, 7));
    }

    function testMissingAccountSignature() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.expectRevert(BatchCallAndSponsor.InvalidAccountSignature.selector);
        account.executeSponsored(calls, quote, "", relayer);
    }

    function testWrongAccountSignature() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(sponsorKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.expectRevert(BatchCallAndSponsor.InvalidAccountSignature.selector);
        account.executeSponsored(calls, quote, auth, relayer);
    }

    function testMissingRelayerSignature() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        vm.expectRevert(BatchCallAndSponsor.InvalidRelayerSignature.selector);
        account.executeSponsored(calls, quote, auth, "");
    }

    function testWrongRelayerSignature() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(accountKey, account.quoteDigest(quote));
        vm.expectRevert(BatchCallAndSponsor.InvalidRelayerSignature.selector);
        account.executeSponsored(calls, quote, auth, relayer);
    }

    function testChangedCallsRefused() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        calls[0].data = abi.encodeCall(IERC20.transfer, (recipient, 124));
        vm.expectRevert(BatchCallAndSponsor.InvalidAccountSignature.selector);
        account.executeSponsored(calls, quote, auth, relayer);
    }

    function testChangedGasCostInvalidatesBothSignatures() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        quote.gasCost += 1;
        vm.expectRevert(BatchCallAndSponsor.InvalidAccountSignature.selector);
        account.executeSponsored(calls, quote, auth, relayer);
        auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        vm.expectRevert(BatchCallAndSponsor.InvalidRelayerSignature.selector);
        account.executeSponsored(calls, quote, auth, relayer);
    }

    function testExpiredQuote() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.deadline = block.timestamp - 1;
        _refused(quote, BatchCallAndSponsor.QuoteExpired.selector);
    }

    function testDeadlineInclusive() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.deadline = block.timestamp;
        _submit(_calls(), quote);
        assertEq(token.balanceOf(sponsor), quote.tokenAmount);
    }

    function testAboveMaximum() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.maxTokenAmount = quote.tokenAmount - 1;
        _refused(quote, BatchCallAndSponsor.QuoteAboveMaximum.selector);
    }

    function testReplayedQuoteWithFreshAccountSignature() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        _submit(calls, quote);
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.expectRevert(BatchCallAndSponsor.QuoteAlreadyUsed.selector);
        account.executeSponsored(calls, quote, auth, relayer);
        assertEq(account.nonce(), 1);
    }

    function testBatchNonceReplayWithUnusedQuote() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.prank(address(account));
        account.execute(calls);
        vm.expectRevert(BatchCallAndSponsor.InvalidAccountSignature.selector);
        account.executeSponsored(calls, quote, auth, relayer);
        assertFalse(account.usedQuoteNonces(sponsor, quote.quoteNonce));
    }

    function testTransferRevertRollsBackBatch() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        vm.mockCallRevert(address(token), abi.encodeCall(IERC20.transfer, (sponsor, quote.tokenAmount)), "refused");
        _refused(quote, BatchCallAndSponsor.TokenTransferFailed.selector);
    }

    function testTransferFalseRollsBackBatch() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        vm.mockCall(address(token), abi.encodeCall(IERC20.transfer, (sponsor, quote.tokenAmount)), abi.encode(false));
        _refused(quote, BatchCallAndSponsor.TokenTransferFailed.selector);
    }

    function testTransferEmptyReturnRollsBackBatch() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        vm.mockCall(address(token), abi.encodeCall(IERC20.transfer, (sponsor, quote.tokenAmount)), "");
        _refused(quote, BatchCallAndSponsor.TokenTransferFailed.selector);
    }

    function testInsufficientTokenBalanceRollsBackBatch() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        calls[0].data = abi.encodeCall(IERC20.transfer, (recipient, 9_000_000));
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.expectRevert(BatchCallAndSponsor.TokenTransferFailed.selector);
        account.executeSponsored(calls, quote, auth, relayer);
        assertEq(token.balanceOf(recipient), 0);
        assertEq(token.balanceOf(address(account)), 10_000_000);
        assertEq(account.nonce(), 0);
        assertFalse(account.usedQuoteNonces(sponsor, quote.quoteNonce));
    }

    function testRevertingBatchCallRollsBackNonce() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        calls[0].data = abi.encodeCall(IERC20.transfer, (recipient, 11_000_000));
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.expectRevert("Call reverted");
        account.executeSponsored(calls, quote, auth, relayer);
        assertEq(account.nonce(), 0);
        assertFalse(account.usedQuoteNonces(sponsor, quote.quoteNonce));
    }

    function testOutsideUpperSpread() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.tokenAmount = INITIAL_RATE * (10_000 + account.MAX_SPREAD_BPS()) / 10_000 + 1;
        quote.maxTokenAmount = quote.tokenAmount;
        _refused(quote, BatchCallAndSponsor.QuoteOutsideSpread.selector);
    }

    function testOutsideLowerSpread() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.tokenAmount = INITIAL_RATE * (10_000 - account.MAX_SPREAD_BPS()) / 10_000 - 1;
        _refused(quote, BatchCallAndSponsor.QuoteOutsideSpread.selector);
    }

    function testSpreadBoundariesAccepted() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.tokenAmount = INITIAL_RATE * (10_000 - account.MAX_SPREAD_BPS()) / 10_000;
        _submit(_calls(), quote);
        quote.quoteNonce++;
        quote.tokenAmount = INITIAL_RATE * (10_000 + account.MAX_SPREAD_BPS()) / 10_000;
        _submit(_calls(), quote);
        assertEq(token.balanceOf(sponsor), 2 * INITIAL_RATE);
    }

    function testSixDecimalConversionRoundsUp() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.tokenAmount = 1;
        quote.gasCost = 1;
        _submit(_calls(), quote);
        assertEq(token.balanceOf(sponsor), 1);
    }

    function testConstructorSetsRateAndConfiguration() public view {
        assertEq(implementation.owner(), address(this));
        assertEq(implementation.rateSource(), address(implementation));
        assertEq(implementation.rate(), INITIAL_RATE);
        assertEq(implementation.rateUpdatedAt(), block.timestamp);
        assertEq(implementation.maxRateAge(), MAX_RATE_AGE);
        assertEq(account.currentRate(), INITIAL_RATE);
    }

    function testConstructorRejectsZeroConfiguration() public {
        vm.expectRevert(BatchCallAndSponsor.InvalidOwner.selector);
        new BatchCallAndSponsor(address(0), INITIAL_RATE, MAX_RATE_AGE);
        vm.expectRevert(BatchCallAndSponsor.InvalidRate.selector);
        new BatchCallAndSponsor(address(this), 0, MAX_RATE_AGE);
        vm.expectRevert(BatchCallAndSponsor.InvalidMaxRateAge.selector);
        new BatchCallAndSponsor(address(this), INITIAL_RATE, 0);
    }

    function testConstructorUsesSuppliedConfiguration() public {
        BatchCallAndSponsor configured = new BatchCallAndSponsor(sponsor, 2 * SID_BASE_UNITS, 17);
        assertEq(configured.owner(), sponsor);
        assertEq(configured.currentRate(), 2 * SID_BASE_UNITS);
        assertEq(configured.maxRateAge(), 17);
        vm.warp(block.timestamp + 18);
        vm.expectRevert(BatchCallAndSponsor.StaleRate.selector);
        configured.currentRate();
        vm.prank(sponsor);
        configured.setRate(INITIAL_RATE);
        assertEq(configured.currentRate(), INITIAL_RATE);
    }

    function testOwnerRateUpdateEmitsAndChangesAcceptedQuote() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        _submit(_calls(), quote);
        vm.warp(block.timestamp + 1);
        uint256 newRate = 2 * SID_BASE_UNITS;
        vm.expectEmit(true, true, true, true, address(implementation));
        emit RateUpdated(newRate, block.timestamp);
        implementation.setRate(newRate);
        assertEq(implementation.rate(), newRate);
        assertEq(implementation.rateUpdatedAt(), block.timestamp);
        assertEq(account.currentRate(), newRate);
        quote.quoteNonce++;
        BatchCallAndSponsor.Call[] memory calls = _calls();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.expectRevert(BatchCallAndSponsor.QuoteOutsideSpread.selector);
        account.executeSponsored(calls, quote, auth, relayer);
        assertEq(account.nonce(), 1);
        assertFalse(account.usedQuoteNonces(sponsor, quote.quoteNonce));
        quote.tokenAmount = newRate;
        _submit(calls, quote);
        assertEq(token.balanceOf(sponsor), INITIAL_RATE + newRate);
    }

    function testNonOwnerCannotSetRate() public {
        vm.prank(sponsor);
        vm.expectRevert(BatchCallAndSponsor.UnauthorizedOwner.selector);
        implementation.setRate(2 * SID_BASE_UNITS);
        assertEq(implementation.rate(), INITIAL_RATE);
        assertEq(implementation.rateUpdatedAt(), 1000);
    }

    function testOwnerCannotWriteRateOnDelegatedAccount() public {
        vm.expectRevert(BatchCallAndSponsor.InvalidRateContext.selector);
        account.setRate(2 * SID_BASE_UNITS);
        assertEq(account.currentRate(), INITIAL_RATE);
    }

    function testOwnershipTransferRequiresAcceptanceAndEmitsEvents() public {
        vm.expectEmit(true, true, false, true, address(implementation));
        emit OwnershipTransferStarted(address(this), sponsor);
        implementation.transferOwnership(sponsor);
        assertEq(implementation.owner(), address(this));
        assertEq(implementation.pendingOwner(), sponsor);
        vm.prank(sponsor);
        vm.expectRevert(BatchCallAndSponsor.UnauthorizedOwner.selector);
        implementation.setRate(2 * SID_BASE_UNITS);
        implementation.setRate(INITIAL_RATE);
        vm.expectEmit(true, true, false, true, address(implementation));
        emit OwnershipTransferred(address(this), sponsor);
        vm.prank(sponsor);
        implementation.acceptOwnership();
        assertEq(implementation.owner(), sponsor);
        assertEq(implementation.pendingOwner(), address(0));
        vm.expectRevert(BatchCallAndSponsor.UnauthorizedOwner.selector);
        implementation.setRate(2 * SID_BASE_UNITS);
        vm.expectRevert(BatchCallAndSponsor.UnauthorizedOwner.selector);
        implementation.transferOwnership(recipient);
        vm.prank(sponsor);
        implementation.setRate(INITIAL_RATE);
        _submit(_calls(), _quote());
        assertEq(token.balanceOf(sponsor), INITIAL_RATE);
    }

    function testOwnershipTransferRejectsUnauthorizedAndZeroNomination() public {
        implementation.transferOwnership(sponsor);
        vm.prank(recipient);
        vm.expectRevert(BatchCallAndSponsor.UnauthorizedOwner.selector);
        implementation.transferOwnership(recipient);
        vm.expectRevert(BatchCallAndSponsor.InvalidOwner.selector);
        implementation.transferOwnership(address(0));
        assertEq(implementation.owner(), address(this));
        assertEq(implementation.pendingOwner(), sponsor);
    }

    function testOwnershipAcceptanceRejectsAbsentOrWrongNominee() public {
        vm.prank(sponsor);
        vm.expectRevert(BatchCallAndSponsor.UnauthorizedOwner.selector);
        implementation.acceptOwnership();
        implementation.transferOwnership(sponsor);
        vm.prank(recipient);
        vm.expectRevert(BatchCallAndSponsor.UnauthorizedOwner.selector);
        implementation.acceptOwnership();
        assertEq(implementation.owner(), address(this));
        assertEq(implementation.pendingOwner(), sponsor);
    }

    function testOnlyLatestOwnershipNomineeCanAccept() public {
        implementation.transferOwnership(sponsor);
        implementation.transferOwnership(recipient);
        vm.prank(sponsor);
        vm.expectRevert(BatchCallAndSponsor.UnauthorizedOwner.selector);
        implementation.acceptOwnership();
        vm.prank(recipient);
        implementation.acceptOwnership();
        assertEq(implementation.owner(), recipient);
        assertEq(implementation.pendingOwner(), address(0));
        vm.prank(recipient);
        vm.expectRevert(BatchCallAndSponsor.UnauthorizedOwner.selector);
        implementation.acceptOwnership();
    }

    function testDelegatedOwnershipWritesRejected() public {
        implementation.transferOwnership(sponsor);
        vm.expectRevert(BatchCallAndSponsor.InvalidRateContext.selector);
        account.transferOwnership(recipient);
        vm.prank(sponsor);
        vm.expectRevert(BatchCallAndSponsor.InvalidRateContext.selector);
        account.acceptOwnership();
        assertEq(implementation.owner(), address(this));
        assertEq(implementation.pendingOwner(), sponsor);
        assertEq(account.owner(), address(0));
        assertEq(account.pendingOwner(), address(0));
        assertEq(account.currentRate(), INITIAL_RATE);
    }

    function testOwnershipRotationPreservesRateAndReplayStorage() public {
        _submit(_calls(), _quote());
        implementation.transferOwnership(sponsor);
        vm.prank(sponsor);
        implementation.acceptOwnership();
        assertEq(uint256(vm.load(address(account), bytes32(uint256(0)))), 1);
        bytes32 sponsorSlot = keccak256(abi.encode(sponsor, uint256(1)));
        bytes32 quoteSlot = keccak256(abi.encode(uint256(7), sponsorSlot));
        assertEq(uint256(vm.load(address(account), quoteSlot)), 1);
        assertEq(uint256(vm.load(address(implementation), bytes32(uint256(2)))), INITIAL_RATE);
        assertEq(uint256(vm.load(address(implementation), bytes32(uint256(3)))), 1000);
        assertEq(account.nonce(), 1);
        assertTrue(account.usedQuoteNonces(sponsor, 7));
        assertEq(account.currentRate(), INITIAL_RATE);
        _refusedAfterRotation();
    }

    function _refusedAfterRotation() internal {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes memory auth = _sign(accountKey, account.sponsoredBatchDigest(calls, quote));
        bytes memory relayer = _sign(sponsorKey, account.quoteDigest(quote));
        vm.expectRevert(BatchCallAndSponsor.QuoteAlreadyUsed.selector);
        account.executeSponsored(calls, quote, auth, relayer);
        assertEq(account.nonce(), 1);
        assertEq(token.balanceOf(sponsor), INITIAL_RATE);
    }

    function testZeroRateUpdateRefused() public {
        vm.warp(block.timestamp + 1);
        vm.expectRevert(BatchCallAndSponsor.InvalidRate.selector);
        implementation.setRate(0);
        assertEq(implementation.rate(), INITIAL_RATE);
        assertEq(implementation.rateUpdatedAt(), 1000);
    }

    function testStaleRateRefusesReadAndSponsorship() public {
        vm.warp(block.timestamp + MAX_RATE_AGE + 1);
        vm.expectRevert(BatchCallAndSponsor.StaleRate.selector);
        implementation.currentRate();
        vm.expectRevert(BatchCallAndSponsor.StaleRate.selector);
        account.currentRate();
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.deadline = block.timestamp;
        _refused(quote, BatchCallAndSponsor.StaleRate.selector);
    }

    function testRateAgeInclusive() public {
        vm.warp(block.timestamp + MAX_RATE_AGE);
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.deadline = block.timestamp;
        _submit(_calls(), quote);
        assertEq(token.balanceOf(sponsor), quote.tokenAmount);
    }

    function testOwnerRefreshRestoresStaleSponsorship() public {
        vm.warp(block.timestamp + MAX_RATE_AGE + 1);
        implementation.setRate(INITIAL_RATE);
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.deadline = block.timestamp;
        _submit(_calls(), quote);
        assertEq(token.balanceOf(sponsor), quote.tokenAmount);
        assertEq(implementation.rateUpdatedAt(), block.timestamp);
    }

    function testInitialRateArithmeticUsesBothDecimalBases() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.gasCost = PAX_BASE_UNITS / 4 + 1;
        uint256 numerator = quote.gasCost * 3114 * SID_BASE_UNITS;
        uint256 denominator = 1000 * PAX_BASE_UNITS;
        uint256 expected = (numerator + denominator - 1) / denominator;
        quote.tokenAmount = expected;
        quote.maxTokenAmount = expected;
        _submit(_calls(), quote);
        assertEq(token.balanceOf(sponsor), expected);
        assertEq(token.balanceOf(address(account)), 10_000_000 - expected - 123);
    }

    function testWrongTokenRefused() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.token = recipient;
        _refused(quote, BatchCallAndSponsor.InvalidQuote.selector);
    }

    function testZeroGasCostRefused() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.gasCost = 0;
        _refused(quote, BatchCallAndSponsor.InvalidQuote.selector);
    }

    function testZeroTokenAmountRefused() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.tokenAmount = 0;
        _refused(quote, BatchCallAndSponsor.InvalidQuote.selector);
    }

    function testDirectExecuteNeedsNoQuoteOrFreshRate() public {
        vm.warp(block.timestamp + MAX_RATE_AGE + 1);
        BatchCallAndSponsor.Call[] memory calls = _calls();
        vm.prank(address(account));
        account.execute(calls);
        assertEq(token.balanceOf(recipient), 123);
        assertEq(account.nonce(), 1);
        assertEq(token.balanceOf(sponsor), 0);
    }

    function testDirectExecuteRejectsOtherCaller() public {
        BatchCallAndSponsor.Call[] memory calls = _calls();
        vm.expectRevert("Invalid authority");
        account.execute(calls);
    }

    function testReceiveAndFallbackAcceptNetworkCoin() public {
        vm.deal(address(this), 2 ether);
        (bool received,) = address(account).call{value: 1 ether}("");
        (bool fallbackReceived,) = address(account).call{value: 1 ether}(hex"12345678");
        assertTrue(received);
        assertTrue(fallbackReceived);
        assertEq(address(account).balance, 2 ether);
    }

    function testDigestsBindEveryQuoteFieldAndChain() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        bytes32 digest = account.quoteDigest(quote);
        for (uint256 field; field < 7; field++) {
            BatchCallAndSponsor.Quote memory changed = _quote();
            if (field == 0) changed.sponsor = recipient;
            if (field == 1) changed.token = recipient;
            if (field == 2) changed.maxTokenAmount++;
            if (field == 3) changed.tokenAmount++;
            if (field == 4) changed.deadline++;
            if (field == 5) changed.quoteNonce++;
            if (field == 6) changed.gasCost++;
            assertNotEq(account.quoteDigest(changed), digest);
            assertNotEq(account.sponsoredBatchDigest(_calls(), changed), account.sponsoredBatchDigest(_calls(), quote));
        }
        assertNotEq(implementation.quoteDigest(quote), digest);
        vm.chainId(block.chainid + 1);
        assertNotEq(account.quoteDigest(quote), digest);
    }

    function testSharedQuoteAndBatchDigestVector() public {
        vm.chainId(1325);
        address vectorAccount = 0x1111111111111111111111111111111111111111;
        vm.etch(vectorAccount, address(implementation).code);
        BatchCallAndSponsor vector = BatchCallAndSponsor(payable(vectorAccount));
        BatchCallAndSponsor.Quote memory quote = BatchCallAndSponsor.Quote(
            0x2222222222222222222222222222222222222222,
            0x21f7b20a555199fa73A238B1a91FD0f549068fEe,
            2_100_000,
            2_000_000,
            1000,
            7,
            1 ether
        );
        BatchCallAndSponsor.Call[] memory calls = new BatchCallAndSponsor.Call[](1);
        calls[0] = BatchCallAndSponsor.Call(0x3333333333333333333333333333333333333333, 0, hex"1234");
        assertEq(vector.quoteDigest(quote), 0x6c11f34e7848d98b1ae328fe84bf47223eb14e5274ae04daf3dc64c304c813ba);
        assertEq(
            vector.sponsoredBatchDigest(calls, quote),
            0xeba39a1c4de2cb2415a6e26ed362f8237cfe005c8d55ebe8f089cf6f7c636d50
        );
    }
}
