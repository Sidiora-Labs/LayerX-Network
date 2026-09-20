package export

import (
	"github.com/sidiora-labs/paxeer-network/consensus/internal/pubsub/query"
	"github.com/sidiora-labs/paxeer-network/consensus/internal/state"
	"github.com/sidiora-labs/paxeer-network/consensus/internal/store"
)

type Query = query.Query

var NewBlockStore = store.NewBlockStore
var NewStore = state.NewStore
var NewQuery = query.New
var QueryAll = query.All
