#include "sandbox.h"
#include "occupancy.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <string.h>
#include <limits.h>

enum {
    DESTROY_BYTES = 84,
    TERMINAL_BYTES = 352,
    CLEANUP_BYTES = 96,
    CLEANUP_BLOBS_PER_ACTIVITY = 3,
    MAX_EXPIRIES_PER_BATCH = 6
};
typedef struct sandbox_destroy_activity {
    uint8_t lease_id[32];
    uint8_t expected_lease_root[32];
    uint64_t expected_sequence;
    uint64_t boundary;
} sandbox_destroy_activity;

static void put_u64(uint8_t *out, uint64_t value) {
    size_t i; for (i = 0U; i < 8U; ++i) out[i] = (uint8_t)(value >> (56U - 8U*i));
}
static uint64_t take_u64(const uint8_t *in) {
    uint64_t value = 0U; size_t i; for (i = 0U; i < 8U; ++i) value=(value<<8U)|in[i]; return value;
}
static void state_key(uint8_t kind, const uint8_t lease[32], uint8_t key[34]) {
    key[0]=(uint8_t)'s'; key[1]=kind; (void)memcpy(key+2U, lease, 32U);
}
static uint16_t take_u16(const uint8_t *p){return (uint16_t)(((uint16_t)p[0]<<8U)|p[1]);}
static uint32_t take_u32(const uint8_t *p){return ((uint32_t)p[0]<<24U)|((uint32_t)p[1]<<16U)|((uint32_t)p[2]<<8U)|p[3];}
static void put_u32(uint8_t *p,uint32_t v){p[0]=(uint8_t)(v>>24U);p[1]=(uint8_t)(v>>16U);p[2]=(uint8_t)(v>>8U);p[3]=(uint8_t)v;}

static lxp_result blob_referenced_elsewhere(
    lxp_module_ctx *ctx, const uint8_t *current_key,
    size_t current_key_length, const uint8_t digest[32], bool *referenced)
{
    size_t index;
    if (ctx == NULL || current_key == NULL || digest == NULL ||
        referenced == NULL)
        return LXP_ERR_NON_CANONICAL;
    *referenced = false;
    for (index = 0U; index < ctx->kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[index];
        const uint8_t *head;
        const uint8_t *manifest;
        size_t head_length;
        size_t manifest_length;
        size_t cursor = 6U;
        uint32_t cell;
        uint32_t count;
        uint8_t manifest_digest[32];
        lxp_result status;
        if (entry->module_id != LXP_MODULE_PROGRAMS ||
            entry->key_length != 73U ||
            memcmp(entry->key, "progstor", 8U) != 0 ||
            (entry->key_length == current_key_length &&
             memcmp(entry->key, current_key, current_key_length) == 0))
            continue;
        status = lxp_ctx_kv_get(ctx, entry->key, entry->key_length,
                                &head, &head_length);
        if (status == LXP_ERR_UNKNOWN_FIELD) continue;
        if (status != LXP_OK) return status;
        if (head_length != 38U || take_u16(head) != 1U)
            return LXP_FATAL_INVARIANT;
        if (lxp_ct_memcmp(head + 6U, digest, 32U) == 0) {
            *referenced = true;
            return LXP_OK;
        }
        status = lxp_ctx_blob_get(ctx, head + 6U,
                                  &manifest, &manifest_length);
        if (status != LXP_OK) return status;
        status = lxp_hash_sha256(manifest, manifest_length,
                                 manifest_digest);
        if (status != LXP_OK) return status;
        count = take_u32(head + 2U);
        if (manifest_length < 6U || take_u16(manifest) != 1U ||
            take_u32(manifest + 2U) != count ||
            lxp_ct_memcmp(manifest_digest, head + 6U, 32U) != 0)
            return LXP_FATAL_INVARIANT;
        for (cell = 0U; cell < count; ++cell) {
            uint16_t key_length;
            if (cursor + 2U > manifest_length)
                return LXP_FATAL_INVARIANT;
            key_length = take_u16(manifest + cursor);
            cursor += 2U;
            if (key_length == 0U ||
                cursor + (size_t)key_length + 36U > manifest_length)
                return LXP_FATAL_INVARIANT;
            if (lxp_ct_memcmp(manifest + cursor + key_length,
                              digest, 32U) == 0) {
                *referenced = true;
                return LXP_OK;
            }
            cursor += (size_t)key_length + 36U;
        }
        if (cursor != manifest_length) return LXP_FATAL_INVARIANT;
    }
    return LXP_OK;
}

static lxp_result reclaim_storage_head(lxp_module_ctx *ctx,const uint8_t *key,size_t key_length,
    uint32_t *next,uint8_t root[32],uint64_t *cells,uint64_t *bytes,bool *complete){
    const uint8_t *head,*manifest;size_t head_len,manifest_len,cursor=6U;uint32_t i,count,removed=0U;lxp_result s;
    *complete=false;s=lxp_ctx_kv_get(ctx,key,key_length,&head,&head_len);if(s==LXP_ERR_UNKNOWN_FIELD){*complete=true;return LXP_OK;}if(s!=LXP_OK)return s;
    if (head_len != 38U) return LXP_FATAL_INVARIANT;
    s = lxp_ctx_blob_get(ctx, head + 6U, &manifest, &manifest_len);
    if (s != LXP_OK) return s;
    if(lxp_ct_is_zero(root,32U)){s=lxp_hash_sha256(head,head_len,root);if(s!=LXP_OK)return s;}
    count=take_u32(head+2U);if(*next>count)return LXP_FATAL_INVARIANT;
    for(i=0U;i<count;++i){uint16_t n;if(cursor+2U>manifest_len)return LXP_FATAL_INVARIANT;n=take_u16(manifest+cursor);cursor+=2U;
        if(cursor+(size_t)n+36U>manifest_len)return LXP_FATAL_INVARIANT;
        if(i>=*next&&removed<CLEANUP_BLOBS_PER_ACTIVITY){
            uint32_t value_length=take_u32(manifest+cursor+n+32U);
            bool referenced=false;
            *bytes+=(uint64_t)n+(uint64_t)value_length;
            if(value_length!=0U){s=blob_referenced_elsewhere(ctx,key,key_length,manifest+cursor+n,&referenced);if(s!=LXP_OK)return s;
                if(!referenced){s=lxp_ctx_blob_del(ctx,manifest+cursor+n);if(s!=LXP_OK)return s;}}
            ++removed;++*next;}
        cursor+=(size_t)n+36U;}
    if(cursor!=manifest_len)return LXP_FATAL_INVARIANT;
    if(*next!=count)return LXP_OK;
    {bool referenced=false;s=blob_referenced_elsewhere(ctx,key,key_length,head+6U,&referenced);if(s!=LXP_OK)return s;
        if(!referenced){s=lxp_ctx_blob_del(ctx,head+6U);if(s!=LXP_OK)return s;}}
    s=lxp_ctx_kv_del(ctx,key,key_length);if(s==LXP_OK){*cells+=count;*complete=true;}return s;
}
static _Thread_local const sandbox_destroy_activity *active_destroy;
static _Thread_local lxp_module_ctx *active_destroy_ctx;

lxp_result layerx_programs_sandbox_destroy_state_length(uint64_t token,uint16_t kind){
    lxp_module_ctx *ctx=(lxp_module_ctx*)(uintptr_t)token;uint8_t key[34];const uint8_t *v;size_t n;lxp_result s;
    if(ctx!=active_destroy_ctx||active_destroy==NULL||kind>2U)return LXP_ERR_AUTH_SCOPE;
    state_key((uint8_t)kind,active_destroy->lease_id,key);s=lxp_ctx_kv_get(ctx,key,sizeof(key),&v,&n);
    return s==LXP_OK&&n<=INT32_MAX?(lxp_result)n:s;
}
lxp_result layerx_programs_sandbox_destroy_state_byte(uint64_t token,uint16_t kind,uint32_t off){
    lxp_module_ctx *ctx=(lxp_module_ctx*)(uintptr_t)token;uint8_t key[34];const uint8_t *v;size_t n;lxp_result s;
    if(ctx!=active_destroy_ctx||active_destroy==NULL||kind>2U)return LXP_ERR_AUTH_SCOPE;
    state_key((uint8_t)kind,active_destroy->lease_id,key);s=lxp_ctx_kv_get(ctx,key,sizeof(key),&v,&n);
    return s==LXP_OK&&off<n?(lxp_result)v[off]:LXP_ERR_NON_CANONICAL;
}
lxp_result layerx_programs_sandbox_destroy_archive(uint64_t token,uint16_t kind,const uint8_t *bytes,uint32_t length){
    lxp_module_ctx *ctx=(lxp_module_ctx*)(uintptr_t)token;uint8_t key[34];
    if(ctx!=active_destroy_ctx||active_destroy==NULL||kind>3U||bytes==NULL||length==0U)return LXP_ERR_AUTH_SCOPE;
    key[0]=(uint8_t)'d';key[1]=(uint8_t)kind;(void)memcpy(key+2U,active_destroy->lease_id,32U);
    return lxp_ctx_kv_put(ctx,key,sizeof(key),bytes,length);
}
lxp_result layerx_programs_sandbox_destroy_charge(uint64_t token,const uint8_t from[32],const uint8_t to[32],
    const uint8_t asset[32],uint64_t hi,uint64_t lo,uint8_t root[32]){
    lxp_module_ctx *ctx=(lxp_module_ctx*)(uintptr_t)token;lxp_transfer_set set;lxp_transfer_source_authority auth;
    lxp_receipt receipt;lxp_result s;
    if(ctx!=active_destroy_ctx||active_destroy==NULL||from==NULL||to==NULL||asset==NULL||root==NULL||(hi==0U&&lo==0U))return LXP_ERR_AUTH_SCOPE;
    (void)memset(&set,0,sizeof(set));(void)memset(&auth,0,sizeof(auth));(void)memset(&receipt,0,sizeof(receipt));
    s=lxp_ctx_account_find(ctx,from,&set.legs[0].from);if(s==LXP_OK)s=lxp_ctx_account_find(ctx,to,&set.legs[0].to);
    if (s != LXP_OK) return s;
    (void)memcpy(set.legs[0].asset_id, asset, 32U);
    set.legs[0].amount = (lxp_u128){hi, lo};
    set.legs[0].reason=LXP_REASON_PAYMENT;set.legs[0].supply_mode=LXP_TRANSFER_CONSERVED;set.leg_count=1U;
    (void)memcpy(auth.authorized_from,from,32U);auth.debit_authority_kind=LXP_AUTH_PROTOCOL_MODULE;auth.protocol_system_capability=true;
    set.context.protocol_system_capability=true;set.context.debit_authority_kind=LXP_AUTH_PROTOCOL_MODULE;
    set.context.source_authorities=&auth;set.context.source_authority_count=1U;
    s=lxp_ctx_emit_programs_maintenance_transfer_set(ctx,&set,&receipt);
    if (s == LXP_OK)
        (void)memcpy(root, receipt.transfer_set_root, 32U);
    return s;
}
lxp_result layerx_programs_sandbox_destroy_refund(uint64_t token,const uint8_t from[32],const uint8_t to[32],
    const uint8_t asset[32],uint64_t hi,uint64_t lo,uint8_t root[32]){
    lxp_module_ctx *ctx=(lxp_module_ctx*)(uintptr_t)token;lxp_transfer_set set;lxp_transfer_source_authority auth;
    lxp_receipt receipt;lxp_result s;
    if(ctx!=active_destroy_ctx||active_destroy==NULL||from==NULL||to==NULL||asset==NULL||root==NULL)return LXP_ERR_AUTH_SCOPE;
    (void)memset(&set,0,sizeof(set));(void)memset(&auth,0,sizeof(auth));(void)memset(&receipt,0,sizeof(receipt));
    s=lxp_ctx_account_find(ctx,from,&set.legs[0].from);if(s==LXP_OK)s=lxp_ctx_account_find(ctx,to,&set.legs[0].to);
    if (s != LXP_OK) return s;
    (void)memcpy(set.legs[0].asset_id, asset, 32U);
    set.legs[0].amount = (lxp_u128){hi, lo};
    set.legs[0].reason=LXP_REASON_ESCROW_RELEASE;set.legs[0].supply_mode=LXP_TRANSFER_CONSERVED;set.leg_count=1U;
    (void)memcpy(auth.authorized_from,from,32U);auth.debit_authority_kind=LXP_AUTH_PROTOCOL_MODULE;auth.protocol_system_capability=true;
    set.context.protocol_system_capability=true;set.context.debit_authority_kind=LXP_AUTH_PROTOCOL_MODULE;
    set.context.source_authorities=&auth;set.context.source_authority_count=1U;
    s=lxp_ctx_emit_programs_maintenance_transfer_set(ctx,&set,&receipt);
    if (s == LXP_OK)
        (void)memcpy(root, receipt.transfer_set_root, 32U);
    return s;
}

lxp_result lxp_programs_sandbox_destroy_decode(lxp_module_ctx *ctx,
    const uint8_t *payload, size_t length, void **decoded) {
    sandbox_destroy_activity *value; void *allocation; lxp_result status;
    if (ctx==NULL || payload==NULL || decoded==NULL || length!=DESTROY_BYTES)
        return LXP_ERR_TRUNCATED;
    if (payload[0]!=1U || payload[1]!=1U || payload[2]!=0U || payload[3]!=0U)
        return LXP_ERR_NON_CANONICAL;
    status=lxp_ctx_arena_alloc(ctx,sizeof(*value),_Alignof(sandbox_destroy_activity),&allocation);
    if (status != LXP_OK) return status;
    value = (sandbox_destroy_activity *)allocation;
    (void)memcpy(value->lease_id,payload+4U,32U);
    (void)memcpy(value->expected_lease_root,payload+36U,32U);
    value->expected_sequence=take_u64(payload+68U); value->boundary=take_u64(payload+76U);
    if(lxp_ct_is_zero(value->lease_id,32U)||lxp_ct_is_zero(value->expected_lease_root,32U)
       ||value->expected_sequence==0U||value->boundary==0U)return LXP_ERR_NON_CANONICAL;
    *decoded=value; return LXP_OK;
}

lxp_result lxp_programs_sandbox_destroy_validate(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority,
    const void *decoded) {
    const sandbox_destroy_activity *value=decoded;
    if(ctx==NULL||activity==NULL||authority==NULL||value==NULL||
       activity->activity_type!=LX_PROGRAMS_SANDBOX_DESTROY||
       value->boundary!=lxp_ctx_batch_number(ctx)||
       value->expected_sequence!=lxp_ctx_global_sequence(ctx)) return LXP_ERR_CONTEXT_MISMATCH;
    /* This ordinal is protocol-owned; an ordinary tenant signature cannot mint sweep authority. */
    if(authority->kind!=LXP_AUTHORITY_PROTOCOL_MODULE) return LXP_ERR_AUTH_SCOPE;
    return lxp_ctx_charge_gas(ctx,DESTROY_BYTES);
}

lxp_result lxp_programs_sandbox_destroy_execute(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority,
    const void *decoded, lxp_effect_buffer *effects) {
    const sandbox_destroy_activity *value=decoded; const uint8_t *lease,*escrow,*ledger;
    size_t lease_len,escrow_len,ledger_len; uint8_t key[34],digest[32],terminal[TERMINAL_BYTES];
    uint8_t terminal_key[34],cleanup_key[34],cleanup[CLEANUP_BYTES]={0},namespace_root[32]={0},snapshot_root[32]={0};
    const uint8_t *prior_cleanup;size_t prior_cleanup_len;uint64_t expiry,cells=0U,bytes=0U;uint32_t next=0U;bool complete=false;lxp_result status;
    (void)effects;
    status=lxp_programs_sandbox_destroy_validate(ctx,activity,authority,decoded);
    state_key(0U,value->lease_id,key);
    if(status==LXP_OK)status=lxp_ctx_kv_get(ctx,key,sizeof(key),&lease,&lease_len);
    state_key(1U,value->lease_id,key);
    if(status==LXP_OK)status=lxp_ctx_kv_get(ctx,key,sizeof(key),&escrow,&escrow_len);
    state_key(2U,value->lease_id,key);
    if(status==LXP_OK)status=lxp_ctx_kv_get(ctx,key,sizeof(key),&ledger,&ledger_len);
    if(status!=LXP_OK)return status;
    if(lease_len<392U||escrow_len<202U||ledger_len==0U||
       memcmp(lease,"LayerX/programs/sandbox/lease-state/v3\0",39U)!=0||
       memcmp(escrow,"LayerX/programs/sandbox/escrow-state/v1\0",40U)!=0||
       memcmp(lease+39U,value->lease_id,32U)!=0||memcmp(escrow+40U,value->lease_id,32U)!=0)
        return LXP_ERR_NON_CANONICAL;
    status=lxp_hash_sha256(lease,lease_len,digest);
    if(status!=LXP_OK||lxp_ct_memcmp(digest,value->expected_lease_root,32U)!=0)
        return LXP_ERR_ROOT_MISMATCH;
    expiry=take_u64(lease+383U); if(value->boundary<expiry)return LXP_ERR_NOT_YET_VALID;
    cleanup_key[0]=(uint8_t)'x';cleanup_key[1]=(uint8_t)'c';(void)memcpy(cleanup_key+2U,value->lease_id,32U);
    status=lxp_ctx_kv_get(ctx,cleanup_key,sizeof(cleanup_key),&prior_cleanup,&prior_cleanup_len);
    if(status==LXP_OK){if(prior_cleanup_len!=CLEANUP_BYTES||memcmp(prior_cleanup,"LXCL1",5U)!=0)return LXP_FATAL_INVARIANT;(void)memcpy(cleanup,prior_cleanup,CLEANUP_BYTES);next=take_u32(cleanup+6U);cells=take_u64(cleanup+10U);bytes=take_u64(cleanup+18U);(void)memcpy(namespace_root,cleanup+26U,32U);(void)memcpy(snapshot_root,cleanup+58U,32U);}
    else if(status==LXP_ERR_UNKNOWN_FIELD){(void)memcpy(cleanup,"LXCL1",5U);status=LXP_OK;}else return status;
    {
        uint8_t storage_key[73];
        (void)memcpy(storage_key,"progstor",8U);
        (void)memcpy(storage_key+8U,lease+103U,32U);
        (void)memcpy(storage_key+41U,lease+167U,32U);
        storage_key[40U]=cleanup[5U]==0U?0U:2U;
        status=reclaim_storage_head(ctx,storage_key,sizeof(storage_key),&next,
            cleanup[5U]==0U?namespace_root:snapshot_root,&cells,&bytes,&complete);
        if(status!=LXP_OK)return status;
        if(complete){++cleanup[5U];next=0U;}
        if(cleanup[5U]<2U){put_u32(cleanup+6U,next);put_u64(cleanup+10U,cells);put_u64(cleanup+18U,bytes);(void)memcpy(cleanup+26U,namespace_root,32U);(void)memcpy(cleanup+58U,snapshot_root,32U);return lxp_ctx_kv_put(ctx,cleanup_key,sizeof(cleanup_key),cleanup,sizeof(cleanup));}
    }
    /* Rust owns exact decoding, namespace/blob enumeration and refund derivation. */
    active_destroy=value;active_destroy_ctx=ctx;
    status=layerx_programs_sandbox_destroy_host((uint64_t)(uintptr_t)ctx,
        value->lease_id,value->expected_lease_root,lxp_ctx_activity_id(ctx),
        value->expected_sequence,value->boundary);
    active_destroy=NULL;active_destroy_ctx=NULL;if(status!=LXP_OK)return status;
    /* Live state is deleted only after host settlement staged refund, reclamation and archive. */
    state_key(0U,value->lease_id,key); status=lxp_ctx_kv_del(ctx,key,sizeof(key));
    state_key(1U,value->lease_id,key); if(status==LXP_OK)status=lxp_ctx_kv_del(ctx,key,sizeof(key));
    state_key(2U,value->lease_id,key); if(status==LXP_OK)status=lxp_ctx_kv_del(ctx,key,sizeof(key));
    if(status==LXP_OK)status=lxp_ctx_kv_del(ctx,cleanup_key,sizeof(cleanup_key));
    if(status!=LXP_OK)return status;
    (void)memset(terminal,0,sizeof(terminal));
    (void)memcpy(terminal,"LXSD1",5U); (void)memcpy(terminal+5U,value->lease_id,32U);
    (void)memcpy(terminal+37U,lxp_ctx_activity_id(ctx),32U); put_u64(terminal+69U,value->expected_sequence);
    put_u64(terminal+77U,value->boundary); (void)memcpy(terminal+85U,value->expected_lease_root,32U);
    active_destroy=value;active_destroy_ctx=ctx;
    status=layerx_programs_sandbox_destroy_terminal((uint64_t)(uintptr_t)ctx,
        terminal+117U,TERMINAL_BYTES-117U);
    active_destroy=NULL;active_destroy_ctx=NULL;
    if(status!=LXP_OK)return status;
    (void)memcpy(terminal+261U,namespace_root,32U);(void)memcpy(terminal+293U,snapshot_root,32U);
    put_u64(terminal+325U,cells);put_u64(terminal+333U,bytes);
    terminal_key[0]=(uint8_t)'t';terminal_key[1]=(uint8_t)'s';
    (void)memcpy(terminal_key+2U,value->lease_id,32U);
    status=lxp_ctx_kv_put(ctx,terminal_key,sizeof(terminal_key),terminal,sizeof(terminal));
    if(status==LXP_OK&&ctx->effects!=NULL){uint8_t frame[256];size_t first=248U;
        (void)memcpy(frame,"LXDT",4U);frame[4]=0U;frame[5]=2U;frame[6]=1U;frame[7]=0U;
        (void)memcpy(frame+8U,terminal,first);status=lxp_ctx_emit_event(ctx,0x090AU,frame,first+8U);
        if(status==LXP_OK){frame[4]=1U;(void)memcpy(frame+8U,terminal+first,sizeof(terminal)-first);
            status=lxp_ctx_emit_event(ctx,0x090AU,frame,sizeof(terminal)-first+8U);}}
    return status;
}

lxp_result lxp_programs_sandbox_finalize_expiry_batch(
    lxp_module_ctx *ctx, uint64_t batch_number)
{
    uint8_t cursor_key[] = "ex/head/v1";
    const uint8_t *cursor;
    size_t cursor_len;
    lxp_result status;
    if (ctx == NULL || batch_number == 0U) return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_kv_get(ctx, cursor_key, sizeof(cursor_key) - 1U,
                            &cursor, &cursor_len);
    if (status == LXP_ERR_UNKNOWN_FIELD) {
        uint8_t genesis[96] = {0};
        (void)memcpy(genesis, "LXEX2", 5U);
        put_u64(genesis + 5U, batch_number - 1U);
        status = lxp_ctx_kv_put(ctx, cursor_key,
                                sizeof(cursor_key) - 1U,
                                genesis, sizeof(genesis));
        if (status != LXP_OK) return status;
        return layerx_programs_sandbox_sweep_host(
            (uint64_t)(uintptr_t)ctx, batch_number,
            MAX_EXPIRIES_PER_BATCH);
    }
    if (status != LXP_OK || cursor_len != 96U ||
        memcmp(cursor, "LXEX2", 5U) != 0 ||
        take_u64(cursor + 5U) != batch_number - 1U)
        return status == LXP_OK ? LXP_ERR_BATCH_GAP : status;
    return layerx_programs_sandbox_sweep_host(
        (uint64_t)(uintptr_t)ctx, batch_number, MAX_EXPIRIES_PER_BATCH);
}

int32_t layerx_programs_sandbox_sweep_host(
    uint64_t token, uint64_t boundary, uint32_t limit)
{
    lxp_module_ctx *ctx = (lxp_module_ctx *)(uintptr_t)token;
    const lxp_module_kv_entry *due[MAX_EXPIRIES_PER_BATCH];
    size_t count = 0U;
    size_t processed = 0U;
    size_t i;
    size_t j;
    uint8_t cursor_key[] = "ex/head/v1";
    uint8_t cursor[96] = {0};
    lxp_result status = LXP_OK;
    if (ctx == NULL || limit == 0U || limit > MAX_EXPIRIES_PER_BATCH)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < ctx->kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_PROGRAMS ||
            entry->key_length != 34U || entry->key[0] != (uint8_t)'s' ||
            entry->key[1] != 0U || entry->value_length < 392U ||
            take_u64(entry->value + 383U) > boundary)
            continue;
        for (j = 0U; j < count &&
             memcmp(due[j]->key + 2U, entry->key + 2U, 32U) < 0; ++j) {}
        if (j < (size_t)limit) {
            if (count < (size_t)limit) ++count;
            if (j + 1U < count)
                (void)memmove(&due[j + 1U], &due[j],
                              (count - j - 1U) * sizeof(due[0]));
            due[j] = entry;
        }
    }
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        sandbox_destroy_activity value;
        lxp_activity activity;
        lxp_authority_resolved authority;
        uint8_t old_id[32];
        uint8_t payload[DESTROY_BYTES];
        uint8_t terminal_key[34];
        const uint8_t *terminal;
        size_t terminal_length;
        void *canonical = NULL;
        (void)memset(&value, 0, sizeof(value));
        (void)memcpy(value.lease_id, due[i]->key + 2U, 32U);
        status = lxp_hash_sha256(due[i]->value, due[i]->value_length,
                                 value.expected_lease_root);
        value.expected_sequence = ctx->global_sequence;
        value.boundary = boundary;
        (void)memset(&activity, 0, sizeof(activity));
        activity.activity_type = LX_PROGRAMS_SANDBOX_DESTROY;
        (void)memset(&authority, 0, sizeof(authority));
        authority.kind = LXP_AUTHORITY_PROTOCOL_MODULE;
        payload[0] = 1U;
        payload[1] = 1U;
        payload[2] = 0U;
        payload[3] = 0U;
        (void)memcpy(payload + 4U, value.lease_id, 32U);
        (void)memcpy(payload + 36U, value.expected_lease_root, 32U);
        put_u64(payload + 68U, value.expected_sequence);
        put_u64(payload + 76U, value.boundary);
        (void)memcpy(old_id, ctx->activity_id, 32U);
        if (status == LXP_OK)
            status = lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, payload,
                                     sizeof(payload), ctx->activity_id);
        if (status == LXP_OK)
            status = lxp_programs_sandbox_destroy_decode(
                ctx, payload, sizeof(payload), &canonical);
        if (status == LXP_OK)
            status = lxp_programs_sandbox_destroy_execute(
                ctx, &activity, &authority, canonical, ctx->effects);
        (void)memcpy(ctx->activity_id, old_id, 32U);
        if (status != LXP_OK) break;
        processed = i + 1U;
        terminal_key[0] = (uint8_t)'t';
        terminal_key[1] = (uint8_t)'s';
        (void)memcpy(terminal_key + 2U, value.lease_id, 32U);
        status = lxp_ctx_kv_get(ctx, terminal_key, sizeof(terminal_key),
                                &terminal, &terminal_length);
        if (status == LXP_ERR_UNKNOWN_FIELD) {
            status = LXP_OK;
            break;
        }
        if (status != LXP_OK || terminal_length != TERMINAL_BYTES) {
            status = status == LXP_OK ? LXP_FATAL_INVARIANT : status;
            break;
        }
    }
    if (status == LXP_OK) {
        uint8_t page_binding[20U + MAX_EXPIRIES_PER_BATCH * 32U] = {0};
        (void)memcpy(cursor, "LXEX2", 5U);
        put_u64(cursor + 5U, boundary);
        put_u64(cursor + 13U, ctx->global_sequence);
        if (processed != 0U)
            (void)memcpy(cursor + 21U, due[processed - 1U]->key + 2U, 32U);
        put_u32(cursor + 53U, (uint32_t)processed);
        put_u64(page_binding, boundary);
        put_u64(page_binding + 8U, ctx->global_sequence);
        put_u32(page_binding + 16U, (uint32_t)processed);
        for (i = 0U; i < processed; ++i)
            (void)memcpy(page_binding + 20U + i * 32U,
                         due[i]->key + 2U, 32U);
        status = lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, page_binding,
                                 20U + processed * 32U, cursor + 57U);
        if (status != LXP_OK) return status;
        status = lxp_ctx_kv_put(ctx, cursor_key,
                                sizeof(cursor_key) - 1U,
                                cursor, sizeof(cursor));
    }
    return status;
}
