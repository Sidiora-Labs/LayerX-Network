#!/bin/bash
paxdbin=$(which ~/go/bin/paxd | tr -d '"')
keyname=$(printf "12345678\n" | $paxdbin keys list --output json | jq ".[0].name" | tr -d '"')
chainid=$($paxdbin status | jq ".NodeInfo.network" | tr -d '"')
paxhome=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)

echo $keyname
echo $paxdbin
echo $chainid
echo $paxhome

# Deploy all contracts
echo "Deploying pax tester contract"

cd $paxhome/loadtest/contracts
# store
echo "Storing..."

pax_tester_res=$(printf "12345678\n" | $paxdbin tx wasm store pax_tester.wasm -y --from=$keyname --chain-id=$chainid --gas=5000000 --fees=1000000uhpx --broadcast-mode=block --output=json)
pax_tester_id=$(python3 parser.py code_id $pax_tester_res)

# instantiate
echo "Instantiating..."
tester_in_res=$(printf "12345678\n" | $paxdbin tx wasm instantiate $pax_tester_id '{}' -y --no-admin --from=$keyname --chain-id=$chainid --gas=5000000 --fees=1000000uhpx --broadcast-mode=block  --label=dex --output=json)
tester_addr=$(python3 parser.py contract_address $tester_in_res)

# TODO fix once implemented in loadtest config
jq '.pax_tester_address = "'$tester_addr'"' $paxhome/loadtest/config.json > $paxhome/loadtest/config_temp.json && mv $paxhome/loadtest/config_temp.json $paxhome/loadtest/config.json


echo "Deployed contracts:"
echo $tester_addr

exit 0
