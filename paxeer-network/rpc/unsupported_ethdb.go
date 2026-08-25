package evmrpc

import (
	"errors"

	"github.com/ethereum/go-ethereum/core/rawdb"
	"github.com/ethereum/go-ethereum/ethdb"
)

var errEthereumChainDatabaseUnavailable = errors.New("Paxeer does not expose an Ethereum chain database")

var unsupportedEthereumChainDatabase = rawdb.NewDatabase(unsupportedEthereumKeyValueStore{})

type unsupportedEthereumKeyValueStore struct{}

func (unsupportedEthereumKeyValueStore) Has([]byte) (bool, error) {
	return false, errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumKeyValueStore) Get([]byte) ([]byte, error) {
	return nil, errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumKeyValueStore) Put([]byte, []byte) error {
	return errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumKeyValueStore) Delete([]byte) error {
	return errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumKeyValueStore) DeleteRange([]byte, []byte) error {
	return errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumKeyValueStore) Stat() (string, error) {
	return "", errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumKeyValueStore) Compact([]byte, []byte) error {
	return errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumKeyValueStore) Close() error {
	return errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumKeyValueStore) NewBatch() ethdb.Batch {
	return unsupportedEthereumBatch{}
}

func (unsupportedEthereumKeyValueStore) NewBatchWithSize(int) ethdb.Batch {
	return unsupportedEthereumBatch{}
}

func (unsupportedEthereumKeyValueStore) NewIterator([]byte, []byte) ethdb.Iterator {
	return unsupportedEthereumIterator{}
}

type unsupportedEthereumBatch struct{}

func (unsupportedEthereumBatch) Put([]byte, []byte) error {
	return errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumBatch) Delete([]byte) error {
	return errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumBatch) ValueSize() int {
	return 0
}

func (unsupportedEthereumBatch) Write() error {
	return errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumBatch) Reset() {}

func (unsupportedEthereumBatch) Replay(ethdb.KeyValueWriter) error {
	return errEthereumChainDatabaseUnavailable
}

type unsupportedEthereumIterator struct{}

func (unsupportedEthereumIterator) Next() bool {
	return false
}

func (unsupportedEthereumIterator) Error() error {
	return errEthereumChainDatabaseUnavailable
}

func (unsupportedEthereumIterator) Key() []byte {
	return nil
}

func (unsupportedEthereumIterator) Value() []byte {
	return nil
}

func (unsupportedEthereumIterator) Release() {}
