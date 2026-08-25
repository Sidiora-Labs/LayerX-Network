#define OPENSSL_API_COMPAT 0x10100000L
#include "layerx/lxp_crypto.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/ecdsa.h>
#include <openssl/obj_mac.h>
#include <string.h>

static const uint8_t half_order[32] = {
    0x7fU,0xffU,0xffU,0xffU,0xffU,0xffU,0xffU,0xffU,
    0xffU,0xffU,0xffU,0xffU,0xffU,0xffU,0xffU,
    0x5dU,0x57U,0x6eU,0x73U,0x57U,0xa4U,0x50U,0x1dU,
    0xdfU,0xe9U,0x2fU,0x46U,0x68U,0x1bU,0x20U,0xa0U
};

bool lxp_secp256k1_sig_is_low_s(const uint8_t signature[64])
{
    size_t i;
    uint8_t nonzero = 0U;
    if (signature == NULL) return false;
    for (i = 32U; i < 64U; ++i) nonzero |= signature[i];
    if (nonzero == 0U) return false;
    for (i = 32U; i < 64U; ++i) {
        if (signature[i] < half_order[i - 32U]) return true;
        if (signature[i] > half_order[i - 32U]) return false;
    }
    return true;
}

static ECDSA_SIG *signature_from_compact(const uint8_t signature[64])
{
    ECDSA_SIG *parsed = ECDSA_SIG_new();
    BIGNUM *r = BN_bin2bn(signature, 32, NULL);
    BIGNUM *s = BN_bin2bn(signature + 32U, 32, NULL);
    if (parsed == NULL || r == NULL || s == NULL || BN_is_zero(r) || BN_is_zero(s) ||
        ECDSA_SIG_set0(parsed, r, s) != 1) {
        BN_free(r); BN_free(s); ECDSA_SIG_free(parsed); return NULL;
    }
    return parsed;
}

lxp_result lxp_secp256k1_verify(const uint8_t *public_key,
                                size_t public_key_length,
                                const uint8_t signature[64],
                                lxp_domain_tag_id domain,
                                const void *message, size_t message_length)
{
    EC_KEY *key;
    const uint8_t *cursor = public_key;
    ECDSA_SIG *parsed;
    uint8_t digest[32];
    int verified;
    if (public_key == NULL || signature == NULL ||
        (public_key_length != 33U && public_key_length != 65U) ||
        !lxp_secp256k1_sig_is_low_s(signature) ||
        lxp_hash_domain(domain, message, message_length, digest) != LXP_OK)
        return LXP_ERR_BAD_SIGNATURE;
    key = EC_KEY_new_by_curve_name(NID_secp256k1);
    parsed = signature_from_compact(signature);
    if (key == NULL || parsed == NULL ||
        o2i_ECPublicKey(&key, &cursor, (long)public_key_length) == NULL ||
        cursor != public_key + public_key_length || EC_KEY_check_key(key) != 1) {
        EC_KEY_free(key); ECDSA_SIG_free(parsed); return LXP_ERR_BAD_SIGNATURE;
    }
    verified = ECDSA_do_verify(digest, 32, parsed, key);
    EC_KEY_free(key); ECDSA_SIG_free(parsed);
    lxp_secure_zero(digest, sizeof(digest));
    return verified == 1 ? LXP_OK : LXP_ERR_BAD_SIGNATURE;
}

static uint64_t rotate_left64(uint64_t value, unsigned shift)
{
    return shift == 0U ? value : (value << shift) | (value >> (64U - shift));
}

static void keccak_permute(uint64_t state[25])
{
    static const uint64_t constants[24] = {
        0x0000000000000001ULL,0x0000000000008082ULL,0x800000000000808aULL,0x8000000080008000ULL,
        0x000000000000808bULL,0x0000000080000001ULL,0x8000000080008081ULL,0x8000000000008009ULL,
        0x000000000000008aULL,0x0000000000000088ULL,0x0000000080008009ULL,0x000000008000000aULL,
        0x000000008000808bULL,0x800000000000008bULL,0x8000000000008089ULL,0x8000000000008003ULL,
        0x8000000000008002ULL,0x8000000000000080ULL,0x000000000000800aULL,0x800000008000000aULL,
        0x8000000080008081ULL,0x8000000000008080ULL,0x0000000080000001ULL,0x8000000080008008ULL
    };
    static const unsigned rotation[25] = {0U,1U,62U,28U,27U,36U,44U,6U,55U,20U,3U,10U,43U,25U,39U,41U,45U,15U,21U,8U,18U,2U,61U,56U,14U};
    size_t round;
    for (round = 0U; round < 24U; ++round) {
        uint64_t c[5], d[5], b[25];
        size_t x,y;
        for (x=0U;x<5U;++x) c[x]=state[x]^state[x+5U]^state[x+10U]^state[x+15U]^state[x+20U];
        for (x=0U;x<5U;++x) d[x]=c[(x+4U)%5U]^rotate_left64(c[(x+1U)%5U],1U);
        for (x=0U;x<5U;++x) for (y=0U;y<5U;++y) state[x+5U*y]^=d[x];
        for (x=0U;x<5U;++x) for (y=0U;y<5U;++y)
            b[y+5U*((2U*x+3U*y)%5U)]=rotate_left64(state[x+5U*y],rotation[x+5U*y]);
        for (x=0U;x<5U;++x) for (y=0U;y<5U;++y)
            state[x+5U*y]=b[x+5U*y]^((~b[(x+1U)%5U+5U*y])&b[(x+2U)%5U+5U*y]);
        state[0]^=constants[round];
    }
}

static void keccak256(const uint8_t *data, size_t length, uint8_t out[32])
{
    uint64_t state[25] = {0};
    uint8_t block[136] = {0};
    size_t i;
    while (length >= sizeof(block)) {
        for (i=0U;i<sizeof(block);++i) state[i/8U]^=(uint64_t)data[i]<<((i%8U)*8U);
        keccak_permute(state); data+=sizeof(block); length-=sizeof(block);
    }
    (void)memcpy(block,data,length); block[length]=0x01U; block[135]^=0x80U;
    for (i=0U;i<sizeof(block);++i) state[i/8U]^=(uint64_t)block[i]<<((i%8U)*8U);
    keccak_permute(state);
    for (i=0U;i<32U;++i) out[i]=(uint8_t)(state[i/8U]>>((i%8U)*8U));
    lxp_secure_zero(state,sizeof(state)); lxp_secure_zero(block,sizeof(block));
}

lxp_result lxp_secp256k1_address(const uint8_t *public_key,
                                 size_t public_key_length,
                                 uint8_t address[20])
{
    EC_GROUP *group;
    EC_POINT *point;
    BN_CTX *context;
    uint8_t uncompressed[65];
    uint8_t hash[32];
    size_t length;
    lxp_result status = LXP_ERR_BAD_SIGNATURE;
    if (public_key == NULL || address == NULL ||
        (public_key_length != 33U && public_key_length != 65U))
        return LXP_ERR_BAD_SIGNATURE;
    group = EC_GROUP_new_by_curve_name(NID_secp256k1);
    point = group == NULL ? NULL : EC_POINT_new(group);
    context = BN_CTX_new();
    if (group != NULL && point != NULL && context != NULL &&
        EC_POINT_oct2point(group, point, public_key, public_key_length,
                           context) == 1 &&
        EC_POINT_is_on_curve(group, point, context) == 1) {
        length = EC_POINT_point2oct(group, point, POINT_CONVERSION_UNCOMPRESSED,
                                    uncompressed, sizeof(uncompressed), context);
        if (length == sizeof(uncompressed)) {
            keccak256(uncompressed + 1U, 64U, hash);
            (void)memcpy(address, hash + 12U, 20U);
            status = LXP_OK;
        }
    }
    EC_POINT_free(point);
    EC_GROUP_free(group);
    BN_CTX_free(context);
    lxp_secure_zero(uncompressed, sizeof(uncompressed));
    lxp_secure_zero(hash, sizeof(hash));
    return status;
}

lxp_result lxp_secp256k1_recover_address(const uint8_t signature[64],
                                         uint8_t recovery_id,
                                         const uint8_t digest[32],
                                         uint8_t address[20])
{
    EC_GROUP *group = NULL;
    BN_CTX *context = NULL;
    BIGNUM *r=NULL,*s=NULL,*order=NULL,*prime=NULL,*x=NULL,*e=NULL,*negative_e=NULL,*inverse=NULL;
    EC_POINT *point_r=NULL,*check=NULL,*sum=NULL,*public_point=NULL;
    uint8_t public_bytes[65], hash[32];
    size_t public_length;
    lxp_result result = LXP_ERR_BAD_SIGNATURE;
    if (signature==NULL || digest==NULL || address==NULL || recovery_id>3U ||
        !lxp_secp256k1_sig_is_low_s(signature)) return LXP_ERR_BAD_SIGNATURE;
    group=EC_GROUP_new_by_curve_name(NID_secp256k1); context=BN_CTX_new();
    r=BN_bin2bn(signature,32,NULL); s=BN_bin2bn(signature+32U,32,NULL);
    order=BN_new(); prime=BN_new(); e=BN_bin2bn(digest,32,NULL);
    negative_e=BN_new();
    if (!group||!context||!r||!s||!order||!prime||!e||!negative_e)
        goto cleanup;
    x=BN_dup(r);
    point_r=EC_POINT_new(group); check=EC_POINT_new(group);
    sum=EC_POINT_new(group); public_point=EC_POINT_new(group);
    if (!x||!point_r||!check||!sum||!public_point ||
        EC_GROUP_get_order(group,order,context)!=1 || EC_GROUP_get_curve(group,prime,NULL,NULL,context)!=1) goto cleanup;
    if ((recovery_id>>1U)!=0U && BN_add(x,x,order)!=1) goto cleanup;
    if (BN_cmp(x,prime)>=0 || EC_POINT_set_compressed_coordinates(group,point_r,x,(int)(recovery_id&1U),context)!=1 ||
        EC_POINT_mul(group,check,NULL,point_r,order,context)!=1 || !EC_POINT_is_at_infinity(group,check)) goto cleanup;
    if (BN_nnmod(e,e,order,context)!=1 || BN_sub(negative_e,order,e)!=1) goto cleanup;
    inverse=BN_mod_inverse(NULL,r,order,context); if (!inverse) goto cleanup;
    if (EC_POINT_mul(group,sum,negative_e,point_r,s,context)!=1 ||
        EC_POINT_mul(group,public_point,NULL,sum,inverse,context)!=1) goto cleanup;
    public_length=EC_POINT_point2oct(group,public_point,POINT_CONVERSION_UNCOMPRESSED,
                                     public_bytes,sizeof(public_bytes),context);
    if (public_length!=65U) goto cleanup;
    keccak256(public_bytes+1U,64U,hash); (void)memcpy(address,hash+12U,20U); result=LXP_OK;
cleanup:
    BN_free(r);BN_free(s);BN_free(order);BN_free(prime);BN_free(x);BN_free(e);BN_free(negative_e);BN_free(inverse);
    EC_POINT_free(point_r);EC_POINT_free(check);EC_POINT_free(sum);EC_POINT_free(public_point);
    BN_CTX_free(context);EC_GROUP_free(group);lxp_secure_zero(public_bytes,sizeof(public_bytes));lxp_secure_zero(hash,sizeof(hash));
    return result;
}
