package types

import (
	tmbytes "github.com/sidiora-labs/paxeer-network/consensus/libs/bytes"

	query "github.com/sidiora-labs/paxeer-network/sdk/types/query"
)

// Querier routes for the evidence module
const (
	QueryEvidence    = "evidence"
	QueryAllEvidence = "all_evidence"
)

// NewQueryEvidenceRequest creates a new instance of QueryEvidenceRequest.
func NewQueryEvidenceRequest(hash tmbytes.HexBytes) *QueryEvidenceRequest {
	return &QueryEvidenceRequest{EvidenceHash: hash}
}

// NewQueryAllEvidenceRequest creates a new instance of QueryAllEvidenceRequest.
func NewQueryAllEvidenceRequest(pageReq *query.PageRequest) *QueryAllEvidenceRequest {
	return &QueryAllEvidenceRequest{Pagination: pageReq}
}

// QueryAllEvidenceParams defines the parameters necessary for querying for all Evidence.
type QueryAllEvidenceParams struct {
	Page  int `json:"page" yaml:"page"`
	Limit int `json:"limit" yaml:"limit"`
}

func NewQueryAllEvidenceParams(page, limit int) QueryAllEvidenceParams {
	return QueryAllEvidenceParams{Page: page, Limit: limit}
}
