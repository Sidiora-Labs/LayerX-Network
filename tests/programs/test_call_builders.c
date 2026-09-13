#include "../../src/modules/programs/call.c"

#include <stdio.h>

#define CHECK(value) do { if (!(value)) { \
    (void)fprintf(stderr, "call builder check failed at %d\n", __LINE__); \
    return 1; \
} } while (0)

int main(void)
{
    static lxp_kernel kernel;
    static lxp_state_store state;
    static lxp_state_journal journal;
    static uint8_t arena_bytes[1U << 20];
    lxp_arena arena;
    lxp_module_ctx ctx;
    lxp_programs_call_activity call = {0};
    lxp_authority_resolved authority = {0};
    lxp_programs_call_catalog_entry catalog = {0};
    uint64_t parameters = 1U;
    uint64_t token = (uint64_t)(uintptr_t)&call;
    const uint8_t wasm[] = {0U, 0x61U, 0x73U, 0x6dU, 1U, 0U, 0U, 0U};
    uint8_t digest[32];
    uint8_t program[32] = {1U};
    CHECK(lxp_state_store_init(&state, 0U) == LXP_OK);
    CHECK(lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) == LXP_OK);
    CHECK(lxp_kernel_register_module(&kernel, programs_module_registration()) == LXP_OK);
    CHECK(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) == LXP_OK);
    CHECK(lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_PROGRAMS, 1U, 0U,
                              1U, 1000000U, &arena, true) == LXP_OK);
    CHECK(lxp_hash_sha256(wasm, sizeof(wasm), digest) == LXP_OK);
    digest[0] ^= 1U;
    CHECK(lxp_programs_artifact_store(&ctx, program, digest, wasm, sizeof(wasm)) ==
          LXP_ERR_ROOT_MISMATCH);
    CHECK(ctx.staged_count == 0U && kernel.blob_count == 0U);
    digest[0] ^= 1U;
    CHECK(lxp_programs_artifact_store(&ctx, program, digest, wasm, sizeof(wasm)) == LXP_OK);
    ctx.call_admission.present = true;
    ctx.call_admission.fee_schedule_version = 1U;
    ctx.call_admission.metering_schedule_version = 1U;
    call.ctx = &ctx;
    call.abi_version = 1U;
    CHECK(layerx_programs_call_terminal_reserve(token, 64U, 128U, 64U) == LXP_OK);
    CHECK(layerx_programs_call_terminal_begin(token, LXP_PROGRAM_TERMINAL_FAILURE,
        LXP_ERR_NON_CANONICAL, 1U, 1U, 1U, 1U, 0U, 0U, 0U, 0U, 0U, 0U,
        0U, 0U, 0U, 0U, 0U, 0U, 2U, 2U, 2U) == LXP_OK);
    CHECK(layerx_programs_call_terminal_publish(token) == LXP_ERR_NON_CANONICAL);
    for (uint16_t section = 0U; section < 3U; ++section) {
        CHECK(layerx_programs_call_terminal_byte(token, section, 1U, 0U) == LXP_ERR_NON_CANONICAL);
        CHECK(layerx_programs_call_terminal_byte(token, section, 0U, 0U) == LXP_OK);
        CHECK(layerx_programs_call_terminal_byte(token, section, 0U, 0U) == LXP_ERR_NON_CANONICAL);
    }
    CHECK(layerx_programs_call_terminal_publish(token) == LXP_ERR_NON_CANONICAL);
    call.authority = &authority;
    call.catalog = &catalog;
    call.catalog_count = 1U;
    CHECK(layerx_programs_call_event_begin(token, 0U, 0U, 0U, 0U, 0U,
        0U, 0U, 0U, 0U, 0U, 0U, 2U, 2U) == LXP_OK);
    CHECK(layerx_programs_call_event_emit(token) == LXP_ERR_NON_CANONICAL);
    CHECK(layerx_programs_call_event_byte(token, 0U, 1U, 0U) == LXP_ERR_NON_CANONICAL);
    CHECK(layerx_programs_call_event_byte(token, 0U, 0U, 0U) == LXP_OK);
    CHECK(layerx_programs_call_event_byte(token, 1U, 0U, 0U) == LXP_OK);
    CHECK(layerx_programs_call_event_emit(token) == LXP_ERR_NON_CANONICAL);
    CHECK(call.emitted_event_count == 0U);
    call_activity_release(&call);
    lxp_module_ctx_rollback(&ctx);
    CHECK(lxp_state_store_destroy(&state) == LXP_OK);
    return 0;
}
