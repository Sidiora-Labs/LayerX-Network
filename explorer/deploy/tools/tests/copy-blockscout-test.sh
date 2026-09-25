#!/usr/bin/env bash
#
# copy-blockscout-test.sh
#
# Exercises explorer/deploy/tools/copy-blockscout-11-to-10.sh against two
# disposable PostgreSQL 16 containers seeded with synthetic rows. Nothing here
# reads a deployment's environment or touches a database it did not create:
# both containers are started by this script, listen on an ephemeral loopback
# port, and are removed on every exit path.
#
# The source schema is shaped like the 11.x one the tool copies from - it
# carries columns the destination has no name for, and an internal transaction
# whose trace address is absent - and the destination schema is shaped like the
# 10.2.6 one it copies into, where that column is NOT NULL.
#
# Scenarios, in order, each against the tool's real exit code and the resulting
# row counts:
#
#   1  a dry run writes nothing
#   2  a full copy into an empty destination, with the trace address derived
#   3  a rerun copies nothing and leaves every row as it is
#   4  a copy interrupted part way through a table stops non-zero and keeps
#      what it had already committed
#   5  the interrupted copy resumes and completes without being told to
#   6  a source that grows after the ceiling is pinned is copied up to the
#      ceiling and verified at the ceiling
#   7  a destination holding rows this tool has no record of is refused, and
#      accepted with the documented flag
#   8  counts that differ at the ceiling exit non-zero and name the table
#
# Requires docker and psql on PATH.

set -euo pipefail

HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
readonly HERE
readonly TOOL="${HERE}/../copy-blockscout-11-to-10.sh"
readonly IMAGE='postgres:16'
readonly SRC_CONTAINER="copy-blockscout-test-src-$$"
readonly DST_CONTAINER="copy-blockscout-test-dst-$$"

WORK=''
SRC_URL=''
DST_URL=''
LAST_RC=0
LAST_LOG=''
LOCK_PID=''
FAILURES=0

log() {
  printf '%s\n' "$*" >&2
}

cleanup() {
  local status=$?
  if [ -n "$LOCK_PID" ]; then
    kill "$LOCK_PID" 2> /dev/null || true
    wait "$LOCK_PID" 2> /dev/null || true
  fi
  docker rm --force --volumes "$SRC_CONTAINER" "$DST_CONTAINER" > /dev/null 2>&1 || true
  [ -n "$WORK" ] && rm -rf "$WORK"
  exit "$status"
}

die() {
  log "error: $*"
  exit 1
}

fail() {
  FAILURES=$((FAILURES + 1))
  log "FAIL   $*"
  if [ -n "$LAST_LOG" ] && [ -f "$LAST_LOG" ]; then
    log "       last tool log: $LAST_LOG"
    sed 's/^/       | /' "$LAST_LOG" >&2
  fi
}

pass() {
  log "ok     $*"
}

src_sql() {
  psql --no-psqlrc --quiet --tuples-only --no-align --set ON_ERROR_STOP=1 \
    --dbname "$SRC_URL" --command "$1"
}

dst_sql() {
  psql --no-psqlrc --quiet --tuples-only --no-align --set ON_ERROR_STOP=1 \
    --dbname "$DST_URL" --command "$1"
}

assert_value() {
  local expected=$1 actual=$2 what=$3
  if [ "$expected" = "$actual" ]; then
    pass "${what}: ${actual}"
  else
    fail "${what}: expected ${expected}, got ${actual}"
  fi
}

assert_count() {
  local side=$1 query=$2 expected=$3 what=$4 actual
  actual=$("${side}_sql" "$query")
  assert_value "$expected" "$actual" "$what"
}

assert_rc() {
  local expected=$1 what=$2
  if [ "$expected" = "$LAST_RC" ]; then
    pass "${what}: exit ${LAST_RC}"
  else
    fail "${what}: expected exit ${expected}, got ${LAST_RC}"
  fi
}

assert_log() {
  local pattern=$1 what=$2
  if grep -Fq "$pattern" "$LAST_LOG"; then
    pass "${what}"
  else
    fail "${what}: the run log does not carry \"${pattern}\""
  fi
}

run_tool() {
  local label=$1
  shift
  LAST_LOG="${WORK}/${label}.log"
  LAST_RC=0
  env SRC_DATABASE_URL="$SRC_URL" DST_DATABASE_URL="$DST_URL" "$@" \
    "$TOOL" > "$LAST_LOG" 2>&1 || LAST_RC=$?
}

start_container() {
  local name=$1 port
  docker run --detach --name "$name" \
    --env POSTGRES_HOST_AUTH_METHOD=trust \
    --env POSTGRES_USER=postgres \
    --env POSTGRES_DB=blockscout \
    --publish 127.0.0.1::5432 \
    "$IMAGE" > /dev/null || die "cannot start ${name}"

  local attempt
  for attempt in $(seq 1 120); do
    if docker exec "$name" pg_isready --username postgres --dbname blockscout > /dev/null 2>&1; then
      break
    fi
    if [ "$attempt" -eq 120 ]; then
      docker logs "$name" >&2 || true
      die "${name} did not become ready"
    fi
    sleep 1
  done

  port=$(docker port "$name" 5432/tcp | head -n 1)
  port=${port##*:}
  [ -n "$port" ] || die "cannot read the published port of ${name}"
  printf 'postgresql://postgres@127.0.0.1:%s/blockscout' "$port"
}

wait_for_connection() {
  local url=$1 attempt
  for attempt in $(seq 1 60); do
    if psql --no-psqlrc --quiet --tuples-only --no-align --dbname "$url" \
      --command 'SELECT 1' > /dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  die "cannot reach a test database over its published port"
}

write_schemas() {
  cat > "${WORK}/source-schema.sql" << 'SQL'
CREATE TABLE addresses (
  hash bytea NOT NULL PRIMARY KEY,
  fetched_coin_balance numeric(100,0),
  fetched_coin_balance_block_number bigint,
  contract_code bytea,
  nonce integer,
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now()
);

CREATE TABLE blocks (
  hash bytea NOT NULL PRIMARY KEY,
  consensus boolean NOT NULL,
  difficulty numeric(50,0),
  gas_limit numeric(100,0) NOT NULL,
  gas_used numeric(100,0) NOT NULL,
  nonce bytea NOT NULL,
  number bigint NOT NULL,
  parent_hash bytea NOT NULL,
  size integer,
  "timestamp" timestamp without time zone NOT NULL,
  miner_hash bytea NOT NULL REFERENCES addresses (hash),
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX one_consensus_block_at_height ON blocks (number) WHERE consensus;

CREATE TABLE transactions (
  hash bytea NOT NULL PRIMARY KEY,
  block_hash bytea REFERENCES blocks (hash) ON DELETE CASCADE,
  block_number integer,
  "index" integer,
  cumulative_gas_used numeric(100,0),
  from_address_hash bytea NOT NULL REFERENCES addresses (hash),
  to_address_hash bytea REFERENCES addresses (hash),
  gas numeric(100,0) NOT NULL,
  gas_price numeric(100,0),
  gas_used numeric(100,0),
  input bytea NOT NULL,
  nonce integer NOT NULL,
  r numeric(100,0) NOT NULL,
  s numeric(100,0) NOT NULL,
  v numeric(100,0) NOT NULL,
  status integer,
  value numeric(100,0) NOT NULL,
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now(),
  block_timestamp timestamp without time zone,
  transaction_type integer
);

CREATE TABLE logs (
  transaction_hash bytea NOT NULL REFERENCES transactions (hash) ON DELETE CASCADE,
  block_hash bytea NOT NULL REFERENCES blocks (hash) ON DELETE CASCADE,
  "index" integer NOT NULL,
  block_number integer,
  address_hash bytea REFERENCES addresses (hash),
  data bytea NOT NULL,
  first_topic bytea,
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now(),
  PRIMARY KEY (transaction_hash, block_hash, "index")
);

CREATE TABLE internal_transactions (
  transaction_hash bytea NOT NULL REFERENCES transactions (hash) ON DELETE CASCADE,
  block_hash bytea NOT NULL REFERENCES blocks (hash) ON DELETE CASCADE,
  block_index integer NOT NULL,
  block_number integer,
  "index" integer NOT NULL,
  call_type text,
  from_address_hash bytea REFERENCES addresses (hash),
  to_address_hash bytea REFERENCES addresses (hash),
  gas numeric(100,0),
  gas_used numeric(100,0),
  input bytea,
  output bytea,
  trace_address integer[],
  transaction_index integer,
  type text NOT NULL,
  value numeric(100,0),
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now(),
  call_type_enum text,
  PRIMARY KEY (block_hash, block_index)
);
SQL

  # The destination carries none of the columns 11.x added, one column of its
  # own that the source cannot fill, and the NOT NULL trace address that the
  # 10.2.6 schema creates.
  cat > "${WORK}/destination-schema.sql" << 'SQL'
CREATE TABLE addresses (
  hash bytea NOT NULL PRIMARY KEY,
  fetched_coin_balance numeric(100,0),
  fetched_coin_balance_block_number bigint,
  contract_code bytea,
  nonce integer,
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now()
);

CREATE TABLE blocks (
  hash bytea NOT NULL PRIMARY KEY,
  consensus boolean NOT NULL,
  difficulty numeric(50,0),
  gas_limit numeric(100,0) NOT NULL,
  gas_used numeric(100,0) NOT NULL,
  nonce bytea NOT NULL,
  number bigint NOT NULL,
  parent_hash bytea NOT NULL,
  size integer,
  "timestamp" timestamp without time zone NOT NULL,
  miner_hash bytea NOT NULL REFERENCES addresses (hash),
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX one_consensus_block_at_height ON blocks (number) WHERE consensus;

CREATE TABLE transactions (
  hash bytea NOT NULL PRIMARY KEY,
  block_hash bytea REFERENCES blocks (hash) ON DELETE CASCADE,
  block_number integer,
  "index" integer,
  cumulative_gas_used numeric(100,0),
  from_address_hash bytea NOT NULL REFERENCES addresses (hash),
  to_address_hash bytea REFERENCES addresses (hash),
  gas numeric(100,0) NOT NULL,
  gas_price numeric(100,0),
  gas_used numeric(100,0),
  has_error_in_internal_transactions boolean,
  input bytea NOT NULL,
  nonce integer NOT NULL,
  r numeric(100,0) NOT NULL,
  s numeric(100,0) NOT NULL,
  v numeric(100,0) NOT NULL,
  status integer,
  value numeric(100,0) NOT NULL,
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now()
);

CREATE TABLE logs (
  transaction_hash bytea NOT NULL REFERENCES transactions (hash) ON DELETE CASCADE,
  block_hash bytea NOT NULL REFERENCES blocks (hash) ON DELETE CASCADE,
  "index" integer NOT NULL,
  block_number integer,
  address_hash bytea REFERENCES addresses (hash),
  data bytea NOT NULL,
  first_topic bytea,
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now(),
  PRIMARY KEY (transaction_hash, block_hash, "index")
);

CREATE TABLE internal_transactions (
  transaction_hash bytea NOT NULL REFERENCES transactions (hash) ON DELETE CASCADE,
  block_hash bytea NOT NULL REFERENCES blocks (hash) ON DELETE CASCADE,
  block_index integer NOT NULL,
  block_number integer,
  "index" integer NOT NULL,
  call_type text,
  from_address_hash bytea REFERENCES addresses (hash),
  to_address_hash bytea REFERENCES addresses (hash),
  gas numeric(100,0),
  gas_used numeric(100,0),
  input bytea,
  output bytea,
  trace_address integer[] NOT NULL,
  transaction_index integer,
  type text NOT NULL,
  value numeric(100,0),
  inserted_at timestamp without time zone NOT NULL DEFAULT now(),
  updated_at timestamp without time zone NOT NULL DEFAULT now(),
  PRIMARY KEY (block_hash, block_index)
);
SQL
}

write_fixtures() {
  cat > "${WORK}/source-seed.sql" << 'SQL'
INSERT INTO addresses (hash)
SELECT decode(lpad(to_hex(g), 40, '0'), 'hex') FROM generate_series(1, 5) AS g;

INSERT INTO blocks (hash, consensus, difficulty, gas_limit, gas_used, nonce, number,
                    parent_hash, size, "timestamp", miner_hash)
SELECT decode(lpad(to_hex(1000 + g), 64, '0'), 'hex'), true, 1, 30000000, 21000,
       '\x0000000000000000'::bytea, g,
       decode(lpad(to_hex(999 + g), 64, '0'), 'hex'), 512,
       now() + (g || ' seconds')::interval,
       decode(lpad(to_hex(1), 40, '0'), 'hex')
FROM generate_series(0, 6) AS g;

INSERT INTO transactions (hash, block_hash, block_number, "index", cumulative_gas_used,
                          from_address_hash, to_address_hash, gas, gas_price, gas_used,
                          input, nonce, r, s, v, status, value, block_timestamp,
                          transaction_type)
SELECT decode(lpad(to_hex(2000 + g), 64, '0'), 'hex'),
       decode(lpad(to_hex(1000 + g), 64, '0'), 'hex'), g, 0, 21000,
       decode(lpad(to_hex(1), 40, '0'), 'hex'),
       decode(lpad(to_hex(2), 40, '0'), 'hex'),
       21000, 1000000000, 21000, '\x'::bytea, g, 1, 2, 27, 1, 1000,
       now() + (g || ' seconds')::interval, 2
FROM generate_series(0, 6) AS g;

INSERT INTO logs (transaction_hash, block_hash, "index", block_number, address_hash,
                  data, first_topic)
SELECT decode(lpad(to_hex(2000 + g), 64, '0'), 'hex'),
       decode(lpad(to_hex(1000 + g), 64, '0'), 'hex'), i, g,
       decode(lpad(to_hex(3), 40, '0'), 'hex'), '\x01'::bytea, NULL
FROM generate_series(0, 6) AS g, generate_series(0, 1) AS i;

INSERT INTO internal_transactions (transaction_hash, block_hash, block_index, block_number,
                                   "index", call_type, from_address_hash, to_address_hash,
                                   gas, gas_used, input, output, trace_address,
                                   transaction_index, type, value, call_type_enum)
VALUES
  (decode(lpad(to_hex(2003), 64, '0'), 'hex'), decode(lpad(to_hex(1003), 64, '0'), 'hex'),
   0, 3, 0, 'call', decode(lpad(to_hex(1), 40, '0'), 'hex'),
   decode(lpad(to_hex(2), 40, '0'), 'hex'), 21000, 20000, '\x'::bytea, '\x'::bytea,
   NULL, 0, 'call', 1, 'call'),
  (decode(lpad(to_hex(2003), 64, '0'), 'hex'), decode(lpad(to_hex(1003), 64, '0'), 'hex'),
   1, 3, 1, 'call', decode(lpad(to_hex(1), 40, '0'), 'hex'),
   decode(lpad(to_hex(4), 40, '0'), 'hex'), 21000, 20000, '\x'::bytea, '\x'::bytea,
   NULL, 0, 'call', 2, 'call'),
  (decode(lpad(to_hex(2005), 64, '0'), 'hex'), decode(lpad(to_hex(1005), 64, '0'), 'hex'),
   0, 5, 0, 'call', decode(lpad(to_hex(1), 40, '0'), 'hex'),
   decode(lpad(to_hex(2), 40, '0'), 'hex'), 21000, 20000, '\x'::bytea, '\x'::bytea,
   ARRAY[0, 1], 0, 'call', 3, 'call');
SQL

  cat > "${WORK}/source-growth.sql" << 'SQL'
INSERT INTO blocks (hash, consensus, difficulty, gas_limit, gas_used, nonce, number,
                    parent_hash, size, "timestamp", miner_hash)
SELECT decode(lpad(to_hex(1000 + g), 64, '0'), 'hex'), true, 1, 30000000, 21000,
       '\x0000000000000000'::bytea, g,
       decode(lpad(to_hex(999 + g), 64, '0'), 'hex'), 512,
       now() + (g || ' seconds')::interval,
       decode(lpad(to_hex(1), 40, '0'), 'hex')
FROM generate_series(7, 9) AS g;

INSERT INTO transactions (hash, block_hash, block_number, "index", cumulative_gas_used,
                          from_address_hash, to_address_hash, gas, gas_price, gas_used,
                          input, nonce, r, s, v, status, value, block_timestamp,
                          transaction_type)
SELECT decode(lpad(to_hex(2000 + g), 64, '0'), 'hex'),
       decode(lpad(to_hex(1000 + g), 64, '0'), 'hex'), g, 0, 21000,
       decode(lpad(to_hex(1), 40, '0'), 'hex'),
       decode(lpad(to_hex(2), 40, '0'), 'hex'),
       21000, 1000000000, 21000, '\x'::bytea, g, 1, 2, 27, 1, 1000,
       now() + (g || ' seconds')::interval, 2
FROM generate_series(7, 9) AS g;

INSERT INTO logs (transaction_hash, block_hash, "index", block_number, address_hash,
                  data, first_topic)
SELECT decode(lpad(to_hex(2000 + g), 64, '0'), 'hex'),
       decode(lpad(to_hex(1000 + g), 64, '0'), 'hex'), i, g,
       decode(lpad(to_hex(3), 40, '0'), 'hex'), '\x01'::bytea, NULL
FROM generate_series(7, 9) AS g, generate_series(0, 1) AS i;
SQL
}

reset_destination() {
  dst_sql "TRUNCATE TABLE internal_transactions, logs, transactions, blocks, addresses CASCADE;
           DROP TABLE IF EXISTS paxeer_x_copy_progress;
           DROP TABLE IF EXISTS paxeer_x_copy_stage;" > /dev/null
}

# A row-level trigger that refuses the blocks above a bound, so a copy stops in
# the middle of a table the way a lost connection or a killed process would,
# after earlier batches have already committed.
install_interruption() {
  dst_sql "
    CREATE OR REPLACE FUNCTION refuse_above_block() RETURNS trigger
    LANGUAGE plpgsql AS \$\$
    BEGIN
      IF NEW.block_number > 3 THEN
        RAISE EXCEPTION 'interrupted at block %', NEW.block_number;
      END IF;
      RETURN NEW;
    END
    \$\$;
    CREATE TRIGGER logs_interruption BEFORE INSERT ON logs
      FOR EACH ROW EXECUTE FUNCTION refuse_above_block();" > /dev/null
}

remove_interruption() {
  dst_sql "DROP TRIGGER logs_interruption ON logs;
           DROP FUNCTION refuse_above_block();" > /dev/null
}

# Holds an EXCLUSIVE lock on the destination's blocks table, which lets every
# read the tool makes through and stops its first insert. Nothing here sleeps
# for an outcome: the lock is taken, waited for, and released by terminating
# the backend that holds it.
hold_blocks_lock() {
  local attempt
  psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 --dbname "$DST_URL" \
    --command "LOCK TABLE public.blocks IN EXCLUSIVE MODE; SELECT pg_sleep(600);" \
    > /dev/null 2>&1 &
  LOCK_PID=$!

  for attempt in $(seq 1 60); do
    if [ "$(dst_sql "SELECT count(*) FROM pg_locks
              WHERE relation = 'public.blocks'::regclass
                AND mode = 'ExclusiveLock' AND granted")" -ge 1 ]; then
      return 0
    fi
    sleep 1
  done
  die "the destination lock was never taken"
}

wait_until_blocked() {
  local attempt
  for attempt in $(seq 1 120); do
    if [ "$(dst_sql "SELECT count(*) FROM pg_locks WHERE NOT granted")" -ge 1 ]; then
      return 0
    fi
    sleep 1
  done
  return 1
}

release_blocks_lock() {
  dst_sql "SELECT pg_terminate_backend(pid) FROM pg_locks
            WHERE relation = 'public.blocks'::regclass
              AND mode = 'ExclusiveLock' AND granted" > /dev/null
  if [ -n "$LOCK_PID" ]; then
    kill "$LOCK_PID" 2> /dev/null || true
    wait "$LOCK_PID" 2> /dev/null || true
    LOCK_PID=''
  fi
}

scenario_dry_run() {
  log "-- 1 a dry run writes nothing"
  run_tool dry-run DRY_RUN=1
  assert_rc 0 'dry run'
  assert_log 'ceiling: block 6' 'the dry run pins and prints the ceiling'
  assert_log 'dry run: nothing was copied and nothing was verified' 'the dry run says it wrote nothing'
  assert_count dst 'SELECT count(*) FROM blocks' 0 'destination blocks after the dry run'
  assert_count dst "SELECT count(*) FROM information_schema.tables
                     WHERE table_schema = 'public' AND table_name = 'paxeer_x_copy_progress'" 0 \
    'progress table after the dry run'
}

scenario_full_copy() {
  log "-- 2 a full copy into an empty destination"
  run_tool full-copy
  assert_rc 0 'full copy'
  assert_log 'ceiling: block 6' 'the full copy pins the ceiling'
  assert_count dst 'SELECT count(*) FROM addresses' 5 'destination addresses'
  assert_count dst 'SELECT count(*) FROM blocks' 7 'destination blocks'
  assert_count dst 'SELECT count(*) FROM transactions' 7 'destination transactions'
  assert_count dst 'SELECT count(*) FROM logs' 14 'destination logs'
  assert_count dst 'SELECT count(*) FROM internal_transactions' 3 'destination internal transactions'

  assert_value '{}' \
    "$(dst_sql "SELECT trace_address::text FROM internal_transactions
                 WHERE block_number = 3 AND \"index\" = 0")" \
    'the root call derives an empty trace address'
  assert_value '{1}' \
    "$(dst_sql "SELECT trace_address::text FROM internal_transactions
                 WHERE block_number = 3 AND \"index\" = 1")" \
    'a nested call derives its position'
  assert_value '{0,1}' \
    "$(dst_sql "SELECT trace_address::text FROM internal_transactions
                 WHERE block_number = 5 AND \"index\" = 0")" \
    'a source trace address is kept as it is'
  assert_count dst 'SELECT count(*) FROM internal_transactions WHERE trace_address IS NULL' 0 \
    'internal transactions without a trace address'

  assert_log 'transactions: source-only columns not copied: block_timestamp transaction_type' \
    'the summary names the transaction columns that were not copied'
  assert_log 'internal_transactions: source-only columns not copied: call_type_enum' \
    'the summary names the internal-transaction columns that were not copied'
  assert_log 'every processed table matches at the ceiling' 'the summary verifies at the ceiling'
}

scenario_rerun() {
  log "-- 3 a rerun copies nothing"
  run_tool rerun
  assert_rc 0 'rerun'
  assert_log 'copied blocks: 0 rows' 'the rerun copies no block'
  assert_log 'copied logs: 0 rows' 'the rerun copies no log'
  assert_log 'copied internal_transactions: 0 rows' 'the rerun copies no internal transaction'
  assert_count dst 'SELECT count(*) FROM logs' 14 'destination logs after the rerun'
  assert_count dst 'SELECT count(*) FROM internal_transactions' 3 \
    'destination internal transactions after the rerun'
}

scenario_interrupted() {
  log "-- 4 a copy interrupted part way through a table"
  reset_destination
  install_interruption
  run_tool interrupted BLOCK_BATCH=2
  assert_rc 2 'interrupted copy'
  assert_log 'stopped at logs' 'the interrupted copy names the table it stopped on'
  assert_count dst 'SELECT count(*) FROM blocks' 7 'destination blocks after the interruption'
  assert_count dst 'SELECT count(*) FROM transactions' 7 'destination transactions after the interruption'
  assert_count dst 'SELECT count(*) FROM logs' 8 'destination logs after the interruption'
  assert_count dst 'SELECT count(*) FROM internal_transactions' 0 \
    'destination internal transactions after the interruption'
  assert_count dst "SELECT highest_key FROM paxeer_x_copy_progress WHERE table_name = 'logs'" 3 \
    'recorded progress for logs'
}

scenario_resume() {
  log "-- 5 the interrupted copy resumes"
  remove_interruption
  run_tool resume BLOCK_BATCH=2
  assert_rc 0 'resumed copy'
  assert_log 'resuming above block_number 3' 'the resumed copy continues where it stopped'
  assert_log 'copied logs: 6 rows' 'the resumed copy writes only the logs that were missing'
  assert_count dst 'SELECT count(*) FROM logs' 14 'destination logs after the resume'
  assert_count dst 'SELECT count(*) FROM internal_transactions' 3 \
    'destination internal transactions after the resume'
  assert_log 'every processed table matches at the ceiling' 'the resumed copy verifies at the ceiling'
}

scenario_growing_source() {
  log "-- 6 a source that grows after the ceiling is pinned"
  reset_destination
  hold_blocks_lock

  local rc_file="${WORK}/growth.rc"
  LAST_LOG="${WORK}/growth.log"
  (
    rc=0
    env SRC_DATABASE_URL="$SRC_URL" DST_DATABASE_URL="$DST_URL" BLOCK_BATCH=100 \
      "$TOOL" > "$LAST_LOG" 2>&1 || rc=$?
    printf '%s' "$rc" > "$rc_file"
  ) &
  local tool_pid=$!

  if wait_until_blocked; then
    pass 'the copy is inside the destination when the source grows'
  else
    fail 'the copy never reached the destination, so the source did not grow during it'
  fi

  psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 --dbname "$SRC_URL" \
    --file "${WORK}/source-growth.sql" > /dev/null

  release_blocks_lock
  wait "$tool_pid" || true
  LAST_RC=$(cat "$rc_file")

  assert_rc 0 'copy against a growing source'
  assert_log 'ceiling: block 6' 'the ceiling stays where it was pinned'
  assert_count src 'SELECT count(*) FROM blocks' 10 'source blocks after the growth'
  assert_count src 'SELECT count(*) FROM logs' 20 'source logs after the growth'
  assert_count dst 'SELECT count(*) FROM blocks' 7 'destination blocks, bounded by the ceiling'
  assert_count dst 'SELECT count(*) FROM logs' 14 'destination logs, bounded by the ceiling'
  assert_log 'every processed table matches at the ceiling' 'growth is not read as a mismatch'
}

scenario_nonempty_destination() {
  log "-- 7 a destination holding rows this tool has no record of"
  dst_sql 'DROP TABLE IF EXISTS paxeer_x_copy_progress' > /dev/null
  run_tool refused
  assert_rc 2 'refused copy'
  assert_log 'set ALLOW_NONEMPTY_DESTINATION=1 to append' 'the refusal names the flag that overrides it'
  assert_count dst 'SELECT count(*) FROM blocks' 7 'destination blocks after the refusal'

  run_tool appended ALLOW_NONEMPTY_DESTINATION=1
  assert_rc 0 'append with the flag'
  assert_log 'ceiling: block 9' 'the appending run pins the ceiling the source has now'
  assert_count dst 'SELECT count(*) FROM blocks' 10 'destination blocks after the append'
  assert_count dst 'SELECT count(*) FROM logs' 20 'destination logs after the append'
  assert_count dst 'SELECT count(*) FROM internal_transactions' 3 \
    'destination internal transactions after the append'
}

scenario_mismatch() {
  log "-- 8 counts that differ at the ceiling"
  dst_sql 'DELETE FROM logs WHERE block_number = 2' > /dev/null
  run_tool mismatch
  assert_rc 3 'copy with a mismatching table'
  assert_log 'mismatch logs: source 20, target 18' 'the summary names the table and both counts'
  assert_log 'done with mismatches' 'the run says it ended with mismatches'
}

main() {
  command -v docker > /dev/null || die 'docker is not on PATH'
  command -v psql > /dev/null || die 'psql is not on PATH'
  [ -x "$TOOL" ] || die "the copy tool is not executable at ${TOOL}"

  trap cleanup EXIT INT TERM
  WORK=$(mktemp -d)

  log "starting two disposable ${IMAGE} containers"
  SRC_URL=$(start_container "$SRC_CONTAINER")
  DST_URL=$(start_container "$DST_CONTAINER")
  wait_for_connection "$SRC_URL"
  wait_for_connection "$DST_URL"

  write_schemas
  write_fixtures

  psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 --dbname "$SRC_URL" \
    --file "${WORK}/source-schema.sql" > /dev/null
  psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 --dbname "$DST_URL" \
    --file "${WORK}/destination-schema.sql" > /dev/null
  psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 --dbname "$SRC_URL" \
    --file "${WORK}/source-seed.sql" > /dev/null

  scenario_dry_run
  scenario_full_copy
  scenario_rerun
  scenario_interrupted
  scenario_resume
  scenario_growing_source
  scenario_nonempty_destination
  scenario_mismatch

  if [ "$FAILURES" -ne 0 ]; then
    log "${FAILURES} assertion(s) failed"
    exit 1
  fi
  log 'every scenario passed'
}

main "$@"
