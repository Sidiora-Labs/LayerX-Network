#!/usr/bin/env sh

NODE_ID=${ID:-0}
INVARIANT_CHECK_INTERVAL=${INVARIANT_CHECK_INTERVAL:-0}

LOG_DIR="build/generated/logs"
mkdir -p $LOG_DIR

echo "Starting the paxd process for node $NODE_ID with invariant check interval=$INVARIANT_CHECK_INTERVAL..."

paxd start --chain-id pax --inv-check-period ${INVARIANT_CHECK_INTERVAL} > "$LOG_DIR/paxd-$NODE_ID.log" 2>&1 &
echo "Node $NODE_ID paxd is started now"
echo "Done" >> build/generated/launch.complete
