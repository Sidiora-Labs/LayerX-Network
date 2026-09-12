package custodyproof

import (
    "bytes"
    "os"
    "path/filepath"
    "testing"
    "syscall"
    "time"

    "github.com/syndtr/goleveldb/leveldb/opt"
)

func realHistory(t *testing.T) (*Request, *History, string, time.Time) {
    t.Helper()
    request := realRequest(t)
    now := request.Bundle.History[len(request.Bundle.History)-1].Commit.Time.Add(time.Second)
    root := t.TempDir()
    key := filepath.Join(root, "attestor.key")
    if err := os.WriteFile(key, bytes.Repeat([]byte{0x55}, 32), 0600); err != nil { t.Fatal(err) }
    history, err := OpenHistory(filepath.Join(root, "history"), key, request.Expected, request.Bundle.Genesis, now)
    if err != nil { t.Fatal(err) }
    t.Cleanup(func() { history.Close() })
    return request, history, key, now
}

func advanceReal(t *testing.T, history *History, entries []LightBlock, now time.Time) {
    t.Helper()
    for start := 0; start < len(entries); start += 7 {
        if err := history.Advance(entries[start:min(start+7, len(entries))], now); err != nil { t.Fatal(err) }
    }
}

func cachedRequest(request *Request) *Request {
    selected := *request
    selected.Bundle = request.Bundle
    if len(selected.Bundle.History) > MaxHistoryChunk {
        selected.Bundle.History = selected.Bundle.History[len(selected.Bundle.History)-MaxHistoryChunk:]
    }
    return &selected
}

func historyStatus(t *testing.T, history *History, now time.Time) HistoryStatus {
    t.Helper()
    status, err := history.Status(now)
    if err != nil { t.Fatal(err) }
    return status
}

func TestRealHistoryRestartAndExpiredCatchup(t *testing.T) {
    request, history, key, now := realHistory(t)
    if historyStatus(t, history, now).Height != 0 || !historyStatus(t, history, now).HistoricalCatchup { t.Fatal("genesis status") }
    advanceReal(t, history, request.Bundle.History, now.Add(2*TrustingPeriod))
    if !historyStatus(t, history, now.Add(2*TrustingPeriod)).HistoricalCatchup { t.Fatal("expired cache concealed") }
    if _, err := history.Verify(cachedRequest(request), now.Add(2*TrustingPeriod)); err == nil { t.Fatal("expired live proof accepted") }
    if err := history.Close(); err != nil { t.Fatal(err) }
    reopened, err := OpenHistory(history.directory, key, request.Expected, request.Bundle.Genesis, now)
    if err != nil { t.Fatal(err) }
    defer reopened.Close()
    if historyStatus(t, reopened, now).Height != request.Bundle.FinalizedHeight+1 || historyStatus(t, reopened, now).HistoricalCatchup {
        t.Fatal("authenticated history restart")
    }
    result, err := reopened.Verify(cachedRequest(request), now)
    if err != nil { t.Fatal(err) }
    if result.DepositID != request.Expected.DepositID { t.Fatal("recovered deposit binding") }
    exported, err := reopened.Export()
    if err != nil || len(exported) != len(request.Bundle.History) { t.Fatal("public history export", err) }
}

func TestRealHistoryRecoversCommittedChunkBeforeAnchor(t *testing.T) {
    request, history, key, now := realHistory(t)
    count := len(request.Bundle.History)
    if count < 3 { t.Fatal("real fixture requires adjacent history") }
    advanceReal(t, history, request.Bundle.History[:count-1], now)
    anchor := filepath.Join(history.directory, "high-water.json")
    prior, err := os.ReadFile(anchor)
    if err != nil { t.Fatal(err) }
    if err := history.Advance(request.Bundle.History[count-1:], now); err != nil { t.Fatal(err) }
    if err := history.Close(); err != nil { t.Fatal(err) }
    if err := os.WriteFile(anchor, prior, 0600); err != nil { t.Fatal(err) }
    recovered, err := OpenHistory(history.directory, key, request.Expected, request.Bundle.Genesis, now)
    if err != nil { t.Fatal(err) }
    defer recovered.Close()
    if historyStatus(t, recovered, now).Height != int64(count) { t.Fatal("committed history regressed") }
    if _, err := recovered.Verify(cachedRequest(request), now); err != nil { t.Fatal(err) }
}

func TestRealHistoryRefusesDatabaseRollbackAndCorruption(t *testing.T) {
    for _, fault := range []string{"rollback", "corruption", "anchor-corruption", "missing-anchor", "deleted-database", "missing-retained-record"} {
        t.Run(fault, func(t *testing.T) {
            request, history, key, now := realHistory(t)
            advanceReal(t, history, request.Bundle.History, now)
            switch fault {
            case "rollback":
                older, err := history.database.Get(recordKey(1), nil)
                if err != nil { t.Fatal(err) }
                if err := history.database.Put([]byte("head"), older, &opt.WriteOptions{Sync:true}); err != nil { t.Fatal(err) }
            case "missing-retained-record":
                if err := history.database.Delete(recordKey(1), &opt.WriteOptions{Sync:true}); err != nil { t.Fatal(err) }
            case "corruption":
                if err := history.database.Put([]byte("head"), []byte("corrupt"), &opt.WriteOptions{Sync:true}); err != nil { t.Fatal(err) }
            }
            if err := history.Close(); err != nil { t.Fatal(err) }
            anchor := filepath.Join(history.directory, "high-water.json")
            switch fault {
            case "anchor-corruption":
                if err := os.WriteFile(anchor, []byte("corrupt"), 0600); err != nil { t.Fatal(err) }
            case "missing-anchor":
                if err := os.Remove(anchor); err != nil { t.Fatal(err) }
            case "deleted-database":
                if err := os.Rename(filepath.Join(history.directory, "history.db"), filepath.Join(history.directory, "retained.db")); err != nil { t.Fatal(err) }
            }
            accepted, err := OpenHistory(history.directory, key, request.Expected, request.Bundle.Genesis, now)
            if err == nil { accepted.Close(); t.Fatal("damaged durable history accepted") }
        })
    }
}

func TestRealHistoryRefusesGapsEquivocationAndInvalidQuorum(t *testing.T) {
    request, history, _, now := realHistory(t)
    if err := history.Advance(request.Bundle.History[1:2], now); err == nil { t.Fatal("caller checkpoint accepted") }
    if err := history.Advance(nil, now); err == nil { t.Fatal("empty work chunk accepted") }
    oversized := make([]LightBlock, MaxHistoryChunk+1)
    if err := history.Advance(oversized, now); err == nil { t.Fatal("unbounded work chunk accepted") }
    advanceReal(t, history, request.Bundle.History, now)
    changed := realRequest(t)
    changed.Bundle.History[0].Commit.Header.AppHash[0] ^= 1
    if err := history.Advance(changed.Bundle.History[:1], now); err == nil { t.Fatal("equivocating cached header accepted") }
    changed = realRequest(t)
    signatures := changed.Bundle.History[0].Commit.Commit.Signatures
    damaged := false
    for index := range signatures {
        if len(signatures[index].Signature) > 0 {
            signatures[index].Signature[0] ^= 1
            damaged = true
            break
        }
    }
    if !damaged { t.Fatal("real fixture has no signatures") }
    if err := history.Advance(changed.Bundle.History[:1], now); err == nil { t.Fatal("invalid cached quorum accepted") }
    changed = realRequest(t)
    changed.Bundle.Genesis[0] ^= 1
    if _, err := history.Verify(cachedRequest(changed), now); err == nil { t.Fatal("changed raw genesis accepted through cache") }
    changed = realRequest(t)
    changed.Bundle.History = changed.Bundle.History[:len(changed.Bundle.History)-1]
    if _, err := history.Verify(changed, now); err == nil { t.Fatal("history ending before final head accepted") }
    if _, err := history.Verify(cachedRequest(request), now); err != nil { t.Fatal("refusal corrupted committed state", err) }
}

func TestRealHistoryRefusesChangedAuthorityAndUnsafePaths(t *testing.T) {
    request, history, key, now := realHistory(t)
    advanceReal(t, history, request.Bundle.History, now)
    if err := history.Close(); err != nil { t.Fatal(err) }
    if err := os.WriteFile(key, bytes.Repeat([]byte{0x56}, 32), 0600); err != nil { t.Fatal(err) }
    if accepted, err := OpenHistory(history.directory, key, request.Expected, request.Bundle.Genesis, now); err == nil {
        accepted.Close(); t.Fatal("replacement history authority accepted")
    }
    if err := os.WriteFile(key, bytes.Repeat([]byte{0x55}, 32), 0600); err != nil { t.Fatal(err) }
    linked := filepath.Join(filepath.Dir(key), "linked-key")
    if err := os.Symlink(key, linked); err != nil { t.Fatal(err) }
    if accepted, err := OpenHistory(history.directory, linked, request.Expected, request.Bundle.Genesis, now); err == nil {
        accepted.Close(); t.Fatal("symlink authority accepted")
    }
    if err := os.Chmod(key, 0644); err != nil { t.Fatal(err) }
    if accepted, err := OpenHistory(history.directory, key, request.Expected, request.Bundle.Genesis, now); err == nil {
        accepted.Close(); t.Fatal("public authority seed accepted")
    }
}

func TestRealHistoryRefusesFIFOAuthorityAndHighWater(t *testing.T) {
    for _, target := range []string{"authority", "high-water"} {
        t.Run(target, func(t *testing.T) {
            request, history, key, now := realHistory(t)
            advanceReal(t, history, request.Bundle.History, now)
            if err := history.Close(); err != nil { t.Fatal(err) }
            path := key
            if target == "high-water" { path = filepath.Join(history.directory, "high-water.json") }
            if err := os.Remove(path); err != nil { t.Fatal(err) }
            if err := syscall.Mkfifo(path, 0600); err != nil { t.Fatal(err) }
            if accepted, err := OpenHistory(history.directory, key, request.Expected, request.Bundle.Genesis, now); err == nil {
                accepted.Close(); t.Fatal("FIFO trust input accepted")
            }
        })
    }
}

func TestRealHistoryPoisonsFailedAnchorWriteUntilReopen(t *testing.T) {
    request, history, key, now := realHistory(t)
    count := len(request.Bundle.History)
    if count < 3 { t.Fatal("real fixture requires adjacent history") }
    advanceReal(t, history, request.Bundle.History[:count-1], now)
    anchor := filepath.Join(history.directory, "high-water.json")
    retained := filepath.Join(history.directory, "retained-anchor.json")
    if err := os.Rename(anchor, retained); err != nil { t.Fatal(err) }
    if err := os.Mkdir(anchor, 0700); err != nil { t.Fatal(err) }
    if err := history.Advance(request.Bundle.History[count-1:], now); err == nil { t.Fatal("failed anchor write accepted") }
    if err := history.Advance(request.Bundle.History[count-1:], now); err == nil { t.Fatal("cached retry bypassed anchor failure") }
    if _, err := history.Verify(cachedRequest(request), now); err == nil { t.Fatal("proof issued without durable anchor") }
    if _, err := history.Export(); err == nil { t.Fatal("history exported after durability failure") }
    if _, err := history.Status(now); err == nil { t.Fatal("successful status after durability failure") }
    if err := history.Close(); err != nil { t.Fatal(err) }
    if err := os.Remove(anchor); err != nil { t.Fatal(err) }
    if err := os.Rename(retained, anchor); err != nil { t.Fatal(err) }
    recovered, err := OpenHistory(history.directory, key, request.Expected, request.Bundle.Genesis, now)
    if err != nil { t.Fatal(err) }
    defer recovered.Close()
    if historyStatus(t, recovered, now).Height != int64(count) { t.Fatal("durable committed header lost") }
    if _, err := recovered.Verify(cachedRequest(request), now); err != nil { t.Fatal(err) }
}
