package config_test

import (
	"github.com/sidiora-labs/paxeer-network/rpc/config"
	"github.com/spf13/viper"
	"testing"
)

func TestHTTPBindAddress(t *testing.T) {
	options := viper.New()
	defaults, err := config.ReadConfig(options)
	if err != nil || defaults.HTTPAddress != "0.0.0.0" {
		t.Fatalf("default bind: %q %v", defaults.HTTPAddress, err)
	}
	options.Set("evm.http_address", "127.0.0.1")
	local, err := config.ReadConfig(options)
	if err != nil || local.HTTPAddress != "127.0.0.1" {
		t.Fatalf("loopback bind: %q %v", local.HTTPAddress, err)
	}
	options.Set("evm.http_address", "localhost")
	if _, err := config.ReadConfig(options); err == nil {
		t.Fatal("accepted nonliteral bind")
	}
}
