#!/usr/bin/env bash
#
# copy-blockscout-11-to-10.sh
#
# Copy the core chain tables of a Blockscout 11.x database into a freshly
# migrated Blockscout 10.2.6 database, under a pinned block ceiling, in a way
# that survives an interruption and ends in a state that can be verified.
#
# The two schemas are close but not identical: 11.x adds columns, renames a
# few, and drops others. Rather than hard-coding a mapping that goes stale on
# the next upstream release, this script asks both databases what columns each
# table actually has, intersects the two lists by column name, and copies only
# that intersection. A column that exists on one side and not on the other is
# reported, never guessed at.
#
# The ceiling
#
#   Before the first table is read the run pins a block ceiling: the highest
#   block number the source has marked consensus at that moment. No row above
#   that ceiling is copied, and the verification counts both sides at that same
#   ceiling, so a source that keeps indexing while the copy runs cannot be read
#   as a mismatch. The ceiling is named in the run summary.
#
# Resuming
#
#   A table that carries a block number is copied in block-aligned batches, one
#   transaction per batch, so a batch is either wholly inserted or not inserted
#   at all and no block is ever half copied. Each batch records the table's
#   progress in public.paxeer_x_copy_progress in the destination, in the same
#   statement as the rows. A later run continues each table above the greater
#   of that record and the table's own highest copied block instead of starting
#   again. Rows are inserted with ON CONFLICT DO NOTHING, so a row already in
#   the destination is left exactly as it is and a rerun with nothing to do
#   writes nothing.
#
#   A table that carries no block number has no key to resume from. It is
#   copied whole under the same conflict handling, so a rerun still writes
#   nothing; the run summary names those tables.
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
#   BLOCK_BATCH                   optional, how many block numbers one copy
#                                 transaction covers; default 100000
#   DRY_RUN                       optional, set to 1 to print the plan and the
#                                 column differences without copying anything
#   ALLOW_NONEMPTY_DESTINATION    optional, set to 1 to append to destination
#                                 tables that already hold rows; without it a
#                                 destination table holding rows this tool has
#                                 no progress record for is an error
#
# Exit codes
#   0  every table in the list was copied, skipped, or planned in a dry run,
#      and every verification matched at the ceiling
#   1  usage or precondition failure, nothing was copied
#   2  a copy or a count failed; the log names the table it stopped on
#   3  the copy finished but at least one table's counts differ at the ceiling;
#      the summary names every such table with both counts
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
#   * anything the destination schema has no table for;
#   * a source column the destination has no column of that name for. Those are
#     reported per table as the table is copied, and the summary repeats them
#     by name for transactions and internal_transactions, the two tables whose
#     11.x shape differs most from 10.2.6.
#
# Derived columns
#
# internal_transactions.trace_address is NOT NULL in the 10.2.6 schema as it is
# created, while an 11.x row may carry no trace address at all. Copying the
# source value straight across therefore puts a null into a column that forbids
# it and the table fails as a whole. The copy derives the value instead, from
# the source's own representation of where the row sits: the root call of a
# transaction, at index 0, gets the empty array Blockscout writes for it, and
# every other row gets its index as a one-element array. The source carries no
# parent link, so this is the row's flat position within its transaction rather
# than a reconstruction of the call tree; it is unique per transaction and
# orders the rows the way the source orders them. A source row that does carry
# a trace address keeps the one it has.
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

# The bookkeeping this tool owns in the destination. The progress table is what
# makes a run resumable; the staging table is where a batch lands before it is
# inserted with the conflict handling COPY itself has none of.
readonly PROGRESS_TABLE='paxeer_x_copy_progress'
readonly STAGE_TABLE='paxeer_x_copy_stage'

# Pinned once, before the first table is read.
CEILING=''
BATCH_SIZE=''

declare -A TABLE_CEILING_COLUMN=()
declare -A TABLE_SOURCE_ONLY=()
declare -a PROCESSED_TABLES=()
declare -a UNBOUNDED_TABLES=()
declare -a MISMATCHES=()

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

is_integer() {
  case ${1:-} in
    '' | *[!0-9-]*) return 1 ;;
    *) [ "$1" -eq "$1" ] 2> /dev/null ;;
  esac
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
# identity or generated column. If neither the source nor a derivation can
# answer for one the copy cannot succeed, and inventing a value would put a
# wrong row in the destination.
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

# The highest block the source has fully written: the largest number among the
# blocks it has marked consensus. Anything above it may still be arriving, so
# nothing above it is copied and nothing above it is counted.
pin_ceiling() {
  local out
  table_exists src blocks || die "the source has no blocks table, so no block ceiling can be pinned"
  out=$(src_query "SELECT max(number) FROM public.blocks WHERE consensus") ||
    die "cannot read the source's highest consensus block"
  [ -n "$out" ] || die "the source has no consensus block, so there is nothing to copy under a ceiling"
  is_integer "$out" || die "the source's highest consensus block is not a number"
  CEILING=$out
}

ensure_progress_table() {
  dst_query "CREATE TABLE IF NOT EXISTS public.$(quote_ident "$PROGRESS_TABLE") (
    table_name  text PRIMARY KEY,
    ceiling     bigint NOT NULL,
    highest_key bigint,
    rows_copied bigint NOT NULL DEFAULT 0,
    updated_at  timestamptz NOT NULL DEFAULT now()
  )" > /dev/null || die "cannot create the progress table in the destination"
}

progress_table_exists() {
  local out
  out=$(dst_query "SELECT 1 FROM information_schema.tables
    WHERE table_schema = 'public' AND table_name = $(sql_literal "$PROGRESS_TABLE") LIMIT 1") || return 2
  [ -n "$out" ]
}

progress_recorded() {
  local table=$1 out rc
  progress_table_exists && rc=0 || rc=$?
  [ "$rc" -le 1 ] || return 2
  [ "$rc" -eq 0 ] || return 1
  out=$(dst_query "SELECT 1 FROM public.$(quote_ident "$PROGRESS_TABLE")
    WHERE table_name = $(sql_literal "$table") LIMIT 1") || return 2
  [ -n "$out" ]
}

# Where a table starts again: the greater of the highest key already in the
# destination table and the highest key this tool recorded for it. The record
# also covers ranges that held no row, which the table itself cannot show.
resume_point() {
  local table=$1 column=$2 recorded='NULL' rc
  progress_table_exists && rc=0 || rc=$?
  [ "$rc" -le 1 ] || return 1
  if [ "$rc" -eq 0 ]; then
    recorded="(SELECT highest_key FROM public.$(quote_ident "$PROGRESS_TABLE")
      WHERE table_name = $(sql_literal "$table"))"
  fi
  dst_query "SELECT GREATEST(
    (SELECT max($(quote_ident "$column")) FROM public.$(quote_ident "$table")),
    ${recorded})"
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

# One batch. The source writes COPY text to its stdout and the destination
# reads it from stdin into an unlogged staging table, so nothing is staged on
# disk; then one statement inserts the staged rows, leaves any row that is
# already there untouched, and advances the table's recorded progress, so the
# record can never run ahead of the rows. Prints the number of rows the insert
# actually added.
copy_batch() {
  local table=$1 column_list=$2 select_list=$3 predicate=$4 high_key=$5
  local inserted

  dst_query "TRUNCATE public.$(quote_ident "$STAGE_TABLE")" > /dev/null || return 1

  if ! psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 \
    --dbname "$SRC_DATABASE_URL" \
    --command "\\copy (SELECT ${select_list} FROM public.$(quote_ident "$table") ${predicate}) TO STDOUT" |
    psql --no-psqlrc --quiet --set ON_ERROR_STOP=1 \
      --dbname "$DST_DATABASE_URL" \
      --command "\\copy public.$(quote_ident "$STAGE_TABLE") (${column_list}) FROM STDIN" > /dev/null; then
    return 1
  fi

  inserted=$(dst_query "
    WITH inserted AS (
      INSERT INTO public.$(quote_ident "$table") (${column_list})
      SELECT ${column_list} FROM public.$(quote_ident "$STAGE_TABLE")
      ON CONFLICT DO NOTHING
      RETURNING 1
    ), counted AS (
      SELECT count(*)::bigint AS row_count FROM inserted
    ), recorded AS (
      INSERT INTO public.$(quote_ident "$PROGRESS_TABLE") AS p
                  (table_name, ceiling, highest_key, rows_copied, updated_at)
      SELECT $(sql_literal "$table"), ${CEILING}, ${high_key}, counted.row_count, now()
        FROM counted
      ON CONFLICT (table_name) DO UPDATE
        SET ceiling     = EXCLUDED.ceiling,
            highest_key = GREATEST(p.highest_key, EXCLUDED.highest_key),
            rows_copied = p.rows_copied + EXCLUDED.rows_copied,
            updated_at  = now()
    )
    SELECT row_count FROM counted") || return 1

  is_integer "$inserted" || return 1
  printf '%s' "$inserted"
}

copy_table() {
  local table=$1
  local -a src_cols=() dst_cols=() shared=() dropped=() added=() missing=()
  local -A derived=()
  local col column_list select_list ceiling_column predicate
  local started elapsed rc first resume lo hi batch_rows copied=0 batches=0

  table_exists src "$table" && rc=0 || rc=$?
  [ "$rc" -le 1 ] || {
    log "abort  ${table}: cannot read the source schema"
    return 1
  }
  if [ "$rc" -eq 1 ]; then
    log "skip   ${table}: not present in the source schema"
    return 0
  fi

  table_exists dst "$table" && rc=0 || rc=$?
  [ "$rc" -le 1 ] || {
    log "abort  ${table}: cannot read the destination schema"
    return 1
  }
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

  # A destination column the source cannot answer for by name, but the source's
  # own representation of the row still can. See "Derived columns" above.
  if [ "$table" = 'internal_transactions' ] && [ -n "${in_dst[trace_address]:-}" ]; then
    if [ -z "${in_src[index]:-}" ]; then
      log "abort  ${table}: the source has no index column, so trace_address cannot be derived"
      return 1
    fi
    if [ -n "${in_src[trace_address]:-}" ]; then
      derived[trace_address]='COALESCE("trace_address", CASE WHEN "index" = 0 THEN ARRAY[]::integer[] ELSE ARRAY["index"] END)'
    else
      derived[trace_address]='CASE WHEN "index" = 0 THEN ARRAY[]::integer[] ELSE ARRAY["index"] END'
    fi
  fi

  # Ordered by the destination's ordinal position, so the SELECT list and the
  # COPY column list are the same names in the same order on both sides.
  for col in "${dst_cols[@]}"; do
    if [ -n "${in_src[$col]:-}" ] || [ -n "${derived[$col]:-}" ]; then
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
    [ -z "${in_src[$col]:-}" ] && [ -z "${derived[$col]:-}" ] && missing+=("$col")
  done < <(mandatory_columns_of_destination "$table")

  if [ ${#missing[@]} -gt 0 ]; then
    log "abort  ${table}: destination requires ${missing[*]}, and the source has no column of that name"
    return 1
  fi

  TABLE_SOURCE_ONLY[$table]="${dropped[*]:-}"
  [ ${#dropped[@]} -gt 0 ] && log "note   ${table}: source-only, not copied: ${dropped[*]}"
  [ ${#added[@]} -gt 0 ] && log "note   ${table}: destination-only, left at its default: ${added[*]}"
  for col in "${!derived[@]}"; do
    log "note   ${table}: ${col} derived from the source's row position rather than copied as it stands"
  done

  # The column the ceiling and the resume point are read on. blocks carries the
  # number itself; every other table that knows its block carries block_number,
  # and it has to be on both sides for the predicate and the counts to agree.
  ceiling_column=''
  if [ "$table" = 'blocks' ] && [ -n "${in_src[number]:-}" ] && [ -n "${in_dst[number]:-}" ]; then
    ceiling_column='number'
  elif [ -n "${in_src[block_number]:-}" ] && [ -n "${in_dst[block_number]:-}" ]; then
    ceiling_column='block_number'
  fi
  TABLE_CEILING_COLUMN[$table]=$ceiling_column
  if [ -z "$ceiling_column" ]; then
    UNBOUNDED_TABLES+=("$table")
    log "note   ${table}: no block number on both sides; copied and counted whole"
  fi

  column_list=''
  select_list=''
  for col in "${shared[@]}"; do
    if [ -n "$column_list" ]; then
      column_list+=", "
      select_list+=", "
    fi
    column_list+="$(quote_ident "$col")"
    if [ -n "${derived[$col]:-}" ]; then
      select_list+="${derived[$col]} AS $(quote_ident "$col")"
    else
      select_list+="$(quote_ident "$col")"
    fi
  done

  if [ "${DRY_RUN:-0}" = "1" ]; then
    if [ -n "$ceiling_column" ]; then
      resume=$(resume_point "$table" "$ceiling_column") || {
        log "abort  ${table}: cannot read the destination's resume point"
        return 1
      }
      log "plan   ${table}: ${#shared[@]} of ${#dst_cols[@]} destination columns, ${ceiling_column} above ${resume:-nothing} up to ${CEILING}"
    else
      log "plan   ${table}: ${#shared[@]} of ${#dst_cols[@]} destination columns, whole table"
    fi
    PROCESSED_TABLES+=("$table")
    return 0
  fi

  destination_has_rows "$table" && rc=0 || rc=$?
  [ "$rc" -le 1 ] || {
    log "abort  ${table}: cannot read the destination table"
    return 1
  }
  if [ "$rc" -eq 0 ]; then
    progress_recorded "$table" && rc=0 || rc=$?
    [ "$rc" -le 1 ] || {
      log "abort  ${table}: cannot read the progress table"
      return 1
    }
    if [ "$rc" -eq 1 ]; then
      if [ "${ALLOW_NONEMPTY_DESTINATION:-0}" != "1" ]; then
        log "abort  ${table}: destination table holds rows no run of this tool recorded; set ALLOW_NONEMPTY_DESTINATION=1 to append"
        return 1
      fi
      log "note   ${table}: destination is not empty, appending"
    fi
  fi

  started=$SECONDS

  # The staging table takes exactly the columns being copied, with the
  # destination's types and none of its constraints, so a batch can land in one
  # COPY and then be inserted with conflict handling.
  dst_query "DROP TABLE IF EXISTS public.$(quote_ident "$STAGE_TABLE")" > /dev/null || {
    log "abort  ${table}: cannot drop the staging table"
    return 1
  }
  dst_query "CREATE UNLOGGED TABLE public.$(quote_ident "$STAGE_TABLE") AS
    SELECT ${column_list} FROM public.$(quote_ident "$table") WITH NO DATA" > /dev/null || {
    log "abort  ${table}: cannot create the staging table"
    return 1
  }

  if [ -n "$ceiling_column" ]; then
    first=$(src_query "SELECT min($(quote_ident "$ceiling_column")) FROM public.$(quote_ident "$table")
      WHERE $(quote_ident "$ceiling_column") <= ${CEILING}") || {
      log "abort  ${table}: cannot read the source's lowest ${ceiling_column}"
      return 1
    }
    resume=$(resume_point "$table" "$ceiling_column") || {
      log "abort  ${table}: cannot read the destination's resume point"
      return 1
    }

    if [ -z "$first" ]; then
      lo=$((CEILING + 1))
      log "note   ${table}: no source row at or below the ceiling"
    elif [ -n "$resume" ]; then
      lo=$((resume + 1))
      [ "$first" -gt "$lo" ] && lo=$first
      [ "$lo" -le "$CEILING" ] && log "note   ${table}: resuming above ${ceiling_column} ${resume}"
    else
      lo=$first
    fi

    while [ "$lo" -le "$CEILING" ]; do
      hi=$((lo + BATCH_SIZE - 1))
      [ "$hi" -gt "$CEILING" ] && hi=$CEILING
      predicate="WHERE $(quote_ident "$ceiling_column") BETWEEN ${lo} AND ${hi}"
      if ! batch_rows=$(copy_batch "$table" "$column_list" "$select_list" "$predicate" "$hi"); then
        log "abort  ${table}: copy failed over ${ceiling_column} ${lo} to ${hi}"
        return 1
      fi
      copied=$((copied + batch_rows))
      batches=$((batches + 1))
      lo=$((hi + 1))
    done
  else
    if ! batch_rows=$(copy_batch "$table" "$column_list" "$select_list" '' 'NULL'); then
      log "abort  ${table}: copy failed"
      return 1
    fi
    copied=$batch_rows
    batches=1
  fi

  dst_query "DROP TABLE IF EXISTS public.$(quote_ident "$STAGE_TABLE")" > /dev/null || {
    log "abort  ${table}: rows copied but the staging table could not be dropped"
    return 1
  }

  if ! reset_sequences_of "$table"; then
    log "abort  ${table}: rows copied but the sequence reset failed"
    return 1
  fi
  dst_query "ANALYZE public.$(quote_ident "$table")" > /dev/null || true

  elapsed=$((SECONDS - started))
  log "copied ${table}: ${copied} rows in ${batches} batches, ${elapsed}s (${#shared[@]} columns)"
  PROCESSED_TABLES+=("$table")
  return 0
}

# Both sides counted with the same predicate at the same ceiling, so a source
# that grew while the copy ran is not read as a row the copy lost.
verify_table() {
  local table=$1
  local ceiling_column=${TABLE_CEILING_COLUMN[$table]:-}
  local predicate='' src_n dst_n

  if [ -n "$ceiling_column" ]; then
    predicate=" WHERE $(quote_ident "$ceiling_column") <= ${CEILING}"
  fi

  src_n=$(src_query "SELECT count(*) FROM public.$(quote_ident "$table")${predicate}") || {
    log "abort  ${table}: cannot count the source at the ceiling"
    return 1
  }
  dst_n=$(dst_query "SELECT count(*) FROM public.$(quote_ident "$table")${predicate}") || {
    log "abort  ${table}: cannot count the destination at the ceiling"
    return 1
  }

  if [ "$src_n" = "$dst_n" ]; then
    log "verify ${table}: source ${src_n}, target ${dst_n} at ceiling ${CEILING}, ok"
  else
    log "verify ${table}: source ${src_n}, target ${dst_n} at ceiling ${CEILING}, MISMATCH"
    MISMATCHES+=("${table}: source ${src_n}, target ${dst_n}")
  fi
  return 0
}

print_summary() {
  local table entry

  log "summary"
  log "  block ceiling: ${CEILING}"
  log "  tables processed: ${#PROCESSED_TABLES[@]}"

  for table in transactions internal_transactions; do
    if [ -n "${TABLE_SOURCE_ONLY[$table]+set}" ]; then
      if [ -n "${TABLE_SOURCE_ONLY[$table]}" ]; then
        log "  ${table}: source-only columns not copied: ${TABLE_SOURCE_ONLY[$table]}"
      else
        log "  ${table}: no source-only columns"
      fi
    fi
  done

  if [ ${#UNBOUNDED_TABLES[@]} -gt 0 ]; then
    log "  no block number to bound them, copied and counted whole: ${UNBOUNDED_TABLES[*]}"
  fi

  if [ ${#MISMATCHES[@]} -eq 0 ]; then
    log "  every processed table matches at the ceiling"
    return 0
  fi

  log "  ${#MISMATCHES[@]} table(s) do not match at ceiling ${CEILING}:"
  for entry in "${MISMATCHES[@]}"; do
    log "  mismatch ${entry}"
  done
  return 1
}

main() {
  command -v psql > /dev/null || die "psql is not on PATH"

  require_env SRC_DATABASE_URL
  require_env DST_DATABASE_URL

  BATCH_SIZE=${BLOCK_BATCH:-100000}
  if ! is_integer "$BATCH_SIZE" || [ "$BATCH_SIZE" -lt 1 ]; then
    die "BLOCK_BATCH must be a positive whole number of blocks"
  fi

  src_query "SELECT 1" > /dev/null || die "cannot query the source database"
  dst_query "SELECT 1" > /dev/null || die "cannot query the destination database"

  local -a tables=()
  read -r -a tables <<< "$(printf '%s' "${TABLES:-$DEFAULT_TABLES}" | tr -s '[:space:]' ' ')"

  [ ${#tables[@]} -gt 0 ] || die "no tables to copy"

  pin_ceiling
  log "ceiling: block ${CEILING}, the source's highest consensus block when this run started"

  if [ "${DRY_RUN:-0}" = "1" ]; then
    log "dry run: no rows will be written"
  else
    ensure_progress_table
  fi
  log "tables: ${#tables[@]}, ${BATCH_SIZE} blocks per batch"

  local table
  for table in "${tables[@]}"; do
    [ -n "$table" ] || continue
    if ! copy_table "$table"; then
      log "stopped at ${table}"
      exit 2
    fi
  done

  if [ "${DRY_RUN:-0}" = "1" ]; then
    log "dry run: nothing was copied and nothing was verified"
    exit 0
  fi

  for table in "${PROCESSED_TABLES[@]}"; do
    verify_table "$table" || exit 2
  done

  if ! print_summary; then
    log "done with mismatches"
    exit 3
  fi

  log "done"
}

main "$@"
