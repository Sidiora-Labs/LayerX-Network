#!/usr/bin/env bash
#
# copy-blockscout-11-to-10.sh
#
# Copy the core chain tables of a Blockscout 11.x database into a freshly
# migrated Blockscout 10.2.6 database.
#
# The two schemas are close but not identical: 11.x adds columns, renames a
# few, and drops others. Rather than hard-coding a mapping that goes stale on
# the next upstream release, this script asks both databases what columns each
# table actually has, intersects the two lists by column name, and copies only
# that intersection. A column that exists on one side and not on the other is
# reported, never guessed at.
#
# Usage
#   SRC_DATABASE_URL=... DST_DATABASE_URL=... ./copy-blockscout-11-to-10.sh
#
# Both connection strings are read from the environment and from nowhere else.
# This script never prints them and never writes them to a file. psql needs the
# string as an argument, so on a shared machine keep the password out of the
# URL and let libpq pick it up from a PGPASSFILE; psql's own connection
# diagnostics may still name a server it could not reach.
#
# Environment
#   SRC_DATABASE_URL              required, the 11.x source
#   DST_DATABASE_URL              required, the freshly migrated 10.2.6 target
#   TABLES                        optional, whitespace-separated table list
#                                 that replaces the default order below
#   DRY_RUN                       optional, set to 1 to print the plan and the
#                                 column differences without copying anything
#   ALLOW_NONEMPTY_DESTINATION    optional, set to 1 to append to destination
#                                 tables that already hold rows; without it a
#                                 non-empty destination table is an error
#
# Exit codes
#   0  every table in the list was copied, skipped, or planned in a dry run
#   1  usage or precondition failure, nothing was copied
#   2  a copy failed; the log names the table it stopped on
#
# What is copied
#
# The default list is the chain data an explorer cannot recompute: addresses,
# blocks, transactions and everything hanging off them. It is ordered so that a
# table is always copied after the tables its foreign keys point at.
#
# What is not copied, on purpose:
#
#   * indexer bookkeeping (pending_block_operations, missing_block_ranges,
#     last_fetched_counters, migrations_status, the *_delete_queue and
#     multichain_search_db_export_* tables) — the 10.2.6 indexer rebuilds
#     these, and carrying 11.x rows over would make it skip work it still has
#     to do;
#   * account data (users, administrators, user_contacts, address_tags) —
#     application state rather than chain state, keyed differently;
#   * anything the destination schema has no table for.
#
# After each table the script resets every sequence that backs a copied column,
# so the first insert into a table with a serial key does not collide with a
# row that came from the source, and runs ANALYZE so the planner sees the new
# row counts.

set -euo pipefail

readonly DEFAULT_TABLES='
  addresses
  blocks
  tokens
  transactions
  logs
  internal_transactions
  token_transfers
  token_instances
  signed_authorizations
  address_coin_balances
  address_coin_balances_daily
  address_token_balances
  address_current_token_balances
  address_names
  block_rewards
  withdrawals
  smart_contracts
  smart_contracts_additional_sources
  proxy_implementations
  contract_methods
  transaction_stats
'

log() {
  printf '%s\n' "$*" >&2
}

die() {
  log "error: $*"
  exit 1
}

require_env() {
  local name=$1
  if [ -z "${!name:-}" ]; then
    die "$name is not set; both SRC_DATABASE_URL and DST_DATABASE_URL come from the environment"
  fi
}

# --no-psqlrc keeps a developer's ~/.psqlrc out of the output; ON_ERROR_STOP
# turns a failed statement into a non-zero exit instead of a skipped line.
src_query() {
  psql --no-psqlrc --tuples-only --no-align --quiet \
    --set ON_ERROR_STOP=1 --dbname "$SRC_DATABASE_URL" --command "$1"
}

dst_query() {
  psql --no-psqlrc --tuples-only --no-align --quiet \
    --set ON_ERROR_STOP=1 --dbname "$DST_DATABASE_URL" --command "$1"
}

sql_literal() {
  local quote="'"
  printf '%s%s%s' "$quote" "${1//$quote/$quote$quote}" "$quote"
}

quote_ident() {
  local quote='"'
  printf '%s%s%s' "$quote" "${1//$quote/$quote$quote}" "$quote"
}

table_exists() {
  local side=$1 table=$2 out
  out=$("${side}_query" "SELECT 1 FROM information_schema.tables
    WHERE table_schema = 'public' AND table_name = $(sql_literal "$table") LIMIT 1") || return 2
  [ -n "$out" ]
}

columns_of() {
  local side=$1 table=$2
  "${side}_query" "SELECT column_name FROM information_schema.columns
    WHERE table_schema = 'public' AND table_name = $(sql_literal "$table")
    ORDER BY ordinal_position"
}

# Destination columns the copy has to fill: NOT NULL, no default, not an
# identity or generated column. If the source has no column of that name the
# copy cannot succeed, and inventing a value would put a wrong row in the
# destination.
mandatory_columns_of_destination() {
  local table=$1
  dst_query "SELECT column_name FROM information_schema.columns
    WHERE table_schema = 'public' AND table_name = $(sql_literal "$table")
      AND is_nullable = 'NO'
      AND column_default IS NULL
      AND is_identity = 'NO'
      AND is_generated = 'NEVER'
    ORDER BY ordinal_position"
}

destination_has_rows() {
  local table=$1 out
  out=$(dst_query "SELECT 1 FROM public.$(quote_ident "$table") LIMIT 1") || return 2
  [ -n "$out" ]
}

# Every sequence owned by a column of this table is moved past the largest
# value the copy brought in. to_jsonb() is how a column named at runtime is
# read; pg_get_serial_sequence() returns NULL for columns without a sequence,
# so tables with none are scanned zero times.
reset_sequences_of() {
  local table=$1
  dst_query "
    SELECT setval(sequence_name,
                  GREATEST(COALESCE(max_value, 0), 1),
                  COALESCE(max_value, 0) > 0)
      FROM (
        SELECT pg_get_serial_sequence($(sql_literal "public.$table"), c.column_name) AS sequence_name,
               (SELECT max((to_jsonb(t) ->> c.column_name)::bigint)
                  FROM public.$(quote_ident "$table") AS t) AS max_value
          FROM information_schema.columns AS c
         WHERE c.table_schema = 'public'
           AND c.table_name = $(sql_literal "$table")
           AND pg_get_serial_sequence($(sql_literal "public.$table"), c.column_name) IS NOT NULL
      ) AS owned" > /dev/null
}

copy_table() {
  local table=$1
  local -a src_cols=() dst_cols=() shared=() dropped=() added=() missing=()
  local col column_list copy_status copied started elapsed rc

  table_exists src "$table" && rc=0 || rc=$?
  [ "$rc" -le 1 ] || { log "abort  ${table}: cannot read the source schema"; return 1; }
  if [ "$rc" -eq 1 ]; then
    log "skip   ${table}: not present in the source schema"
    return 0
  fi

  table_exists dst "$table" && rc=0 || rc=$?
  [ "$rc" -le 1 ] || { log "abort  ${table}: cannot read the destination schema"; return 1; }
  if [ "$rc" -eq 1 ]; then
    log "skip   ${table}: not present in the destination schema"
    return 0
  fi

  mapfile -t src_cols < <(columns_of src "$table")
  mapfile -t dst_cols < <(columns_of dst "$table")

  if [ ${#src_cols[@]} -eq 0 ] || [ ${#dst_cols[@]} -eq 0 ]; then
    log "abort  ${table}: information_schema returned no columns on one side"
    return 1
  fi

  local -A in_src=() in_dst=()
  for col in "${src_cols[@]}"; do in_src[$col]=1; done
  for col in "${dst_cols[@]}"; do in_dst[$col]=1; done

  # Ordered by the destination's ordinal position, so the SELECT list and the
  # COPY column list are the same names in the same order on both sides.
  for col in "${dst_cols[@]}"; do
    if [ -n "${in_src[$col]:-}" ]; then
      shared+=("$col")
    else
      added+=("$col")
    fi
  done
  for col in "${src_cols[@]}"; do
    [ -z "${in_dst[$col]:-}" ] && dropped+=("$col")
  done

  if [ ${#shared[@]} -eq 0 ]; then
    log "skip   ${table}: the two schemas share no column names"
    return 0
  fi

  while IFS= read -r col; do
    [ -n "$col" ] || continue
    [ -z "${in_src[$col]:-}" ] && missing+=("$col")
  done < <(mandatory_columns_of_destination "$table")

  if [ ${#missing[@]} -gt 0 ]; then
    log "abort  ${table}: destination requires ${missing[*]}, and the source has no column of that name"
    return 1
  fi

  [ ${#dropped[@]} -gt 0 ] && log "note   ${table}: source-only, not copied: ${dropped[*]}"
  [ ${#added[@]} -gt 0 ] && log "note   ${table}: destination-only, left at its default: ${added[*]}"

  column_list=""
  for col in "${shared[@]}"; do
    [ -n "$column_list" ] && column_list+=", "
    column_list+="$(quote_ident "$col")"
  done

  if [ "${DRY_RUN:-0}" = "1" ]; then
    log "plan   ${table}: ${#shared[@]} of ${#dst_cols[@]} destination columns"
    return 0
  fi

  destination_has_rows "$table" && rc=0 || rc=$?
  [ "$rc" -le 1 ] || { log "abort  ${table}: cannot read the destination table"; return 1; }
  if [ "$rc" -eq 0 ]; then
    if [ "${ALLOW_NONEMPTY_DESTINATION:-0}" != "1" ]; then
      log "abort  ${table}: destination table is not empty; set ALLOW_NONEMPTY_DESTINATION=1 to append"
      return 1
    fi
    log "note   ${table}: destination is not empty, appending"
  fi

  started=$SECONDS

  # Streamed, never staged on disk: the source writes COPY text to its stdout
  # and the destination reads it from stdin. The receiving psql reports
  # "COPY <rows>", which is the count logged below.
  if ! copy_status=$(
    psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 \
      --dbname "$SRC_DATABASE_URL" \
      --command "\\copy (SELECT ${column_list} FROM public.$(quote_ident "$table")) TO STDOUT" |
    psql --no-psqlrc --set ON_ERROR_STOP=1 \
      --dbname "$DST_DATABASE_URL" \
      --command "\\copy public.$(quote_ident "$table") (${column_list}) FROM STDIN"
  ); then
    log "abort  ${table}: copy failed"
    return 1
  fi

  elapsed=$((SECONDS - started))
  copied=$(printf '%s\n' "$copy_status" | sed -n 's/^COPY \([0-9][0-9]*\)$/\1/p' | tail -n 1)
  [ -n "$copied" ] || copied="unknown"

  if ! reset_sequences_of "$table"; then
    log "abort  ${table}: rows copied but the sequence reset failed"
    return 1
  fi
  dst_query "ANALYZE public.$(quote_ident "$table")" > /dev/null || true

  log "copied ${table}: ${copied} rows in ${elapsed}s (${#shared[@]} columns)"
  return 0
}

main() {
  command -v psql > /dev/null || die "psql is not on PATH"

  require_env SRC_DATABASE_URL
  require_env DST_DATABASE_URL

  src_query "SELECT 1" > /dev/null || die "cannot query the source database"
  dst_query "SELECT 1" > /dev/null || die "cannot query the destination database"

  local -a tables=()
  read -r -a tables <<< "$(printf '%s' "${TABLES:-$DEFAULT_TABLES}" | tr -s '[:space:]' ' ')"

  [ ${#tables[@]} -gt 0 ] || die "no tables to copy"

  [ "${DRY_RUN:-0}" = "1" ] && log "dry run: no rows will be written"
  log "tables: ${#tables[@]}"

  local table
  for table in "${tables[@]}"; do
    [ -n "$table" ] || continue
    if ! copy_table "$table"; then
      log "stopped at ${table}"
      exit 2
    fi
  done

  log "done"
}

main "$@"
