// SPDX-License-Identifier: MIT
pragma solidity ^0.8.27;

import {ERC20} from "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import {BANK_CONTRACT} from "./precompiles/IBank.sol";

/// @dev Proxy state lives only in the ERC-7201 namespace paxeer.storage.SidioraNativeERC20.
/// Offsets: 0 initialized, 1 denom, 2 name, 3 symbol, 4 decimals, 5 allowances.
/// Inherited ERC20 storage is never read or written through the proxy. Legacy slots,
/// including balances and allowances, remain untouched; allowances here start empty.
/// The upgrade must initialize atomically and confirm this namespace is unused in
/// the deployed implementation's layout, which is not supplied with this contract.
contract SidioraNativeERC20 is ERC20 {
    error AlreadyInitialized();
    error NotInitialized();
    error BankTransferFailed();

    struct NativeStorage {
        bool initialized;
        string denom;
        string name;
        string symbol;
        uint8 decimals;
        mapping(address => mapping(address => uint256)) allowances;
    }

    constructor() ERC20("", "") {
        _nativeStorage().initialized = true;
    }

    function initialize() external {
        NativeStorage storage state = _nativeStorage();
        if (state.initialized) revert AlreadyInitialized();
        state.initialized = true;
        state.denom = _sidioraDenom();
        state.name = "Sidiora";
        state.symbol = "SID";
        state.decimals = 6;
    }

    function denom() public view returns (string memory) {
        return _initializedStorage().denom;
    }

    function name() public view override returns (string memory) {
        return _initializedStorage().name;
    }

    function symbol() public view override returns (string memory) {
        return _initializedStorage().symbol;
    }

    function decimals() public view override returns (uint8) {
        return _initializedStorage().decimals;
    }

    function balanceOf(address account) public view override returns (uint256) {
        return BANK_CONTRACT.balance(account, denom());
    }

    function totalSupply() public view override returns (uint256) {
        return BANK_CONTRACT.supply(denom());
    }

    function allowance(address owner, address spender) public view override returns (uint256) {
        return _initializedStorage().allowances[owner][spender];
    }

    function _approve(address owner, address spender, uint256 value, bool emitEvent) internal override {
        if (owner == address(0)) revert ERC20InvalidApprover(address(0));
        if (spender == address(0)) revert ERC20InvalidSpender(address(0));
        _initializedStorage().allowances[owner][spender] = value;
        if (emitEvent) emit Approval(owner, spender, value);
    }

    function _update(address from, address to, uint256 value) internal override {
        string memory nativeDenom = denom();
        try BANK_CONTRACT.send(from, to, nativeDenom, value) returns (bool success) {
            if (!success) revert BankTransferFailed();
        } catch {
            revert BankTransferFailed();
        }
        emit Transfer(from, to, value);
    }

    function _initializedStorage() private view returns (NativeStorage storage state) {
        state = _nativeStorage();
        if (!state.initialized) revert NotInitialized();
    }

    function _nativeStorage() private pure returns (NativeStorage storage state) {
        bytes32 slot = keccak256(abi.encode(uint256(keccak256("paxeer.storage.SidioraNativeERC20")) - 1))
            & ~bytes32(uint256(0xff));
        assembly {
            state.slot := slot
        }
    }

    /// @dev Matches layerxbridge/types.SidioraDenom: factory/{ModuleAddress()}/usid.
    /// ModuleAddress is the first 20 bytes of SHA-256("layerxbridge"), Bech32 encoded
    /// with the SDK account prefix pax. No EVM address association is involved.
    function _sidioraDenom() private pure returns (string memory) {
        bytes memory alphabet = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
        bytes memory creator = new bytes(42);
        creator[0] = "p";
        creator[1] = "a";
        creator[2] = "x";
        creator[3] = "1";
        uint256 checksum = 1;
        bytes memory expandedPrefix = hex"03030300100118";
        for (uint256 i; i < expandedPrefix.length; ++i) {
            checksum = _polymod(checksum, uint8(expandedPrefix[i]));
        }
        uint160 moduleAddress = uint160(bytes20(sha256("layerxbridge")));
        for (uint256 i; i < 32; ++i) {
            uint256 digit = (uint256(moduleAddress) >> (155 - 5 * i)) & 31;
            creator[4 + i] = alphabet[digit];
            checksum = _polymod(checksum, digit);
        }
        for (uint256 i; i < 6; ++i) {
            checksum = _polymod(checksum, 0);
        }
        checksum ^= 1;
        for (uint256 i; i < 6; ++i) {
            creator[36 + i] = alphabet[(checksum >> (5 * (5 - i))) & 31];
        }
        return string.concat("factory/", string(creator), "/usid");
    }

    function _polymod(uint256 checksum, uint256 value) private pure returns (uint256) {
        uint256 top = checksum >> 25;
        checksum = ((checksum & 0x1ffffff) << 5) ^ value;
        uint256[5] memory generators = [uint256(0x3b6a57b2), 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
        for (uint256 i; i < 5; ++i) {
            if ((top >> i) & 1 != 0) checksum ^= generators[i];
        }
        return checksum;
    }
}
