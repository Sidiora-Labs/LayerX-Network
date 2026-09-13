#define OPENSSL_API_COMPAT 0x10100000L
#include "layerx/lxp_crypto.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/ecdsa.h>
#include <openssl/obj_mac.h>
#include <string.h>

static int compact_signature(const ECDSA_SIG *signature, uint8_t out[64])
{
    const BIGNUM *r,*s; ECDSA_SIG_get0(signature,&r,&s);
    return BN_bn2binpad(r,out,32)==32 && BN_bn2binpad(s,out+32U,32)==32 ? 0 : 1;
}

int main(void)
{
    EC_KEY *key=EC_KEY_new_by_curve_name(NID_secp256k1);
    BIGNUM *private_value=BN_new();
    const EC_GROUP *group;
    EC_POINT *public_point;
    uint8_t public_key[33], signature[64], digest[32], address[20], other[20];
    size_t public_length;
    ECDSA_SIG *signed_digest;
    static const uint8_t message[]={'d','i','d',':','l','x',0U,0U,0U,17U};
    uint8_t recovery;
    int found=0;
    if (!key||!private_value||BN_set_word(private_value,1U)!=1 || EC_KEY_set_private_key(key,private_value)!=1) return 1;
    group=EC_KEY_get0_group(key); public_point=EC_POINT_new(group);
    if (!public_point || EC_POINT_mul(group,public_point,private_value,NULL,NULL,NULL)!=1 ||
        EC_KEY_set_public_key(key,public_point)!=1) return 1;
    public_length=EC_POINT_point2oct(group,public_point,POINT_CONVERSION_COMPRESSED,public_key,sizeof(public_key),NULL);
    if (public_length!=33U || lxp_hash_domain(LXP_DOMAIN_CHECKPOINT_CERTIFICATE,message,sizeof(message),digest)!=LXP_OK) return 1;
    signed_digest=ECDSA_do_sign(digest,32,key); if (!signed_digest || compact_signature(signed_digest,signature)!=0) return 1;
    if (!lxp_secp256k1_sig_is_low_s(signature)) {
        const BIGNUM *r,*s; BIGNUM *low; BIGNUM *order=BN_new(); ECDSA_SIG_get0(signed_digest,&r,&s);
        if (!order || EC_GROUP_get_order(group, order, NULL) != 1) return 1;
        low = BN_new();
        if (!low || BN_sub(low, order, s) != 1) return 1;
        if (BN_bn2binpad(low, signature + 32U, 32) != 32) return 1;
        BN_free(low);
        BN_free(order);
    }
    if (lxp_secp256k1_verify(public_key,public_length,signature,LXP_DOMAIN_CHECKPOINT_CERTIFICATE,message,sizeof(message))!=LXP_OK ||
        lxp_secp256k1_verify(public_key,public_length,signature,LXP_DOMAIN_ACTIVITY_ID,message,sizeof(message))!=LXP_ERR_BAD_SIGNATURE) return 1;
    for(recovery=0U;recovery<4U;++recovery) if(lxp_secp256k1_recover_address(signature,recovery,digest,address)==LXP_OK){
        if(!found){(void)memcpy(other,address,20U);found=1;} else if(memcmp(other,address,20U)!=0){/* alternate x candidate */}
    }
    if(!found || lxp_secp256k1_recover_address(signature,4U,digest,address)!=LXP_ERR_BAD_SIGNATURE) return 1;
    {
        BIGNUM *order = BN_new();
        if (order == NULL || EC_GROUP_get_order(group, order, NULL) != 1 ||
            BN_bn2binpad(order, signature, 32) != 32) return 1;
        for (recovery = 0U; recovery < 4U; ++recovery)
            if (lxp_secp256k1_recover_address(signature, recovery, digest,
                                              address) != LXP_ERR_BAD_SIGNATURE)
                return 1;
        (void)memset(signature, 0, 32U);
        if (lxp_secp256k1_recover_address(signature, 0U, digest, address) !=
            LXP_ERR_BAD_SIGNATURE) return 1;
        if (BN_rshift1(order, order) != 1 ||
            BN_bn2binpad(order, signature + 32U, 32) != 32 ||
            !lxp_secp256k1_sig_is_low_s(signature) ||
            BN_add_word(order, 1U) != 1 ||
            BN_bn2binpad(order, signature + 32U, 32) != 32 ||
            lxp_secp256k1_sig_is_low_s(signature)) return 1;
        BN_free(order);
    }
    signature[32U]=0xffU;
    ECDSA_SIG_free(signed_digest);EC_POINT_free(public_point);BN_free(private_value);EC_KEY_free(key);
    return !lxp_secp256k1_sig_is_low_s(signature) ? 0 : 1;
}
