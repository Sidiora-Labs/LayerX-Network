--
-- PostgreSQL database dump
--

-- Dumped from database version 17.2 (Debian 17.2-1.pgdg120+1)
-- Dumped by pg_dump version 17.2 (Debian 17.2-1.pgdg120+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: btree_gin; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS btree_gin WITH SCHEMA public;


--
-- Name: EXTENSION btree_gin; Type: COMMENT; Schema: -; Owner: 
--

COMMENT ON EXTENSION btree_gin IS 'support for indexing common datatypes in GIN';


--
-- Name: btree_gist; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS btree_gist WITH SCHEMA public;


--
-- Name: EXTENSION btree_gist; Type: COMMENT; Schema: -; Owner: 
--

COMMENT ON EXTENSION btree_gist IS 'support for indexing common datatypes in GiST';


--
-- Name: citext; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS citext WITH SCHEMA public;


--
-- Name: EXTENSION citext; Type: COMMENT; Schema: -; Owner: 
--

COMMENT ON EXTENSION citext IS 'data type for case-insensitive character strings';


--
-- Name: pg_trgm; Type: EXTENSION; Schema: -; Owner: -
--

CREATE EXTENSION IF NOT EXISTS pg_trgm WITH SCHEMA public;


--
-- Name: EXTENSION pg_trgm; Type: COMMENT; Schema: -; Owner: 
--

COMMENT ON EXTENSION pg_trgm IS 'text similarity measurement and index searching based on trigrams';


--
-- Name: entry_point_version; Type: TYPE; Schema: public; Owner: postgres
--

CREATE TYPE public.entry_point_version AS ENUM (
    'v0.6',
    'v0.7'
);


ALTER TYPE public.entry_point_version OWNER TO postgres;

--
-- Name: metadata_tag_record; Type: TYPE; Schema: public; Owner: postgres
--

CREATE TYPE public.metadata_tag_record AS (
	id integer,
	address_hash bytea,
	metadata jsonb,
	addresses_index integer
);


ALTER TYPE public.metadata_tag_record OWNER TO postgres;

--
-- Name: proxy_type; Type: TYPE; Schema: public; Owner: postgres
--

CREATE TYPE public.proxy_type AS ENUM (
    'eip1167',
    'eip1967',
    'eip1822',
    'eip930',
    'master_copy',
    'basic_implementation',
    'basic_get_implementation',
    'comptroller',
    'eip2535',
    'clone_with_immutable_arguments',
    'eip7702',
    'unknown'
);


ALTER TYPE public.proxy_type OWNER TO postgres;

--
-- Name: sponsor_type; Type: TYPE; Schema: public; Owner: postgres
--

CREATE TYPE public.sponsor_type AS ENUM (
    'wallet_deposit',
    'wallet_balance',
    'paymaster_sponsor',
    'paymaster_hybrid'
);


ALTER TYPE public.sponsor_type OWNER TO postgres;

--
-- Name: transaction_actions_protocol; Type: TYPE; Schema: public; Owner: postgres
--

CREATE TYPE public.transaction_actions_protocol AS ENUM (
    'uniswap_v3',
    'opensea_v1_1',
    'wrapping',
    'approval',
    'zkbob',
    'aave_v3'
);


ALTER TYPE public.transaction_actions_protocol OWNER TO postgres;

--
-- Name: transaction_actions_type; Type: TYPE; Schema: public; Owner: postgres
--

CREATE TYPE public.transaction_actions_type AS ENUM (
    'mint_nft',
    'mint',
    'burn',
    'collect',
    'swap',
    'sale',
    'cancel',
    'transfer',
    'wrap',
    'unwrap',
    'approve',
    'revoke',
    'withdraw',
    'deposit',
    'borrow',
    'supply',
    'repay',
    'flash_loan',
    'enable_collateral',
    'disable_collateral',
    'liquidation_call'
);


ALTER TYPE public.transaction_actions_type OWNER TO postgres;

--
-- Name: convert(text[]); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.convert(text[]) RETURNS bytea[]
    LANGUAGE plpgsql
    AS $_$
  DECLARE
    s bytea[] := ARRAY[]::bytea[];
    x text;
  BEGIN
    FOREACH x IN ARRAY $1
    LOOP
      s := array_append(s, decode(replace(x, '0x', ''), 'hex'));
    END LOOP;
    RETURN s;
  END;
  $_$;


ALTER FUNCTION public.convert(text[]) OWNER TO postgres;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: account_api_keys; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_api_keys (
    identity_id bigint NOT NULL,
    name character varying(255) NOT NULL,
    value uuid NOT NULL,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL
);


ALTER TABLE public.account_api_keys OWNER TO postgres;

--
-- Name: account_api_plans; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_api_plans (
    id integer NOT NULL,
    max_req_per_second smallint,
    name character varying(255) NOT NULL,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL
);


ALTER TABLE public.account_api_plans OWNER TO postgres;

--
-- Name: account_api_plans_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_api_plans_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_api_plans_id_seq OWNER TO postgres;

--
-- Name: account_api_plans_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_api_plans_id_seq OWNED BY public.account_api_plans.id;


--
-- Name: account_custom_abis; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_custom_abis (
    id integer NOT NULL,
    identity_id bigint NOT NULL,
    abi jsonb NOT NULL,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL,
    address_hash_hash bytea,
    address_hash bytea,
    name bytea,
    user_created boolean DEFAULT true
);


ALTER TABLE public.account_custom_abis OWNER TO postgres;

--
-- Name: account_custom_abis_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_custom_abis_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_custom_abis_id_seq OWNER TO postgres;

--
-- Name: account_custom_abis_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_custom_abis_id_seq OWNED BY public.account_custom_abis.id;


--
-- Name: account_identities; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_identities (
    id bigint NOT NULL,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL,
    plan_id bigint DEFAULT 1,
    uid bytea,
    uid_hash bytea,
    email bytea,
    avatar bytea,
    verification_email_sent_at timestamp without time zone,
    otp_sent_at timestamp without time zone
);


ALTER TABLE public.account_identities OWNER TO postgres;

--
-- Name: account_identities_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_identities_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_identities_id_seq OWNER TO postgres;

--
-- Name: account_identities_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_identities_id_seq OWNED BY public.account_identities.id;


--
-- Name: account_public_tags_requests; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_public_tags_requests (
    id bigint NOT NULL,
    identity_id bigint,
    company character varying(255),
    website character varying(255),
    tags character varying(255),
    description text,
    additional_comment character varying(255),
    request_type character varying(255),
    is_owner boolean,
    remove_reason text,
    request_id character varying(255),
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL,
    addresses bytea[],
    email bytea,
    full_name bytea
);


ALTER TABLE public.account_public_tags_requests OWNER TO postgres;

--
-- Name: account_public_tags_requests_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_public_tags_requests_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_public_tags_requests_id_seq OWNER TO postgres;

--
-- Name: account_public_tags_requests_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_public_tags_requests_id_seq OWNED BY public.account_public_tags_requests.id;


--
-- Name: account_tag_addresses; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_tag_addresses (
    id bigint NOT NULL,
    identity_id bigint,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL,
    address_hash_hash bytea,
    name bytea,
    address_hash bytea,
    user_created boolean DEFAULT true
);


ALTER TABLE public.account_tag_addresses OWNER TO postgres;

--
-- Name: account_tag_addresses_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_tag_addresses_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_tag_addresses_id_seq OWNER TO postgres;

--
-- Name: account_tag_addresses_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_tag_addresses_id_seq OWNED BY public.account_tag_addresses.id;


--
-- Name: account_tag_transactions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_tag_transactions (
    id bigint NOT NULL,
    identity_id bigint,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL,
    transaction_hash_hash bytea,
    name bytea,
    transaction_hash bytea,
    user_created boolean DEFAULT true
);


ALTER TABLE public.account_tag_transactions OWNER TO postgres;

--
-- Name: account_tag_transactions_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_tag_transactions_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_tag_transactions_id_seq OWNER TO postgres;

--
-- Name: account_tag_transactions_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_tag_transactions_id_seq OWNED BY public.account_tag_transactions.id;


--
-- Name: account_watchlist_addresses; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_watchlist_addresses (
    id bigint NOT NULL,
    watchlist_id bigint,
    watch_coin_input boolean DEFAULT true,
    watch_coin_output boolean DEFAULT true,
    watch_erc_20_input boolean DEFAULT true,
    watch_erc_20_output boolean DEFAULT true,
    watch_erc_721_input boolean DEFAULT true,
    watch_erc_721_output boolean DEFAULT true,
    watch_erc_1155_input boolean DEFAULT true,
    watch_erc_1155_output boolean DEFAULT true,
    notify_email boolean DEFAULT true,
    notify_epns boolean DEFAULT false,
    notify_feed boolean DEFAULT true,
    notify_inapp boolean DEFAULT false,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL,
    address_hash_hash bytea,
    name bytea,
    address_hash bytea,
    watch_erc_404_input boolean DEFAULT true,
    watch_erc_404_output boolean DEFAULT true,
    user_created boolean DEFAULT true
);


ALTER TABLE public.account_watchlist_addresses OWNER TO postgres;

--
-- Name: account_watchlist_addresses_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_watchlist_addresses_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_watchlist_addresses_id_seq OWNER TO postgres;

--
-- Name: account_watchlist_addresses_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_watchlist_addresses_id_seq OWNED BY public.account_watchlist_addresses.id;


--
-- Name: account_watchlist_notifications; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_watchlist_notifications (
    id bigint NOT NULL,
    watchlist_address_id bigint,
    direction character varying(255),
    type character varying(255),
    method character varying(255),
    block_number integer,
    amount numeric,
    transaction_fee numeric,
    viewed_at timestamp without time zone,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    name bytea,
    subject bytea,
    from_address_hash bytea,
    to_address_hash bytea,
    transaction_hash bytea,
    subject_hash bytea,
    from_address_hash_hash bytea,
    to_address_hash_hash bytea,
    transaction_hash_hash bytea,
    watchlist_id bigint NOT NULL
);


ALTER TABLE public.account_watchlist_notifications OWNER TO postgres;

--
-- Name: account_watchlist_notifications_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_watchlist_notifications_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_watchlist_notifications_id_seq OWNER TO postgres;

--
-- Name: account_watchlist_notifications_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_watchlist_notifications_id_seq OWNED BY public.account_watchlist_notifications.id;


--
-- Name: account_watchlist_notifications_watchlist_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_watchlist_notifications_watchlist_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_watchlist_notifications_watchlist_id_seq OWNER TO postgres;

--
-- Name: account_watchlist_notifications_watchlist_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_watchlist_notifications_watchlist_id_seq OWNED BY public.account_watchlist_notifications.watchlist_id;


--
-- Name: account_watchlists; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.account_watchlists (
    id bigint NOT NULL,
    name character varying(255) DEFAULT 'default'::character varying,
    identity_id bigint,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL
);


ALTER TABLE public.account_watchlists OWNER TO postgres;

--
-- Name: account_watchlists_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.account_watchlists_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.account_watchlists_id_seq OWNER TO postgres;

--
-- Name: account_watchlists_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.account_watchlists_id_seq OWNED BY public.account_watchlists.id;


--
-- Name: address_coin_balances; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.address_coin_balances (
    address_hash bytea NOT NULL,
    block_number bigint NOT NULL,
    value numeric(100,0) DEFAULT NULL::numeric,
    value_fetched_at timestamp without time zone,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.address_coin_balances OWNER TO postgres;

--
-- Name: address_coin_balances_daily; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.address_coin_balances_daily (
    address_hash bytea NOT NULL,
    day date NOT NULL,
    value numeric(100,0) DEFAULT NULL::numeric,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.address_coin_balances_daily OWNER TO postgres;

--
-- Name: address_contract_code_fetch_attempts; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.address_contract_code_fetch_attempts (
    address_hash bytea NOT NULL,
    retries_number smallint,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.address_contract_code_fetch_attempts OWNER TO postgres;

--
-- Name: address_current_token_balances; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.address_current_token_balances (
    id bigint NOT NULL,
    address_hash bytea NOT NULL,
    block_number bigint NOT NULL,
    token_contract_address_hash bytea NOT NULL,
    value numeric,
    value_fetched_at timestamp without time zone,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    old_value numeric,
    token_id numeric(78,0),
    token_type character varying(255)
);


ALTER TABLE public.address_current_token_balances OWNER TO postgres;

--
-- Name: address_current_token_balances_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.address_current_token_balances_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.address_current_token_balances_id_seq OWNER TO postgres;

--
-- Name: address_current_token_balances_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.address_current_token_balances_id_seq OWNED BY public.address_current_token_balances.id;


--
-- Name: address_names; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.address_names (
    address_hash bytea NOT NULL,
    name character varying(255) NOT NULL,
    "primary" boolean DEFAULT false NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    metadata jsonb,
    id integer NOT NULL
);


ALTER TABLE public.address_names OWNER TO postgres;

--
-- Name: address_names_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.address_names_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.address_names_id_seq OWNER TO postgres;

--
-- Name: address_names_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.address_names_id_seq OWNED BY public.address_names.id;


--
-- Name: address_tags; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.address_tags (
    id integer NOT NULL,
    label character varying(255) NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    display_name character varying(255)
);


ALTER TABLE public.address_tags OWNER TO postgres;

--
-- Name: address_tags_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.address_tags_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.address_tags_id_seq OWNER TO postgres;

--
-- Name: address_tags_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.address_tags_id_seq OWNED BY public.address_tags.id;


--
-- Name: address_to_tags; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.address_to_tags (
    id bigint NOT NULL,
    address_hash bytea NOT NULL,
    tag_id integer NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.address_to_tags OWNER TO postgres;

--
-- Name: address_to_tags_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.address_to_tags_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.address_to_tags_id_seq OWNER TO postgres;

--
-- Name: address_to_tags_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.address_to_tags_id_seq OWNED BY public.address_to_tags.id;


--
-- Name: address_token_balances; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.address_token_balances (
    id bigint NOT NULL,
    address_hash bytea NOT NULL,
    block_number bigint NOT NULL,
    token_contract_address_hash bytea NOT NULL,
    value numeric,
    value_fetched_at timestamp without time zone,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    token_id numeric(78,0),
    token_type character varying(255),
    refetch_after timestamp without time zone,
    retries_count smallint
);


ALTER TABLE public.address_token_balances OWNER TO postgres;

--
-- Name: address_token_balances_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.address_token_balances_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.address_token_balances_id_seq OWNER TO postgres;

--
-- Name: address_token_balances_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.address_token_balances_id_seq OWNED BY public.address_token_balances.id;


--
-- Name: addresses; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.addresses (
    fetched_coin_balance numeric(100,0),
    fetched_coin_balance_block_number bigint,
    hash bytea NOT NULL,
    contract_code bytea,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    nonce integer,
    decompiled boolean,
    verified boolean,
    gas_used bigint,
    transactions_count integer,
    token_transfers_count integer
);


ALTER TABLE public.addresses OWNER TO postgres;

--
-- Name: administrators; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.administrators (
    id bigint NOT NULL,
    role character varying(255) NOT NULL,
    user_id bigint NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.administrators OWNER TO postgres;

--
-- Name: administrators_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.administrators_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.administrators_id_seq OWNER TO postgres;

--
-- Name: administrators_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.administrators_id_seq OWNED BY public.administrators.id;


--
-- Name: block_rewards; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.block_rewards (
    address_hash bytea NOT NULL,
    address_type character varying(255) NOT NULL,
    block_hash bytea NOT NULL,
    reward numeric(100,0),
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL
);


ALTER TABLE public.block_rewards OWNER TO postgres;

--
-- Name: block_second_degree_relations; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.block_second_degree_relations (
    nephew_hash bytea NOT NULL,
    uncle_hash bytea NOT NULL,
    uncle_fetched_at timestamp without time zone,
    index integer
);


ALTER TABLE public.block_second_degree_relations OWNER TO postgres;

--
-- Name: blocks; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.blocks (
    consensus boolean NOT NULL,
    difficulty numeric(50,0),
    gas_limit numeric(100,0) NOT NULL,
    gas_used numeric(100,0) NOT NULL,
    hash bytea NOT NULL,
    miner_hash bytea NOT NULL,
    nonce bytea NOT NULL,
    number bigint NOT NULL,
    parent_hash bytea NOT NULL,
    size integer,
    "timestamp" timestamp without time zone NOT NULL,
    total_difficulty numeric(50,0),
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    refetch_needed boolean DEFAULT false,
    base_fee_per_gas numeric(100,0),
    is_empty boolean
);


ALTER TABLE public.blocks OWNER TO postgres;

--
-- Name: constants; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.constants (
    key character varying(255) NOT NULL,
    value character varying(255),
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.constants OWNER TO postgres;

--
-- Name: contract_methods; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.contract_methods (
    id bigint NOT NULL,
    identifier integer NOT NULL,
    abi jsonb NOT NULL,
    type character varying(255) NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.contract_methods OWNER TO postgres;

--
-- Name: contract_methods_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.contract_methods_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.contract_methods_id_seq OWNER TO postgres;

--
-- Name: contract_methods_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.contract_methods_id_seq OWNED BY public.contract_methods.id;


--
-- Name: contract_verification_status; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.contract_verification_status (
    uid character varying(64) NOT NULL,
    status smallint NOT NULL,
    address_hash bytea NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.contract_verification_status OWNER TO postgres;

--
-- Name: decompiled_smart_contracts; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.decompiled_smart_contracts (
    id bigint NOT NULL,
    decompiler_version character varying(255) NOT NULL,
    decompiled_source_code text NOT NULL,
    address_hash bytea NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.decompiled_smart_contracts OWNER TO postgres;

--
-- Name: decompiled_smart_contracts_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.decompiled_smart_contracts_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.decompiled_smart_contracts_id_seq OWNER TO postgres;

--
-- Name: decompiled_smart_contracts_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.decompiled_smart_contracts_id_seq OWNED BY public.decompiled_smart_contracts.id;


--
-- Name: emission_rewards; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.emission_rewards (
    block_range int8range NOT NULL,
    reward numeric
);


ALTER TABLE public.emission_rewards OWNER TO postgres;

--
-- Name: event_notifications; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.event_notifications (
    id bigint NOT NULL,
    data text NOT NULL
);


ALTER TABLE public.event_notifications OWNER TO postgres;

--
-- Name: event_notifications_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.event_notifications_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.event_notifications_id_seq OWNER TO postgres;

--
-- Name: event_notifications_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.event_notifications_id_seq OWNED BY public.event_notifications.id;


--
-- Name: internal_transactions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.internal_transactions (
    call_type character varying(255),
    created_contract_code bytea,
    error character varying(255),
    gas numeric(100,0),
    gas_used numeric(100,0),
    index integer NOT NULL,
    init bytea,
    input bytea,
    output bytea,
    trace_address integer[] NOT NULL,
    type character varying(255) NOT NULL,
    value numeric(100,0) NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    created_contract_address_hash bytea,
    from_address_hash bytea,
    to_address_hash bytea,
    transaction_hash bytea NOT NULL,
    block_number integer,
    transaction_index integer,
    block_hash bytea NOT NULL,
    block_index integer NOT NULL,
    CONSTRAINT call_has_error_or_result CHECK ((((type)::text <> 'call'::text) OR ((gas IS NOT NULL) AND (((error IS NULL) AND (gas_used IS NOT NULL) AND (output IS NOT NULL)) OR ((error IS NOT NULL) AND (output IS NULL)))))),
    CONSTRAINT create_has_error_or_result CHECK ((((type)::text <> 'create'::text) OR ((gas IS NOT NULL) AND (((error IS NULL) AND (created_contract_address_hash IS NOT NULL) AND (created_contract_code IS NOT NULL) AND (gas_used IS NOT NULL)) OR ((error IS NOT NULL) AND (created_contract_address_hash IS NULL) AND (created_contract_code IS NULL) AND (gas_used IS NULL))))))
);


ALTER TABLE public.internal_transactions OWNER TO postgres;

--
-- Name: last_fetched_counters; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.last_fetched_counters (
    counter_type character varying(255) NOT NULL,
    value numeric(100,0),
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.last_fetched_counters OWNER TO postgres;

--
-- Name: logs; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.logs (
    data bytea NOT NULL,
    index integer NOT NULL,
    first_topic bytea,
    second_topic bytea,
    third_topic bytea,
    fourth_topic bytea,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    address_hash bytea,
    transaction_hash bytea NOT NULL,
    block_hash bytea NOT NULL,
    block_number integer
);


ALTER TABLE public.logs OWNER TO postgres;

--
-- Name: market_history; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.market_history (
    id bigint NOT NULL,
    date date NOT NULL,
    closing_price numeric,
    opening_price numeric,
    market_cap numeric,
    tvl numeric,
    secondary_coin boolean DEFAULT false
);


ALTER TABLE public.market_history OWNER TO postgres;

--
-- Name: market_history_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.market_history_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.market_history_id_seq OWNER TO postgres;

--
-- Name: market_history_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.market_history_id_seq OWNED BY public.market_history.id;


--
-- Name: massive_blocks; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.massive_blocks (
    number bigint NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.massive_blocks OWNER TO postgres;

--
-- Name: migrations_status; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.migrations_status (
    migration_name character varying(255) NOT NULL,
    status character varying(255),
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    meta jsonb
);


ALTER TABLE public.migrations_status OWNER TO postgres;

--
-- Name: missing_balance_of_tokens; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.missing_balance_of_tokens (
    token_contract_address_hash bytea NOT NULL,
    block_number bigint,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    currently_implemented boolean
);


ALTER TABLE public.missing_balance_of_tokens OWNER TO postgres;

--
-- Name: missing_block_ranges; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.missing_block_ranges (
    id bigint NOT NULL,
    from_number integer,
    to_number integer
);


ALTER TABLE public.missing_block_ranges OWNER TO postgres;

--
-- Name: missing_block_ranges_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.missing_block_ranges_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.missing_block_ranges_id_seq OWNER TO postgres;

--
-- Name: missing_block_ranges_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.missing_block_ranges_id_seq OWNED BY public.missing_block_ranges.id;


--
-- Name: pending_block_operations; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.pending_block_operations (
    block_hash bytea NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    block_number integer
);


ALTER TABLE public.pending_block_operations OWNER TO postgres;

--
-- Name: proxy_implementations; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.proxy_implementations (
    proxy_address_hash bytea NOT NULL,
    address_hashes bytea[] NOT NULL,
    names character varying(255)[] NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    proxy_type public.proxy_type
);


ALTER TABLE public.proxy_implementations OWNER TO postgres;

--
-- Name: proxy_smart_contract_verification_statuses; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.proxy_smart_contract_verification_statuses (
    uid character varying(64) NOT NULL,
    status smallint NOT NULL,
    contract_address_hash bytea,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.proxy_smart_contract_verification_statuses OWNER TO postgres;

--
-- Name: scam_address_badge_mappings; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.scam_address_badge_mappings (
    address_hash bytea NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.scam_address_badge_mappings OWNER TO postgres;

--
-- Name: schema_migrations; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.schema_migrations (
    version bigint NOT NULL,
    inserted_at timestamp(0) without time zone
);


ALTER TABLE public.schema_migrations OWNER TO postgres;

--
-- Name: signed_authorizations; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.signed_authorizations (
    transaction_hash bytea NOT NULL,
    index integer NOT NULL,
    chain_id bigint NOT NULL,
    address bytea NOT NULL,
    nonce integer NOT NULL,
    v integer NOT NULL,
    r numeric(100,0) NOT NULL,
    s numeric(100,0) NOT NULL,
    authority bytea,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.signed_authorizations OWNER TO postgres;

--
-- Name: smart_contract_audit_reports; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.smart_contract_audit_reports (
    id bigint NOT NULL,
    address_hash bytea NOT NULL,
    is_approved boolean DEFAULT false,
    submitter_name character varying(255) NOT NULL,
    submitter_email character varying(255) NOT NULL,
    is_project_owner boolean DEFAULT false,
    project_name character varying(255) NOT NULL,
    project_url character varying(255) NOT NULL,
    audit_company_name character varying(255) NOT NULL,
    audit_report_url character varying(255) NOT NULL,
    audit_publish_date date NOT NULL,
    request_id character varying(255),
    comment text,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.smart_contract_audit_reports OWNER TO postgres;

--
-- Name: smart_contract_audit_reports_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.smart_contract_audit_reports_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.smart_contract_audit_reports_id_seq OWNER TO postgres;

--
-- Name: smart_contract_audit_reports_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.smart_contract_audit_reports_id_seq OWNED BY public.smart_contract_audit_reports.id;


--
-- Name: smart_contracts; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.smart_contracts (
    id bigint NOT NULL,
    name character varying(255) NOT NULL,
    compiler_version character varying(255) NOT NULL,
    optimization boolean NOT NULL,
    contract_source_code text NOT NULL,
    abi jsonb,
    address_hash bytea NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    constructor_arguments text,
    optimization_runs bigint,
    evm_version character varying(255),
    external_libraries jsonb[] DEFAULT ARRAY[]::jsonb[],
    verified_via_sourcify boolean,
    is_vyper_contract boolean,
    partially_verified boolean,
    file_path text,
    is_changed_bytecode boolean DEFAULT false,
    bytecode_checked_at timestamp without time zone DEFAULT ((now() AT TIME ZONE 'utc'::text) - '1 day'::interval),
    contract_code_md5 character varying(255) NOT NULL,
    compiler_settings jsonb,
    verified_via_eth_bytecode_db boolean,
    license_type smallint DEFAULT 1 NOT NULL,
    verified_via_verifier_alliance boolean,
    certified boolean,
    is_blueprint boolean,
    language smallint
);


ALTER TABLE public.smart_contracts OWNER TO postgres;

--
-- Name: smart_contracts_additional_sources; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.smart_contracts_additional_sources (
    id bigint NOT NULL,
    file_name character varying(255) NOT NULL,
    contract_source_code text NOT NULL,
    address_hash bytea NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.smart_contracts_additional_sources OWNER TO postgres;

--
-- Name: smart_contracts_additional_sources_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.smart_contracts_additional_sources_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.smart_contracts_additional_sources_id_seq OWNER TO postgres;

--
-- Name: smart_contracts_additional_sources_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.smart_contracts_additional_sources_id_seq OWNED BY public.smart_contracts_additional_sources.id;


--
-- Name: smart_contracts_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.smart_contracts_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.smart_contracts_id_seq OWNER TO postgres;

--
-- Name: smart_contracts_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.smart_contracts_id_seq OWNED BY public.smart_contracts.id;


--
-- Name: token_instance_metadata_refetch_attempts; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.token_instance_metadata_refetch_attempts (
    token_contract_address_hash bytea NOT NULL,
    token_id numeric(78,0) NOT NULL,
    retries_number smallint,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.token_instance_metadata_refetch_attempts OWNER TO postgres;

--
-- Name: token_instances; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.token_instances (
    token_id numeric(78,0) NOT NULL,
    token_contract_address_hash bytea NOT NULL,
    metadata jsonb,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    error character varying(255),
    owner_address_hash bytea,
    owner_updated_at_block bigint,
    owner_updated_at_log_index integer,
    refetch_after timestamp without time zone,
    retries_count smallint DEFAULT 0 NOT NULL,
    thumbnails jsonb,
    media_type character varying(255),
    cdn_upload_error character varying(255),
    is_banned boolean DEFAULT false
);


ALTER TABLE public.token_instances OWNER TO postgres;

--
-- Name: token_transfer_token_id_migrator_progress; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.token_transfer_token_id_migrator_progress (
    id bigint NOT NULL,
    last_processed_block_number integer,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.token_transfer_token_id_migrator_progress OWNER TO postgres;

--
-- Name: token_transfer_token_id_migrator_progress_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.token_transfer_token_id_migrator_progress_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.token_transfer_token_id_migrator_progress_id_seq OWNER TO postgres;

--
-- Name: token_transfer_token_id_migrator_progress_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.token_transfer_token_id_migrator_progress_id_seq OWNED BY public.token_transfer_token_id_migrator_progress.id;


--
-- Name: token_transfers; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.token_transfers (
    transaction_hash bytea NOT NULL,
    log_index integer NOT NULL,
    from_address_hash bytea NOT NULL,
    to_address_hash bytea NOT NULL,
    amount numeric,
    token_contract_address_hash bytea NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    block_number integer,
    block_hash bytea NOT NULL,
    amounts numeric[],
    token_ids numeric(78,0)[],
    token_type character varying(255),
    block_consensus boolean DEFAULT true
);


ALTER TABLE public.token_transfers OWNER TO postgres;

--
-- Name: tokens; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.tokens (
    name text,
    symbol text,
    total_supply numeric,
    decimals numeric,
    type character varying(255) NOT NULL,
    cataloged boolean DEFAULT false,
    contract_address_hash bytea NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    holder_count integer,
    skip_metadata boolean,
    fiat_value numeric,
    circulating_market_cap numeric,
    total_supply_updated_at_block bigint,
    icon_url character varying(255),
    is_verified_via_admin_panel boolean DEFAULT false,
    volume_24h numeric,
    metadata_updated_at timestamp without time zone
);


ALTER TABLE public.tokens OWNER TO postgres;

--
-- Name: transaction_actions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.transaction_actions (
    hash bytea NOT NULL,
    protocol public.transaction_actions_protocol NOT NULL,
    data jsonb DEFAULT '{}'::jsonb NOT NULL,
    type public.transaction_actions_type NOT NULL,
    log_index integer NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.transaction_actions OWNER TO postgres;

--
-- Name: transaction_forks; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.transaction_forks (
    hash bytea NOT NULL,
    index integer NOT NULL,
    uncle_hash bytea NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.transaction_forks OWNER TO postgres;

--
-- Name: transaction_stats; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.transaction_stats (
    id bigint NOT NULL,
    date date,
    number_of_transactions integer,
    gas_used numeric(100,0),
    total_fee numeric(100,0)
);


ALTER TABLE public.transaction_stats OWNER TO postgres;

--
-- Name: transaction_stats_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.transaction_stats_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.transaction_stats_id_seq OWNER TO postgres;

--
-- Name: transaction_stats_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.transaction_stats_id_seq OWNED BY public.transaction_stats.id;


--
-- Name: transactions; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.transactions (
    cumulative_gas_used numeric(100,0),
    error character varying(255),
    gas numeric(100,0) NOT NULL,
    gas_price numeric(100,0),
    gas_used numeric(100,0),
    hash bytea NOT NULL,
    index integer,
    input bytea NOT NULL,
    nonce integer NOT NULL,
    r numeric(100,0) NOT NULL,
    s numeric(100,0) NOT NULL,
    status integer,
    v numeric(100,0) NOT NULL,
    value numeric(100,0) NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    block_hash bytea,
    block_number integer,
    from_address_hash bytea NOT NULL,
    to_address_hash bytea,
    created_contract_address_hash bytea,
    created_contract_code_indexed_at timestamp without time zone,
    earliest_processing_start timestamp without time zone,
    old_block_hash bytea,
    revert_reason text,
    max_priority_fee_per_gas numeric(100,0),
    max_fee_per_gas numeric(100,0),
    type integer,
    has_error_in_internal_transactions boolean,
    block_timestamp timestamp without time zone,
    block_consensus boolean DEFAULT true,
    CONSTRAINT collated_block_number CHECK (((block_hash IS NULL) OR (block_number IS NOT NULL))),
    CONSTRAINT collated_cumalative_gas_used CHECK (((block_hash IS NULL) OR (cumulative_gas_used IS NOT NULL))),
    CONSTRAINT collated_gas_price CHECK (((block_hash IS NULL) OR (gas_price IS NOT NULL))),
    CONSTRAINT collated_gas_used CHECK (((block_hash IS NULL) OR (gas_used IS NOT NULL))),
    CONSTRAINT collated_index CHECK (((block_hash IS NULL) OR (index IS NOT NULL))),
    CONSTRAINT error CHECK (((status = 0) OR ((status <> 0) AND (error IS NULL)))),
    CONSTRAINT pending_block_number CHECK (((block_hash IS NOT NULL) OR (block_number IS NULL))),
    CONSTRAINT pending_cumalative_gas_used CHECK (((block_hash IS NOT NULL) OR (cumulative_gas_used IS NULL))),
    CONSTRAINT pending_gas_used CHECK (((block_hash IS NOT NULL) OR (gas_used IS NULL))),
    CONSTRAINT pending_index CHECK (((block_hash IS NOT NULL) OR (index IS NULL))),
    CONSTRAINT status CHECK ((((block_hash IS NULL) AND (status IS NULL)) OR (block_hash IS NOT NULL) OR ((status = 0) AND ((error)::text = 'dropped/replaced'::text))))
);


ALTER TABLE public.transactions OWNER TO postgres;

--
-- Name: user_contacts; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.user_contacts (
    id bigint NOT NULL,
    email public.citext NOT NULL,
    user_id bigint NOT NULL,
    "primary" boolean DEFAULT false,
    verified boolean DEFAULT false,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.user_contacts OWNER TO postgres;

--
-- Name: user_contacts_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.user_contacts_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.user_contacts_id_seq OWNER TO postgres;

--
-- Name: user_contacts_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.user_contacts_id_seq OWNED BY public.user_contacts.id;


--
-- Name: user_operations; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.user_operations (
    hash bytea NOT NULL,
    sender bytea NOT NULL,
    nonce bytea NOT NULL,
    init_code bytea,
    call_data bytea NOT NULL,
    call_gas_limit numeric(100,0) NOT NULL,
    verification_gas_limit numeric(100,0) NOT NULL,
    pre_verification_gas numeric(100,0) NOT NULL,
    max_fee_per_gas numeric(100,0) NOT NULL,
    max_priority_fee_per_gas numeric(100,0) NOT NULL,
    paymaster_and_data bytea,
    signature bytea NOT NULL,
    aggregator bytea,
    aggregator_signature bytea,
    entry_point bytea NOT NULL,
    transaction_hash bytea NOT NULL,
    block_number integer NOT NULL,
    block_hash bytea NOT NULL,
    bundle_index integer NOT NULL,
    index integer NOT NULL,
    user_logs_start_index integer NOT NULL,
    user_logs_count integer NOT NULL,
    bundler bytea NOT NULL,
    factory bytea,
    paymaster bytea,
    status boolean NOT NULL,
    revert_reason bytea,
    gas numeric(100,0) NOT NULL,
    gas_price numeric(100,0) NOT NULL,
    gas_used numeric(100,0) NOT NULL,
    sponsor_type public.sponsor_type NOT NULL,
    inserted_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL,
    entry_point_version public.entry_point_version DEFAULT 'v0.6'::public.entry_point_version NOT NULL
);


ALTER TABLE public.user_operations OWNER TO postgres;

--
-- Name: user_ops_indexer_migrations; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.user_ops_indexer_migrations (
    version character varying NOT NULL,
    applied_at bigint NOT NULL
);


ALTER TABLE public.user_ops_indexer_migrations OWNER TO postgres;

--
-- Name: users; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.users (
    id bigint NOT NULL,
    username public.citext NOT NULL,
    password_hash character varying(255) NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.users OWNER TO postgres;

--
-- Name: users_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.users_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.users_id_seq OWNER TO postgres;

--
-- Name: users_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.users_id_seq OWNED BY public.users.id;


--
-- Name: validators; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.validators (
    address_hash bytea NOT NULL,
    is_validator boolean,
    payout_key_hash bytea,
    info_updated_at_block bigint,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


ALTER TABLE public.validators OWNER TO postgres;

--
-- Name: withdrawals; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.withdrawals (
    index integer NOT NULL,
    validator_index integer NOT NULL,
    amount numeric(100,0) NOT NULL,
    inserted_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    address_hash bytea NOT NULL,
    block_hash bytea NOT NULL
);


ALTER TABLE public.withdrawals OWNER TO postgres;

--
-- Name: account_api_plans id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_api_plans ALTER COLUMN id SET DEFAULT nextval('public.account_api_plans_id_seq'::regclass);


--
-- Name: account_custom_abis id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_custom_abis ALTER COLUMN id SET DEFAULT nextval('public.account_custom_abis_id_seq'::regclass);


--
-- Name: account_identities id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_identities ALTER COLUMN id SET DEFAULT nextval('public.account_identities_id_seq'::regclass);


--
-- Name: account_public_tags_requests id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_public_tags_requests ALTER COLUMN id SET DEFAULT nextval('public.account_public_tags_requests_id_seq'::regclass);


--
-- Name: account_tag_addresses id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_tag_addresses ALTER COLUMN id SET DEFAULT nextval('public.account_tag_addresses_id_seq'::regclass);


--
-- Name: account_tag_transactions id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_tag_transactions ALTER COLUMN id SET DEFAULT nextval('public.account_tag_transactions_id_seq'::regclass);


--
-- Name: account_watchlist_addresses id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_watchlist_addresses ALTER COLUMN id SET DEFAULT nextval('public.account_watchlist_addresses_id_seq'::regclass);


--
-- Name: account_watchlist_notifications id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_watchlist_notifications ALTER COLUMN id SET DEFAULT nextval('public.account_watchlist_notifications_id_seq'::regclass);


--
-- Name: account_watchlist_notifications watchlist_id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_watchlist_notifications ALTER COLUMN watchlist_id SET DEFAULT nextval('public.account_watchlist_notifications_watchlist_id_seq'::regclass);


--
-- Name: account_watchlists id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_watchlists ALTER COLUMN id SET DEFAULT nextval('public.account_watchlists_id_seq'::regclass);


--
-- Name: address_current_token_balances id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_current_token_balances ALTER COLUMN id SET DEFAULT nextval('public.address_current_token_balances_id_seq'::regclass);


--
-- Name: address_names id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_names ALTER COLUMN id SET DEFAULT nextval('public.address_names_id_seq'::regclass);


--
-- Name: address_tags id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_tags ALTER COLUMN id SET DEFAULT nextval('public.address_tags_id_seq'::regclass);


--
-- Name: address_to_tags id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_to_tags ALTER COLUMN id SET DEFAULT nextval('public.address_to_tags_id_seq'::regclass);


--
-- Name: address_token_balances id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_token_balances ALTER COLUMN id SET DEFAULT nextval('public.address_token_balances_id_seq'::regclass);


--
-- Name: administrators id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.administrators ALTER COLUMN id SET DEFAULT nextval('public.administrators_id_seq'::regclass);


--
-- Name: contract_methods id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.contract_methods ALTER COLUMN id SET DEFAULT nextval('public.contract_methods_id_seq'::regclass);


--
-- Name: decompiled_smart_contracts id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.decompiled_smart_contracts ALTER COLUMN id SET DEFAULT nextval('public.decompiled_smart_contracts_id_seq'::regclass);


--
-- Name: event_notifications id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.event_notifications ALTER COLUMN id SET DEFAULT nextval('public.event_notifications_id_seq'::regclass);


--
-- Name: market_history id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.market_history ALTER COLUMN id SET DEFAULT nextval('public.market_history_id_seq'::regclass);


--
-- Name: missing_block_ranges id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.missing_block_ranges ALTER COLUMN id SET DEFAULT nextval('public.missing_block_ranges_id_seq'::regclass);


--
-- Name: smart_contract_audit_reports id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.smart_contract_audit_reports ALTER COLUMN id SET DEFAULT nextval('public.smart_contract_audit_reports_id_seq'::regclass);


--
-- Name: smart_contracts id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.smart_contracts ALTER COLUMN id SET DEFAULT nextval('public.smart_contracts_id_seq'::regclass);


--
-- Name: smart_contracts_additional_sources id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.smart_contracts_additional_sources ALTER COLUMN id SET DEFAULT nextval('public.smart_contracts_additional_sources_id_seq'::regclass);


--
-- Name: token_transfer_token_id_migrator_progress id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.token_transfer_token_id_migrator_progress ALTER COLUMN id SET DEFAULT nextval('public.token_transfer_token_id_migrator_progress_id_seq'::regclass);


--
-- Name: transaction_stats id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transaction_stats ALTER COLUMN id SET DEFAULT nextval('public.transaction_stats_id_seq'::regclass);


--
-- Name: user_contacts id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.user_contacts ALTER COLUMN id SET DEFAULT nextval('public.user_contacts_id_seq'::regclass);


--
-- Name: users id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.users ALTER COLUMN id SET DEFAULT nextval('public.users_id_seq'::regclass);


--
-- Name: account_api_keys account_api_keys_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_api_keys
    ADD CONSTRAINT account_api_keys_pkey PRIMARY KEY (value);


--
-- Name: account_api_plans account_api_plans_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_api_plans
    ADD CONSTRAINT account_api_plans_pkey PRIMARY KEY (id);


--
-- Name: account_custom_abis account_custom_abis_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_custom_abis
    ADD CONSTRAINT account_custom_abis_pkey PRIMARY KEY (id);


--
-- Name: account_identities account_identities_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_identities
    ADD CONSTRAINT account_identities_pkey PRIMARY KEY (id);


--
-- Name: account_public_tags_requests account_public_tags_requests_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_public_tags_requests
    ADD CONSTRAINT account_public_tags_requests_pkey PRIMARY KEY (id);


--
-- Name: account_tag_addresses account_tag_addresses_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_tag_addresses
    ADD CONSTRAINT account_tag_addresses_pkey PRIMARY KEY (id);


--
-- Name: account_tag_transactions account_tag_transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_tag_transactions
    ADD CONSTRAINT account_tag_transactions_pkey PRIMARY KEY (id);


--
-- Name: account_watchlist_addresses account_watchlist_addresses_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_watchlist_addresses
    ADD CONSTRAINT account_watchlist_addresses_pkey PRIMARY KEY (id);


--
-- Name: account_watchlist_notifications account_watchlist_notifications_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_watchlist_notifications
    ADD CONSTRAINT account_watchlist_notifications_pkey PRIMARY KEY (id);


--
-- Name: account_watchlists account_watchlists_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_watchlists
    ADD CONSTRAINT account_watchlists_pkey PRIMARY KEY (id);


--
-- Name: address_coin_balances_daily address_coin_balances_daily_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_coin_balances_daily
    ADD CONSTRAINT address_coin_balances_daily_pkey PRIMARY KEY (address_hash, day);


--
-- Name: address_coin_balances address_coin_balances_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_coin_balances
    ADD CONSTRAINT address_coin_balances_pkey PRIMARY KEY (address_hash, block_number);


--
-- Name: address_contract_code_fetch_attempts address_contract_code_fetch_attempts_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_contract_code_fetch_attempts
    ADD CONSTRAINT address_contract_code_fetch_attempts_pkey PRIMARY KEY (address_hash);


--
-- Name: address_current_token_balances address_current_token_balances_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_current_token_balances
    ADD CONSTRAINT address_current_token_balances_pkey PRIMARY KEY (id);


--
-- Name: address_names address_names_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_names
    ADD CONSTRAINT address_names_pkey PRIMARY KEY (id);


--
-- Name: address_tags address_tags_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_tags
    ADD CONSTRAINT address_tags_pkey PRIMARY KEY (label);


--
-- Name: address_to_tags address_to_tags_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_to_tags
    ADD CONSTRAINT address_to_tags_pkey PRIMARY KEY (id);


--
-- Name: address_token_balances address_token_balances_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_token_balances
    ADD CONSTRAINT address_token_balances_pkey PRIMARY KEY (id);


--
-- Name: addresses addresses_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.addresses
    ADD CONSTRAINT addresses_pkey PRIMARY KEY (hash);


--
-- Name: administrators administrators_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.administrators
    ADD CONSTRAINT administrators_pkey PRIMARY KEY (id);


--
-- Name: block_rewards block_rewards_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.block_rewards
    ADD CONSTRAINT block_rewards_pkey PRIMARY KEY (address_hash, block_hash, address_type);


--
-- Name: block_second_degree_relations block_second_degree_relations_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.block_second_degree_relations
    ADD CONSTRAINT block_second_degree_relations_pkey PRIMARY KEY (nephew_hash, uncle_hash);


--
-- Name: blocks blocks_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.blocks
    ADD CONSTRAINT blocks_pkey PRIMARY KEY (hash);


--
-- Name: internal_transactions call_has_call_type; Type: CHECK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE public.internal_transactions
    ADD CONSTRAINT call_has_call_type CHECK ((((type)::text <> 'call'::text) OR (call_type IS NOT NULL))) NOT VALID;


--
-- Name: internal_transactions call_has_input; Type: CHECK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE public.internal_transactions
    ADD CONSTRAINT call_has_input CHECK ((((type)::text <> 'call'::text) OR (input IS NOT NULL))) NOT VALID;


--
-- Name: constants constants_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.constants
    ADD CONSTRAINT constants_pkey PRIMARY KEY (key);


--
-- Name: contract_methods contract_methods_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.contract_methods
    ADD CONSTRAINT contract_methods_pkey PRIMARY KEY (id);


--
-- Name: contract_verification_status contract_verification_status_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.contract_verification_status
    ADD CONSTRAINT contract_verification_status_pkey PRIMARY KEY (uid);


--
-- Name: internal_transactions create_has_init; Type: CHECK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE public.internal_transactions
    ADD CONSTRAINT create_has_init CHECK ((((type)::text <> 'create'::text) OR (init IS NOT NULL))) NOT VALID;


--
-- Name: decompiled_smart_contracts decompiled_smart_contracts_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.decompiled_smart_contracts
    ADD CONSTRAINT decompiled_smart_contracts_pkey PRIMARY KEY (id);


--
-- Name: emission_rewards emission_rewards_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.emission_rewards
    ADD CONSTRAINT emission_rewards_pkey PRIMARY KEY (block_range);


--
-- Name: event_notifications event_notifications_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.event_notifications
    ADD CONSTRAINT event_notifications_pkey PRIMARY KEY (id);


--
-- Name: internal_transactions internal_transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.internal_transactions
    ADD CONSTRAINT internal_transactions_pkey PRIMARY KEY (block_hash, block_index);


--
-- Name: last_fetched_counters last_fetched_counters_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.last_fetched_counters
    ADD CONSTRAINT last_fetched_counters_pkey PRIMARY KEY (counter_type);


--
-- Name: logs logs_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.logs
    ADD CONSTRAINT logs_pkey PRIMARY KEY (transaction_hash, block_hash, index);


--
-- Name: market_history market_history_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.market_history
    ADD CONSTRAINT market_history_pkey PRIMARY KEY (id);


--
-- Name: massive_blocks massive_blocks_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.massive_blocks
    ADD CONSTRAINT massive_blocks_pkey PRIMARY KEY (number);


--
-- Name: migrations_status migrations_status_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.migrations_status
    ADD CONSTRAINT migrations_status_pkey PRIMARY KEY (migration_name);


--
-- Name: missing_balance_of_tokens missing_balance_of_tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.missing_balance_of_tokens
    ADD CONSTRAINT missing_balance_of_tokens_pkey PRIMARY KEY (token_contract_address_hash);


--
-- Name: missing_block_ranges missing_block_ranges_no_overlap; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.missing_block_ranges
    ADD CONSTRAINT missing_block_ranges_no_overlap EXCLUDE USING gist (int4range(to_number, from_number, '[]'::text) WITH &&);


--
-- Name: missing_block_ranges missing_block_ranges_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.missing_block_ranges
    ADD CONSTRAINT missing_block_ranges_pkey PRIMARY KEY (id);


--
-- Name: emission_rewards no_overlapping_ranges; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.emission_rewards
    ADD CONSTRAINT no_overlapping_ranges EXCLUDE USING gist (block_range WITH &&);


--
-- Name: pending_block_operations pending_block_operations_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.pending_block_operations
    ADD CONSTRAINT pending_block_operations_pkey PRIMARY KEY (block_hash);


--
-- Name: proxy_implementations proxy_implementations_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.proxy_implementations
    ADD CONSTRAINT proxy_implementations_pkey PRIMARY KEY (proxy_address_hash);


--
-- Name: proxy_smart_contract_verification_statuses proxy_smart_contract_verification_statuses_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.proxy_smart_contract_verification_statuses
    ADD CONSTRAINT proxy_smart_contract_verification_statuses_pkey PRIMARY KEY (uid);


--
-- Name: scam_address_badge_mappings scam_address_badge_mappings_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.scam_address_badge_mappings
    ADD CONSTRAINT scam_address_badge_mappings_pkey PRIMARY KEY (address_hash);


--
-- Name: schema_migrations schema_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.schema_migrations
    ADD CONSTRAINT schema_migrations_pkey PRIMARY KEY (version);


--
-- Name: internal_transactions selfdestruct_has_from_and_to_address; Type: CHECK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE public.internal_transactions
    ADD CONSTRAINT selfdestruct_has_from_and_to_address CHECK ((((type)::text <> 'selfdestruct'::text) OR ((from_address_hash IS NOT NULL) AND (gas IS NULL) AND (to_address_hash IS NOT NULL)))) NOT VALID;


--
-- Name: signed_authorizations signed_authorizations_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.signed_authorizations
    ADD CONSTRAINT signed_authorizations_pkey PRIMARY KEY (transaction_hash, index);


--
-- Name: smart_contract_audit_reports smart_contract_audit_reports_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.smart_contract_audit_reports
    ADD CONSTRAINT smart_contract_audit_reports_pkey PRIMARY KEY (id);


--
-- Name: smart_contracts_additional_sources smart_contracts_additional_sources_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.smart_contracts_additional_sources
    ADD CONSTRAINT smart_contracts_additional_sources_pkey PRIMARY KEY (id);


--
-- Name: smart_contracts smart_contracts_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.smart_contracts
    ADD CONSTRAINT smart_contracts_pkey PRIMARY KEY (id);


--
-- Name: token_instance_metadata_refetch_attempts token_instance_metadata_refetch_attempts_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.token_instance_metadata_refetch_attempts
    ADD CONSTRAINT token_instance_metadata_refetch_attempts_pkey PRIMARY KEY (token_contract_address_hash, token_id);


--
-- Name: token_instances token_instances_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.token_instances
    ADD CONSTRAINT token_instances_pkey PRIMARY KEY (token_id, token_contract_address_hash);


--
-- Name: token_transfer_token_id_migrator_progress token_transfer_token_id_migrator_progress_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.token_transfer_token_id_migrator_progress
    ADD CONSTRAINT token_transfer_token_id_migrator_progress_pkey PRIMARY KEY (id);


--
-- Name: token_transfers token_transfers_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.token_transfers
    ADD CONSTRAINT token_transfers_pkey PRIMARY KEY (transaction_hash, block_hash, log_index);


--
-- Name: tokens tokens_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.tokens
    ADD CONSTRAINT tokens_pkey PRIMARY KEY (contract_address_hash);


--
-- Name: transaction_actions transaction_actions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transaction_actions
    ADD CONSTRAINT transaction_actions_pkey PRIMARY KEY (hash, log_index);


--
-- Name: transaction_forks transaction_forks_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transaction_forks
    ADD CONSTRAINT transaction_forks_pkey PRIMARY KEY (uncle_hash, index);


--
-- Name: transaction_stats transaction_stats_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transaction_stats
    ADD CONSTRAINT transaction_stats_pkey PRIMARY KEY (id);


--
-- Name: transactions transactions_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transactions
    ADD CONSTRAINT transactions_pkey PRIMARY KEY (hash);


--
-- Name: user_contacts user_contacts_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.user_contacts
    ADD CONSTRAINT user_contacts_pkey PRIMARY KEY (id);


--
-- Name: user_operations user_operations_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.user_operations
    ADD CONSTRAINT user_operations_pkey PRIMARY KEY (hash);


--
-- Name: user_ops_indexer_migrations user_ops_indexer_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.user_ops_indexer_migrations
    ADD CONSTRAINT user_ops_indexer_migrations_pkey PRIMARY KEY (version);


--
-- Name: users users_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.users
    ADD CONSTRAINT users_pkey PRIMARY KEY (id);


--
-- Name: validators validators_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.validators
    ADD CONSTRAINT validators_pkey PRIMARY KEY (address_hash);


--
-- Name: withdrawals withdrawals_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.withdrawals
    ADD CONSTRAINT withdrawals_pkey PRIMARY KEY (index);


--
-- Name: account_api_keys_identity_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_api_keys_identity_id_index ON public.account_api_keys USING btree (identity_id);


--
-- Name: account_api_plans_id_max_req_per_second_name_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX account_api_plans_id_max_req_per_second_name_index ON public.account_api_plans USING btree (id, max_req_per_second, name);


--
-- Name: account_custom_abis_identity_id_address_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX account_custom_abis_identity_id_address_hash_hash_index ON public.account_custom_abis USING btree (identity_id, address_hash_hash) WHERE (user_created = true);


--
-- Name: account_custom_abis_identity_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_custom_abis_identity_id_index ON public.account_custom_abis USING btree (identity_id);


--
-- Name: account_identities_uid_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX account_identities_uid_hash_index ON public.account_identities USING btree (uid_hash);


--
-- Name: account_tag_addresses_address_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_tag_addresses_address_hash_hash_index ON public.account_tag_addresses USING btree (address_hash_hash);


--
-- Name: account_tag_addresses_identity_id_address_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX account_tag_addresses_identity_id_address_hash_hash_index ON public.account_tag_addresses USING btree (identity_id, address_hash_hash) WHERE (user_created = true);


--
-- Name: account_tag_addresses_identity_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_tag_addresses_identity_id_index ON public.account_tag_addresses USING btree (identity_id);


--
-- Name: account_tag_transactions_identity_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_tag_transactions_identity_id_index ON public.account_tag_transactions USING btree (identity_id);


--
-- Name: account_tag_transactions_identity_id_tx_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX account_tag_transactions_identity_id_tx_hash_hash_index ON public.account_tag_transactions USING btree (identity_id, transaction_hash_hash) WHERE (user_created = true);


--
-- Name: account_tag_transactions_tx_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_tag_transactions_tx_hash_hash_index ON public.account_tag_transactions USING btree (transaction_hash_hash);


--
-- Name: account_watchlist_addresses_address_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_watchlist_addresses_address_hash_hash_index ON public.account_watchlist_addresses USING btree (address_hash_hash);


--
-- Name: account_watchlist_addresses_watchlist_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_watchlist_addresses_watchlist_id_index ON public.account_watchlist_addresses USING btree (watchlist_id);


--
-- Name: account_watchlist_notifications_from_address_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_watchlist_notifications_from_address_hash_hash_index ON public.account_watchlist_notifications USING btree (from_address_hash_hash);


--
-- Name: account_watchlist_notifications_to_address_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_watchlist_notifications_to_address_hash_hash_index ON public.account_watchlist_notifications USING btree (to_address_hash_hash);


--
-- Name: account_watchlist_notifications_transaction_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_watchlist_notifications_transaction_hash_hash_index ON public.account_watchlist_notifications USING btree (transaction_hash_hash);


--
-- Name: account_watchlist_notifications_watchlist_address_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_watchlist_notifications_watchlist_address_id_index ON public.account_watchlist_notifications USING btree (watchlist_address_id);


--
-- Name: account_watchlist_notifications_watchlist_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_watchlist_notifications_watchlist_id_index ON public.account_watchlist_notifications USING btree (watchlist_id);


--
-- Name: account_watchlists_identity_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX account_watchlists_identity_id_index ON public.account_watchlists USING btree (identity_id);


--
-- Name: address_coin_balances_block_number_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_coin_balances_block_number_index ON public.address_coin_balances USING btree (block_number);


--
-- Name: address_coin_balances_value_fetched_at_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_coin_balances_value_fetched_at_index ON public.address_coin_balances USING btree (value_fetched_at);


--
-- Name: address_contract_code_fetch_attempts_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_contract_code_fetch_attempts_address_hash_index ON public.address_contract_code_fetch_attempts USING btree (address_hash);


--
-- Name: address_cur_token_balances_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_cur_token_balances_index ON public.address_current_token_balances USING btree (block_number);


--
-- Name: address_current_token_balances_token_contract_address_hash__val; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_current_token_balances_token_contract_address_hash__val ON public.address_current_token_balances USING btree (token_contract_address_hash, value DESC, address_hash DESC) WHERE ((address_hash <> '\x0000000000000000000000000000000000000000'::bytea) AND (value > (0)::numeric));


--
-- Name: address_current_token_balances_token_contract_address_hash_valu; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_current_token_balances_token_contract_address_hash_valu ON public.address_current_token_balances USING btree (token_contract_address_hash, value);


--
-- Name: address_current_token_balances_token_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_current_token_balances_token_id_index ON public.address_current_token_balances USING btree (token_id);


--
-- Name: address_decompiler_version; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX address_decompiler_version ON public.decompiled_smart_contracts USING btree (address_hash, decompiler_version);


--
-- Name: address_names_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX address_names_address_hash_index ON public.address_names USING btree (address_hash) WHERE ("primary" = true);


--
-- Name: address_tags_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX address_tags_id_index ON public.address_tags USING btree (id);


--
-- Name: address_tags_label_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX address_tags_label_index ON public.address_tags USING btree (label);


--
-- Name: address_to_tags_address_hash_tag_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX address_to_tags_address_hash_tag_id_index ON public.address_to_tags USING btree (address_hash, tag_id);


--
-- Name: address_token_balances_address_hash_token_contract_address_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_token_balances_address_hash_token_contract_address_hash ON public.address_token_balances USING btree (address_hash, token_contract_address_hash, block_number);


--
-- Name: address_token_balances_block_number_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_token_balances_block_number_address_hash_index ON public.address_token_balances USING btree (block_number, address_hash);


--
-- Name: address_token_balances_token_contract_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_token_balances_token_contract_address_hash_index ON public.address_token_balances USING btree (token_contract_address_hash);


--
-- Name: address_token_balances_token_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX address_token_balances_token_id_index ON public.address_token_balances USING btree (token_id);


--
-- Name: addresses_fetched_coin_balance_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX addresses_fetched_coin_balance_hash_index ON public.addresses USING btree (fetched_coin_balance DESC, hash) WHERE (fetched_coin_balance > (0)::numeric);


--
-- Name: addresses_fetched_coin_balance_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX addresses_fetched_coin_balance_index ON public.addresses USING btree (fetched_coin_balance);


--
-- Name: addresses_inserted_at_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX addresses_inserted_at_index ON public.addresses USING btree (inserted_at);


--
-- Name: administrators_user_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX administrators_user_id_index ON public.administrators USING btree (user_id);


--
-- Name: audit_report_unique_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX audit_report_unique_index ON public.smart_contract_audit_reports USING btree (address_hash, audit_report_url, audit_publish_date, audit_company_name);


--
-- Name: block_rewards_block_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX block_rewards_block_hash_index ON public.block_rewards USING btree (block_hash);


--
-- Name: blocks_consensus_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX blocks_consensus_index ON public.blocks USING btree (consensus);


--
-- Name: blocks_date; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX blocks_date ON public.blocks USING btree (date("timestamp"), number);


--
-- Name: blocks_inserted_at_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX blocks_inserted_at_index ON public.blocks USING btree (inserted_at);


--
-- Name: blocks_is_empty_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX blocks_is_empty_index ON public.blocks USING btree (is_empty);


--
-- Name: blocks_miner_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX blocks_miner_hash_index ON public.blocks USING btree (miner_hash);


--
-- Name: blocks_miner_hash_number_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX blocks_miner_hash_number_index ON public.blocks USING btree (miner_hash, number);


--
-- Name: blocks_number_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX blocks_number_index ON public.blocks USING btree (number);


--
-- Name: blocks_timestamp_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX blocks_timestamp_index ON public.blocks USING btree ("timestamp");


--
-- Name: consensus_block_hashes_refetch_needed; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX consensus_block_hashes_refetch_needed ON public.blocks USING btree (hash) WHERE (consensus AND refetch_needed);


--
-- Name: contract_methods_identifier_abi_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX contract_methods_identifier_abi_index ON public.contract_methods USING btree (identifier, abi);


--
-- Name: contract_methods_inserted_at_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX contract_methods_inserted_at_index ON public.contract_methods USING btree (inserted_at);


--
-- Name: email_unique_for_user; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX email_unique_for_user ON public.user_contacts USING btree (user_id, email);


--
-- Name: empty_consensus_blocks; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX empty_consensus_blocks ON public.blocks USING btree (consensus) WHERE (is_empty IS NULL);


--
-- Name: fetched_current_token_balances; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX fetched_current_token_balances ON public.address_current_token_balances USING btree (address_hash, token_contract_address_hash, COALESCE(token_id, ('-1'::integer)::numeric));


--
-- Name: fetched_token_balances; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX fetched_token_balances ON public.address_token_balances USING btree (address_hash, token_contract_address_hash, COALESCE(token_id, ('-1'::integer)::numeric), block_number);


--
-- Name: internal_transactions_created_contract_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX internal_transactions_created_contract_address_hash_index ON public.internal_transactions USING btree (created_contract_address_hash);


--
-- Name: internal_transactions_created_contract_address_hash_partial_ind; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX internal_transactions_created_contract_address_hash_partial_ind ON public.internal_transactions USING btree (created_contract_address_hash, block_number DESC, transaction_index DESC, index DESC) WHERE ((((type)::text = 'call'::text) AND (index > 0)) OR ((type)::text <> 'call'::text));


--
-- Name: internal_transactions_from_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX internal_transactions_from_address_hash_index ON public.internal_transactions USING btree (from_address_hash);


--
-- Name: internal_transactions_from_address_hash_partial_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX internal_transactions_from_address_hash_partial_index ON public.internal_transactions USING btree (from_address_hash, block_number DESC, transaction_index DESC, index DESC) WHERE ((((type)::text = 'call'::text) AND (index > 0)) OR ((type)::text <> 'call'::text));


--
-- Name: internal_transactions_to_address_hash_partial_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX internal_transactions_to_address_hash_partial_index ON public.internal_transactions USING btree (to_address_hash, block_number DESC, transaction_index DESC, index DESC) WHERE ((((type)::text = 'call'::text) AND (index > 0)) OR ((type)::text <> 'call'::text));


--
-- Name: internal_transactions_transaction_hash_index_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX internal_transactions_transaction_hash_index_index ON public.internal_transactions USING btree (transaction_hash, index);


--
-- Name: logs_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX logs_address_hash_index ON public.logs USING btree (address_hash);


--
-- Name: logs_address_hash_transaction_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX logs_address_hash_transaction_hash_index ON public.logs USING btree (address_hash, transaction_hash);


--
-- Name: logs_block_number_ASC__index_ASC_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX "logs_block_number_ASC__index_ASC_index" ON public.logs USING btree (block_number, index);


--
-- Name: logs_block_number_DESC__index_DESC_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX "logs_block_number_DESC__index_DESC_index" ON public.logs USING btree (block_number DESC, index DESC);


--
-- Name: logs_first_topic_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX logs_first_topic_index ON public.logs USING btree (first_topic);


--
-- Name: logs_fourth_topic_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX logs_fourth_topic_index ON public.logs USING btree (fourth_topic);


--
-- Name: logs_index_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX logs_index_index ON public.logs USING btree (index);


--
-- Name: logs_second_topic_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX logs_second_topic_index ON public.logs USING btree (second_topic);


--
-- Name: logs_third_topic_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX logs_third_topic_index ON public.logs USING btree (third_topic);


--
-- Name: logs_transaction_hash_index_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX logs_transaction_hash_index_index ON public.logs USING btree (transaction_hash, index);


--
-- Name: market_history_date_secondary_coin_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX market_history_date_secondary_coin_index ON public.market_history USING btree (date, secondary_coin);


--
-- Name: method_id; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX method_id ON public.transactions USING btree (SUBSTRING(input FROM 1 FOR 4));


--
-- Name: missing_block_ranges_from_number_DESC_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX "missing_block_ranges_from_number_DESC_index" ON public.missing_block_ranges USING btree (from_number DESC);


--
-- Name: missing_block_ranges_from_number_to_number_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX missing_block_ranges_from_number_to_number_index ON public.missing_block_ranges USING btree (from_number, to_number);


--
-- Name: nephew_hash_to_uncle_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX nephew_hash_to_uncle_hash ON public.block_second_degree_relations USING btree (nephew_hash, uncle_hash);


--
-- Name: one_consensus_block_at_height; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX one_consensus_block_at_height ON public.blocks USING btree (number) WHERE consensus;


--
-- Name: one_consensus_child_per_parent; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX one_consensus_child_per_parent ON public.blocks USING btree (parent_hash) WHERE consensus;


--
-- Name: one_primary_per_user; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX one_primary_per_user ON public.user_contacts USING btree (user_id) WHERE "primary";


--
-- Name: owner_role_limit; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX owner_role_limit ON public.administrators USING btree (role) WHERE ((role)::text = 'owner'::text);


--
-- Name: pending_block_operations_block_number_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX pending_block_operations_block_number_index ON public.pending_block_operations USING btree (block_number);


--
-- Name: pending_txs_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX pending_txs_index ON public.transactions USING btree (inserted_at, hash) WHERE ((block_hash IS NULL) AND ((error IS NULL) OR ((error)::text <> 'dropped/replaced'::text)));


--
-- Name: proxy_implementations_proxy_type_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX proxy_implementations_proxy_type_index ON public.proxy_implementations USING btree (proxy_type);


--
-- Name: signed_authorizations_authority_nonce_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX signed_authorizations_authority_nonce_index ON public.signed_authorizations USING btree (authority, nonce);


--
-- Name: smart_contract_audit_reports_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX smart_contract_audit_reports_address_hash_index ON public.smart_contract_audit_reports USING btree (address_hash);


--
-- Name: smart_contracts_additional_sources_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX smart_contracts_additional_sources_address_hash_index ON public.smart_contracts_additional_sources USING btree (address_hash);


--
-- Name: smart_contracts_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX smart_contracts_address_hash_index ON public.smart_contracts USING btree (address_hash);


--
-- Name: smart_contracts_certified_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX smart_contracts_certified_index ON public.smart_contracts USING btree (certified);


--
-- Name: smart_contracts_contract_code_md5_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX smart_contracts_contract_code_md5_index ON public.smart_contracts USING btree (contract_code_md5);


--
-- Name: smart_contracts_trgm_idx; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX smart_contracts_trgm_idx ON public.smart_contracts USING gin (to_tsvector('english'::regconfig, (name)::text));


--
-- Name: token_instance_metadata_refetch_attempts_token_contract_address; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_instance_metadata_refetch_attempts_token_contract_address ON public.token_instance_metadata_refetch_attempts USING btree (token_contract_address_hash, token_id);


--
-- Name: token_instances_error_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_instances_error_index ON public.token_instances USING btree (error);


--
-- Name: token_instances_owner_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_instances_owner_address_hash_index ON public.token_instances USING btree (owner_address_hash);


--
-- Name: token_instances_token_contract_address_hash_token_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_instances_token_contract_address_hash_token_id_index ON public.token_instances USING btree (token_contract_address_hash, token_id);


--
-- Name: token_instances_token_id_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_instances_token_id_index ON public.token_instances USING btree (token_id);


--
-- Name: token_transfers_block_consensus_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_block_consensus_index ON public.token_transfers USING btree (block_consensus);


--
-- Name: token_transfers_block_number_ASC_log_index_ASC_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX "token_transfers_block_number_ASC_log_index_ASC_index" ON public.token_transfers USING btree (block_number, log_index);


--
-- Name: token_transfers_block_number_DESC_log_index_DESC_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX "token_transfers_block_number_DESC_log_index_DESC_index" ON public.token_transfers USING btree (block_number DESC, log_index DESC);


--
-- Name: token_transfers_block_number_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_block_number_index ON public.token_transfers USING btree (block_number);


--
-- Name: token_transfers_from_address_hash_block_number_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_from_address_hash_block_number_index ON public.token_transfers USING btree (from_address_hash, block_number);


--
-- Name: token_transfers_from_address_hash_transaction_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_from_address_hash_transaction_hash_index ON public.token_transfers USING btree (from_address_hash, transaction_hash);


--
-- Name: token_transfers_to_address_hash_block_number_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_to_address_hash_block_number_index ON public.token_transfers USING btree (to_address_hash, block_number);


--
-- Name: token_transfers_to_address_hash_transaction_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_to_address_hash_transaction_hash_index ON public.token_transfers USING btree (to_address_hash, transaction_hash);


--
-- Name: token_transfers_token_contract_address_hash__block_number_DESC_; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX "token_transfers_token_contract_address_hash__block_number_DESC_" ON public.token_transfers USING btree (token_contract_address_hash, block_number DESC, log_index DESC);


--
-- Name: token_transfers_token_contract_address_hash_token_ids_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_token_contract_address_hash_token_ids_index ON public.token_transfers USING gin (token_contract_address_hash, token_ids);


--
-- Name: token_transfers_token_contract_address_hash_transaction_hash_in; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_token_contract_address_hash_transaction_hash_in ON public.token_transfers USING btree (token_contract_address_hash, transaction_hash);


--
-- Name: token_transfers_token_type_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_token_type_index ON public.token_transfers USING btree (token_type);


--
-- Name: token_transfers_transaction_hash_log_index_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX token_transfers_transaction_hash_log_index_index ON public.token_transfers USING btree (transaction_hash, log_index);


--
-- Name: tokens_contract_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX tokens_contract_address_hash_index ON public.tokens USING btree (contract_address_hash);


--
-- Name: tokens_symbol_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX tokens_symbol_index ON public.tokens USING btree (symbol);


--
-- Name: tokens_trgm_idx; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX tokens_trgm_idx ON public.tokens USING gin (to_tsvector('english'::regconfig, ((symbol || ' '::text) || name)));


--
-- Name: tokens_type_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX tokens_type_index ON public.tokens USING btree (type);


--
-- Name: transaction_actions_protocol_type_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transaction_actions_protocol_type_index ON public.transaction_actions USING btree (protocol, type);


--
-- Name: transaction_stats_date_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX transaction_stats_date_index ON public.transaction_stats USING btree (date);


--
-- Name: transactions_block_consensus_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_block_consensus_index ON public.transactions USING btree (block_consensus);


--
-- Name: transactions_block_hash_error_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_block_hash_error_index ON public.transactions USING btree (block_hash, error);


--
-- Name: transactions_block_hash_index_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX transactions_block_hash_index_index ON public.transactions USING btree (block_hash, index);


--
-- Name: transactions_block_number_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_block_number_index ON public.transactions USING btree (block_number);


--
-- Name: transactions_block_timestamp_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_block_timestamp_index ON public.transactions USING btree (block_timestamp);


--
-- Name: transactions_created_contract_address_hash_with_pending_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_created_contract_address_hash_with_pending_index ON public.transactions USING btree (created_contract_address_hash, block_number DESC, index DESC, inserted_at DESC, hash);


--
-- Name: transactions_created_contract_address_hash_with_pending_index_a; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_created_contract_address_hash_with_pending_index_a ON public.transactions USING btree (created_contract_address_hash, block_number, index, inserted_at, hash DESC);


--
-- Name: transactions_created_contract_code_indexed_at_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_created_contract_code_indexed_at_index ON public.transactions USING btree (created_contract_code_indexed_at);


--
-- Name: transactions_from_address_hash_with_pending_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_from_address_hash_with_pending_index ON public.transactions USING btree (from_address_hash, block_number DESC, index DESC, inserted_at DESC, hash);


--
-- Name: transactions_from_address_hash_with_pending_index_asc; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_from_address_hash_with_pending_index_asc ON public.transactions USING btree (from_address_hash, block_number, index, inserted_at, hash DESC);


--
-- Name: transactions_inserted_at_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_inserted_at_index ON public.transactions USING btree (inserted_at);


--
-- Name: transactions_nonce_from_address_hash_block_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_nonce_from_address_hash_block_hash_index ON public.transactions USING btree (nonce, from_address_hash, block_hash);


--
-- Name: transactions_recent_collated_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_recent_collated_index ON public.transactions USING btree (block_number DESC, index DESC);


--
-- Name: transactions_status_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_status_index ON public.transactions USING btree (status);


--
-- Name: transactions_to_address_hash_with_pending_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_to_address_hash_with_pending_index ON public.transactions USING btree (to_address_hash, block_number DESC, index DESC, inserted_at DESC, hash);


--
-- Name: transactions_to_address_hash_with_pending_index_asc; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_to_address_hash_with_pending_index_asc ON public.transactions USING btree (to_address_hash, block_number, index, inserted_at, hash DESC);


--
-- Name: transactions_updated_at_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX transactions_updated_at_index ON public.transactions USING btree (updated_at);


--
-- Name: uncataloged_tokens; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX uncataloged_tokens ON public.tokens USING btree (cataloged) WHERE (cataloged = false);


--
-- Name: uncle_hash_to_nephew_hash; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX uncle_hash_to_nephew_hash ON public.block_second_degree_relations USING btree (uncle_hash, nephew_hash);


--
-- Name: unfetched_address_token_balances_v2_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX unfetched_address_token_balances_v2_index ON public.address_token_balances USING btree (id) WHERE ((((address_hash <> '\x0000000000000000000000000000000000000000'::bytea) AND ((token_type)::text = 'ERC-721'::text)) OR ((token_type)::text = 'ERC-20'::text) OR ((token_type)::text = 'ERC-1155'::text) OR ((token_type)::text = 'ERC-404'::text)) AND ((value_fetched_at IS NULL) OR (value IS NULL)));


--
-- Name: unfetched_balances; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX unfetched_balances ON public.address_coin_balances USING btree (address_hash, block_number) WHERE (value_fetched_at IS NULL);


--
-- Name: unfetched_uncles; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX unfetched_uncles ON public.block_second_degree_relations USING btree (nephew_hash, uncle_hash) WHERE (uncle_fetched_at IS NULL);


--
-- Name: unique_address_names; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX unique_address_names ON public.address_names USING btree (address_hash, name);


--
-- Name: unique_username; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX unique_username ON public.users USING btree (username);


--
-- Name: unique_watchlist_id_address_hash_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE UNIQUE INDEX unique_watchlist_id_address_hash_hash_index ON public.account_watchlist_addresses USING btree (watchlist_id, address_hash_hash) WHERE (user_created = true);


--
-- Name: user_operations_block_number_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX user_operations_block_number_hash_index ON public.user_operations USING btree (block_number DESC, hash DESC);


--
-- Name: user_operations_block_number_transaction_hash_bundle_index_inde; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX user_operations_block_number_transaction_hash_bundle_index_inde ON public.user_operations USING btree (block_number DESC, transaction_hash DESC, bundle_index DESC);


--
-- Name: user_operations_bundler_transaction_hash_bundle_index_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX user_operations_bundler_transaction_hash_bundle_index_index ON public.user_operations USING btree (bundler, transaction_hash, bundle_index);


--
-- Name: user_operations_factory_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX user_operations_factory_index ON public.user_operations USING btree (factory);


--
-- Name: user_operations_paymaster_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX user_operations_paymaster_index ON public.user_operations USING btree (paymaster);


--
-- Name: user_operations_sender_factory_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX user_operations_sender_factory_index ON public.user_operations USING btree (sender, factory);


--
-- Name: user_operations_transaction_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX user_operations_transaction_hash_index ON public.user_operations USING btree (transaction_hash);


--
-- Name: withdrawals_address_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX withdrawals_address_hash_index ON public.withdrawals USING btree (address_hash);


--
-- Name: withdrawals_block_hash_index; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX withdrawals_block_hash_index ON public.withdrawals USING btree (block_hash);


--
-- Name: account_api_keys account_api_keys_identity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_api_keys
    ADD CONSTRAINT account_api_keys_identity_id_fkey FOREIGN KEY (identity_id) REFERENCES public.account_identities(id) ON DELETE CASCADE;


--
-- Name: account_custom_abis account_custom_abis_identity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_custom_abis
    ADD CONSTRAINT account_custom_abis_identity_id_fkey FOREIGN KEY (identity_id) REFERENCES public.account_identities(id) ON DELETE CASCADE;


--
-- Name: account_identities account_identities_plan_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_identities
    ADD CONSTRAINT account_identities_plan_id_fkey FOREIGN KEY (plan_id) REFERENCES public.account_api_plans(id);


--
-- Name: account_public_tags_requests account_public_tags_requests_identity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_public_tags_requests
    ADD CONSTRAINT account_public_tags_requests_identity_id_fkey FOREIGN KEY (identity_id) REFERENCES public.account_identities(id);


--
-- Name: account_tag_addresses account_tag_addresses_identity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_tag_addresses
    ADD CONSTRAINT account_tag_addresses_identity_id_fkey FOREIGN KEY (identity_id) REFERENCES public.account_identities(id) ON DELETE CASCADE;


--
-- Name: account_tag_transactions account_tag_transactions_identity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_tag_transactions
    ADD CONSTRAINT account_tag_transactions_identity_id_fkey FOREIGN KEY (identity_id) REFERENCES public.account_identities(id) ON DELETE CASCADE;


--
-- Name: account_watchlist_addresses account_watchlist_addresses_watchlist_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_watchlist_addresses
    ADD CONSTRAINT account_watchlist_addresses_watchlist_id_fkey FOREIGN KEY (watchlist_id) REFERENCES public.account_watchlists(id) ON DELETE CASCADE;


--
-- Name: account_watchlists account_watchlists_identity_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.account_watchlists
    ADD CONSTRAINT account_watchlists_identity_id_fkey FOREIGN KEY (identity_id) REFERENCES public.account_identities(id) ON DELETE CASCADE;


--
-- Name: address_to_tags address_to_tags_tag_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.address_to_tags
    ADD CONSTRAINT address_to_tags_tag_id_fkey FOREIGN KEY (tag_id) REFERENCES public.address_tags(id);


--
-- Name: administrators administrators_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.administrators
    ADD CONSTRAINT administrators_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: block_rewards block_rewards_block_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.block_rewards
    ADD CONSTRAINT block_rewards_block_hash_fkey FOREIGN KEY (block_hash) REFERENCES public.blocks(hash) ON DELETE CASCADE;


--
-- Name: block_second_degree_relations block_second_degree_relations_nephew_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.block_second_degree_relations
    ADD CONSTRAINT block_second_degree_relations_nephew_hash_fkey FOREIGN KEY (nephew_hash) REFERENCES public.blocks(hash);


--
-- Name: internal_transactions internal_transactions_block_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.internal_transactions
    ADD CONSTRAINT internal_transactions_block_hash_fkey FOREIGN KEY (block_hash) REFERENCES public.blocks(hash);


--
-- Name: internal_transactions internal_transactions_transaction_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.internal_transactions
    ADD CONSTRAINT internal_transactions_transaction_hash_fkey FOREIGN KEY (transaction_hash) REFERENCES public.transactions(hash) ON DELETE CASCADE;


--
-- Name: logs logs_block_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.logs
    ADD CONSTRAINT logs_block_hash_fkey FOREIGN KEY (block_hash) REFERENCES public.blocks(hash);


--
-- Name: logs logs_transaction_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.logs
    ADD CONSTRAINT logs_transaction_hash_fkey FOREIGN KEY (transaction_hash) REFERENCES public.transactions(hash) ON DELETE CASCADE;


--
-- Name: pending_block_operations pending_block_operations_block_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.pending_block_operations
    ADD CONSTRAINT pending_block_operations_block_hash_fkey FOREIGN KEY (block_hash) REFERENCES public.blocks(hash) ON DELETE CASCADE;


--
-- Name: proxy_smart_contract_verification_statuses proxy_smart_contract_verification_statuses_contract_address_has; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.proxy_smart_contract_verification_statuses
    ADD CONSTRAINT proxy_smart_contract_verification_statuses_contract_address_has FOREIGN KEY (contract_address_hash) REFERENCES public.smart_contracts(address_hash) ON DELETE CASCADE;


--
-- Name: scam_address_badge_mappings scam_address_badge_mappings_address_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.scam_address_badge_mappings
    ADD CONSTRAINT scam_address_badge_mappings_address_hash_fkey FOREIGN KEY (address_hash) REFERENCES public.addresses(hash) ON DELETE CASCADE;


--
-- Name: signed_authorizations signed_authorizations_transaction_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.signed_authorizations
    ADD CONSTRAINT signed_authorizations_transaction_hash_fkey FOREIGN KEY (transaction_hash) REFERENCES public.transactions(hash) ON DELETE CASCADE;


--
-- Name: smart_contract_audit_reports smart_contract_audit_reports_address_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.smart_contract_audit_reports
    ADD CONSTRAINT smart_contract_audit_reports_address_hash_fkey FOREIGN KEY (address_hash) REFERENCES public.smart_contracts(address_hash) ON DELETE CASCADE;


--
-- Name: smart_contracts_additional_sources smart_contracts_additional_sources_address_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.smart_contracts_additional_sources
    ADD CONSTRAINT smart_contracts_additional_sources_address_hash_fkey FOREIGN KEY (address_hash) REFERENCES public.smart_contracts(address_hash) ON DELETE CASCADE;


--
-- Name: token_instances token_instances_token_contract_address_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.token_instances
    ADD CONSTRAINT token_instances_token_contract_address_hash_fkey FOREIGN KEY (token_contract_address_hash) REFERENCES public.tokens(contract_address_hash);


--
-- Name: token_transfers token_transfers_block_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.token_transfers
    ADD CONSTRAINT token_transfers_block_hash_fkey FOREIGN KEY (block_hash) REFERENCES public.blocks(hash);


--
-- Name: token_transfers token_transfers_transaction_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.token_transfers
    ADD CONSTRAINT token_transfers_transaction_hash_fkey FOREIGN KEY (transaction_hash) REFERENCES public.transactions(hash) ON DELETE CASCADE;


--
-- Name: transaction_actions transaction_actions_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transaction_actions
    ADD CONSTRAINT transaction_actions_hash_fkey FOREIGN KEY (hash) REFERENCES public.transactions(hash) ON UPDATE CASCADE ON DELETE CASCADE;


--
-- Name: transaction_forks transaction_forks_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transaction_forks
    ADD CONSTRAINT transaction_forks_hash_fkey FOREIGN KEY (hash) REFERENCES public.transactions(hash) ON DELETE CASCADE;


--
-- Name: transaction_forks transaction_forks_uncle_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transaction_forks
    ADD CONSTRAINT transaction_forks_uncle_hash_fkey FOREIGN KEY (uncle_hash) REFERENCES public.blocks(hash) ON DELETE CASCADE;


--
-- Name: transactions transactions_block_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.transactions
    ADD CONSTRAINT transactions_block_hash_fkey FOREIGN KEY (block_hash) REFERENCES public.blocks(hash) ON DELETE CASCADE;


--
-- Name: user_contacts user_contacts_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.user_contacts
    ADD CONSTRAINT user_contacts_user_id_fkey FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE CASCADE;


--
-- Name: withdrawals withdrawals_block_hash_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.withdrawals
    ADD CONSTRAINT withdrawals_block_hash_fkey FOREIGN KEY (block_hash) REFERENCES public.blocks(hash) ON DELETE CASCADE;


--
-- PostgreSQL database dump complete
--

