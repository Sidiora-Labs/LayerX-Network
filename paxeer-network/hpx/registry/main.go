// hpx-registry — the HyperPax node distribution + peer registry service.
//
// Two jobs in one tiny stdlib-only binary:
//
//  1. Serve the install artifacts (paxd binary, libwasmvm .so, genesis,
//     fullnode/validator config variants, the hpx CLI + install scripts).
//     These live under ARTIFACTS_DIR and are served at the URL root, so e.g.
//     GET /get-hpx.sh, /hpx, /paxd, /genesis.json, /lib/libwasmvm.x86_64.so,
//     /config/fullnode/config.toml, /chain-info.json.
//
//  2. Maintain the live peer registry. A node POSTs /api/register with its
//     CometBFT node id once it is up; the server records it and returns the
//     current peer list. Any node (new or updating) can pull /api/peers[.txt]
//     to learn every known peer, so the mesh grows automatically.
//
// State is a single JSON file under DATA_DIR (registry.json), guarded by a
// mutex. No external dependencies, no database.
package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
)

// ---- config ----------------------------------------------------------------

type config struct {
	Addr         string
	ArtifactsDir string
	DataDir      string
	RegisterTok  string   // optional; if set, POST /api/register requires X-HPX-Token
	SeedPeers    []string // always-present peers (e.g. the validator), id@host:port
	ChainID      string

	// State sync: a fresh node cannot blocksync from genesis because the chain
	// is pruned, so it bootstraps from a recent snapshot. We compute a trusted
	// light-client anchor (height+hash) from a live RPC on demand.
	StateSyncRPC     string // http base, e.g. http://1.2.3.4:26657 (defaults to seed host)
	StateSyncServers string // value for config.toml rpc-servers (>=2 entries)
	TrustOffset      int    // trust-height = latest - TrustOffset
	TrustPeriod      string
}

func loadConfig() config {
	c := config{
		Addr:         env("HPX_ADDR", ":8099"),
		ArtifactsDir: env("HPX_ARTIFACTS_DIR", "/srv/hpx/artifacts"),
		DataDir:      env("HPX_DATA_DIR", "/srv/hpx/data"),
		RegisterTok:  os.Getenv("HPX_REGISTER_TOKEN"),
		ChainID:      env("HPX_CHAIN_ID", "hyperpax_125-1"),
	}
	for _, p := range strings.Split(os.Getenv("HPX_SEED_PEERS"), ",") {
		if p = strings.TrimSpace(p); p != "" {
			c.SeedPeers = append(c.SeedPeers, p)
		}
	}
	// Derive the state-sync RPC host from the first seed peer (id@host:port)
	// unless explicitly overridden.
	seedHost := ""
	if len(c.SeedPeers) > 0 {
		if at := strings.Index(c.SeedPeers[0], "@"); at >= 0 {
			hp := c.SeedPeers[0][at+1:]
			if h, _, err := net.SplitHostPort(hp); err == nil {
				seedHost = h
			}
		}
	}
	c.StateSyncRPC = env("HPX_STATESYNC_RPC", "")
	if c.StateSyncRPC == "" && seedHost != "" {
		c.StateSyncRPC = "http://" + seedHost + ":26657"
	}
	c.StateSyncServers = env("HPX_STATESYNC_RPC_SERVERS", "")
	if c.StateSyncServers == "" && seedHost != "" {
		rpc := seedHost + ":26657"
		c.StateSyncServers = rpc + "," + rpc
	}
	c.TrustOffset = envInt("HPX_STATESYNC_TRUST_OFFSET", 2000)
	c.TrustPeriod = env("HPX_STATESYNC_TRUST_PERIOD", "168h0m0s")
	return c
}

func envInt(k string, def int) int {
	if v := strings.TrimSpace(os.Getenv(k)); v != "" {
		if n, err := strconv.Atoi(v); err == nil {
			return n
		}
	}
	return def
}

func env(k, def string) string {
	if v := strings.TrimSpace(os.Getenv(k)); v != "" {
		return v
	}
	return def
}

// ---- registry model --------------------------------------------------------

type Node struct {
	NodeID    string `json:"node_id"`
	Moniker   string `json:"moniker"`
	IP        string `json:"ip"`
	P2PPort   int    `json:"p2p_port"`
	Type      string `json:"type"` // fullnode | validator
	Version   string `json:"version"`
	FirstSeen string `json:"first_seen"`
	LastSeen  string `json:"last_seen"`
}

// Peer renders the CometBFT persistent-peer string id@host:port.
func (n Node) Peer() string {
	port := n.P2PPort
	if port == 0 {
		port = 26656
	}
	return fmt.Sprintf("%s@%s:%d", n.NodeID, n.IP, port)
}

type registry struct {
	mu    sync.Mutex
	path  string
	Nodes map[string]*Node `json:"nodes"` // keyed by node_id
}

func openRegistry(dir string) (*registry, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, err
	}
	r := &registry{path: filepath.Join(dir, "registry.json"), Nodes: map[string]*Node{}}
	b, err := os.ReadFile(r.path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return r, nil
		}
		return nil, err
	}
	if len(b) > 0 {
		if err := json.Unmarshal(b, r); err != nil {
			return nil, fmt.Errorf("parse registry.json: %w", err)
		}
		if r.Nodes == nil {
			r.Nodes = map[string]*Node{}
		}
	}
	return r, nil
}

// saveLocked persists the registry; caller must hold r.mu.
func (r *registry) saveLocked() error {
	b, err := json.MarshalIndent(r, "", "  ")
	if err != nil {
		return err
	}
	tmp := r.path + ".tmp"
	if err := os.WriteFile(tmp, b, 0o644); err != nil {
		return err
	}
	return os.Rename(tmp, r.path)
}

func (r *registry) upsert(n *Node) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	now := time.Now().UTC().Format(time.RFC3339)
	if ex, ok := r.Nodes[n.NodeID]; ok {
		n.FirstSeen = ex.FirstSeen
	} else {
		n.FirstSeen = now
	}
	n.LastSeen = now
	r.Nodes[n.NodeID] = n
	return r.saveLocked()
}

// list returns all nodes sorted by first_seen (stable peer ordering).
func (r *registry) list() []*Node {
	r.mu.Lock()
	defer r.mu.Unlock()
	out := make([]*Node, 0, len(r.Nodes))
	for _, n := range r.Nodes {
		out = append(out, n)
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].FirstSeen != out[j].FirstSeen {
			return out[i].FirstSeen < out[j].FirstSeen
		}
		return out[i].NodeID < out[j].NodeID
	})
	return out
}

// ---- server ----------------------------------------------------------------

type server struct {
	cfg config
	reg *registry
}

func main() {
	cfg := loadConfig()
	reg, err := openRegistry(cfg.DataDir)
	if err != nil {
		log.Fatalf("registry: %v", err)
	}
	s := &server{cfg: cfg, reg: reg}

	mux := http.NewServeMux()
	mux.HandleFunc("/healthz", s.handleHealth)
	mux.HandleFunc("/api/register", s.handleRegister)
	mux.HandleFunc("/api/peers", s.handlePeers)
	mux.HandleFunc("/api/peers.txt", s.handlePeersTxt)
	mux.HandleFunc("/api/nodes", s.handleNodes)
	mux.HandleFunc("/api/myip", s.handleMyIP)
	mux.HandleFunc("/api/statesync", s.handleStateSync)

	// Everything else is a static artifact (binary, genesis, configs, scripts).
	fs := http.FileServer(http.Dir(cfg.ArtifactsDir))
	mux.Handle("/", noCacheScripts(fs))

	srv := &http.Server{
		Addr:              cfg.Addr,
		Handler:           logreq(mux),
		ReadHeaderTimeout: 15 * time.Second,
	}
	log.Printf("hpx-registry listening on %s (artifacts=%s data=%s chain=%s seeds=%d)",
		cfg.Addr, cfg.ArtifactsDir, cfg.DataDir, cfg.ChainID, len(cfg.SeedPeers))
	log.Fatal(srv.ListenAndServe())
}

func (s *server) handleHealth(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{"ok": true, "chain_id": s.cfg.ChainID})
}

// peerStrings returns seeds first, then every registered node, de-duplicated.
// If exclude is non-empty, the node with that id is omitted (so a node never
// lists itself as a peer).
func (s *server) peerStrings(exclude string) []string {
	seen := map[string]bool{}
	var peers []string
	add := func(p string) {
		if p == "" || seen[p] {
			return
		}
		seen[p] = true
		peers = append(peers, p)
	}
	for _, p := range s.cfg.SeedPeers {
		// allow excluding a seed too, by id prefix
		if exclude != "" && strings.HasPrefix(p, exclude+"@") {
			continue
		}
		add(p)
	}
	for _, n := range s.reg.list() {
		if exclude != "" && n.NodeID == exclude {
			continue
		}
		add(n.Peer())
	}
	return peers
}

func (s *server) handleRegister(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if s.cfg.RegisterTok != "" && r.Header.Get("X-HPX-Token") != s.cfg.RegisterTok {
		http.Error(w, "unauthorized", http.StatusUnauthorized)
		return
	}
	var n Node
	if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1<<16)).Decode(&n); err != nil {
		http.Error(w, "bad json: "+err.Error(), http.StatusBadRequest)
		return
	}
	n.NodeID = strings.ToLower(strings.TrimSpace(n.NodeID))
	if !isHexID(n.NodeID) {
		http.Error(w, "invalid node_id (expect 40-char hex CometBFT id)", http.StatusBadRequest)
		return
	}
	if n.IP == "" {
		n.IP = clientIP(r) // fall back to the source address
	}
	if net.ParseIP(n.IP) == nil {
		http.Error(w, "invalid ip", http.StatusBadRequest)
		return
	}
	if n.P2PPort == 0 {
		n.P2PPort = 26656
	}
	switch n.Type {
	case "fullnode", "validator":
	default:
		n.Type = "fullnode"
	}
	if err := s.reg.upsert(&n); err != nil {
		http.Error(w, "save failed", http.StatusInternalServerError)
		return
	}
	log.Printf("registered %s (%s) %s type=%s ver=%s", n.NodeID, n.Moniker, n.Peer(), n.Type, n.Version)
	writeJSON(w, http.StatusOK, map[string]any{
		"ok":               true,
		"node":             n,
		"peers":            s.peerStrings(n.NodeID),
		"persistent_peers": strings.Join(s.peerStrings(n.NodeID), ","),
	})
}

func (s *server) handlePeers(w http.ResponseWriter, r *http.Request) {
	self := strings.ToLower(strings.TrimSpace(r.URL.Query().Get("self")))
	peers := s.peerStrings(self)
	writeJSON(w, http.StatusOK, map[string]any{
		"chain_id":         s.cfg.ChainID,
		"count":            len(peers),
		"seeds":            s.cfg.SeedPeers,
		"peers":            peers,
		"persistent_peers": strings.Join(peers, ","),
	})
}

func (s *server) handlePeersTxt(w http.ResponseWriter, r *http.Request) {
	self := strings.ToLower(strings.TrimSpace(r.URL.Query().Get("self")))
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	fmt.Fprint(w, strings.Join(s.peerStrings(self), ","))
}

func (s *server) handleNodes(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{
		"chain_id": s.cfg.ChainID,
		"count":    len(s.reg.Nodes),
		"nodes":    s.reg.list(),
	})
}

func (s *server) handleMyIP(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "text/plain; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	fmt.Fprint(w, clientIP(r))
}

// handleStateSync returns config.toml [statesync] parameters for a fresh node:
// a recent trusted light-client anchor (height+hash) plus the rpc-servers to
// fetch a snapshot from. The chain is pruned, so blocksync-from-genesis is
// impossible; new nodes MUST state sync.
func (s *server) handleStateSync(w http.ResponseWriter, _ *http.Request) {
	w.Header().Set("Cache-Control", "no-store")
	if s.cfg.StateSyncRPC == "" || s.cfg.StateSyncServers == "" {
		writeJSON(w, http.StatusOK, map[string]any{"enable": false, "reason": "no state-sync rpc configured"})
		return
	}
	latest, err := rpcLatestHeight(s.cfg.StateSyncRPC)
	if err != nil || latest <= 0 {
		writeJSON(w, http.StatusOK, map[string]any{"enable": false, "reason": fmt.Sprintf("status query failed: %v", err)})
		return
	}
	trustHeight := latest - s.cfg.TrustOffset
	if trustHeight < 1 {
		trustHeight = 1
	}
	hash, err := rpcBlockHash(s.cfg.StateSyncRPC, trustHeight)
	if err != nil || hash == "" {
		writeJSON(w, http.StatusOK, map[string]any{"enable": false, "reason": fmt.Sprintf("commit query failed: %v", err)})
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"enable":       true,
		"rpc_servers":  s.cfg.StateSyncServers,
		"trust_height": trustHeight,
		"trust_hash":   hash,
		"trust_period": s.cfg.TrustPeriod,
		"latest":       latest,
	})
}

var rpcClient = &http.Client{Timeout: 8 * time.Second}

// rpcGet fetches a CometBFT RPC endpoint and unwraps the optional jsonrpc
// "result" envelope (pax-tendermint's /status returns a bare object).
func rpcGet(base, path string) (map[string]json.RawMessage, error) {
	resp, err := rpcClient.Get(strings.TrimRight(base, "/") + path)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	var raw map[string]json.RawMessage
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return nil, err
	}
	if res, ok := raw["result"]; ok {
		var inner map[string]json.RawMessage
		if err := json.Unmarshal(res, &inner); err == nil {
			return inner, nil
		}
	}
	return raw, nil
}

func rpcLatestHeight(base string) (int, error) {
	m, err := rpcGet(base, "/status")
	if err != nil {
		return 0, err
	}
	var si struct {
		Latest string `json:"latest_block_height"`
	}
	if err := json.Unmarshal(m["sync_info"], &si); err != nil {
		return 0, err
	}
	return strconv.Atoi(si.Latest)
}

func rpcBlockHash(base string, height int) (string, error) {
	m, err := rpcGet(base, "/commit?height="+strconv.Itoa(height))
	if err != nil {
		return "", err
	}
	var sh struct {
		Commit struct {
			BlockID struct {
				Hash string `json:"hash"`
			} `json:"block_id"`
		} `json:"commit"`
	}
	if err := json.Unmarshal(m["signed_header"], &sh); err != nil {
		return "", err
	}
	return strings.ToUpper(sh.Commit.BlockID.Hash), nil
}

// ---- helpers ---------------------------------------------------------------

func isHexID(s string) bool {
	if len(s) != 40 {
		return false
	}
	for _, c := range s {
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')) {
			return false
		}
	}
	return true
}

// clientIP prefers the X-Forwarded-For left-most entry (we sit behind Caddy),
// then falls back to the TCP source address.
func clientIP(r *http.Request) string {
	if xff := r.Header.Get("X-Forwarded-For"); xff != "" {
		if ip := strings.TrimSpace(strings.Split(xff, ",")[0]); ip != "" {
			return ip
		}
	}
	if xr := strings.TrimSpace(r.Header.Get("X-Real-IP")); xr != "" {
		return xr
	}
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}

func writeJSON(w http.ResponseWriter, code int, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(v)
}

// noCacheScripts disables caching for the install scripts + chain-info so a
// re-publish is picked up immediately; large immutable blobs keep default caching.
func noCacheScripts(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case strings.HasSuffix(r.URL.Path, ".sh"),
			r.URL.Path == "/hpx",
			strings.HasSuffix(r.URL.Path, ".json"),
			strings.HasSuffix(r.URL.Path, ".toml"):
			w.Header().Set("Cache-Control", "no-store")
		}
		next.ServeHTTP(w, r)
	})
}

func logreq(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		next.ServeHTTP(w, r)
		log.Printf("%s %s %s", clientIP(r), r.Method, r.URL.Path)
	})
}
