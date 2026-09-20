#!/bin/bash

paxdbin=$(which ~/go/bin/paxd | tr -d '"')
keyname=$(printf "12345678\n" | $paxdbin keys list --output json | jq ".[0].name" | tr -d '"')
keyaddress=$(printf "12345678\n" | $paxdbin keys list --output json | jq ".[0].address" | tr -d '"')
chainid=$($paxdbin status | jq ".NodeInfo.network" | tr -d '"')
paxhome=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)

source "$(dirname "$0")/../utils/_tx_helpers.sh"

cd $paxhome || exit
echo "Deploying first set of contracts..."

beginning_block_height=$($paxdbin status | jq -r '.SyncInfo.latest_block_height')
echo "$beginning_block_height" > $paxhome/integration_test/contracts/wasm_beginning_block_height.txt
echo "$keyaddress"  > $paxhome/integration_test/contracts/wasm_creator_id.txt

# store first set of contracts
for i in {1..10}
do
    echo "Storing first set contract #$i..."
    contract_id=$(store_wasm integration_test/contracts/mars.wasm) || exit 1
    instantiate_wasm "$contract_id" '{}' dex --no-admin >/dev/null || exit 1
    echo "Got contract id $contract_id for iteration $i"
done

first_set_block_height=$($paxdbin status | jq -r '.SyncInfo.latest_block_height')
echo "$first_set_block_height" > $paxhome/integration_test/contracts/wasm_first_set_block_height.txt

sleep 5

# store second set of contracts
for i in {11..20}
do
    echo "Storing second set contract #$i..."
    contract_id=$(store_wasm integration_test/contracts/saturn.wasm) || exit 1
    instantiate_wasm "$contract_id" '{}' dex --no-admin >/dev/null || exit 1
    echo "Got contract id $contract_id for iteration $i"
done

second_set_block_height=$($paxdbin status | jq -r '.SyncInfo.latest_block_height')
echo "$second_set_block_height" > $paxhome/integration_test/contracts/wasm_second_set_block_height.txt

sleep 5

# store third set of contracts
for i in {21..30}
do
    echo "Storing third set contract #$i..."
    contract_id=$(store_wasm integration_test/contracts/venus.wasm) || exit 1
    instantiate_wasm "$contract_id" '{}' dex --no-admin >/dev/null || exit 1
    echo "Got contract id $contract_id for iteration $i"
done

third_set_block_height=$($paxdbin status | jq -r '.SyncInfo.latest_block_height')
echo "$third_set_block_height" > $paxhome/integration_test/contracts/wasm_third_set_block_height.txt

num_stored=$(paxd q wasm list-code --count-total --limit 100 --output json | jq -r ".code_infos | length")
echo $num_stored

exit 0
