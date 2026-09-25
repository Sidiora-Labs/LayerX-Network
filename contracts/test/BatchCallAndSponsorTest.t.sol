// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";
import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {BatchCallAndSponsor} from "../src/BatchCallAndSponsor.sol";
import {IOracle} from "../src/precompiles/IOracle.sol";

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
    address internal oracle;

    function setUp() public {
        (address accountAddress, uint256 key) = makeAddrAndKey("account");
        accountKey = key;
        (sponsor, sponsorKey) = makeAddrAndKey("sponsor");
        recipient = makeAddr("recipient");
        implementation = new BatchCallAndSponsor();
        vm.signAndAttachDelegation(address(implementation), accountKey);
        account = BatchCallAndSponsor(payable(accountAddress));
        SidioraFixture tokenImplementation = new SidioraFixture();
        vm.etch(account.SIDIORA(), address(tokenImplementation).code);
        token = SidioraFixture(account.SIDIORA());
        token.mint(address(account), 10_000_000);
        oracle = account.ORACLE();
        vm.warp(1000);
        _rates("1.000000000000000000", "2.000000000000000000", 1000);
    }

    function _rates(string memory sid, string memory pax, int64 updated) internal {
        IOracle.DenomOracleExchangeRatePair[] memory rates = new IOracle.DenomOracleExchangeRatePair[](2);
        rates[0] = IOracle.DenomOracleExchangeRatePair("usid", IOracle.OracleExchangeRate(sid, "10", updated));
        rates[1] = IOracle.DenomOracleExchangeRatePair("uhpx", IOracle.OracleExchangeRate(pax, "10", updated));
        vm.mockCall(oracle, abi.encodeCall(IOracle.getExchangeRates, ()), abi.encode(rates));
    }

    function _quote() internal view returns (BatchCallAndSponsor.Quote memory) {
        return BatchCallAndSponsor.Quote(sponsor, address(token), 2_100_000, 2_000_000, 1100, 7, 1 ether);
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
        emit Sponsored(sponsor, address(token), 2_000_000, 7);
        vm.prank(sponsor);
        account.executeSponsored(calls, quote, auth, relayer);
        assertEq(token.balanceOf(sponsor), 2_000_000);
        assertEq(token.balanceOf(recipient), 123);
        assertEq(token.balanceOf(address(account)), 7_999_877);
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
        quote.maxTokenAmount = 3_000_000;
        quote.tokenAmount = 2_100_001;
        _refused(quote, BatchCallAndSponsor.QuoteOutsideSpread.selector);
    }

    function testOutsideLowerSpread() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.tokenAmount = 1_899_999;
        _refused(quote, BatchCallAndSponsor.QuoteOutsideSpread.selector);
    }

    function testSpreadBoundariesAccepted() public {
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.tokenAmount = 1_900_000;
        _submit(_calls(), quote);
        quote.quoteNonce++;
        quote.tokenAmount = 2_100_000;
        _submit(_calls(), quote);
        assertEq(token.balanceOf(sponsor), 4_000_000);
    }

    function testSixDecimalConversionRoundsUp() public {
        _rates("3", "1", 1000);
        BatchCallAndSponsor.Quote memory quote = _quote();
        quote.tokenAmount = 1;
        quote.gasCost = 1;
        _submit(_calls(), quote);
        assertEq(token.balanceOf(sponsor), 1);
    }

    function testOracleNoRate() public {
        IOracle.DenomOracleExchangeRatePair[] memory rates = new IOracle.DenomOracleExchangeRatePair[](0);
        vm.mockCall(oracle, abi.encodeCall(IOracle.getExchangeRates, ()), abi.encode(rates));
        _refused(_quote(), BatchCallAndSponsor.OracleUnavailable.selector);
    }

    function testOracleAbiSelectorsAndTupleDecoding() public {
        assertEq(IOracle.getExchangeRates.selector, bytes4(keccak256("getExchangeRates()")));
        assertEq(IOracle.getOracleTwaps.selector, bytes4(keccak256("getOracleTwaps(uint64)")));
        IOracle.DenomOracleExchangeRatePair[] memory rates = IOracle(oracle).getExchangeRates();
        assertEq(rates.length, 2);
        assertEq(rates[0].denom, "usid");
        assertEq(rates[0].oracleExchangeRateVal.exchangeRate, "1.000000000000000000");
        assertEq(rates[0].oracleExchangeRateVal.lastUpdate, "10");
        assertEq(rates[0].oracleExchangeRateVal.lastUpdateTimestamp, 1000);

        IOracle.OracleTwap[] memory twaps = new IOracle.OracleTwap[](1);
        twaps[0] = IOracle.OracleTwap("usid", "1.000000000000000000", 300);
        vm.mockCall(oracle, abi.encodeWithSignature("getOracleTwaps(uint64)", uint64(300)), abi.encode(twaps));
        IOracle.OracleTwap[] memory decoded = IOracle(oracle).getOracleTwaps(300);
        assertEq(decoded.length, 1);
        assertEq(decoded[0].denom, "usid");
        assertEq(decoded[0].twap, "1.000000000000000000");
        assertEq(decoded[0].lookbackSeconds, 300);
    }

    function testOracleMissingPaxRate() public {
        IOracle.DenomOracleExchangeRatePair[] memory rates = new IOracle.DenomOracleExchangeRatePair[](1);
        rates[0] = IOracle.DenomOracleExchangeRatePair("usid", IOracle.OracleExchangeRate("1", "10", 1000));
        vm.mockCall(oracle, abi.encodeCall(IOracle.getExchangeRates, ()), abi.encode(rates));
        _refused(_quote(), BatchCallAndSponsor.OracleUnavailable.selector);
    }

    function testOracleRetiredRefusesSponsorship() public {
        vm.mockCallRevert(
            oracle,
            abi.encodeCall(IOracle.getExchangeRates, ()),
            abi.encodeWithSignature("Error(string)", "oracle precompile is retired; oracle data queries are disabled")
        );
        _refused(_quote(), BatchCallAndSponsor.OracleUnavailable.selector);
    }

    function testStaleRate() public {
        _rates("1", "2", 699);
        _refused(_quote(), BatchCallAndSponsor.StaleOracleRate.selector);
    }

    function testFutureRate() public {
        _rates("1", "2", 1001);
        _refused(_quote(), BatchCallAndSponsor.StaleOracleRate.selector);
    }

    function testMissingTimestamp() public {
        _rates("1", "2", 0);
        _refused(_quote(), BatchCallAndSponsor.StaleOracleRate.selector);
    }

    function testInvalidDecimalRates() public {
        string[8] memory invalid = ["0", "-1", "1.2.3", "1e18", ".1", "1.", "1.0000000000000000001", ""];
        for (uint256 i; i < invalid.length; i++) {
            _rates(invalid[i], "2", 1000);
            _refused(_quote(), BatchCallAndSponsor.InvalidOracleRate.selector);
        }
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

    function testDirectExecuteNeedsNoQuoteOrOracle() public {
        vm.mockCallRevert(oracle, abi.encodeCall(IOracle.getExchangeRates, ()), "unavailable");
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
