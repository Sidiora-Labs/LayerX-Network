#!/bin/bash

jq '.validators = []' ~/.pax/config/genesis.json > ~/.pax/config/tmp_genesis.json
cd build/generated/gentx
IDX=0
for FILE in *
do
    jq '.validators['$IDX'] |= .+ {}' ~/.pax/config/tmp_genesis.json > ~/.pax/config/tmp_genesis_step_1.json && rm ~/.pax/config/tmp_genesis.json
    KEY=$(jq '.body.messages[0].pubkey.key' $FILE -c)
    DELEGATION=$(jq -r '.body.messages[0].value.amount' $FILE)
    POWER=$(($DELEGATION / 1000000))
    jq '.validators['$IDX'] += {"power":"'$POWER'"}' ~/.pax/config/tmp_genesis_step_1.json > ~/.pax/config/tmp_genesis_step_2.json && rm ~/.pax/config/tmp_genesis_step_1.json
    jq '.validators['$IDX'] += {"pub_key":{"type":"tendermint/PubKeyEd25519","value":'$KEY'}}' ~/.pax/config/tmp_genesis_step_2.json > ~/.pax/config/tmp_genesis_step_3.json && rm ~/.pax/config/tmp_genesis_step_2.json
    mv ~/.pax/config/tmp_genesis_step_3.json ~/.pax/config/tmp_genesis.json
    IDX=$(($IDX+1))
done

mv ~/.pax/config/tmp_genesis.json ~/.pax/config/genesis.json
