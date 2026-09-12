package custodyproof

import (
    "bytes"
    "crypto/ed25519"
    "crypto/sha256"
    "encoding/binary"
    "encoding/json"
    "errors"
    "fmt"
    "os"
    "path/filepath"
    "syscall"
    "time"

    "github.com/sidiora-labs/paxeer-network/consensus/types"
    "github.com/syndtr/goleveldb/leveldb"
    "github.com/syndtr/goleveldb/leveldb/opt"
)

const MaxHistoryChunk = 128
const MaxHistoryRecord = 1024 * 1024

type historyRecord struct {
    GenesisSHA256 string `json:"genesis_sha256"`
    CometChainID string `json:"comet_chain_id"`
    Height int64 `json:"height"`
    Previous []byte `json:"previous"`
    Prefix []byte `json:"prefix"`
    Block *LightBlock `json:"block,omitempty"`
    Signature []byte `json:"signature"`
}

type HistoryStatus struct {
    Version string `json:"version"`
    Height int64 `json:"height"`
    HistoricalCatchup bool `json:"historical_catchup"`
    Prefix string `json:"prefix"`
}

type History struct {
    directory string
    database *leveldb.DB
    key ed25519.PrivateKey
    expected Expected
    genesis *types.GenesisDoc
    genesisDigest [32]byte
    validators *types.ValidatorSet
    head *historyRecord
    failure error
}

func protectedFile(path string, maximum int) ([]byte, error) {
    file, err := os.OpenFile(path, os.O_RDONLY|syscall.O_NOFOLLOW|syscall.O_NONBLOCK, 0)
    if err != nil { return nil, err }
    defer file.Close()
    info, err := file.Stat()
    if err != nil { return nil, err }
    stat, valid := info.Sys().(*syscall.Stat_t)
    if !valid || !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 || stat.Uid != uint32(os.Geteuid()) ||
        stat.Nlink != 1 || info.Size() <= 0 || info.Size() > int64(maximum) {
        return nil, errors.New("protected history file identity")
    }
    data := make([]byte, info.Size())
    if _, err := file.ReadAt(data, 0); err != nil { return nil, err }
    return data, nil
}

func privateDirectory(path string) error {
    absolute, err := filepath.Abs(path)
    if err != nil { return err }
    if err := os.MkdirAll(absolute, 0700); err != nil { return err }
    for current := absolute; current != filepath.Dir(current); current = filepath.Dir(current) {
        info, err := os.Lstat(current)
        if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 { return errors.New("history directory identity") }
    }
    info, err := os.Lstat(absolute)
    if err != nil { return err }
    stat, valid := info.Sys().(*syscall.Stat_t)
    if !valid || stat.Uid != uint32(os.Geteuid()) || info.Mode().Perm()&0077 != 0 {
        return errors.New("history directory permissions")
    }
    return nil
}

func recordKey(height int64) []byte {
    key := make([]byte, 9)
    key[0] = 'h'
    binary.BigEndian.PutUint64(key[1:], uint64(height))
    return key
}

func recordMessage(record *historyRecord) ([]byte, error) {
    value := *record
    value.Signature = nil
    data, err := json.Marshal(value)
    if err != nil { return nil, err }
    return append([]byte("LX:CUSTODY:HISTORY:v2"), data...), nil
}

func (history *History) seal(record *historyRecord) ([]byte, error) {
    message, err := recordMessage(record)
    if err != nil { return nil, err }
    record.Signature = ed25519.Sign(history.key, message)
    encoded, err := json.Marshal(record)
    if err != nil || len(encoded) > MaxHistoryRecord { return nil, errors.New("history record bound") }
    return encoded, nil
}

func (history *History) decode(encoded []byte) (*historyRecord, error) {
    if len(encoded) == 0 || len(encoded) > MaxHistoryRecord { return nil, errors.New("history record bound") }
    var record historyRecord
    decoder := json.NewDecoder(bytes.NewReader(encoded))
    decoder.DisallowUnknownFields()
    if err := decoder.Decode(&record); err != nil { return nil, err }
    canonical, err := json.Marshal(record)
    if err != nil || !bytes.Equal(encoded, canonical) { return nil, errors.New("noncanonical history record") }
    if record.GenesisSHA256 != history.expected.GenesisSHA256 || record.CometChainID != history.expected.CometChainID ||
        record.Height < 0 || len(record.Prefix) != 32 || len(record.Previous) != 32 {
        return nil, errors.New("history record identity")
    }
    message, err := recordMessage(&record)
    if err != nil || !ed25519.Verify(history.key.Public().(ed25519.PublicKey), message, record.Signature) {
        return nil, errors.New("history record authentication")
    }
    if record.Height == 0 {
        if record.Block != nil { return nil, errors.New("history genesis record") }
    } else if record.Block == nil || record.Block.Commit.ValidateBasic(history.expected.CometChainID) != nil ||
        record.Block.Commit.Height != record.Height {
        return nil, errors.New("history header identity")
    }
    return &record, nil
}

func (history *History) get(height int64) (*historyRecord, error) {
    encoded, err := history.database.Get(recordKey(height), nil)
    if err != nil { return nil, fmt.Errorf("authenticated history unavailable at %d: %w", height, err) }
    record, err := history.decode(encoded)
    if err != nil || record.Height != height { return nil, errors.New("authenticated history height") }
    return record, nil
}

func (history *History) writeHighWater(encoded []byte) error {
    temporary, err := os.CreateTemp(history.directory, ".high-water-")
    if err != nil { return err }
    name := temporary.Name()
    defer os.Remove(name)
    if _, err := temporary.Write(encoded); err != nil { temporary.Close(); return err }
    if err := temporary.Sync(); err != nil { temporary.Close(); return err }
    if err := temporary.Close(); err != nil { return err }
    if err := os.Rename(name, filepath.Join(history.directory, "high-water.json")); err != nil { return err }
    directory, err := os.Open(history.directory)
    if err != nil { return err }
    defer directory.Close()
    return directory.Sync()
}

func OpenHistory(directory, keyPath string, expected Expected, genesisBytes []byte, now time.Time) (*History, error) {
    genesis, validators, err := genesisValidators(genesisBytes, expected, now)
    if err != nil { return nil, err }
    seed, err := protectedFile(keyPath, ed25519.SeedSize)
    if err != nil || len(seed) != ed25519.SeedSize { return nil, errors.New("history attestor key") }
    if err := privateDirectory(directory); err != nil { return nil, err }
    anchorPath := filepath.Join(directory, "high-water.json")
    anchor, anchorErr := protectedFile(anchorPath, MaxHistoryRecord)
    if anchorErr != nil && !os.IsNotExist(anchorErr) { return nil, anchorErr }
    dbPath := filepath.Join(directory, "history.db")
    info, statErr := os.Lstat(dbPath)
    if statErr == nil && (!info.IsDir() || info.Mode()&os.ModeSymlink != 0) { return nil, errors.New("history database identity") }
    if statErr != nil && !os.IsNotExist(statErr) { return nil, statErr }
    if statErr != nil && anchorErr == nil { return nil, errors.New("history database rollback or deletion") }
    database, err := leveldb.OpenFile(dbPath, &opt.Options{ErrorIfMissing: anchorErr == nil,
        WriteBuffer: 1024*1024, BlockCacheCapacity: 4*1024*1024, OpenFilesCacheCapacity: 16,
        Strict: opt.StrictAll})
    if err != nil { return nil, err }
    history := &History{directory: directory, database: database, key: ed25519.NewKeyFromSeed(seed),
        expected: expected, genesis: genesis, genesisDigest: sha256.Sum256(genesisBytes), validators: validators}
    if err := history.openHead(anchor, anchorErr == nil); err != nil { database.Close(); return nil, err }
    if err := history.checkIndex(); err != nil { database.Close(); return nil, err }
    return history, nil
}

func (history *History) checkIndex() error {
    iterator := history.database.NewIterator(nil, nil)
    defer iterator.Release()
    count := int64(0)
    for iterator.Next() {
        count++
        if count > MaxHistory+3 { return errors.New("history retained record bound") }
        key := iterator.Key()
        if bytes.Equal(key, []byte("head")) { continue }
        if len(key) != 9 || key[0] != 'h' { return errors.New("history record key") }
        height := binary.BigEndian.Uint64(key[1:])
        first := max(int64(2), history.head.Height-MaxHistory+1)
        if height > uint64(history.head.Height) || (height > 1 && height < uint64(first)) {
            return errors.New("history retained height window")
        }
    }
    if err := iterator.Error(); err != nil { return err }
    if count != 2+min(history.head.Height, int64(MaxHistory+1)) {
        return errors.New("history retained index gap")
    }
    return nil
}

func (history *History) openHead(anchor []byte, anchored bool) error {
    encoded, err := history.database.Get([]byte("head"), nil)
    if errors.Is(err, leveldb.ErrNotFound) && !anchored {
        digest := sha256.Sum256([]byte(history.expected.GenesisSHA256+history.expected.CometChainID))
        record := &historyRecord{GenesisSHA256: history.expected.GenesisSHA256, CometChainID: history.expected.CometChainID,
            Previous: make([]byte, 32), Prefix: digest[:]}
        encoded, err = history.seal(record)
        if err != nil { return err }
        batch := new(leveldb.Batch)
        batch.Put([]byte("head"), encoded)
        batch.Put(recordKey(0), encoded)
        if err := history.database.Write(batch, &opt.WriteOptions{Sync:true}); err != nil { return err }
    } else if err != nil { return err }
    head, err := history.decode(encoded)
    if err != nil { return err }
    if !anchored {
        if head.Height != 0 { return errors.New("missing authenticated history high-water") }
    } else {
        highWater, err := history.decode(anchor)
        if err != nil { return err }
        if highWater.Height > head.Height || head.Height-highWater.Height > MaxHistoryChunk {
            return errors.New("history high-water rollback")
        }
        previous := highWater
        for height := highWater.Height; height <= head.Height; height++ {
            record, err := history.get(height)
            if err != nil { return err }
            if height == highWater.Height {
                if !bytes.Equal(record.Signature, highWater.Signature) { return errors.New("history high-water equivocation") }
            } else if !bytes.Equal(record.Previous, previous.Prefix) {
                return errors.New("interrupted history prefix")
            }
            previous = record
            if height == head.Height { break }
        }
        if !bytes.Equal(previous.Signature, head.Signature) { return errors.New("history head disagreement") }
    }
    history.head = head
    return history.writeHighWater(encoded)
}

func (history *History) Close() error { return history.database.Close() }

func (history *History) Status(now time.Time) (HistoryStatus, error) {
    if history.failure != nil { return HistoryStatus{}, history.failure }
    historical := history.head.Height == 0 || !history.head.Block.Commit.Time.Add(TrustingPeriod).After(now)
    return HistoryStatus{Version: Version, Height: history.head.Height,
        HistoricalCatchup: historical, Prefix: hexBytes(history.head.Prefix)}, nil
}

func (history *History) Advance(entries []LightBlock, now time.Time) error {
    if history.failure != nil { return history.failure }
    if len(entries) == 0 || len(entries) > MaxHistoryChunk { return errors.New("history work chunk bound") }
    batch := new(leveldb.Batch)
    head := history.head
    var lastEncoded []byte
    for index := range entries {
        entry := &entries[index]
        if entry.Commit.Header == nil { return errors.New("missing history header") }
        height := entry.Commit.Height
        if height <= 0 || (index > 0 && height != entries[index-1].Commit.Height+1) {
            return errors.New("history chunk sequence")
        }
        var previous *types.SignedHeader
        if height <= head.Height {
            record, err := history.get(height)
            if err != nil { return err }
            if !bytes.Equal(record.Block.Commit.Hash(), entry.Commit.Hash()) { return errors.New("history equivocation") }
            if height > 1 {
                parent, err := history.get(height-1)
                if err != nil { return err }
                previous = &parent.Block.Commit.SignedHeader
            }
            if err := verifyEntry(previous, entry, history.genesis, history.validators, now); err != nil { return err }
            continue
        }
        if height != head.Height+1 { return errors.New("history gap") }
        if head.Height > 0 { previous = &head.Block.Commit.SignedHeader }
        if err := verifyEntry(previous, entry, history.genesis, history.validators, now); err != nil { return err }
        data, err := json.Marshal(entry)
        if err != nil { return err }
        digest := sha256.Sum256(append(append([]byte{}, head.Prefix...), data...))
        record := &historyRecord{GenesisSHA256: history.expected.GenesisSHA256, CometChainID: history.expected.CometChainID,
            Height: height, Previous: head.Prefix, Prefix: digest[:], Block: entry}
        lastEncoded, err = history.seal(record)
        if err != nil { return err }
        batch.Put(recordKey(height), lastEncoded)
        if height > MaxHistory+1 { batch.Delete(recordKey(height-MaxHistory)) }
        head = record
    }
    if head.Height == history.head.Height { return nil }
    batch.Put([]byte("head"), lastEncoded)
    if err := history.database.Write(batch, &opt.WriteOptions{Sync:true}); err != nil {
        history.failure = fmt.Errorf("history durability failed; authenticated reopen required: %w", err)
        return history.failure
    }
    history.head = head
    if err := history.writeHighWater(lastEncoded); err != nil {
        history.failure = fmt.Errorf("history durability failed; authenticated reopen required: %w", err)
        return history.failure
    }
    return nil
}

func (history *History) Verify(request *Request, now time.Time) (*Result, error) {
    if request == nil || request.Bundle.Version != Version ||
        request.Expected.ChainID != history.expected.ChainID ||
        request.Expected.GenesisSHA256 != history.expected.GenesisSHA256 ||
        request.Expected.CometChainID != history.expected.CometChainID || len(request.Bundle.History) == 0 ||
        sha256.Sum256(request.Bundle.Genesis) != history.genesisDigest {
        return nil, errors.New("history request identity")
    }
    if err := history.Advance(request.Bundle.History, now); err != nil { return nil, err }
    if request.Bundle.FinalizedHeight <= 0 || request.Bundle.FinalizedHeight >= int64(^uint64(0)>>1) ||
        request.Bundle.FinalizedHeight+1 != history.head.Height ||
        request.Bundle.History[len(request.Bundle.History)-1].Commit.Height != history.head.Height {
        return nil, errors.New("unverified live head")
    }
    return verifyState(request, now, func(height int64) (*types.SignedHeader, error) {
        record, err := history.get(height)
        if err != nil { return nil, err }
        return &record.Block.Commit.SignedHeader, nil
    })
}

func (history *History) Export() ([]LightBlock, error) {
    if history.failure != nil { return nil, history.failure }
    if history.head.Height <= 0 || history.head.Height > MaxHistory { return nil, errors.New("individual history export bound") }
    result := make([]LightBlock, 0, history.head.Height)
    for height := int64(1); height <= history.head.Height; height++ {
        record, err := history.get(height)
        if err != nil { return nil, err }
        result = append(result, *record.Block)
    }
    return result, nil
}
