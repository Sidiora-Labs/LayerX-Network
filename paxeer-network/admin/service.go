package admin

import (
	"context"
	"log/slog"
	"sort"
	"strings"

	"github.com/paxeer-network/paxlog"
	"github.com/sidiora-labs/paxeer-network/admin/types"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

type service struct {
	types.UnimplementedAdminServiceServer
}

func (s *service) SetLogLevel(_ context.Context, req *types.SetLogLevelRequest) (*types.SetLogLevelResponse, error) {
	if req.Pattern == "" {
		return nil, status.Error(codes.InvalidArgument, "pattern is required")
	}

	var lvl slog.Level
	if err := lvl.UnmarshalText([]byte(req.Level)); err != nil {
		return nil, status.Errorf(codes.InvalidArgument, "invalid log level %q: %v", req.Level, err)
	}

	var affected int
	if req.Pattern == "*" {
		paxlog.SetDefaultLevel(lvl, true)
		affected = len(paxlog.ListLoggers())
	} else {
		affected = paxlog.SetLevel(req.Pattern, lvl)
		if affected == 0 {
			return nil, status.Errorf(codes.NotFound, "no loggers matched pattern %q", req.Pattern)
		}
	}

	return &types.SetLogLevelResponse{
		Pattern:  req.Pattern,
		Level:    req.Level,
		Affected: int32(affected), //nolint:gosec
	}, nil
}

func (s *service) GetLogLevel(_ context.Context, req *types.GetLogLevelRequest) (*types.GetLogLevelResponse, error) {
	if req.Logger == "" {
		return nil, status.Error(codes.InvalidArgument, "logger name is required")
	}

	lvl, ok := paxlog.GetLevel(req.Logger)
	if !ok {
		return nil, status.Errorf(codes.NotFound, "logger %q not found", req.Logger)
	}

	return &types.GetLogLevelResponse{
		Logger: req.Logger,
		Level:  strings.ToLower(lvl.String()),
	}, nil
}

func (s *service) ListLoggers(_ context.Context, req *types.ListLoggersRequest) (*types.ListLoggersResponse, error) {
	names := paxlog.ListLoggers()
	sort.Strings(names)

	loggers := make([]types.LoggerInfo, 0, len(names))
	for _, name := range names {
		if req.Prefix != "" && !strings.HasPrefix(name, req.Prefix) {
			continue
		}
		lvl, _ := paxlog.GetLevel(name)
		loggers = append(loggers, types.LoggerInfo{
			Name:  name,
			Level: strings.ToLower(lvl.String()),
		})
	}

	return &types.ListLoggersResponse{Loggers: loggers}, nil
}
