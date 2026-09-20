#include "../../cmd/layerxd/lxp_daemon_finality_authority.c"

static lxp_guarantor_cert certificate;
static char record_json[1400];
static char receipt_json[4096];
static json_token anchor_tokens[256];

static void word_u64(uint8_t *record, size_t index, uint64_t value)
{
    abi_u64(record + index * 32U, value);
}

static int record_matches(const uint8_t *record, const uint8_t checkpoint_id[32],
                          lxp_daemon_anchor_ladder ladder)
{
    json_document doc;
    uint64_t submitted = 0U, finalized = 0U;
    record_json[0] = '"';
    encode_hex(record, ANCHOR_CHECKPOINT_BYTES, record_json + 1U);
    (void)strcat(record_json, "\"");
    doc = (json_document){anchor_tokens, 0U, record_json, record_json + strlen(record_json)};
    if (parse_value(&doc, 0U) != 0) return -1;
    return anchor_checkpoint_matches(anchor_tokens, &certificate, checkpoint_id, ladder,
                                     &submitted, &finalized);
}

static int ladder_of(const char *text, lxp_daemon_anchor_ladder *ladder)
{
    json_document doc = {anchor_tokens, 0U, text, text + strlen(text)};
    if (parse_value(&doc, 0U) != 0) return -2;
    return anchor_ladder_decode(anchor_tokens, ladder);
}

static int receipt_matches(const char *address, const char *topic, const uint8_t *data,
                           const lxp_daemon_settlement_registration_evidence *registration,
                           unsigned copies)
{
    json_document doc;
    char batch[67], id[67], transaction[67], payload[195], log[1024];
    uint8_t word[32];
    unsigned i;
    abi_u64(word, certificate.checkpoint.header.batch_number);
    encode_hex(word, 32U, batch);
    encode_hex(registration->checkpoint_id, 32U, id);
    encode_hex(registration->transaction_id, 32U, transaction);
    encode_hex(data, 96U, payload);
    (void)snprintf(log, sizeof(log),
        "{\"address\":\"%s\",\"topics\":[\"%s\",\"%s\",\"%s\"],\"data\":\"%s\","
        "\"transactionHash\":\"%s\",\"blockHash\":\"0x%064x\",\"blockNumber\":\"0x9\",\"removed\":false}",
        address, topic, batch, id, payload, transaction, 7U);
    (void)snprintf(receipt_json, sizeof(receipt_json), "{\"blockHash\":\"0x%064x\",\"logs\":[", 7U);
    for (i = 0U; i < copies; ++i) {
        if (i != 0U) (void)strcat(receipt_json, ",");
        (void)strcat(receipt_json, log);
    }
    (void)strcat(receipt_json, "]}");
    doc = (json_document){anchor_tokens, 0U, receipt_json, receipt_json + strlen(receipt_json)};
    if (parse_value(&doc, 0U) != 0) return -1;
    return submitted_event(&doc, anchor_tokens, &certificate, registration);
}

static int anchor_checks(void)
{
    static const char anchor[] = "0x0000000000000000000000000000000000001014";
    static const char submitted[] = "0xf732efc9df2e7589898899f85ca5e6cb25fa1c619461d96e81e82be9ccf14416";
    static const char finalized[] = "0x4da1187de98bd3c2616ff7203c50ea1687402222f6ef36e094181bf5cdd2f932";
    static const uint8_t status_of[4] = {0x4eU, 0xb4U, 0x77U, 0x10U};
    static const uint8_t checkpoint_of[4] = {0x2dU, 0x58U, 0x8bU, 0x18U};
    lxp_daemon_settlement_registration_evidence registration;
    lxp_batch_header *header = &certificate.checkpoint.header;
    lxp_daemon_anchor_ladder ladder = LXP_DAEMON_ANCHOR_FINAL;
    uint8_t record[ANCHOR_CHECKPOINT_BYTES], altered[ANCHOR_CHECKPOINT_BYTES];
    uint8_t checkpoint_id[32], data[96], selector[4];
    char topic[67];
    size_t index;
    (void)memset(&certificate, 0, sizeof(certificate));
    (void)memset(&registration, 0, sizeof(registration));
    header->batch_number = 12U; header->epoch = 3U; header->first_sequence = 40U;
    header->last_sequence = 44U; header->timestamp_ms = 1700000000123ULL;
    (void)memset(header->previous_state_root, 0x11, 32U);
    (void)memset(header->resulting_state_root, 0x22, 32U);
    (void)memset(header->receipt_merkle_root, 0x33, 32U);
    (void)memset(header->data_availability_root, 0x44, 32U);
    (void)memset(header->sequencer_id, 0x55, 32U);
    certificate.attestation_count = 3U;
    (void)memset(checkpoint_id, 0x66, 32U);
    (void)memcpy(registration.checkpoint_id, checkpoint_id, 32U);
    (void)memset(registration.transaction_id, 0x77, 32U);
    registration.observed_block_number = 9U;
    if (lxp_paxeer_abi_selector(LXP_DAEMON_ANCHOR_STATUS_OF, selector) != LXP_OK || memcmp(selector, status_of, 4U) != 0 ||
        lxp_paxeer_abi_selector(LXP_DAEMON_ANCHOR_CHECKPOINT, selector) != LXP_OK || memcmp(selector, checkpoint_of, 4U) != 0 ||
        event_topic("CheckpointSubmitted(uint64,bytes32,bytes32,bytes32,uint8)", topic) != LXP_OK || strcmp(topic, submitted) != 0)
        return 1;
    if (ladder_of("\"0x0000000000000000000000000000000000000000000000000000000000000000\"", &ladder) != 0 || ladder != LXP_DAEMON_ANCHOR_INSTANT ||
        ladder_of("\"0x0000000000000000000000000000000000000000000000000000000000000001\"", &ladder) != 0 || ladder != LXP_DAEMON_ANCHOR_SEALED ||
        ladder_of("\"0x0000000000000000000000000000000000000000000000000000000000000002\"", &ladder) != 0 || ladder != LXP_DAEMON_ANCHOR_FINAL ||
        ladder_of("\"0x0000000000000000000000000000000000000000000000000000000000000003\"", &ladder) == 0 ||
        ladder_of("\"0x0100000000000000000000000000000000000000000000000000000000000002\"", &ladder) == 0 ||
        ladder_of("\"0x02\"", &ladder) == 0 || ladder_of("2", &ladder) == 0)
        return 2;
    (void)memset(record, 0, sizeof(record));
    word_u64(record, 0U, header->batch_number);
    (void)memcpy(record + 32U, checkpoint_id, 32U);
    (void)memset(record + 64U, 0x99, 32U);
    word_u64(record, 3U, header->epoch);
    word_u64(record, 4U, header->first_sequence);
    word_u64(record, 5U, header->last_sequence);
    (void)memcpy(record + 192U, header->previous_state_root, 32U);
    (void)memcpy(record + 224U, header->resulting_state_root, 32U);
    (void)memcpy(record + 256U, header->receipt_merkle_root, 32U);
    (void)memcpy(record + 288U, header->data_availability_root, 32U);
    (void)memcpy(record + 320U, header->sequencer_id, 32U);
    word_u64(record, 11U, header->timestamp_ms);
    word_u64(record, 12U, 2U);
    word_u64(record, 13U, 3U);
    word_u64(record, 14U, 31U);
    word_u64(record, 16U, 9U);
    word_u64(record, 17U, 9U);
    if (record_matches(record, checkpoint_id, LXP_DAEMON_ANCHOR_FINAL) != 1 ||
        record_matches(record, checkpoint_id, LXP_DAEMON_ANCHOR_SEALED) != 0)
        return 3;
    for (index = 0U; index < ANCHOR_CHECKPOINT_WORDS; ++index) {
        if (index == 2U || index == 14U || index == 16U) continue;
        (void)memcpy(altered, record, sizeof(record));
        altered[index * 32U + 31U] ^= 1U;
        if (index == 17U) word_u64(altered, 17U, 8U);
        if (record_matches(altered, checkpoint_id, LXP_DAEMON_ANCHOR_FINAL) != 0) return 4;
    }
    (void)memcpy(altered, record, sizeof(record));
    word_u64(altered, 12U, 1U);
    word_u64(altered, 17U, 0U);
    if (record_matches(altered, checkpoint_id, LXP_DAEMON_ANCHOR_SEALED) != 1 ||
        record_matches(altered, checkpoint_id, LXP_DAEMON_ANCHOR_FINAL) != 0)
        return 5;
    (void)memcpy(data, header->resulting_state_root, 32U);
    (void)memcpy(data + 32U, header->receipt_merkle_root, 32U);
    abi_u64(data + 64U, 3U);
    if (receipt_matches(anchor, submitted, data, &registration, 1U) != 1 ||
        receipt_matches(anchor, submitted, data, &registration, 2U) != 0 ||
        receipt_matches(anchor, submitted, data, &registration, 0U) != 0 ||
        receipt_matches(anchor, finalized, data, &registration, 1U) != 0 ||
        receipt_matches("0x0000000000000000000000000000000000001013", submitted, data, &registration, 1U) != 0)
        return 6;
    data[95] = 2U;
    if (receipt_matches(anchor, submitted, data, &registration, 1U) != 0) return 7;
    data[95] = 3U;
    registration.observed_block_number = 10U;
    if (receipt_matches(anchor, submitted, data, &registration, 1U) != 0) return 8;
    return 0;
}

int main(void)
{
    static const char *const valid[] = {
        "0", "1", "1005", "-10", "0.1", "-0.2", "1e3", "1E-3", "1e+3"
    };
    static const char *const invalid[] = {
        "", "-", "01", "-01", "+1", "1.", ".1", "1e", "1e+", "NaN", "Infinity", "1x"
    };
    static const char *const documents[] = {
        "{\"id\":1,\"result\":{\"blockTimestamp\":1005,\"removed\":false,\"contractAddress\":null}}",
        "{\"id\":1,\"id\":1}", "{\"result\":01}", "{\"result\":1e+}"
    };
    json_token tokens[64];
    size_t i;
    for (i = 0U; i < sizeof(valid) / sizeof(valid[0]); ++i)
        if (!json_number(valid[i], strlen(valid[i]))) return 1;
    for (i = 0U; i < sizeof(invalid) / sizeof(invalid[0]); ++i)
        if (json_number(invalid[i], strlen(invalid[i]))) return 1;
    for (i = 0U; i < sizeof(documents) / sizeof(documents[0]); ++i) {
        json_document doc = {tokens, 0U, documents[i], documents[i] + strlen(documents[i])};
        int status = parse_value(&doc, 0U);
        if ((i == 0U && (status != 0 || doc.cursor != doc.end)) || (i != 0U && status == 0)) return 1;
    }
    {
        int anchor = anchor_checks();
        if (anchor != 0) { (void)fprintf(stderr, "anchor finality check %d failed\n", anchor); return 1; }
    }
    (void)puts("finality JSON number grammar and duplicate-key rejection passed");
    return 0;
}
