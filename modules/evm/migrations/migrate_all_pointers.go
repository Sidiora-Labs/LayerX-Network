package migrations

import (
	"encoding/json"
	"fmt"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/sdk/store/prefix"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/utils"
)

const pointerRegistryVersionBytes = 2

func decodeERCPointerRegistryEntry(key, value []byte) (string, common.Address, error) {
	if len(key) <= pointerRegistryVersionBytes {
		return "", common.Address{}, fmt.Errorf("pointer registry key is too short: got %d bytes", len(key))
	}
	if len(value) != common.AddressLength {
		return "", common.Address{}, fmt.Errorf("pointer registry value has invalid address length: got %d bytes", len(value))
	}
	return string(key[:len(key)-pointerRegistryVersionBytes]), common.BytesToAddress(value), nil
}

func decodePointerRegistryKey(key []byte) (string, error) {
	if len(key) <= pointerRegistryVersionBytes {
		return "", fmt.Errorf("pointer registry key is too short: got %d bytes", len(key))
	}
	return string(key[:len(key)-pointerRegistryVersionBytes]), nil
}

func stringPointerMetadata(field string, output interface{}) (string, error) {
	value, ok := output.(string)
	if !ok {
		return "", fmt.Errorf("pointer metadata %s has type %T, want string", field, output)
	}
	return value, nil
}

func uint8PointerMetadata(field string, output interface{}) (uint8, error) {
	value, ok := output.(uint8)
	if !ok {
		return 0, fmt.Errorf("pointer metadata %s has type %T, want uint8", field, output)
	}
	return value, nil
}

func runERCPointerUpgrade(kind, pointee string, run func() error) error {
	if err := run(); err != nil {
		return fmt.Errorf("upgrade %s pointer %q: %w", kind, pointee, err)
	}
	return nil
}

func MigrateERCNativePointers(ctx sdk.Context, k *keeper.Keeper) error {
	iter := prefix.NewStore(ctx.KVStore(k.GetStoreKey()), append(types.PointerRegistryPrefix, types.PointerERC20NativePrefix...)).ReverseIterator(nil, nil)
	defer func() { _ = iter.Close() }()
	seen := map[string]struct{}{}
	for ; iter.Valid(); iter.Next() {
		token, addr, err := decodeERCPointerRegistryEntry(iter.Key(), iter.Value())
		if err != nil {
			return fmt.Errorf("decode native pointer registry entry: %w", err)
		}
		if _, ok := seen[token]; ok {
			continue
		}
		seen[token] = struct{}{}
		oName, err := k.QueryERCSingleOutput(ctx, "native", addr, "name")
		if err != nil {
			return fmt.Errorf("query native pointer %q name: %w", token, err)
		}
		name, err := stringPointerMetadata("name", oName)
		if err != nil {
			return fmt.Errorf("decode native pointer %q metadata: %w", token, err)
		}
		oSymbol, err := k.QueryERCSingleOutput(ctx, "native", addr, "symbol")
		if err != nil {
			return fmt.Errorf("query native pointer %q symbol: %w", token, err)
		}
		symbol, err := stringPointerMetadata("symbol", oSymbol)
		if err != nil {
			return fmt.Errorf("decode native pointer %q metadata: %w", token, err)
		}
		oDecimals, err := k.QueryERCSingleOutput(ctx, "native", addr, "decimals")
		if err != nil {
			return fmt.Errorf("query native pointer %q decimals: %w", token, err)
		}
		decimals, err := uint8PointerMetadata("decimals", oDecimals)
		if err != nil {
			return fmt.Errorf("decode native pointer %q metadata: %w", token, err)
		}
		if err := runERCPointerUpgrade("native", token, func() error {
			return k.RunWithOneOffEVMInstance(ctx, func(e *vm.EVM) error {
				_, err := k.UpsertERCNativePointer(ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx)), e, token, utils.ERCMetadata{
					Name:     name,
					Symbol:   symbol,
					Decimals: decimals,
				})
				return err
			}, func(s1, s2 string) {
				logger.Error("Failed to upgrade pointer for token at step", "token", token, "from-step", s1, "to-step", s2)
			})
		}); err != nil {
			return err
		}
	}
	return nil
}

func MigrateERCCW20Pointers(ctx sdk.Context, k *keeper.Keeper) error {
	iter := prefix.NewStore(ctx.KVStore(k.GetStoreKey()), append(types.PointerRegistryPrefix, types.PointerERC20CW20Prefix...)).ReverseIterator(nil, nil)
	defer func() { _ = iter.Close() }()
	seen := map[string]struct{}{}
	for ; iter.Valid(); iter.Next() {
		cwAddr, addr, err := decodeERCPointerRegistryEntry(iter.Key(), iter.Value())
		if err != nil {
			return fmt.Errorf("decode cw20 pointer registry entry: %w", err)
		}
		if _, ok := seen[cwAddr]; ok {
			continue
		}
		seen[cwAddr] = struct{}{}
		oName, err := k.QueryERCSingleOutput(ctx, "cw20", addr, "name")
		if err != nil {
			return fmt.Errorf("query cw20 pointer %q name: %w", cwAddr, err)
		}
		name, err := stringPointerMetadata("name", oName)
		if err != nil {
			return fmt.Errorf("decode cw20 pointer %q metadata: %w", cwAddr, err)
		}
		oSymbol, err := k.QueryERCSingleOutput(ctx, "cw20", addr, "symbol")
		if err != nil {
			return fmt.Errorf("query cw20 pointer %q symbol: %w", cwAddr, err)
		}
		symbol, err := stringPointerMetadata("symbol", oSymbol)
		if err != nil {
			return fmt.Errorf("decode cw20 pointer %q metadata: %w", cwAddr, err)
		}
		if err := runERCPointerUpgrade("cw20", cwAddr, func() error {
			return k.RunWithOneOffEVMInstance(ctx, func(e *vm.EVM) error {
				_, err := k.UpsertERCCW20Pointer(ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx)), e, cwAddr, utils.ERCMetadata{
					Name:   name,
					Symbol: symbol,
				})
				return err
			}, func(s1, s2 string) {
				logger.Error("Failed to upgrade pointer at step", "pointer", cwAddr, "from-step", s1, "to-step", s2)
			})
		}); err != nil {
			return err
		}
	}
	return nil
}

func MigrateERCCW721Pointers(ctx sdk.Context, k *keeper.Keeper) error {
	iter := prefix.NewStore(ctx.KVStore(k.GetStoreKey()), append(types.PointerRegistryPrefix, types.PointerERC721CW721Prefix...)).ReverseIterator(nil, nil)
	defer func() { _ = iter.Close() }()
	seen := map[string]struct{}{}
	for ; iter.Valid(); iter.Next() {
		cwAddr, addr, err := decodeERCPointerRegistryEntry(iter.Key(), iter.Value())
		if err != nil {
			return fmt.Errorf("decode cw721 pointer registry entry: %w", err)
		}
		if _, ok := seen[cwAddr]; ok {
			continue
		}
		seen[cwAddr] = struct{}{}
		oName, err := k.QueryERCSingleOutput(ctx, "cw721", addr, "name")
		if err != nil {
			return fmt.Errorf("query cw721 pointer %q name: %w", cwAddr, err)
		}
		name, err := stringPointerMetadata("name", oName)
		if err != nil {
			return fmt.Errorf("decode cw721 pointer %q metadata: %w", cwAddr, err)
		}
		oSymbol, err := k.QueryERCSingleOutput(ctx, "cw721", addr, "symbol")
		if err != nil {
			return fmt.Errorf("query cw721 pointer %q symbol: %w", cwAddr, err)
		}
		symbol, err := stringPointerMetadata("symbol", oSymbol)
		if err != nil {
			return fmt.Errorf("decode cw721 pointer %q metadata: %w", cwAddr, err)
		}
		if err := runERCPointerUpgrade("cw721", cwAddr, func() error {
			return k.RunWithOneOffEVMInstance(ctx, func(e *vm.EVM) error {
				_, err := k.UpsertERCCW721Pointer(ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx)), e, cwAddr, utils.ERCMetadata{
					Name:   name,
					Symbol: symbol,
				})
				return err
			}, func(s1, s2 string) {
				logger.Error("Failed to upgrade pointer at step", "pointer", cwAddr, "from-step", s1, "to-step", s2)
			})
		}); err != nil {
			return err
		}
	}
	return nil
}

func MigrateERCCW1155Pointers(ctx sdk.Context, k *keeper.Keeper) error {
	iter := prefix.NewStore(ctx.KVStore(k.GetStoreKey()), append(types.PointerRegistryPrefix, types.PointerERC1155CW1155Prefix...)).ReverseIterator(nil, nil)
	defer func() { _ = iter.Close() }()
	seen := map[string]struct{}{}
	for ; iter.Valid(); iter.Next() {
		cwAddr, addr, err := decodeERCPointerRegistryEntry(iter.Key(), iter.Value())
		if err != nil {
			return fmt.Errorf("decode cw1155 pointer registry entry: %w", err)
		}
		if _, ok := seen[cwAddr]; ok {
			continue
		}
		seen[cwAddr] = struct{}{}
		oName, err := k.QueryERCSingleOutput(ctx, "cw1155", addr, "name")
		if err != nil {
			return fmt.Errorf("query cw1155 pointer %q name: %w", cwAddr, err)
		}
		name, err := stringPointerMetadata("name", oName)
		if err != nil {
			return fmt.Errorf("decode cw1155 pointer %q metadata: %w", cwAddr, err)
		}
		oSymbol, err := k.QueryERCSingleOutput(ctx, "cw1155", addr, "symbol")
		if err != nil {
			return fmt.Errorf("query cw1155 pointer %q symbol: %w", cwAddr, err)
		}
		symbol, err := stringPointerMetadata("symbol", oSymbol)
		if err != nil {
			return fmt.Errorf("decode cw1155 pointer %q metadata: %w", cwAddr, err)
		}
		if err := runERCPointerUpgrade("cw1155", cwAddr, func() error {
			return k.RunWithOneOffEVMInstance(ctx, func(e *vm.EVM) error {
				_, err := k.UpsertERCCW1155Pointer(ctx.WithGasMeter(sdk.NewInfiniteGasMeterWithMultiplier(ctx)), e, cwAddr, utils.ERCMetadata{
					Name:   name,
					Symbol: symbol,
				})
				return err
			}, func(s1, s2 string) {
				logger.Error("Failed to upgrade pointer at step", "pointer", cwAddr, "from-step", s1, "to-step", s2)
			})
		}); err != nil {
			return err
		}
	}
	return nil
}

func MigrateCWERC20Pointers(ctx sdk.Context, k *keeper.Keeper) error {
	iter := prefix.NewStore(ctx.KVStore(k.GetStoreKey()), append(types.PointerRegistryPrefix, types.PointerCW20ERC20Prefix...)).ReverseIterator(nil, nil)
	defer func() { _ = iter.Close() }()
	bz, err := json.Marshal(map[string]interface{}{})
	if err != nil {
		return fmt.Errorf("encode cw20 pointer migration message: %w", err)
	}
	moduleAcct := k.AccountKeeper().GetModuleAddress(types.ModuleName)
	codeID := k.GetStoredPointerCodeID(ctx, types.PointerType_ERC20)
	seen := map[string]struct{}{}
	for ; iter.Valid(); iter.Next() {
		evmAddr, err := decodePointerRegistryKey(iter.Key())
		if err != nil {
			return fmt.Errorf("decode cw20 wrapper registry entry: %w", err)
		}
		if _, ok := seen[evmAddr]; ok {
			continue
		}
		seen[evmAddr] = struct{}{}
		addr, err := sdk.AccAddressFromBech32(string(iter.Value()))
		if err != nil {
			return fmt.Errorf("decode cw20 wrapper %q address: %w", evmAddr, err)
		}
		_, err = k.WasmKeeper().Migrate(ctx, addr, moduleAcct, codeID, bz)
		if err != nil {
			return fmt.Errorf("migrate cw20 wrapper %q to code ID %d: %w", evmAddr, codeID, err)
		}
	}
	return nil
}

func MigrateCWERC721Pointers(ctx sdk.Context, k *keeper.Keeper) error {
	iter := prefix.NewStore(ctx.KVStore(k.GetStoreKey()), append(types.PointerRegistryPrefix, types.PointerCW721ERC721Prefix...)).ReverseIterator(nil, nil)
	defer func() { _ = iter.Close() }()
	bz, err := json.Marshal(map[string]interface{}{})
	if err != nil {
		return fmt.Errorf("encode cw721 pointer migration message: %w", err)
	}
	moduleAcct := k.AccountKeeper().GetModuleAddress(types.ModuleName)
	codeID := k.GetStoredPointerCodeID(ctx, types.PointerType_ERC721)
	seen := map[string]struct{}{}
	for ; iter.Valid(); iter.Next() {
		evmAddr, err := decodePointerRegistryKey(iter.Key())
		if err != nil {
			return fmt.Errorf("decode cw721 wrapper registry entry: %w", err)
		}
		if _, ok := seen[evmAddr]; ok {
			continue
		}
		seen[evmAddr] = struct{}{}
		addr, err := sdk.AccAddressFromBech32(string(iter.Value()))
		if err != nil {
			return fmt.Errorf("decode cw721 wrapper %q address: %w", evmAddr, err)
		}
		_, err = k.WasmKeeper().Migrate(ctx, addr, moduleAcct, codeID, bz)
		if err != nil {
			return fmt.Errorf("migrate cw721 wrapper %q to code ID %d: %w", evmAddr, codeID, err)
		}
	}
	return nil
}

func MigrateCWERC1155Pointers(ctx sdk.Context, k *keeper.Keeper) error {
	iter := prefix.NewStore(ctx.KVStore(k.GetStoreKey()), append(types.PointerRegistryPrefix, types.PointerCW1155ERC1155Prefix...)).ReverseIterator(nil, nil)
	defer func() { _ = iter.Close() }()
	bz, err := json.Marshal(map[string]interface{}{})
	if err != nil {
		return fmt.Errorf("encode cw1155 pointer migration message: %w", err)
	}
	moduleAcct := k.AccountKeeper().GetModuleAddress(types.ModuleName)
	codeID := k.GetStoredPointerCodeID(ctx, types.PointerType_ERC1155)
	seen := map[string]struct{}{}
	for ; iter.Valid(); iter.Next() {
		evmAddr, err := decodePointerRegistryKey(iter.Key())
		if err != nil {
			return fmt.Errorf("decode cw1155 wrapper registry entry: %w", err)
		}
		if _, ok := seen[evmAddr]; ok {
			continue
		}
		seen[evmAddr] = struct{}{}
		addr, err := sdk.AccAddressFromBech32(string(iter.Value()))
		if err != nil {
			return fmt.Errorf("decode cw1155 wrapper %q address: %w", evmAddr, err)
		}
		_, err = k.WasmKeeper().Migrate(ctx, addr, moduleAcct, codeID, bz)
		if err != nil {
			return fmt.Errorf("migrate cw1155 wrapper %q to code ID %d: %w", evmAddr, codeID, err)
		}
	}
	return nil
}
