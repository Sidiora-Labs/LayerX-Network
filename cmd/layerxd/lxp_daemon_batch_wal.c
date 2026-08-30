#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L

#include "lxp_daemon_batch_wal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_activity.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_receipt.h"
#include "layerx/programs.h"

#include <errno.h>
#include <dirent.h>
#include <fcntl.h>
#include <limits.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/file.h>
#include <unistd.h>
#include <pthread.h>

enum {
    WAL_VERSION = 1,
    WAL_FIXED_BYTES = 762,
    WAL_PROOF_BYTES = 1033,
    WAL_DIGEST_BYTES = 32,
    WAL_ITEM_OVERHEAD_MAX = LXP_DAEMON_BATCH_WAL_MAX_ITEMS *
        (12 + WAL_PROOF_BYTES),
    WAL_MAX_BYTES = LXP_MAX_BATCH_BODY_BYTES + WAL_FIXED_BYTES +
        WAL_DIGEST_BYTES + WAL_ITEM_OVERHEAD_MAX
};

static const uint8_t wal_magic[8] = {'L','X','P','B','W','A','L','1'};
static const uint8_t wal_domain[24] = {
    'l','a','y','e','r','x','-','p','r','e','p','a','r','e','d','-','b','a','t','c','h','-','v','1'
};
static pthread_mutex_t wal_replace_mutex=PTHREAD_MUTEX_INITIALIZER;
static uint64_t wal_temporary_counter;

struct lxp_daemon_batch_wal_record {
    lxp_daemon_batch_wal_state state;
    lxp_daemon_batch_wal_input view;
    lxp_byte_span activities[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_byte_span receipts[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_byte_span events[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_merkle_proof proofs[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    uint8_t *owned;
    size_t owned_length;
};

static void put_u64(uint8_t *p, uint64_t v)
{
    size_t i;
    for(i=0U;i<8U;++i)p[i]=(uint8_t)(v>>(56U-8U*i));
}

lxp_result lxp_daemon_batch_bind_prefix(
 const lxp_byte_span *canonical_activities,size_t count,
 const uint8_t base_state_root[32],
 uint64_t first_sequence,uint64_t batch_number,lxp_arena *arena,
 lxp_kernel_execution *executions,lxp_batch_roots *roots,
 uint8_t batch_id[32])
{
    lxp_batch_roots computed;
    uint8_t preimage[88],computed_batch_id[32];
    size_t index,total=0U;
    lxp_result status;
    if(canonical_activities==NULL || count==0U ||
       count>LXP_DAEMON_BATCH_WAL_MAX_ITEMS ||
       base_state_root==NULL ||
       first_sequence==0U || batch_number==0U || arena==NULL ||
       executions==NULL || roots==NULL || batch_id==NULL ||
       count-1U>UINT64_MAX-first_sequence)
        return LXP_ERR_NON_CANONICAL;
    for(index=0U;index<count;++index) {
        if(canonical_activities[index].bytes==NULL ||
           canonical_activities[index].length==0U ||
           total>SIZE_MAX-canonical_activities[index].length ||
           total+canonical_activities[index].length>
               LXP_MAX_BATCH_BODY_BYTES)
            return LXP_ERR_LENGTH_LIMIT;
        total+=canonical_activities[index].length;
    }
    status=lxp_batch_roots_compute(
        &(lxp_batch_root_inputs){canonical_activities,count,NULL,0U,
                                 NULL,0U,NULL,0U,NULL,0U},arena,&computed);
    if(status==LXP_OK) {
        (void)memcpy(preimage,base_state_root,32U);
        (void)memcpy(preimage+32U,computed.activity_merkle_root,32U);
        put_u64(preimage+64U,first_sequence);
        put_u64(preimage+72U,first_sequence+(uint64_t)count-1U);
        put_u64(preimage+80U,batch_number);
        status=lxp_hash_context_value(preimage,sizeof(preimage),
                                      computed_batch_id);
    }
    if(status==LXP_OK) {
        for(index=0U;index<count;++index) {
            (void)memcpy(executions[index].batch_id,computed_batch_id,32U);
            (void)memcpy(executions[index].activity_root,
                         computed.activity_merkle_root,32U);
            executions[index].global_sequence=first_sequence+index;
        }
        *roots=computed;
        (void)memcpy(batch_id,computed_batch_id,32U);
    }
    lxp_secure_zero(preimage,sizeof(preimage));
    lxp_secure_zero(computed_batch_id,sizeof(computed_batch_id));
    return status;
}

static void put_u16(uint8_t *p, uint16_t v) { p[0]=(uint8_t)(v>>8); p[1]=(uint8_t)v; }
static void put_u32(uint8_t *p, uint32_t v) { p[0]=(uint8_t)(v>>24); p[1]=(uint8_t)(v>>16); p[2]=(uint8_t)(v>>8); p[3]=(uint8_t)v; }
static uint16_t get_u16(const uint8_t *p) { return (uint16_t)(((uint16_t)p[0]<<8)|p[1]); }
static uint32_t get_u32(const uint8_t *p) { return ((uint32_t)p[0]<<24)|((uint32_t)p[1]<<16)|((uint32_t)p[2]<<8)|p[3]; }
static uint64_t get_u64(const uint8_t *p) { uint64_t v=0U; size_t i; for(i=0;i<8U;++i)v=(v<<8)|p[i]; return v; }

static bool add_size(size_t *total, size_t value)
{
    if (*total > SIZE_MAX - value || *total + value > WAL_MAX_BYTES)
        return false;
    *total += value;
    return true;
}

static bool decimal_component(const char *text,size_t length,
                              uint64_t maximum)
{
    uint64_t value=0U;
    size_t index;
    if(text==NULL || length==0U || length>20U ||
       (length>1U && text[0]=='0'))return false;
    for(index=0U;index<length;++index) {
        uint8_t digit;
        if(text[index]<'0' || text[index]>'9')return false;
        digit=(uint8_t)(text[index]-'0');
        if(value>maximum/10U ||
           (value==maximum/10U && digit>maximum%10U))return false;
        value=value*10U+digit;
    }
    return true;
}

static bool boundary_equal(const lxp_kernel_batch_boundary *a,
                           const lxp_kernel_batch_boundary *b)
{
    return a->next_sequence == b->next_sequence &&
        lxp_ct_memcmp(a->canonical_state_root,b->canonical_state_root,32U)==0 &&
        lxp_ct_memcmp(a->receipt_state_root,b->receipt_state_root,32U)==0;
}

static void encode_boundary(uint8_t *p, const lxp_kernel_batch_boundary *b)
{
    (void)memcpy(p,b->canonical_state_root,32U);
    (void)memcpy(p+32U,b->receipt_state_root,32U);
    put_u64(p+64U,b->next_sequence);
}

static void decode_boundary(const uint8_t *p, lxp_kernel_batch_boundary *b)
{
    (void)memcpy(b->canonical_state_root,p,32U);
    (void)memcpy(b->receipt_state_root,p+32U,32U);
    b->next_sequence=get_u64(p+64U);
}

static lxp_result wal_digest(const uint8_t *bytes, size_t body_length,
                             uint8_t digest[32])
{
    lxp_hash_context context;
    lxp_hash_init(&context);
    if (lxp_hash_update(&context,wal_domain,sizeof(wal_domain))!=LXP_OK ||
        lxp_hash_update(&context,bytes,body_length)!=LXP_OK)
        return LXP_FATAL_INVARIANT;
    return lxp_hash_final(&context,digest);
}

static lxp_result spans_root(const lxp_byte_span *spans, size_t count,
                             uint8_t root[32])
{
    uint8_t hashes[LXP_DAEMON_BATCH_WAL_MAX_ITEMS][32];
    size_t level_count=count,i;
    lxp_result status=LXP_OK;
    for(i=0U;i<count && status==LXP_OK;++i)
        status=lxp_merkle_leaf_hash(spans[i].bytes,spans[i].length,hashes[i]);
    while(status==LXP_OK && level_count>1U) {
        size_t next=(level_count+1U)/2U;
        for(i=0U;i<next && status==LXP_OK;++i) {
            size_t right=i*2U+1U;
            if(right>=level_count)right=i*2U;
            status=lxp_merkle_node_hash(hashes[i*2U],hashes[right],hashes[i]);
        }
        level_count=next;
    }
    if(status==LXP_OK)(void)memcpy(root,hashes[0],32U);
    lxp_secure_zero(hashes,sizeof(hashes));
    return status;
}

static lxp_result validate_canonical_items(
    const lxp_daemon_batch_wal_input *in, const lxp_batch_header *header)
{
    uint8_t *scratch=(uint8_t *)malloc(WAL_MAX_BYTES);
    uint8_t expected_batch_id[32],batch_preimage[88];
    uint8_t activity_id[32];
    lxp_receipt previous;
    size_t i;
    lxp_result status;
    if(scratch==NULL)return LXP_ERR_ARENA_EXHAUSTED;
    (void)memcpy(batch_preimage,in->base.receipt_state_root,32U);
    (void)memcpy(batch_preimage+32U,header->activity_merkle_root,32U);
    put_u64(batch_preimage+64U,in->first_sequence);
    put_u64(batch_preimage+72U,in->last_sequence);
    put_u64(batch_preimage+80U,in->batch_number);
    status=lxp_hash_context_value(batch_preimage,sizeof(batch_preimage),
                                  expected_batch_id);
    (void)memset(&previous,0,sizeof(previous));
    for(i=0U;status==LXP_OK && i<in->count;++i) {
        lxp_activity activity;
        lxp_receipt receipt;
        lxp_byte_span projected={NULL,0U};
        lxp_byte_span reencoded={NULL,0U};
        lxp_arena arena;
        status=lxp_activity_decode(in->activities[i].bytes,
                                   in->activities[i].length,&activity);
        if(status==LXP_OK)status=lxp_activity_check_envelope(&activity,
                                                             in->network_id);
        if(status==LXP_OK &&
           (!lxp_protocol_version_supported(activity.protocol_version) ||
            activity.protocol_version!=in->protocol_version))
            status=LXP_ERR_VERSION_UNSUPPORTED;
        if(status==LXP_OK)status=lxp_activity_verify_payload_hash(&activity);
        if(status==LXP_OK)status=lxp_activity_verify_signature(&activity);
        if(status==LXP_OK)status=lxp_activity_id(in->activities[i].bytes,
                                                 in->activities[i].length,
                                                 activity_id);
        if(status==LXP_OK)status=lxp_receipt_decode(in->receipts[i].bytes,
                                                    in->receipts[i].length,
                                                    true,&receipt);
        if(status==LXP_OK)status=lxp_arena_init(&arena,scratch,WAL_MAX_BYTES);
        if(status==LXP_OK)status=lxp_activity_encode(&activity,&arena,&reencoded);
        if(status==LXP_OK &&
           (reencoded.length!=in->activities[i].length ||
            lxp_ct_memcmp(reencoded.bytes,in->activities[i].bytes,
                          reencoded.length)!=0))
            status=LXP_ERR_NON_CANONICAL;
        if(status==LXP_OK)status=lxp_arena_reset(&arena,0U);
        if(status==LXP_OK)status=lxp_receipt_verify(
            &receipt,in->authorization.public_key,&arena);
        if(status==LXP_OK)status=lxp_receipt_encode(&receipt,true,&arena,
                                                    &reencoded);
        if(status==LXP_OK &&
           (reencoded.length!=in->receipts[i].length ||
            lxp_ct_memcmp(reencoded.bytes,in->receipts[i].bytes,
                          reencoded.length)!=0))
            status=LXP_ERR_NON_CANONICAL;
        if(status==LXP_OK)status=lxp_arena_reset(&arena,0U);
        if(status==LXP_OK &&
           (receipt.protocol_version!=in->protocol_version ||
            receipt.global_sequence!=in->first_sequence+i ||
            receipt.timestamp!=in->timestamp_ms ||
            receipt.parameter_version!=in->parameter_version ||
            lxp_ct_memcmp(receipt.activity_id,activity_id,32U)!=0 ||
            lxp_ct_memcmp(receipt.activity_root,
                          header->activity_merkle_root,32U)!=0 ||
            lxp_ct_memcmp(receipt.batch_id,expected_batch_id,32U)!=0 ||
            (receipt.program_outcome.present &&
             (receipt.program_outcome.fee_schedule_version!=
                  in->fee_schedule_version ||
              receipt.program_outcome.metering_schedule_version!=
                  in->metering_schedule_version)) ||
            (i==0U && lxp_ct_memcmp(receipt.previous_state_root,
                                    in->base.receipt_state_root,32U)!=0) ||
            (i!=0U && lxp_ct_memcmp(receipt.previous_state_root,
                                    previous.resulting_state_root,32U)!=0)))
            status=LXP_FATAL_REPLAY_DIVERGENCE;
        if(status==LXP_OK)status=lxp_programs_project_receipt_events(
            &receipt,&arena,&projected);
        if(status==LXP_OK &&
           (projected.length!=in->events[i].length ||
            lxp_ct_memcmp(projected.bytes,in->events[i].bytes,
                          projected.length)!=0))
            status=LXP_FATAL_REPLAY_DIVERGENCE;
        if(status==LXP_OK)previous=receipt;
        lxp_secure_zero(scratch,WAL_MAX_BYTES);
    }
    if(status==LXP_OK &&
       lxp_ct_memcmp(previous.resulting_state_root,
                     in->settled.receipt_state_root,32U)!=0)
        status=LXP_FATAL_REPLAY_DIVERGENCE;
    lxp_secure_zero(activity_id,sizeof(activity_id));
    lxp_secure_zero(expected_batch_id,sizeof(expected_batch_id));
    lxp_secure_zero(batch_preimage,sizeof(batch_preimage));
    lxp_secure_zero(&previous,sizeof(previous));
    lxp_secure_zero(scratch,WAL_MAX_BYTES);free(scratch);
    return status;
}

static lxp_result validate_input(const lxp_daemon_batch_wal_input *in)
{
    lxp_batch_header header;
    lxp_arena signature_arena;
    uint8_t signature_storage[512];
    size_t i,j;
    uint8_t activity_root[32],event_root[32],empty_root[32],publication[32];
    lxp_result status;
    if (in==NULL || in->count==0U || in->count>LXP_DAEMON_BATCH_WAL_MAX_ITEMS ||
        in->activities==NULL || in->receipts==NULL || in->events==NULL ||
        in->receipt_proofs==NULL || in->canonical_header.bytes==NULL ||
        in->canonical_header.length!=LXP_BATCH_HEADER_ENCODED_SIZE ||
        in->protocol_version==0U || in->network_id==0U || in->epoch==0U ||
        in->batch_number==0U || in->parameter_version==0U ||
        in->fee_schedule_version==0U || in->metering_schedule_version==0U ||
        in->timestamp_ms==0U ||
        in->first_sequence==0U || in->last_sequence<in->first_sequence ||
        in->last_sequence==UINT64_MAX ||
        in->count-1U>UINT64_MAX-in->first_sequence ||
        in->last_sequence!=in->first_sequence+(uint64_t)in->count-1U ||
        in->base.next_sequence!=in->first_sequence ||
        in->settled.next_sequence==0U ||
        in->settled.next_sequence!=in->last_sequence+1U ||
        lxp_ct_is_zero(in->publication_digest,32U) ||
        !in->authorization.authorized)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_batch_header_decode(in->canonical_header.bytes,
                                in->canonical_header.length,&header)!=LXP_OK ||
        header.protocol_version!=in->protocol_version ||
        header.network_id!=in->network_id || header.epoch!=in->epoch ||
        header.batch_number!=in->batch_number ||
        header.timestamp_ms!=in->timestamp_ms ||
        header.first_sequence!=in->first_sequence ||
        header.last_sequence!=in->last_sequence ||
        lxp_ct_memcmp(header.previous_state_root,
                      in->base.receipt_state_root,32U)!=0 ||
        lxp_ct_memcmp(header.resulting_state_root,
                      in->settled.receipt_state_root,32U)!=0 ||
        lxp_ct_memcmp(header.sequencer_id,
                      in->authorization.sequencer_id,32U)!=0 ||
        in->batch_number<in->authorization.first_batch_number ||
        in->batch_number>in->authorization.last_batch_number ||
        lxp_arena_init(&signature_arena,signature_storage,
                       sizeof(signature_storage))!=LXP_OK ||
        lxp_batch_verify_signature(&header,in->header_signature,64U,
                                   &in->authorization,
                                   &signature_arena)!=LXP_OK)
        return LXP_ERR_BAD_SIGNATURE;
    for (i=0U;i<in->count;++i) {
        uint8_t leaf[32];
        const lxp_merkle_proof *proof=&in->receipt_proofs[i];
        if ((in->activities[i].length!=0U && in->activities[i].bytes==NULL) ||
            in->activities[i].length==0U ||
            (in->receipts[i].length!=0U && in->receipts[i].bytes==NULL) ||
            in->receipts[i].length==0U ||
            (in->events[i].length!=0U && in->events[i].bytes==NULL) ||
            in->activities[i].length>UINT32_MAX ||
            in->receipts[i].length>UINT32_MAX ||
            in->events[i].length>UINT32_MAX || proof->depth>LXP_MERKLE_MAX_DEPTH ||
            proof->leaf_index!=(uint32_t)i || proof->leaf_count!=in->count ||
            lxp_merkle_leaf_hash(in->receipts[i].bytes,
                                 in->receipts[i].length,leaf)!=LXP_OK ||
            lxp_merkle_proof_verify(leaf,proof,
                                    header.receipt_merkle_root)!=LXP_OK)
            return LXP_ERR_NON_CANONICAL;
        for(j=proof->depth;j<LXP_MERKLE_MAX_DEPTH;++j)
            if(!lxp_ct_is_zero(proof->siblings[j],32U))
                return LXP_ERR_NON_CANONICAL;
    }
    if(spans_root(in->activities,in->count,activity_root)!=LXP_OK ||
       spans_root(in->events,in->count,event_root)!=LXP_OK ||
       lxp_merkle_leaf_hash(NULL,0U,empty_root)!=LXP_OK ||
       lxp_ct_memcmp(activity_root,header.activity_merkle_root,32U)!=0 ||
       lxp_ct_memcmp(event_root,header.event_merkle_root,32U)!=0 ||
       lxp_ct_memcmp(empty_root,header.oracle_root,32U)!=0 ||
       lxp_ct_memcmp(empty_root,header.data_availability_root,32U)!=0)
        return LXP_ERR_ROOT_MISMATCH;
    status=validate_canonical_items(in,&header);
    if(status!=LXP_OK)return status;
    if(lxp_kernel_batch_publication_digest(
           &in->base,&in->settled,in->activities,in->receipts,in->events,
           in->count,publication)!=LXP_OK ||
       lxp_ct_memcmp(publication,in->publication_digest,32U)!=0)
        return LXP_ERR_CONTEXT_MISMATCH;
    return LXP_OK;
}

static lxp_result encode_record(const lxp_daemon_batch_wal_input *in,
                                lxp_daemon_batch_wal_state state,
                                uint8_t **encoded, size_t *encoded_length)
{
    size_t length=WAL_FIXED_BYTES+WAL_DIGEST_BYTES, body_length=0U;
    size_t offset=0U, i, j;
    uint8_t *bytes, digest[32];
    lxp_result status=validate_input(in);
    if (status!=LXP_OK || (state!=LXP_DAEMON_BATCH_WAL_PREPARED &&
        state!=LXP_DAEMON_BATCH_WAL_ABORTED &&
        state!=LXP_DAEMON_BATCH_WAL_COMMITTED)) return status;
    for(i=0U;i<in->count;++i) {
        if(body_length>SIZE_MAX-in->activities[i].length ||
           body_length+in->activities[i].length>
               LXP_MAX_BATCH_BODY_BYTES)
            return LXP_ERR_LENGTH_LIMIT;
        body_length+=in->activities[i].length;
        if(body_length>SIZE_MAX-in->receipts[i].length ||
           body_length+in->receipts[i].length>
               LXP_MAX_BATCH_BODY_BYTES)
            return LXP_ERR_LENGTH_LIMIT;
        body_length+=in->receipts[i].length;
        if(body_length>SIZE_MAX-in->events[i].length ||
           body_length+in->events[i].length>
               LXP_MAX_BATCH_BODY_BYTES)
            return LXP_ERR_LENGTH_LIMIT;
        body_length+=in->events[i].length;
        if (!add_size(&length,12U) ||
            !add_size(&length,in->activities[i].length) ||
            !add_size(&length,in->receipts[i].length) ||
            !add_size(&length,in->events[i].length) ||
            !add_size(&length,WAL_PROOF_BYTES)) return LXP_ERR_LENGTH_LIMIT;
    }
    bytes=(uint8_t *)calloc(1U,length);
    if(bytes==NULL)return LXP_ERR_IO;
#define COPY(src,n) do { (void)memcpy(bytes+offset,(src),(n)); offset+=(n); } while(0)
    COPY(wal_magic,8U); put_u16(bytes+offset,WAL_VERSION); offset+=2U;
    bytes[offset++]=(uint8_t)state; bytes[offset++]=0U;
    put_u64(bytes+offset,(uint64_t)length); offset+=8U;
    put_u16(bytes+offset,in->protocol_version); offset+=2U;
    put_u32(bytes+offset,in->network_id); offset+=4U;
    put_u64(bytes+offset,in->epoch); offset+=8U;
    put_u64(bytes+offset,in->batch_number); offset+=8U;
    put_u64(bytes+offset,in->timestamp_ms); offset+=8U;
    put_u32(bytes+offset,in->parameter_version); offset+=4U;
    put_u32(bytes+offset,in->fee_schedule_version); offset+=4U;
    put_u32(bytes+offset,in->metering_schedule_version); offset+=4U;
    put_u64(bytes+offset,in->first_sequence); offset+=8U;
    put_u64(bytes+offset,in->last_sequence); offset+=8U;
    put_u16(bytes+offset,(uint16_t)in->count); offset+=2U;
    encode_boundary(bytes+offset,&in->base); offset+=72U;
    encode_boundary(bytes+offset,&in->settled); offset+=72U;
    COPY(in->publication_digest,32U);
    COPY(in->authorization.sequencer_id,32U);
    COPY(in->authorization.public_key,32U);
    put_u64(bytes+offset,in->authorization.first_batch_number); offset+=8U;
    put_u64(bytes+offset,in->authorization.last_batch_number); offset+=8U;
    bytes[offset++]=in->authorization.authorized; memset(bytes+offset,0,7U); offset+=7U;
    COPY(in->canonical_header.bytes,LXP_BATCH_HEADER_ENCODED_SIZE);
    COPY(in->header_signature,64U);
    for(i=0U;i<in->count;++i) {
        put_u32(bytes+offset,(uint32_t)in->activities[i].length); offset+=4U;
        put_u32(bytes+offset,(uint32_t)in->receipts[i].length); offset+=4U;
        put_u32(bytes+offset,(uint32_t)in->events[i].length); offset+=4U;
        COPY(in->activities[i].bytes,in->activities[i].length);
        COPY(in->receipts[i].bytes,in->receipts[i].length);
        COPY(in->events[i].bytes,in->events[i].length);
        put_u32(bytes+offset,in->receipt_proofs[i].leaf_index); offset+=4U;
        put_u32(bytes+offset,in->receipt_proofs[i].leaf_count); offset+=4U;
        bytes[offset++]=in->receipt_proofs[i].depth;
        for(j=0U;j<LXP_MERKLE_MAX_DEPTH;++j) COPY(in->receipt_proofs[i].siblings[j],32U);
    }
#undef COPY
    if(offset+WAL_DIGEST_BYTES!=length || wal_digest(bytes,offset,digest)!=LXP_OK) {
        lxp_secure_zero(bytes,length); free(bytes); return LXP_FATAL_INVARIANT;
    }
    (void)memcpy(bytes+offset,digest,32U);
    *encoded=bytes; *encoded_length=length; return LXP_OK;
}

static lxp_result paths(const char *directory, char final[4096])
{
    int n;
    if(directory==NULL || directory[0]=='\0')return LXP_ERR_NON_CANONICAL;
    n=snprintf(final,4096,"%s/prepared-batch.lxw",directory);
    return n<0 || n>=4096 ? LXP_ERR_LENGTH_LIMIT : LXP_OK;
}

static lxp_result durable_replace(const char *directory,const uint8_t *bytes,
                                  size_t length,bool require_absent,
                                  const uint8_t *expected_current,
                                  size_t expected_current_length)
{
    char final[4096],temp[96]={0}; size_t offset=0U; int fd=-1,dfd=-1,n;
    bool locked=false,temp_named=false,directory_changed=false;
    lxp_result status=paths(directory,final);
    (void)final;
    if(status==LXP_OK) {
        if(pthread_mutex_lock(&wal_replace_mutex)!=0)status=LXP_ERR_IO;
        else locked=true;
    }
    if(status==LXP_OK) {
        dfd=open(directory,O_RDONLY|O_DIRECTORY|O_CLOEXEC|O_NOFOLLOW);
        if(dfd<0)status=LXP_ERR_IO;
    }
    if(status==LXP_OK && flock(dfd,LOCK_EX)!=0)status=LXP_ERR_IO;
    if(status==LXP_OK && require_absent) {
        struct stat existing;
        if(fstatat(dfd,"prepared-batch.lxw",&existing,
                   AT_SYMLINK_NOFOLLOW)==0)
            status=LXP_ERR_CONTEXT_MISMATCH;
        else if(errno!=ENOENT)status=LXP_ERR_IO;
    }
    if(status==LXP_OK && expected_current!=NULL) {
        struct stat existing;
        uint8_t buffer[4096];
        size_t compared=0U;
        int current=openat(dfd,"prepared-batch.lxw",
                           O_RDONLY|O_CLOEXEC|O_NOFOLLOW);
        if(current<0 || fstat(current,&existing)!=0 || existing.st_size<0 ||
           (uint64_t)existing.st_size!=(uint64_t)expected_current_length)
            status=LXP_FATAL_REPLAY_DIVERGENCE;
        while(status==LXP_OK && compared<expected_current_length) {
            size_t wanted=expected_current_length-compared;
            ssize_t count;
            if(wanted>sizeof(buffer))wanted=sizeof(buffer);
            count=read(current,buffer,wanted);
            if(count>0) {
                if(lxp_ct_memcmp(buffer,expected_current+compared,
                                 (size_t)count)!=0)
                    status=LXP_FATAL_REPLAY_DIVERGENCE;
                compared+=(size_t)count;
            } else if(count<0 && errno==EINTR)continue;
            else status=LXP_ERR_IO;
        }
        if(current>=0 && close(current)!=0 && status==LXP_OK)
            status=LXP_ERR_IO;
        lxp_secure_zero(buffer,sizeof(buffer));
    }
    if(status==LXP_OK) {
        uint64_t starting_counter=wal_temporary_counter;
        do {
            ++wal_temporary_counter;
            n=snprintf(temp,sizeof(temp),".prepared-batch.%llu.%llu.tmp",
                       (unsigned long long)getpid(),
                       (unsigned long long)wal_temporary_counter);
            if(n<0 || (size_t)n>=sizeof(temp)) {
                status=LXP_ERR_LENGTH_LIMIT;break;
            }
            fd=openat(dfd,temp,O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC|O_NOFOLLOW,0600);
            if(fd>=0){temp_named=true;break;}
            if(errno!=EEXIST){status=LXP_ERR_IO;break;}
        } while(wal_temporary_counter!=starting_counter);
        if(status==LXP_OK && fd<0)status=LXP_ERR_IO;
    }
    while(status==LXP_OK && offset<length) {
        ssize_t written=write(fd,bytes+offset,length-offset);
        if(written>0)offset+=(size_t)written; else if(written<0 && errno==EINTR)continue; else status=LXP_ERR_IO;
    }
    if(status==LXP_OK && fdatasync(fd)!=0)status=LXP_ERR_IO;
    if(fd>=0 && close(fd)!=0 && status==LXP_OK)status=LXP_ERR_IO;
    fd=-1;
    if(status==LXP_OK && require_absent) {
        if(linkat(dfd,temp,dfd,"prepared-batch.lxw",0)!=0)
            status=errno==EEXIST ? LXP_ERR_CONTEXT_MISMATCH : LXP_ERR_IO;
        else {
            directory_changed=true;
            if(unlinkat(dfd,temp,0)!=0)status=LXP_ERR_IO;
            else temp_named=false;
        }
    } else if(status==LXP_OK &&
              renameat(dfd,temp,dfd,"prepared-batch.lxw")!=0) {
        status=LXP_ERR_IO;
    } else if(status==LXP_OK) {
        directory_changed=true;
    }
    if(status!=LXP_OK && dfd>=0 && temp_named &&
       unlinkat(dfd,temp,0)==0) {
        temp_named=false;directory_changed=true;
    }
    if(dfd>=0 && directory_changed && fsync(dfd)!=0 && status==LXP_OK)
        status=LXP_ERR_IO;
    if(dfd>=0)(void)close(dfd);
    if(locked && pthread_mutex_unlock(&wal_replace_mutex)!=0 && status==LXP_OK)
        status=LXP_ERR_IO;
    return status;
}

lxp_result lxp_daemon_batch_wal_write_prepared(const char *directory,
 const lxp_daemon_batch_wal_input *input,uint8_t digest[32])
{
    uint8_t *bytes=NULL; size_t length=0U; lxp_result status;
    if(digest==NULL)return LXP_ERR_NON_CANONICAL;
    status=encode_record(input,LXP_DAEMON_BATCH_WAL_PREPARED,&bytes,&length);
    if(status==LXP_OK)status=durable_replace(
        directory,bytes,length,true,NULL,0U);
    if(status==LXP_OK)(void)memcpy(digest,input->publication_digest,32U);
    if(bytes!=NULL){lxp_secure_zero(bytes,length);free(bytes);} return status;
}

lxp_result lxp_daemon_batch_wal_commit_kernel(
 const char *directory,const lxp_daemon_batch_wal_input *input,
 lxp_kernel *kernel,lxp_identity_store *identities,
 const lxp_activity *activities,lxp_kernel_prepared_batch *prepared,
 lxp_daemon_batch_wal_checkpoint_fn checkpoint,void *checkpoint_context,
 lxp_daemon_batch_wal_record **record)
{
    lxp_daemon_batch_wal_record *loaded=NULL;
    lxp_daemon_batch_wal_record *preexisting=NULL;
    lxp_daemon_batch_wal_recovery recovery;
    lxp_kernel_batch_boundary live;
    uint8_t fsynced_digest[32];
    bool present=false,preexisting_present=false,live_committed=false;
    lxp_result status;
    if(input==NULL || kernel==NULL || identities==NULL || activities==NULL ||
       prepared==NULL || checkpoint==NULL || record==NULL)
        return LXP_ERR_NON_CANONICAL;
    *record=NULL;
    status=lxp_daemon_batch_wal_load(directory,&input->authorization,
                                     &preexisting,&preexisting_present);
    if(status==LXP_OK && preexisting_present)status=LXP_ERR_CONTEXT_MISMATCH;
    lxp_daemon_batch_wal_destroy(preexisting);
    if(status==LXP_OK)status=lxp_daemon_batch_wal_write_prepared(
        directory,input,fsynced_digest);
    if(status==LXP_OK)status=lxp_daemon_batch_wal_load(
        directory,&input->authorization,&loaded,&present);
    if(status==LXP_OK && !present)status=LXP_ERR_LOG_TRUNCATED;
    if(status==LXP_OK &&
       lxp_ct_memcmp(lxp_daemon_batch_wal_view(loaded)->publication_digest,
                     fsynced_digest,32U)!=0)
        status=LXP_ERR_CONTEXT_MISMATCH;
    if(status==LXP_OK)status=lxp_kernel_batch_boundary_read(kernel,&live);
    if(status==LXP_OK)status=lxp_daemon_batch_wal_classify(
        loaded,&live,&recovery);
    if(status==LXP_OK && recovery!=LXP_DAEMON_BATCH_WAL_DISCARD_BASE)
        status=LXP_FATAL_REPLAY_DIVERGENCE;
    if(status==LXP_OK)status=lxp_kernel_commit_prepared_batch(
        kernel,identities,prepared,fsynced_digest);
    if(status==LXP_OK)live_committed=true;
    if(status==LXP_OK)status=checkpoint(
        checkpoint_context,&loaded->view.settled);
    if(status==LXP_OK)status=lxp_kernel_finalize_prepared_batch_publication(
        kernel,activities,prepared,fsynced_digest);
    if(status!=LXP_OK && live_committed)status=LXP_FATAL_INVARIANT;
    if(status==LXP_OK){*record=loaded;loaded=NULL;}
    lxp_secure_zero(fsynced_digest,sizeof(fsynced_digest));
    lxp_daemon_batch_wal_destroy(loaded);
    return status;
}

static lxp_result read_record(const char *directory,uint8_t **bytes,size_t *length,bool *present)
{
    char final[4096]; struct stat st; size_t offset=0U; int fd=-1,dfd=-1;
    DIR *stream=NULL;
    bool swept=false;
    lxp_result status=paths(directory,final); (void)final;
    if(status!=LXP_OK)return status;
    dfd=open(directory,O_RDONLY|O_DIRECTORY|O_CLOEXEC|O_NOFOLLOW);
    if(dfd<0)return LXP_ERR_IO;
    if(flock(dfd,LOCK_EX)!=0){(void)close(dfd);return LXP_ERR_IO;}
    {
        int scan_fd=dup(dfd);
        if(scan_fd<0)status=LXP_ERR_IO;
        else {
            stream=fdopendir(scan_fd);
            if(stream==NULL){(void)close(scan_fd);status=LXP_ERR_IO;}
        }
    }
    if(status==LXP_OK) {
        struct dirent *entry;
        for(;;) {
            errno=0;
            entry=readdir(stream);
            if(entry==NULL) {
                if(errno!=0)status=LXP_ERR_IO;
                break;
            }
            const char *name=entry->d_name;
            static const char prefix[]=".prepared-batch.";
            static const char suffix[]=".tmp";
            size_t prefix_length=sizeof(prefix)-1U;
            size_t name_length=strlen(name),position=prefix_length;
            bool valid=name_length>prefix_length+sizeof(suffix) &&
                memcmp(name,prefix,prefix_length)==0;
            size_t digits=0U,digits_start=position;
            while(valid && position<name_length &&
                  name[position]>='0' && name[position]<='9') {
                ++position;++digits;
            }
            valid=valid && decimal_component(name+digits_start,digits,
                                              (uint64_t)INT_MAX) &&
                position<name_length &&
                name[position++]=='.';
            digits=0U;
            digits_start=position;
            while(valid && position<name_length &&
                  name[position]>='0' && name[position]<='9') {
                ++position;++digits;
            }
            valid=valid && decimal_component(name+digits_start,digits,
                                              UINT64_MAX) &&
                name_length-position==sizeof(suffix)-1U &&
                memcmp(name+position,suffix,sizeof(suffix)-1U)==0;
            if(valid) {
                if(unlinkat(dfd,name,0)!=0 && errno!=ENOENT) {
                    status=LXP_ERR_IO;break;
                }
                swept=true;
            }
        }
    }
    if(stream!=NULL && closedir(stream)!=0 && status==LXP_OK)
        status=LXP_ERR_IO;
    if(status==LXP_OK && swept && fsync(dfd)!=0)status=LXP_ERR_IO;
    if(status!=LXP_OK){(void)close(dfd);return status;}
    fd=openat(dfd,"prepared-batch.lxw",O_RDONLY|O_CLOEXEC|O_NOFOLLOW);
    if(fd<0){int saved=errno;(void)close(dfd);if(saved==ENOENT){*present=false;return LXP_OK;}return LXP_ERR_IO;}
    if(fstat(fd,&st)!=0 || st.st_size<(off_t)(WAL_FIXED_BYTES+WAL_DIGEST_BYTES) ||
       st.st_size>(off_t)WAL_MAX_BYTES){(void)close(fd);(void)close(dfd);return LXP_ERR_LOG_CORRUPT;}
    *length=(size_t)st.st_size; *bytes=(uint8_t *)malloc(*length);
    if(*bytes==NULL){(void)close(fd);(void)close(dfd);return LXP_ERR_IO;}
    while(offset<*length){ssize_t n=read(fd,*bytes+offset,*length-offset);if(n>0)offset+=(size_t)n;else if(n<0&&errno==EINTR)continue;else{status=LXP_ERR_IO;break;}}
    if(close(fd)!=0 && status==LXP_OK)status=LXP_ERR_IO;
    if(close(dfd)!=0 && status==LXP_OK)status=LXP_ERR_IO;
    if(status!=LXP_OK){lxp_secure_zero(*bytes,*length);free(*bytes);*bytes=NULL;return status;}
    *present=true; return LXP_OK;
}

lxp_result lxp_daemon_batch_wal_load(const char *directory,
 const lxp_sequencer_authorization *authorization,
 lxp_daemon_batch_wal_record **out,bool *present)
{
    lxp_daemon_batch_wal_record *r=NULL; uint8_t *bytes=NULL,digest[32];
    size_t length=0U,offset=0U,i,j; lxp_result status;
    if(authorization==NULL||out==NULL||present==NULL)return LXP_ERR_NON_CANONICAL;
    *out=NULL; *present=false; status=read_record(directory,&bytes,&length,present);
    if(status!=LXP_OK||!*present)return status;
    if(memcmp(bytes,wal_magic,8U)!=0 || get_u16(bytes+8U)!=WAL_VERSION ||
       bytes[11U]!=0U ||
       get_u64(bytes+12U)!=(uint64_t)length ||
       wal_digest(bytes,length-32U,digest)!=LXP_OK ||
       lxp_ct_memcmp(digest,bytes+length-32U,32U)!=0){status=LXP_ERR_LOG_CORRUPT;goto fail;}
    r=(lxp_daemon_batch_wal_record *)calloc(1U,sizeof(*r)); if(r==NULL){status=LXP_ERR_IO;goto fail;}
    r->owned=bytes;r->owned_length=length;bytes=NULL; offset=10U;
    r->state=(lxp_daemon_batch_wal_state)r->owned[offset++]; offset++;
    offset+=8U; r->view.protocol_version=get_u16(r->owned+offset);offset+=2U;
    r->view.network_id=get_u32(r->owned+offset);offset+=4U;
    r->view.epoch=get_u64(r->owned+offset);offset+=8U;
    r->view.batch_number=get_u64(r->owned+offset);offset+=8U;
    r->view.timestamp_ms=get_u64(r->owned+offset);offset+=8U;
    r->view.parameter_version=get_u32(r->owned+offset);offset+=4U;
    r->view.fee_schedule_version=get_u32(r->owned+offset);offset+=4U;
    r->view.metering_schedule_version=get_u32(r->owned+offset);offset+=4U;
    r->view.first_sequence=get_u64(r->owned+offset);offset+=8U;
    r->view.last_sequence=get_u64(r->owned+offset);offset+=8U;
    r->view.count=get_u16(r->owned+offset);offset+=2U;
    decode_boundary(r->owned+offset,&r->view.base);offset+=72U;
    decode_boundary(r->owned+offset,&r->view.settled);offset+=72U;
    (void)memcpy(r->view.publication_digest,r->owned+offset,32U);offset+=32U;
    (void)memcpy(r->view.authorization.sequencer_id,r->owned+offset,32U);offset+=32U;
    (void)memcpy(r->view.authorization.public_key,r->owned+offset,32U);offset+=32U;
    r->view.authorization.first_batch_number=get_u64(r->owned+offset);offset+=8U;
    r->view.authorization.last_batch_number=get_u64(r->owned+offset);offset+=8U;
    r->view.authorization.authorized=r->owned[offset++];offset+=7U;
    r->view.canonical_header.bytes=r->owned+offset;r->view.canonical_header.length=LXP_BATCH_HEADER_ENCODED_SIZE;offset+=LXP_BATCH_HEADER_ENCODED_SIZE;
    (void)memcpy(r->view.header_signature,r->owned+offset,64U);offset+=64U;
    if((r->state!=LXP_DAEMON_BATCH_WAL_PREPARED &&
        r->state!=LXP_DAEMON_BATCH_WAL_ABORTED &&
        r->state!=LXP_DAEMON_BATCH_WAL_COMMITTED) ||
       !lxp_ct_is_zero(r->owned+offset-7U-LXP_BATCH_HEADER_ENCODED_SIZE-64U,
                       7U) ||
       r->view.count==0U||r->view.count>LXP_DAEMON_BATCH_WAL_MAX_ITEMS ||
       r->view.authorization.authorized!=authorization->authorized ||
       r->view.authorization.first_batch_number!=authorization->first_batch_number ||
       r->view.authorization.last_batch_number!=authorization->last_batch_number ||
       lxp_ct_memcmp(r->view.authorization.sequencer_id,
                     authorization->sequencer_id,32U)!=0 ||
       lxp_ct_memcmp(r->view.authorization.public_key,
                     authorization->public_key,32U)!=0){status=LXP_ERR_BAD_SIGNATURE;goto fail;}
    for(i=0U;i<r->view.count;++i){uint32_t al,rl,el;
        if(offset>length-32U || length-32U-offset<12U){status=LXP_ERR_LOG_TRUNCATED;goto fail;}
        al=get_u32(r->owned+offset);offset+=4U;rl=get_u32(r->owned+offset);offset+=4U;el=get_u32(r->owned+offset);offset+=4U;
        if((size_t)al>length-32U-offset){status=LXP_ERR_LOG_TRUNCATED;goto fail;}r->activities[i].bytes=r->owned+offset;r->activities[i].length=al;offset+=al;
        if((size_t)rl>length-32U-offset){status=LXP_ERR_LOG_TRUNCATED;goto fail;}r->receipts[i].bytes=r->owned+offset;r->receipts[i].length=rl;offset+=rl;
        if((size_t)el>length-32U-offset){status=LXP_ERR_LOG_TRUNCATED;goto fail;}r->events[i].bytes=r->owned+offset;r->events[i].length=el;offset+=el;
        if(length-32U-offset<WAL_PROOF_BYTES){status=LXP_ERR_LOG_TRUNCATED;goto fail;}
        r->proofs[i].leaf_index=get_u32(r->owned+offset);offset+=4U;r->proofs[i].leaf_count=get_u32(r->owned+offset);offset+=4U;r->proofs[i].depth=r->owned[offset++];
        for(j=0U;j<LXP_MERKLE_MAX_DEPTH;++j){(void)memcpy(r->proofs[i].siblings[j],r->owned+offset,32U);offset+=32U;}
    }
    if(offset!=length-32U){status=LXP_ERR_TRAILING_BYTES;goto fail;}
    r->view.activities=r->activities;r->view.receipts=r->receipts;r->view.events=r->events;r->view.receipt_proofs=r->proofs;
    status=validate_input(&r->view);if(status!=LXP_OK)goto fail;*out=r;return LXP_OK;
fail:
    if(r!=NULL)lxp_daemon_batch_wal_destroy(r);
    if(bytes!=NULL){lxp_secure_zero(bytes,length);free(bytes);} *present=false;return status;
}

lxp_result lxp_daemon_batch_wal_classify(const lxp_daemon_batch_wal_record *r,
 const lxp_kernel_batch_boundary *live,lxp_daemon_batch_wal_recovery *recovery)
{
    if(r==NULL||live==NULL||recovery==NULL)return LXP_ERR_NON_CANONICAL;
    if(boundary_equal(live,&r->view.base)) {
        if(r->state==LXP_DAEMON_BATCH_WAL_PREPARED)
            *recovery=LXP_DAEMON_BATCH_WAL_DISCARD_BASE;
        else if(r->state==LXP_DAEMON_BATCH_WAL_ABORTED)
            *recovery=LXP_DAEMON_BATCH_WAL_ALREADY_ABORTED;
        else return LXP_FATAL_REPLAY_DIVERGENCE;
        return LXP_OK;
    }
    if(boundary_equal(live,&r->view.settled)) {
        if(r->state==LXP_DAEMON_BATCH_WAL_PREPARED)
            *recovery=LXP_DAEMON_BATCH_WAL_FINALIZE_SETTLED;
        else if(r->state==LXP_DAEMON_BATCH_WAL_COMMITTED)
            *recovery=LXP_DAEMON_BATCH_WAL_ALREADY_COMMITTED;
        else return LXP_FATAL_REPLAY_DIVERGENCE;
        return LXP_OK;
    }
    if(r->state!=LXP_DAEMON_BATCH_WAL_PREPARED &&
       r->state!=LXP_DAEMON_BATCH_WAL_ABORTED &&
       r->state!=LXP_DAEMON_BATCH_WAL_COMMITTED)
        return LXP_ERR_LOG_CORRUPT;
    return LXP_FATAL_REPLAY_DIVERGENCE;
}

lxp_result lxp_daemon_batch_wal_transition(const char *directory,
 lxp_daemon_batch_wal_record *record,const lxp_kernel_batch_boundary *live,
 lxp_daemon_batch_wal_state state)
{
    uint8_t *bytes=NULL;size_t length=0U;lxp_result status;
    if(record==NULL || live==NULL ||
       (state!=LXP_DAEMON_BATCH_WAL_ABORTED && state!=LXP_DAEMON_BATCH_WAL_COMMITTED) ||
       record->state!=LXP_DAEMON_BATCH_WAL_PREPARED)return LXP_ERR_NON_CANONICAL;
    if((state==LXP_DAEMON_BATCH_WAL_ABORTED &&
        !boundary_equal(live,&record->view.base)) ||
       (state==LXP_DAEMON_BATCH_WAL_COMMITTED &&
        !boundary_equal(live,&record->view.settled)))
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status=encode_record(&record->view,state,&bytes,&length);
    if(status==LXP_OK)status=durable_replace(
        directory,bytes,length,false,record->owned,record->owned_length);
    if(status==LXP_OK)record->state=state;
    if(bytes!=NULL){lxp_secure_zero(bytes,length);free(bytes);}return status;
}

lxp_result lxp_daemon_batch_wal_retire(const char *directory,
 const lxp_daemon_batch_wal_record *record,
 const lxp_kernel_batch_boundary *live)
{
    char final[4096];
    int dfd=-1,fd=-1;
    bool locked=false,present=true;
    uint8_t *expected=NULL,*actual=NULL;
    size_t expected_length=0U,offset=0U;
    struct stat information;
    lxp_result status;
    if(record==NULL || live==NULL)return LXP_ERR_NON_CANONICAL;
    if((record->state!=LXP_DAEMON_BATCH_WAL_ABORTED ||
         !boundary_equal(live,&record->view.base)) &&
        (record->state!=LXP_DAEMON_BATCH_WAL_COMMITTED ||
         !boundary_equal(live,&record->view.settled)))
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status=encode_record(&record->view,record->state,
                         &expected,&expected_length);
    if(status==LXP_OK)status=paths(directory,final);
    (void)final;
    if(status==LXP_OK) {
        if(pthread_mutex_lock(&wal_replace_mutex)!=0)status=LXP_ERR_IO;
        else locked=true;
    }
    if(status==LXP_OK) {
        dfd=open(directory,O_RDONLY|O_DIRECTORY|O_CLOEXEC|O_NOFOLLOW);
        if(dfd<0)status=LXP_ERR_IO;
    }
    if(status==LXP_OK && flock(dfd,LOCK_EX)!=0)status=LXP_ERR_IO;
    if(status==LXP_OK) {
        fd=openat(dfd,"prepared-batch.lxw",O_RDONLY|O_CLOEXEC|O_NOFOLLOW);
        if(fd<0 && errno==ENOENT)present=false;
        else if(fd<0)status=LXP_ERR_IO;
    }
    if(status==LXP_OK && present) {
        if(fstat(fd,&information)!=0 || information.st_size<0 ||
           (uint64_t)information.st_size!=(uint64_t)expected_length)
            status=LXP_FATAL_REPLAY_DIVERGENCE;
        else {
            actual=(uint8_t *)malloc(expected_length);
            if(actual==NULL)status=LXP_ERR_IO;
        }
    }
    while(status==LXP_OK && present && offset<expected_length) {
        ssize_t count=read(fd,actual+offset,expected_length-offset);
        if(count>0)offset+=(size_t)count;
        else if(count<0 && errno==EINTR)continue;
        else status=LXP_ERR_IO;
    }
    if(fd>=0 && close(fd)!=0 && status==LXP_OK)status=LXP_ERR_IO;
    fd=-1;
    if(status==LXP_OK && present &&
       lxp_ct_memcmp(actual,expected,expected_length)!=0)
        status=LXP_FATAL_REPLAY_DIVERGENCE;
    if(status==LXP_OK && present &&
       unlinkat(dfd,"prepared-batch.lxw",0)!=0)status=LXP_ERR_IO;
    if(status==LXP_OK && fsync(dfd)!=0)status=LXP_ERR_IO;
    if(fd>=0)(void)close(fd);
    if(dfd>=0)(void)close(dfd);
    if(locked && pthread_mutex_unlock(&wal_replace_mutex)!=0 &&
       status==LXP_OK)status=LXP_ERR_IO;
    if(actual!=NULL){lxp_secure_zero(actual,expected_length);free(actual);}
    if(expected!=NULL){lxp_secure_zero(expected,expected_length);free(expected);}
    return status;
}

const lxp_daemon_batch_wal_input *lxp_daemon_batch_wal_view(const lxp_daemon_batch_wal_record *r){return r==NULL?NULL:&r->view;}
lxp_daemon_batch_wal_state lxp_daemon_batch_wal_record_state(const lxp_daemon_batch_wal_record *r){return r==NULL?0:r->state;}
void lxp_daemon_batch_wal_destroy(lxp_daemon_batch_wal_record *r){if(r!=NULL){if(r->owned!=NULL){lxp_secure_zero(r->owned,r->owned_length);free(r->owned);}lxp_secure_zero(r,sizeof(*r));free(r);}}
