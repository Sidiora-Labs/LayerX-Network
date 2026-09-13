#include "layerx/lxp_handover.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_da.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_maintenance.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_replica.h"

#include <openssl/evp.h>
#include <stdlib.h>
#include <string.h>

static const uint8_t certificate_domain[] = "LXP/sequencer-handover/v1";
static const uint8_t evidence_domain[] = "LXP/sequencer-handover-evidence/v1";
static const uint8_t finality_domain[] = "LXP/handover-finality/v1";
static const uint8_t recovery_domain[] = "LXP/recovery-with-handover/v1";
static const uint8_t authority_key[32] = "handover-authority";

static uint64_t read_integer(const uint8_t *bytes, size_t width)
{
    uint64_t value = 0U;
    for (size_t i = 0U; i < width; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static void write_integer(uint8_t *bytes, uint64_t value, size_t width)
{
    for (size_t i = 0U; i < width; ++i)
        bytes[i] = (uint8_t)(value >> ((width - i - 1U) * 8U));
}

lxp_result lxp_handover_sequencer_id(const uint8_t public_key[32],
                                    uint8_t identifier[32])
{
    static const uint8_t digits[] = "0123456789abcdef";
    uint8_t preimage[81] = "layerx-sequencer:";
    if (public_key == NULL || identifier == NULL ||
        !lxp_ed25519_pubkey_is_canonical(public_key))
        return LXP_ERR_BAD_SIGNATURE;
    for (size_t i = 0U; i < 32U; ++i) {
        preimage[17U + i * 2U] = digits[public_key[i] >> 4U];
        preimage[18U + i * 2U] = digits[public_key[i] & 15U];
    }
    return lxp_hash_sha256(preimage, sizeof(preimage), identifier);
}

lxp_result lxp_handover_genesis_authority(const lxp_genesis_manifest *manifest,
                                         uint8_t public_key[32], bool *present)
{
    if (manifest == NULL || public_key == NULL || present == NULL ||
        manifest->parameter_count > LXP_GENESIS_MAX_PARAMETERS)
        return LXP_ERR_NON_CANONICAL;
    *present = false;
    (void)memset(public_key, 0, 32U);
    for (size_t i = 0U; i < manifest->parameter_count; ++i) {
        const lxp_genesis_parameter *parameter = &manifest->parameters[i];
        if (memcmp(parameter->key, authority_key, sizeof(authority_key)) != 0) continue;
        if (parameter->module_id != LXP_MODULE_GOVERNANCE ||
            *present || manifest->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT ||
            !lxp_ed25519_pubkey_is_canonical(parameter->value) ||
            memcmp(parameter->value, manifest->signer_public_key, 32U) == 0)
            return LXP_ERR_AUTH_SCOPE;
        (void)memcpy(public_key, parameter->value, 32U);
        *present = true;
    }
    return LXP_OK;
}

static lxp_result certificate_validate(const lxp_handover_certificate *certificate)
{
    uint8_t identifier[32];
    lxp_result status;
    if (certificate == NULL || certificate->network_id == 0U ||
        certificate->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT ||
        certificate->old_epoch == 0U || certificate->old_epoch == UINT64_MAX ||
        certificate->new_epoch != certificate->old_epoch + 1U ||
        certificate->predecessor_batch == 0U || certificate->predecessor_batch == UINT64_MAX ||
        certificate->activation_batch != certificate->predecessor_batch + 1U ||
        certificate->predecessor_last_sequence == 0U ||
        certificate->predecessor_last_sequence == UINT64_MAX ||
        memcmp(certificate->old_public_key, certificate->new_public_key, 32U) == 0 ||
        lxp_ct_is_zero(certificate->predecessor_header_hash, 32U) ||
        lxp_ct_is_zero(certificate->predecessor_state_root, 32U) ||
        lxp_ct_is_zero(certificate->predecessor_checkpoint_id, 32U) ||
        lxp_ct_is_zero(certificate->finality_evidence_digest, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_handover_sequencer_id(certificate->old_public_key, identifier);
    if (status == LXP_OK && memcmp(identifier, certificate->old_sequencer_id, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_handover_sequencer_id(certificate->new_public_key, identifier);
    if (status == LXP_OK && memcmp(identifier, certificate->new_sequencer_id, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    return status;
}

lxp_result lxp_handover_certificate_encode(const lxp_handover_certificate *certificate,
    uint8_t bytes[LXP_HANDOVER_CERTIFICATE_BYTES])
{
    size_t offset = sizeof(certificate_domain);
    lxp_result status;
    if (bytes == NULL) return LXP_ERR_NON_CANONICAL;
    status = certificate_validate(certificate);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes, certificate_domain, offset);
#define INTEGER(field, width) do { write_integer(bytes + offset, certificate->field, width); offset += width; } while (0)
#define BYTES(field, length) do { (void)memcpy(bytes + offset, certificate->field, length); offset += length; } while (0)
    INTEGER(network_id, 4U);
    INTEGER(protocol_version, 2U);
    INTEGER(old_epoch, 8U);
    INTEGER(new_epoch, 8U);
    BYTES(old_sequencer_id, 32U);
    BYTES(old_public_key, 32U);
    BYTES(new_sequencer_id, 32U);
    BYTES(new_public_key, 32U);
    INTEGER(predecessor_batch, 8U);
    INTEGER(predecessor_last_sequence, 8U);
    BYTES(predecessor_header_hash, 32U);
    BYTES(predecessor_state_root, 32U);
    BYTES(predecessor_checkpoint_id, 32U);
    INTEGER(activation_batch, 8U);
    BYTES(finality_evidence_digest, 32U);
    BYTES(governance_signature, 64U);
#undef BYTES
#undef INTEGER
    return offset == LXP_HANDOVER_CERTIFICATE_BYTES ? LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lxp_handover_certificate_decode(lxp_byte_span encoded,
                                           lxp_handover_certificate *certificate)
{
    lxp_handover_certificate parsed = {0};
    size_t offset = sizeof(certificate_domain);
    lxp_result status;
    if (certificate == NULL || encoded.bytes == NULL ||
        encoded.length != LXP_HANDOVER_CERTIFICATE_BYTES ||
        memcmp(encoded.bytes, certificate_domain, offset) != 0)
        return LXP_ERR_NON_CANONICAL;
#define INTEGER(field, width, type) do { parsed.field = (type)read_integer(encoded.bytes + offset, width); offset += width; } while (0)
#define BYTES(field, length) do { (void)memcpy(parsed.field, encoded.bytes + offset, length); offset += length; } while (0)
    INTEGER(network_id, 4U, uint32_t);
    INTEGER(protocol_version, 2U, uint16_t);
    INTEGER(old_epoch, 8U, uint64_t);
    INTEGER(new_epoch, 8U, uint64_t);
    BYTES(old_sequencer_id, 32U);
    BYTES(old_public_key, 32U);
    BYTES(new_sequencer_id, 32U);
    BYTES(new_public_key, 32U);
    INTEGER(predecessor_batch, 8U, uint64_t);
    INTEGER(predecessor_last_sequence, 8U, uint64_t);
    BYTES(predecessor_header_hash, 32U);
    BYTES(predecessor_state_root, 32U);
    BYTES(predecessor_checkpoint_id, 32U);
    INTEGER(activation_batch, 8U, uint64_t);
    BYTES(finality_evidence_digest, 32U);
    BYTES(governance_signature, 64U);
#undef BYTES
#undef INTEGER
    if (offset != encoded.length) return LXP_FATAL_INVARIANT;
    status = certificate_validate(&parsed);
    if (status == LXP_OK) *certificate = parsed;
    return status;
}

lxp_result lxp_handover_certificate_sign(lxp_handover_certificate *certificate,
                                         const uint8_t private_key[32])
{
    uint8_t bytes[LXP_HANDOVER_CERTIFICATE_BYTES];
    uint8_t signature[64];
    size_t signature_length = sizeof(signature);
    EVP_PKEY *key;
    EVP_MD_CTX *context;
    lxp_result status;
    int signed_ok;
    if (private_key == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_handover_certificate_encode(certificate, bytes);
    if (status != LXP_OK) return status;
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, private_key, 32U);
    context = key == NULL ? NULL : EVP_MD_CTX_new();
    signed_ok = context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, signature, &signature_length,
                       bytes, sizeof(bytes) - sizeof(signature)) == 1 &&
        signature_length == sizeof(signature);
    if (signed_ok) (void)memcpy(certificate->governance_signature, signature, sizeof(signature));
    lxp_secure_zero(signature, sizeof(signature));
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return signed_ok ? LXP_OK : LXP_ERR_BAD_SIGNATURE;
}

lxp_result lxp_handover_certificate_verify(const lxp_handover_certificate *certificate,
                                           const uint8_t public_key[32])
{
    uint8_t bytes[LXP_HANDOVER_CERTIFICATE_BYTES];
    lxp_result status;
    if (public_key == NULL || certificate == NULL ||
        memcmp(public_key, certificate->old_public_key, 32U) == 0 ||
        memcmp(public_key, certificate->new_public_key, 32U) == 0)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_handover_certificate_encode(certificate, bytes);
    if (status == LXP_OK)
        status = lxp_ed25519_verify_raw(public_key, certificate->governance_signature,
                                         bytes, sizeof(bytes) - 64U);
    return status;
}

lxp_result lxp_handover_finality_digest(lxp_byte_span checkpoint_payload,
                                        lxp_byte_span finality_proof,
                                        uint8_t digest[32])
{
    lxp_hash_context hash;
    uint8_t length[4];
    lxp_result status;
    if (digest == NULL || checkpoint_payload.bytes == NULL || checkpoint_payload.length == 0U ||
        finality_proof.bytes == NULL || finality_proof.length == 0U ||
        checkpoint_payload.length > LXP_HANDOVER_MAX_EVIDENCE_BYTES ||
        finality_proof.length > LXP_HANDOVER_MAX_EVIDENCE_BYTES - checkpoint_payload.length)
        return LXP_ERR_LENGTH_LIMIT;
    lxp_hash_init(&hash);
    status = lxp_hash_update(&hash, finality_domain, sizeof(finality_domain));
    write_integer(length, checkpoint_payload.length, 4U);
    if (status == LXP_OK) status = lxp_hash_update(&hash, length, sizeof(length));
    if (status == LXP_OK) status = lxp_hash_update(&hash, checkpoint_payload.bytes, checkpoint_payload.length);
    write_integer(length, finality_proof.length, 4U);
    if (status == LXP_OK) status = lxp_hash_update(&hash, length, sizeof(length));
    if (status == LXP_OK) status = lxp_hash_update(&hash, finality_proof.bytes, finality_proof.length);
    return status == LXP_OK ? lxp_hash_final(&hash, digest) : status;
}

static lxp_result evidence_validate(const lxp_handover_evidence *evidence)
{
    lxp_batch_header header;
    uint8_t digest[32];
    size_t fixed = sizeof(evidence_domain) + LXP_HANDOVER_CERTIFICATE_BYTES + 354U + 64U + 12U;
    lxp_result status;
    if (evidence == NULL || evidence->predecessor_header.bytes == NULL ||
        evidence->predecessor_header.length != 354U ||
        evidence->checkpoint_payload.length > LXP_HANDOVER_MAX_EVIDENCE_BYTES - fixed ||
        evidence->finality_proof.length > LXP_HANDOVER_MAX_EVIDENCE_BYTES - fixed -
                                            evidence->checkpoint_payload.length)
        return LXP_ERR_LENGTH_LIMIT;
    status = certificate_validate(&evidence->certificate);
    if (status == LXP_OK)
        status = lxp_batch_header_decode(evidence->predecessor_header.bytes,
                                          evidence->predecessor_header.length, &header);
    if (status == LXP_OK &&
        (header.protocol_version != evidence->certificate.protocol_version ||
         header.network_id != evidence->certificate.network_id ||
         header.epoch != evidence->certificate.old_epoch ||
         header.batch_number != evidence->certificate.predecessor_batch ||
         header.last_sequence != evidence->certificate.predecessor_last_sequence ||
         memcmp(header.sequencer_id, evidence->certificate.old_sequencer_id, 32U) != 0 ||
         memcmp(header.resulting_state_root, evidence->certificate.predecessor_state_root, 32U) != 0))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_handover_finality_digest(evidence->checkpoint_payload,
                                              evidence->finality_proof, digest);
    if (status == LXP_OK &&
        memcmp(digest, evidence->certificate.finality_evidence_digest, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    return status;
}

lxp_result lxp_handover_evidence_encode(const lxp_handover_evidence *evidence,
                                        lxp_arena *arena, lxp_byte_span *encoded)
{
    uint8_t certificate[LXP_HANDOVER_CERTIFICATE_BYTES];
    size_t length;
    size_t offset = sizeof(evidence_domain);
    void *memory;
    uint8_t *bytes;
    lxp_result status;
    if (arena == NULL || encoded == NULL) return LXP_ERR_NON_CANONICAL;
    status = evidence_validate(evidence);
    if (status == LXP_OK)
        status = lxp_handover_certificate_encode(&evidence->certificate, certificate);
    if (status != LXP_OK) return status;
    length = offset + sizeof(certificate) + 12U + evidence->predecessor_header.length +
        sizeof(evidence->predecessor_signature) + evidence->checkpoint_payload.length +
        evidence->finality_proof.length;
    status = lxp_arena_alloc(arena, length, 1U, &memory);
    if (status != LXP_OK) return status;
    bytes = memory;
    (void)memcpy(bytes, evidence_domain, offset);
    (void)memcpy(bytes + offset, certificate, sizeof(certificate));
    offset += sizeof(certificate);
#define SPAN(field) do { \
    write_integer(bytes + offset, evidence->field.length, 4U); offset += 4U; \
    (void)memcpy(bytes + offset, evidence->field.bytes, evidence->field.length); \
    offset += evidence->field.length; \
} while (0)
    SPAN(predecessor_header);
    (void)memcpy(bytes + offset, evidence->predecessor_signature, sizeof(evidence->predecessor_signature));
    offset += sizeof(evidence->predecessor_signature);
    SPAN(checkpoint_payload);
    SPAN(finality_proof);
#undef SPAN
    if (offset != length) return LXP_FATAL_INVARIANT;
    *encoded = (lxp_byte_span){bytes, length};
    return LXP_OK;
}

static lxp_result read_span(lxp_byte_span encoded, size_t *offset,
                            size_t maximum, lxp_byte_span *span)
{
    size_t length;
    if (*offset > encoded.length || encoded.length - *offset < 4U)
        return LXP_ERR_TRUNCATED;
    length = (size_t)read_integer(encoded.bytes + *offset, 4U);
    *offset += 4U;
    if (length == 0U || length > maximum) return LXP_ERR_LENGTH_LIMIT;
    if (length > encoded.length - *offset) return LXP_ERR_TRUNCATED;
    *span = (lxp_byte_span){encoded.bytes + *offset, length};
    *offset += length;
    return LXP_OK;
}

lxp_result lxp_handover_evidence_decode(lxp_byte_span encoded,
                                        lxp_handover_evidence *evidence)
{
    lxp_handover_evidence parsed = {0};
    size_t offset = sizeof(evidence_domain);
    lxp_result status;
    if (evidence == NULL || encoded.bytes == NULL ||
        encoded.length < offset + LXP_HANDOVER_CERTIFICATE_BYTES ||
        encoded.length > LXP_HANDOVER_MAX_EVIDENCE_BYTES ||
        memcmp(encoded.bytes, evidence_domain, offset) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_handover_certificate_decode(
        (lxp_byte_span){encoded.bytes + offset, LXP_HANDOVER_CERTIFICATE_BYTES}, &parsed.certificate);
    offset += LXP_HANDOVER_CERTIFICATE_BYTES;
    if (status == LXP_OK) status = read_span(encoded, &offset, 354U, &parsed.predecessor_header);
    if (status == LXP_OK) {
        if (encoded.length - offset < sizeof(parsed.predecessor_signature))
            return LXP_ERR_TRUNCATED;
        (void)memcpy(parsed.predecessor_signature, encoded.bytes + offset, sizeof(parsed.predecessor_signature));
        offset += sizeof(parsed.predecessor_signature);
    }
    if (status == LXP_OK)
        status = read_span(encoded, &offset, LXP_HANDOVER_MAX_EVIDENCE_BYTES, &parsed.checkpoint_payload);
    if (status == LXP_OK)
        status = read_span(encoded, &offset, LXP_HANDOVER_MAX_EVIDENCE_BYTES, &parsed.finality_proof);
    if (status == LXP_OK && offset != encoded.length) status = LXP_ERR_TRAILING_BYTES;
    if (status == LXP_OK) status = evidence_validate(&parsed);
    if (status == LXP_OK) *evidence = parsed;
    return status;
}

lxp_result lxp_handover_evidence_verify_binding(const lxp_handover_evidence *evidence,
    const uint8_t governance_public_key[32], const lxp_sequencer_authorization *previous,
    uint64_t previous_epoch, uint64_t previous_batch, uint64_t next_sequence,
    const uint8_t previous_state_root[32], lxp_arena *arena)
{
    lxp_batch_header header;
    uint8_t digest[32];
    const lxp_handover_certificate *certificate;
    lxp_result status;
    if (evidence == NULL || governance_public_key == NULL || previous == NULL ||
        previous_state_root == NULL || arena == NULL || next_sequence == 0U)
        return LXP_ERR_NON_CANONICAL;
    certificate = &evidence->certificate;
    status = evidence_validate(evidence);
    if (status == LXP_OK)
        status = lxp_handover_certificate_verify(certificate, governance_public_key);
    if (status == LXP_OK &&
        (!previous->authorized || certificate->old_epoch != previous_epoch ||
         certificate->predecessor_batch != previous_batch ||
         certificate->predecessor_last_sequence != next_sequence - 1U ||
         previous_batch < previous->first_batch_number || previous_batch > previous->last_batch_number ||
         memcmp(certificate->old_sequencer_id, previous->sequencer_id, 32U) != 0 ||
         memcmp(certificate->old_public_key, previous->public_key, 32U) != 0 ||
         memcmp(certificate->predecessor_state_root, previous_state_root, 32U) != 0))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_batch_header_decode(evidence->predecessor_header.bytes,
                                          evidence->predecessor_header.length, &header);
    if (status == LXP_OK)
        status = lxp_batch_verify_signature(&header, evidence->predecessor_signature,
                                             sizeof(evidence->predecessor_signature), previous, arena);
    if (status == LXP_OK) status = lxp_batch_header_hash(&header, arena, digest);
    if (status == LXP_OK && memcmp(digest, certificate->predecessor_header_hash, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    return status;
}

bool lxp_handover_recovery_is_envelope(lxp_byte_span encoded)
{
    return encoded.bytes != NULL && encoded.length >= sizeof(recovery_domain) &&
        memcmp(encoded.bytes, recovery_domain, sizeof(recovery_domain)) == 0;
}

static lxp_result recovery_structure(lxp_byte_span encoded)
{
    lxp_codec_reader reader;
    lxp_byte_span value;
    uint32_t count;
    uint16_t previous = 0U, module;
    uint64_t next, receipt, projection;
    lxp_result status = lxp_codec_reader_init(&reader, encoded.bytes, encoded.length);
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK && count > LXP_DA_MAX_MODULE_ROOTS) status = LXP_ERR_LENGTH_LIMIT;
    for (uint32_t i = 0U; status == LXP_OK && i < count; ++i) {
        status = lxp_codec_read_u16(&reader, &module);
        if (status == LXP_OK && i != 0U && module <= previous) status = LXP_ERR_UNSORTED_SEQUENCE;
        if (status == LXP_OK) status = lxp_codec_read_bytes(&reader, &value, 32U);
        if (status == LXP_OK && value.length != 32U) status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK) previous = module;
    }
    if (status == LXP_OK)
        status = lxp_codec_read_bytes(&reader, &value, LXP_DA_MAX_ACCOUNT_FRONTIER_BYTES);
    if (status == LXP_OK) status = lxp_codec_read_u64(&reader, &next);
    if (status == LXP_OK) status = lxp_codec_read_u64(&reader, &receipt);
    if (status == LXP_OK) status = lxp_codec_read_u64(&reader, &projection);
    if (status == LXP_OK && (next == 0U || receipt >= next || projection > receipt))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    return status;
}

lxp_result lxp_handover_recovery_encode(lxp_byte_span canonical_recovery,
    lxp_byte_span canonical_evidence, lxp_arena *arena, lxp_byte_span *encoded)
{
    lxp_handover_evidence evidence;
    size_t offset = sizeof(recovery_domain);
    size_t length;
    uint8_t *bytes;
    void *memory;
    lxp_result status;
    if (arena == NULL || encoded == NULL || canonical_recovery.bytes == NULL ||
        canonical_recovery.length == 0U ||
        canonical_recovery.length > LXP_HANDOVER_MAX_RECOVERY_BYTES - offset - 8U ||
        canonical_evidence.length > LXP_HANDOVER_MAX_RECOVERY_BYTES - offset - 8U -
                                        canonical_recovery.length ||
        lxp_handover_recovery_is_envelope(canonical_recovery))
        return LXP_ERR_LENGTH_LIMIT;
    status = recovery_structure(canonical_recovery);
    if (status == LXP_OK) status = lxp_handover_evidence_decode(canonical_evidence, &evidence);
    if (status != LXP_OK) return status;
    length = offset + 8U + canonical_recovery.length + canonical_evidence.length;
    status = lxp_arena_alloc(arena, length, 1U, &memory);
    if (status != LXP_OK) return status;
    bytes = memory;
    (void)memcpy(bytes, recovery_domain, offset);
    write_integer(bytes + offset, canonical_recovery.length, 4U); offset += 4U;
    (void)memcpy(bytes + offset, canonical_recovery.bytes, canonical_recovery.length);
    offset += canonical_recovery.length;
    write_integer(bytes + offset, canonical_evidence.length, 4U); offset += 4U;
    (void)memcpy(bytes + offset, canonical_evidence.bytes, canonical_evidence.length);
    *encoded = (lxp_byte_span){bytes, length};
    return LXP_OK;
}

lxp_result lxp_handover_recovery_decode(lxp_byte_span encoded,
    lxp_byte_span *canonical_recovery, lxp_byte_span *canonical_evidence)
{
    lxp_handover_evidence evidence;
    lxp_byte_span recovery, packet;
    size_t offset = sizeof(recovery_domain);
    lxp_result status;
    if (canonical_recovery == NULL || canonical_evidence == NULL ||
        !lxp_handover_recovery_is_envelope(encoded) ||
        encoded.length > LXP_HANDOVER_MAX_RECOVERY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = read_span(encoded, &offset, LXP_HANDOVER_MAX_RECOVERY_BYTES, &recovery);
    if (status == LXP_OK && lxp_handover_recovery_is_envelope(recovery))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) status = recovery_structure(recovery);
    if (status == LXP_OK)
        status = read_span(encoded, &offset, LXP_HANDOVER_MAX_EVIDENCE_BYTES, &packet);
    if (status == LXP_OK && offset != encoded.length) status = LXP_ERR_TRAILING_BYTES;
    if (status == LXP_OK) status = lxp_handover_evidence_decode(packet, &evidence);
    if (status == LXP_OK) {
        *canonical_recovery = recovery;
        *canonical_evidence = packet;
    }
    return status;
}

static const uint8_t history_current_key[32] = "handover-current";

static lxp_result history_key(uint64_t epoch, uint8_t key[32])
{
    static const uint8_t domain[] = "LXP/handover-history/v1";
    uint8_t preimage[sizeof(domain) + 8U];
    (void)memcpy(preimage, domain, sizeof(domain));
    write_integer(preimage + sizeof(domain), epoch, 8U);
    return lxp_hash_sha256(preimage, sizeof(preimage), key);
}

static lxp_result history_epoch(const lxp_kernel *kernel, uint64_t *epoch)
{
    bool found = false;
    *epoch = 1U;
    if (kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_FATAL_INVARIANT;
    for (size_t i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_GOVERNANCE || entry->key_length != 32U ||
            memcmp(entry->key, history_current_key, 32U) != 0) continue;
        if (found || entry->value_length != 8U) return LXP_FATAL_INVARIANT;
        *epoch = read_integer(entry->value, 8U);
        if (*epoch < 2U || *epoch - 1U > LXP_KERNEL_MAX_BLOBS)
            return LXP_FATAL_INVARIANT;
        found = true;
    }
    return LXP_OK;
}

static lxp_result history_blob(const lxp_kernel *kernel, uint64_t epoch,
    lxp_byte_span *encoded)
{
    uint8_t key[32], digest[32];
    bool found = false;
    lxp_result status = history_key(epoch, key);
    if (status != LXP_OK) return status;
    if (kernel->blob_count > LXP_KERNEL_MAX_BLOBS ||
        kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV) return LXP_FATAL_INVARIANT;
    for (size_t i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_GOVERNANCE || entry->key_length != 32U ||
            memcmp(entry->key, key, 32U) != 0) continue;
        if (found || entry->value_length != 32U) return LXP_FATAL_INVARIANT;
        (void)memcpy(digest, entry->value, 32U);
        found = true;
    }
    if (!found) return LXP_ERR_UNKNOWN_FIELD;
    found = false;
    *encoded = (lxp_byte_span){NULL, 0U};
    for (size_t i = 0U; i < kernel->blob_count; ++i) {
        const lxp_module_blob *blob = &kernel->blobs[i];
        if (blob->module_id != LXP_MODULE_GOVERNANCE || memcmp(blob->key, digest, 32U) != 0)
            continue;
        if (found || blob->deleted || blob->bytes == NULL || blob->length == 0U ||
            blob->length > LXP_HANDOVER_MAX_EVIDENCE_BYTES)
            return LXP_FATAL_INVARIANT;
        uint8_t actual[32];
        status = lxp_hash_sha256(blob->bytes, blob->length, actual);
        if (status != LXP_OK) return status;
        if (memcmp(actual, digest, 32U) != 0) return LXP_ERR_ROOT_MISMATCH;
        *encoded = (lxp_byte_span){blob->bytes, blob->length};
        found = true;
    }
    return found ? LXP_OK : LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lxp_handover_kernel_initialize(lxp_kernel *kernel,
    const lxp_genesis_manifest *manifest, lxp_handover_finality_verify_fn verify,
    void *context)
{
    lxp_handover_state state = {0};
    lxp_result status;
    if (kernel == NULL || manifest == NULL || ((verify == NULL) != (context == NULL)))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_handover_genesis_authority(manifest, state.governance_public_key,
                                            &state.enabled);
    if (status != LXP_OK) return status;
    if (state.enabled) {
        state.network_id = manifest->network_id;
        (void)memcpy(state.genesis_authorization.public_key, manifest->signer_public_key, 32U);
        status = lxp_handover_sequencer_id(manifest->signer_public_key,
                                           state.genesis_authorization.sequencer_id);
        if (status != LXP_OK) return status;
        state.genesis_authorization.authorized = 1U;
        state.genesis_authorization.first_batch_number = 1U;
        state.genesis_authorization.last_batch_number = UINT64_MAX;
        state.verify_finality = verify;
        state.finality_context = context;
    }
    kernel->handover = state;
    return LXP_OK;
}

lxp_result lxp_handover_history_latest(const lxp_kernel *kernel,
    lxp_byte_span *encoded)
{
    uint64_t epoch;
    lxp_result status;
    if (kernel == NULL || encoded == NULL) return LXP_ERR_NON_CANONICAL;
    *encoded = (lxp_byte_span){NULL, 0U};
    status = history_epoch(kernel, &epoch);
    if (status != LXP_OK || epoch == 1U) return status;
    if (!kernel->handover.enabled) return LXP_ERR_AUTH_SCOPE;
    return history_blob(kernel, epoch, encoded);
}

static lxp_result history_resolve(const lxp_kernel *kernel,
    uint64_t value, bool by_sequence, lxp_sequencer_authorization *authorization,
    uint64_t *epoch, lxp_arena *arena)
{
    lxp_sequencer_authorization current, selected = {0};
    uint64_t latest, selected_epoch = 0U, first_sequence = 1U;
    lxp_result status;
    if (kernel == NULL || authorization == NULL || epoch == NULL || arena == NULL ||
        !kernel->handover.enabled || value == 0U)
        return LXP_ERR_AUTH_SCOPE;
    status = history_epoch(kernel, &latest);
    if (status != LXP_OK) return status;
    current = kernel->handover.genesis_authorization;
    for (uint64_t number = 2U; number <= latest; ++number) {
        lxp_byte_span encoded;
        lxp_handover_evidence evidence;
        size_t mark = lxp_arena_mark(arena);
        status = history_blob(kernel, number, &encoded);
        if (status == LXP_OK) status = lxp_handover_evidence_decode(encoded, &evidence);
        if (status == LXP_OK && (evidence.certificate.new_epoch != number ||
            evidence.certificate.network_id != kernel->handover.network_id ||
            evidence.certificate.predecessor_batch < current.first_batch_number ||
            evidence.certificate.predecessor_last_sequence < first_sequence))
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK)
            status = lxp_handover_evidence_verify_binding(&evidence,
                kernel->handover.governance_public_key, &current, number - 1U,
                evidence.certificate.predecessor_batch,
                evidence.certificate.predecessor_last_sequence + 1U,
                evidence.certificate.predecessor_state_root, arena);
        if (lxp_arena_reset(arena, mark) != LXP_OK) return LXP_FATAL_INVARIANT;
        if (status != LXP_OK) return status;
        current.last_batch_number = evidence.certificate.predecessor_batch;
        if (by_sequence ? value >= first_sequence && value <= evidence.certificate.predecessor_last_sequence :
            value >= current.first_batch_number && value <= current.last_batch_number) {
            selected = current;
            selected_epoch = number - 1U;
        }
        (void)memcpy(current.public_key, evidence.certificate.new_public_key, 32U);
        (void)memcpy(current.sequencer_id, evidence.certificate.new_sequencer_id, 32U);
        current.first_batch_number = evidence.certificate.activation_batch;
        current.last_batch_number = UINT64_MAX;
        first_sequence = evidence.certificate.predecessor_last_sequence + 1U;
    }
    if (value >= (by_sequence ? first_sequence : current.first_batch_number)) {
        selected = current;
        selected_epoch = latest;
    }
    if (selected_epoch == 0U) return LXP_ERR_AUTH_SCOPE;
    *authorization = selected;
    *epoch = selected_epoch;
    return LXP_OK;
}

lxp_result lxp_handover_history_resolve(const lxp_kernel *kernel,
    uint64_t batch_number, lxp_sequencer_authorization *authorization,
    uint64_t *epoch, lxp_arena *arena)
{
    return history_resolve(kernel, batch_number, false, authorization, epoch, arena);
}

lxp_result lxp_handover_history_resolve_sequence(const lxp_kernel *kernel,
    uint64_t global_sequence, lxp_sequencer_authorization *authorization,
    uint64_t *epoch, lxp_arena *arena)
{
    return history_resolve(kernel, global_sequence, true, authorization, epoch, arena);
}

lxp_result lxp_handover_prepare(lxp_kernel *kernel,
    const lxp_activity *activity, uint64_t batch_number, lxp_arena *arena)
{
    lxp_handover_evidence evidence;
    lxp_sequencer_authorization previous;
    uint64_t previous_epoch;
    lxp_result status;
    if (kernel == NULL || activity == NULL || arena == NULL ||
        activity->activity_type != LXP_GOVERNANCE_HANDOVER ||
        activity->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT ||
        !kernel->handover.enabled || kernel->handover.pending ||
        kernel->handover.verify_finality == NULL || kernel->state == NULL ||
        activity->network_id != kernel->handover.network_id || batch_number < 2U ||
        activity->authority.length != 32U || activity->authority.bytes == NULL ||
        memcmp(activity->authority.bytes, kernel->handover.governance_public_key, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK) status = lxp_handover_evidence_decode(activity->payload, &evidence);
    if (status == LXP_OK && evidence.certificate.activation_batch != batch_number)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_handover_history_resolve(kernel, batch_number - 1U,
                                               &previous, &previous_epoch, arena);
    if (status == LXP_OK && previous_epoch != kernel->epoch)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_handover_evidence_verify_binding(&evidence,
            kernel->handover.governance_public_key, &previous, kernel->epoch,
            batch_number - 1U, kernel->state->next_sequence, kernel->current_state_root, arena);
    if (status == LXP_OK)
        status = kernel->handover.verify_finality(kernel->handover.finality_context,
                                                   &evidence, arena);
    if (status == LXP_OK)
        status = lxp_hash_sha256(activity->payload.bytes, activity->payload.length,
                                 kernel->handover.pending_evidence_digest);
    if (status == LXP_OK) {
        kernel->handover.pending_certificate = evidence.certificate;
        kernel->handover.pending = true;
        kernel->epoch = evidence.certificate.new_epoch;
    }
    return status;
}

lxp_result lxp_handover_stage(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority)
{
    lxp_handover_evidence evidence;
    uint8_t digest[32], key[32], epoch[8];
    const uint8_t *prior;
    size_t prior_length;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || ctx->kernel == NULL ||
        !ctx->mutable || ctx->module_id != LXP_MODULE_GOVERNANCE ||
        activity->activity_type != LXP_GOVERNANCE_HANDOVER ||
        !ctx->kernel->handover.enabled || !ctx->kernel->handover.pending ||
        authority->kind != LXP_AUTHORITY_OWNER ||
        memcmp(authority->verified_key, ctx->kernel->handover.governance_public_key, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_hash_sha256(activity->payload.bytes, activity->payload.length, digest);
    if (status == LXP_OK && memcmp(digest, ctx->kernel->handover.pending_evidence_digest, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) status = lxp_handover_evidence_decode(activity->payload, &evidence);
    if (status == LXP_OK && (evidence.certificate.new_epoch != ctx->epoch ||
        evidence.certificate.activation_batch != ctx->batch_number ||
        evidence.certificate.predecessor_last_sequence + 1U != ctx->global_sequence))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) status = history_key(evidence.certificate.new_epoch, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &prior, &prior_length);
    if (status != LXP_ERR_UNKNOWN_FIELD) return status == LXP_OK ? LXP_ERR_SEQUENCE_REUSED : status;
    status = lxp_ctx_blob_put(ctx, digest, activity->payload.bytes, activity->payload.length);
    if (status == LXP_OK) status = lxp_ctx_kv_put(ctx, key, sizeof(key), digest, sizeof(digest));
    write_integer(epoch, evidence.certificate.new_epoch, sizeof(epoch));
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(ctx, history_current_key, 32U, epoch, sizeof(epoch));
    if (status == LXP_OK) status = lxp_ctx_emit_event(ctx, 0x7109U, digest, sizeof(digest));
    return status;
}

lxp_result lxp_handover_incoming(const lxp_kernel *kernel,
    const lxp_batch_body *body, lxp_arena *arena,
    lxp_sequencer_authorization *authorization, lxp_handover_state *prospective)
{
    lxp_byte_span *activities, packet = {NULL, 0U}, recovery, latest;
    lxp_activity activity;
    lxp_kernel *candidate = NULL;
    uint64_t epoch;
    size_t count, mark;
    bool activation;
    lxp_result status;
    if (kernel == NULL || kernel->state == NULL || body == NULL || arena == NULL ||
        authorization == NULL || prospective == NULL || body->header.batch_number == 0U ||
        body->header.first_sequence != kernel->state->next_sequence ||
        memcmp(body->header.previous_state_root, kernel->current_state_root, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    *prospective = kernel->handover;
    if (!kernel->handover.enabled)
        return body->header.epoch == kernel->epoch &&
            !lxp_handover_recovery_is_envelope(body->recovery_metadata) ? LXP_OK : LXP_ERR_AUTH_SCOPE;
    if (body->header.network_id != kernel->handover.network_id ||
        body->header.protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT ||
        body->header.epoch < kernel->epoch || kernel->handover.pending)
        return LXP_ERR_CONTEXT_MISMATCH;
    activation = body->header.epoch != kernel->epoch;
    if (activation && (kernel->epoch == UINT64_MAX || body->header.epoch != kernel->epoch + 1U))
        return LXP_ERR_AUTH_SCOPE;
    mark = lxp_arena_mark(arena);
    status = lxp_replay_section_decode(&body->activities, arena, &activities, &count);
    for (size_t i = 0U; status == LXP_OK && i < count; ++i) {
        status = lxp_activity_decode(activities[i].bytes, activities[i].length, &activity);
        if (status == LXP_OK && (activity.activity_type == LXP_GOVERNANCE_HANDOVER) != activation)
            status = LXP_ERR_AUTH_SCOPE;
    }
    if (status == LXP_OK && activation && count != 1U) status = LXP_ERR_BATCH_GAP;
    if (status == LXP_OK && lxp_handover_recovery_is_envelope(body->recovery_metadata))
        status = lxp_handover_recovery_decode(body->recovery_metadata, &recovery, &packet);
    if (status == LXP_OK && activation && (packet.length == 0U ||
        packet.length != activity.payload.length ||
        memcmp(packet.bytes, activity.payload.bytes, packet.length) != 0 ||
        body->header.first_sequence == UINT64_MAX ||
        body->header.last_sequence != body->header.first_sequence + 1U))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK && activation) {
        candidate = malloc(sizeof(*candidate));
        if (candidate == NULL) status = LXP_ERR_ARENA_EXHAUSTED;
        else {
            *candidate = *kernel;
            status = lxp_handover_prepare(candidate, &activity, body->header.batch_number, arena);
        }
        if (status == LXP_OK) {
            *prospective = candidate->handover;
            (void)memset(authorization, 0, sizeof(*authorization));
            (void)memcpy(authorization->public_key, prospective->pending_certificate.new_public_key, 32U);
            (void)memcpy(authorization->sequencer_id, prospective->pending_certificate.new_sequencer_id, 32U);
            authorization->first_batch_number = body->header.batch_number;
            authorization->last_batch_number = UINT64_MAX;
            authorization->authorized = 1U;
        }
    } else if (status == LXP_OK) {
        status = lxp_handover_history_latest(kernel, &latest);
        if (status == LXP_OK && (latest.length != packet.length ||
            (latest.length != 0U && memcmp(latest.bytes, packet.bytes, latest.length) != 0)))
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK)
            status = lxp_handover_history_resolve(kernel, body->header.batch_number,
                                                   authorization, &epoch, arena);
        if (status == LXP_OK && epoch != body->header.epoch) status = LXP_ERR_CONTEXT_MISMATCH;
    }
    free(candidate);
    if (lxp_arena_reset(arena, mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    return status;
}

lxp_result lxp_handover_trust_initialize(lxp_handover_trust_chain *chain,
    const lxp_genesis_manifest *manifest)
{
    bool enabled;
    lxp_result status;
    if (chain == NULL || manifest == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(chain, 0, sizeof(*chain));
    status = lxp_handover_genesis_authority(manifest, chain->governance_public_key, &enabled);
    if (status != LXP_OK || !enabled) return status == LXP_OK ? LXP_ERR_AUTH_SCOPE : status;
    chain->network_id = manifest->network_id;
    (void)memcpy(chain->genesis_authorization.public_key, manifest->signer_public_key, 32U);
    status = lxp_handover_sequencer_id(manifest->signer_public_key,
        chain->genesis_authorization.sequencer_id);
    if (status != LXP_OK) return status;
    chain->genesis_authorization.first_batch_number = 1U;
    chain->genesis_authorization.last_batch_number = UINT64_MAX;
    chain->genesis_authorization.authorized = 1U;
    chain->current_authorization = chain->genesis_authorization;
    chain->epoch = 1U;
    chain->predecessor.network_id = manifest->network_id;
    chain->predecessor.protocol_version = manifest->protocol_version;
    (void)memcpy(chain->predecessor.resulting_state_root, manifest->genesis_receipt_state_root, 32U);
    return LXP_OK;
}

lxp_result lxp_handover_trust_accept(lxp_handover_trust_chain *chain,
    const lxp_batch_body *body, lxp_handover_trust_finality_fn verify_finality,
    void *context, lxp_arena *arena)
{
    lxp_sequencer_authorization authorization;
    lxp_handover_evidence evidence;
    lxp_activity activation;
    lxp_receipt *receipt = NULL;
    lxp_byte_span *activities, *receipts, *events, *oracles;
    lxp_byte_span recovery, packet = {NULL, 0U}, previous;
    lxp_batch_roots roots;
    size_t activity_count = 0U, receipt_count, event_count, oracle_count, mark;
    uint8_t digest[32], availability[32], activity_id[32];
    bool transition;
    lxp_result status;
    if (chain == NULL || body == NULL || arena == NULL || verify_finality == NULL || context == NULL ||
        chain->network_id == 0U || chain->transition_count > LXP_HANDOVER_MAX_TRANSITIONS ||
        body->header.protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT ||
        body->header.network_id != chain->network_id || chain->predecessor.batch_number == UINT64_MAX ||
        body->header.batch_number != chain->predecessor.batch_number + 1U ||
        chain->predecessor.last_sequence == UINT64_MAX ||
        body->header.first_sequence != chain->predecessor.last_sequence + 1U ||
        body->header.last_sequence < body->header.first_sequence || body->header.last_sequence == UINT64_MAX ||
        memcmp(body->header.previous_state_root, chain->predecessor.resulting_state_root, 32U) != 0 ||
        body->header.epoch < chain->epoch)
        return LXP_ERR_CONTEXT_MISMATCH;
    transition = body->header.epoch != chain->epoch;
    if (transition && (chain->epoch == UINT64_MAX || body->header.epoch != chain->epoch + 1U ||
        chain->transition_count == LXP_HANDOVER_MAX_TRANSITIONS)) return LXP_ERR_AUTH_SCOPE;
    mark = lxp_arena_mark(arena);
    authorization = chain->current_authorization;
    status = lxp_batch_availability_root(body, arena, availability);
    if (status == LXP_OK && memcmp(availability, body->header.data_availability_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_replay_section_decode(&body->activities, arena, &activities, &activity_count);
    if (status == LXP_OK)
        status = lxp_da_receipt_section_decode(body->receipts, arena, &receipts, &receipt_count,
            &events, &event_count);
    if (status == LXP_OK)
        status = lxp_replay_section_decode(&body->oracle_inputs, arena, &oracles, &oracle_count);
    if (status == LXP_OK)
        status = lxp_batch_roots_compute(&(lxp_batch_root_inputs){activities, activity_count,
            receipts, receipt_count, events, event_count, oracles, oracle_count, NULL, 0U}, arena, &roots);
    if (status == LXP_OK && (memcmp(roots.activity_merkle_root, body->header.activity_merkle_root, 32U) != 0 ||
        memcmp(roots.receipt_merkle_root, body->header.receipt_merkle_root, 32U) != 0 ||
        memcmp(roots.event_merkle_root, body->header.event_merkle_root, 32U) != 0 ||
        memcmp(roots.oracle_root, body->header.oracle_root, 32U) != 0)) status = LXP_ERR_ROOT_MISMATCH;
    for (size_t i = 0U; status == LXP_OK && i < activity_count; ++i) {
        status = lxp_activity_decode(activities[i].bytes, activities[i].length, &activation);
        if (status == LXP_OK && (activation.activity_type == LXP_GOVERNANCE_HANDOVER) != transition)
            status = LXP_ERR_AUTH_SCOPE;
    }
    if (status == LXP_OK && lxp_handover_recovery_is_envelope(body->recovery_metadata))
        status = lxp_handover_recovery_decode(body->recovery_metadata, &recovery, &packet);
    if (status == LXP_OK && packet.length != 0U)
        status = lxp_hash_sha256(packet.bytes, packet.length, digest);
    if (status == LXP_OK && transition) {
        if (activity_count != 1U || receipt_count != 2U || event_count != 2U || oracle_count != 0U ||
            body->header.last_sequence != body->header.first_sequence + 1U || packet.length == 0U ||
            packet.length != activation.payload.length ||
            memcmp(packet.bytes, activation.payload.bytes, packet.length) != 0)
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK) status = lxp_activity_check_envelope(&activation, chain->network_id);
        if (status == LXP_OK) status = lxp_activity_verify_payload_hash(&activation);
        if (status == LXP_OK) status = lxp_activity_verify_signature(&activation);
        if (status == LXP_OK && (activation.protocol_version != body->header.protocol_version ||
            activation.authority.length != 32U ||
            memcmp(activation.authority.bytes, chain->governance_public_key, 32U) != 0))
            status = LXP_ERR_AUTH_SCOPE;
        if (status == LXP_OK)
            status = lxp_activity_check_timestamp_bound(activation.timestamp_bound,
                body->header.timestamp_ms, UINT64_C(300000));
        if (status == LXP_OK) status = lxp_handover_evidence_decode(packet, &evidence);
        if (status == LXP_OK)
            status = lxp_handover_evidence_verify_binding(&evidence, chain->governance_public_key,
                &authorization, chain->epoch, chain->predecessor.batch_number,
                body->header.first_sequence, body->header.previous_state_root, arena);
        if (status == LXP_OK) status = lxp_batch_header_encode(&chain->predecessor, arena, &previous);
        if (status == LXP_OK && (previous.length != evidence.predecessor_header.length ||
            memcmp(previous.bytes, evidence.predecessor_header.bytes, previous.length) != 0 ||
            memcmp(chain->predecessor_signature, evidence.predecessor_signature, 64U) != 0))
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK)
            status = verify_finality(context, &chain->predecessor, chain->predecessor_signature,
                &evidence, arena);
        if (status == LXP_OK) {
            (void)memcpy(authorization.public_key, evidence.certificate.new_public_key, 32U);
            (void)memcpy(authorization.sequencer_id, evidence.certificate.new_sequencer_id, 32U);
            authorization.first_batch_number = evidence.certificate.activation_batch;
            authorization.last_batch_number = UINT64_MAX;
            receipt = malloc(sizeof(*receipt));
            if (receipt == NULL) status = LXP_ERR_ARENA_EXHAUSTED;
        }
        if (status == LXP_OK) status = lxp_receipt_decode(receipts[0].bytes, receipts[0].length, true, receipt);
        if (status == LXP_OK) status = lxp_receipt_verify(receipt, authorization.public_key, arena);
        if (status == LXP_OK) status = lxp_activity_id(activities[0].bytes, activities[0].length, activity_id);
        if (status == LXP_OK && (receipt->protocol_version != body->header.protocol_version ||
            receipt->module_id != LXP_MODULE_GOVERNANCE || receipt->module_version != 1U ||
            receipt->result_code != LXP_OK || receipt->global_sequence != body->header.first_sequence ||
            receipt->timestamp != body->header.timestamp_ms ||
            memcmp(receipt->activity_id, activity_id, 32U) != 0 ||
            memcmp(receipt->previous_state_root, body->header.previous_state_root, 32U) != 0 ||
            memcmp(receipt->activity_root, body->header.activity_merkle_root, 32U) != 0))
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK) {
            lxp_batch_maintenance maintenance;
            status = lxp_batch_maintenance_decode(receipts[1].bytes, receipts[1].length, &maintenance);
            if (status == LXP_OK && (maintenance.epoch != body->header.epoch ||
                maintenance.batch_number != body->header.batch_number ||
                maintenance.global_sequence != body->header.last_sequence ||
                maintenance.timestamp_ms != body->header.timestamp_ms)) status = LXP_ERR_CONTEXT_MISMATCH;
            if (status == LXP_OK) {
                lxp_programs_occupancy_receipt occupancy;
                status = lxp_programs_occupancy_receipt_decode(maintenance.occupancy.bytes,
                    maintenance.occupancy.length, &occupancy);
                if (status == LXP_OK && (memcmp(occupancy.previous_state_root,
                    receipt->resulting_state_root, 32U) != 0 ||
                    memcmp(occupancy.resulting_state_root, body->header.resulting_state_root, 32U) != 0))
                    status = LXP_ERR_CONTEXT_MISMATCH;
            }
        }
    } else if (status == LXP_OK && (chain->transition_count == 0U ? packet.length != 0U :
        packet.length == 0U || memcmp(digest, chain->evidence_digests[chain->transition_count - 1U], 32U) != 0))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) status = lxp_batch_verify_signature(&body->header,
        body->sequencer_signature, 64U, &authorization, arena);
    if (status == LXP_OK) {
        if (transition) {
            chain->transitions[chain->transition_count] = evidence.certificate;
            (void)memcpy(chain->evidence_digests[chain->transition_count], digest, 32U);
            ++chain->transition_count;
        }
        chain->current_authorization = authorization;
        chain->predecessor = body->header;
        (void)memcpy(chain->predecessor_signature, body->sequencer_signature, 64U);
        chain->epoch = body->header.epoch;
    }
    free(receipt);
    if (lxp_arena_reset(arena, mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    return status;
}

lxp_result lxp_handover_trust_scan_log(lxp_handover_trust_chain *chain,
    const lxp_log *log, uint64_t *offset,
    lxp_handover_trust_finality_fn verify_finality, void *context, lxp_arena *arena)
{
    lxp_result status = LXP_OK;
    if (chain == NULL || log == NULL || offset == NULL || arena == NULL ||
        verify_finality == NULL || context == NULL || *offset > log->write_offset)
        return LXP_ERR_NON_CANONICAL;
    while (status == LXP_OK && *offset < log->write_offset) {
        lxp_log_record_header record;
        lxp_batch_body body;
        lxp_byte_span canonical;
        uint8_t *bytes = NULL;
        size_t mark = lxp_arena_mark(arena);
        status = lxp_log_read(log, *offset, &record, NULL, 0U);
        if (status == LXP_ERR_LENGTH_LIMIT) status = LXP_OK;
        if (status == LXP_OK && (record.record_kind != (uint8_t)LXP_LOG_BATCH_BODY ||
            record.body_length == 0U || record.body_length > LXP_MAX_BATCH_BODY_BYTES ||
            log->write_offset - *offset < LXP_LOG_HEADER_BYTES ||
            record.body_length > log->write_offset - *offset - LXP_LOG_HEADER_BYTES))
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK) {
            bytes = malloc(record.body_length);
            if (bytes == NULL) status = LXP_ERR_ARENA_EXHAUSTED;
        }
        if (status == LXP_OK) status = lxp_log_read(log, *offset, &record, bytes, record.body_length);
        if (status == LXP_OK) status = lxp_batch_body_decode(bytes, record.body_length, &body);
        if (status == LXP_OK && record.global_sequence != body.header.last_sequence)
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK) status = lxp_batch_body_encode(&body, arena, &canonical);
        if (status == LXP_OK && (canonical.length != record.body_length ||
            memcmp(canonical.bytes, bytes, canonical.length) != 0)) status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK) status = lxp_handover_trust_accept(chain,
            &body, verify_finality, context, arena);
        if (status == LXP_OK) *offset += LXP_LOG_HEADER_BYTES + record.body_length;
        free(bytes);
        if (lxp_arena_reset(arena, mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    }
    return status;
}

lxp_result lxp_handover_trust_authorization(const lxp_handover_trust_chain *chain,
    uint64_t batch_number, lxp_sequencer_authorization *authorization, uint64_t *epoch)
{
    lxp_sequencer_authorization current;
    if (chain == NULL || authorization == NULL || epoch == NULL || batch_number == 0U ||
        chain->epoch != chain->transition_count + 1U ||
        chain->transition_count > LXP_HANDOVER_MAX_TRANSITIONS) return LXP_ERR_AUTH_SCOPE;
    current = chain->genesis_authorization;
    for (size_t i = 0U; i < chain->transition_count; ++i) {
        const lxp_handover_certificate *certificate = &chain->transitions[i];
        current.last_batch_number = certificate->predecessor_batch;
        if (batch_number >= current.first_batch_number && batch_number <= current.last_batch_number) {
            *authorization = current; *epoch = i + 1U; return LXP_OK;
        }
        (void)memcpy(current.public_key, certificate->new_public_key, 32U);
        (void)memcpy(current.sequencer_id, certificate->new_sequencer_id, 32U);
        current.first_batch_number = certificate->activation_batch;
        current.last_batch_number = UINT64_MAX;
    }
    if (batch_number < current.first_batch_number) return LXP_ERR_AUTH_SCOPE;
    *authorization = current; *epoch = chain->epoch; return LXP_OK;
}

lxp_result lxp_handover_trust_authorization_sequence(const lxp_handover_trust_chain *chain,
    uint64_t global_sequence, lxp_sequencer_authorization *authorization, uint64_t *epoch)
{
    uint64_t batch = 1U;
    if (chain == NULL || global_sequence == 0U ||
        global_sequence > chain->predecessor.last_sequence ||
        chain->transition_count > LXP_HANDOVER_MAX_TRANSITIONS) return LXP_ERR_AUTH_SCOPE;
    for (size_t i = 0U; i < chain->transition_count; ++i) {
        const lxp_handover_certificate *certificate = &chain->transitions[i];
        if (global_sequence <= certificate->predecessor_last_sequence) break;
        batch = certificate->activation_batch;
    }
    return lxp_handover_trust_authorization(chain, batch, authorization, epoch);
}

lxp_result lxp_handover_trust_matches_kernel(const lxp_handover_trust_chain *chain,
    const lxp_kernel *kernel)
{
    uint64_t epoch;
    lxp_result status;
    if (chain == NULL || kernel == NULL || kernel->state == NULL || !kernel->handover.enabled ||
        kernel->handover.pending || kernel->handover.network_id != chain->network_id ||
        memcmp(kernel->handover.governance_public_key, chain->governance_public_key, 32U) != 0 ||
        kernel->epoch != chain->epoch || kernel->state->next_sequence != chain->predecessor.last_sequence + 1U ||
        memcmp(kernel->current_state_root, chain->predecessor.resulting_state_root, 32U) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status = history_epoch(kernel, &epoch);
    if (status != LXP_OK || epoch != chain->epoch) return LXP_FATAL_REPLAY_DIVERGENCE;
    for (size_t i = 0U; i < chain->transition_count; ++i) {
        lxp_byte_span encoded;
        uint8_t digest[32];
        status = history_blob(kernel, i + 2U, &encoded);
        if (status == LXP_OK) status = lxp_hash_sha256(encoded.bytes, encoded.length, digest);
        if (status != LXP_OK || memcmp(digest, chain->evidence_digests[i], 32U) != 0)
            return LXP_FATAL_REPLAY_DIVERGENCE;
    }
    return LXP_OK;
}
