package com.sidiora.layerx.sdk.examples;

import com.sidiora.layerx.sdk.*;
import com.sidiora.layerx.sdk.verify.LocalVerifier;
import java.math.BigInteger;
import java.util.HexFormat;

/**
 * Example: Offline receipt verification.
 * 
 * <p>Demonstrates trustless verification of a LayerX receipt without
 * contacting the service.
 */
public final class OfflineVerificationExample {
    public static void main(String[] args) {
        byte[] canonicalReceipt = HexFormat.of().parseHex(
            args.length > 0 ? args[0] : sampleReceiptHex());
        
        byte[] batchId = new byte[32];
        byte[] asset = new byte[32];
        byte[] previousStateRoot = new byte[32];
        byte[] resultingStateRoot = new byte[32];
        byte[] sequencerPublicKey = new byte[32];

        var authorized = new LocalVerifier.AuthorizedReceiptBatch(
            batchId, asset, previousStateRoot, resultingStateRoot, sequencerPublicKey);

        try {
            var verified = LocalVerifier.verifyReceiptOutcome(canonicalReceipt, authorized);
            System.out.println("Verification level: " + verified.level().wire());
            System.out.println("Activity ID: " + HexFormat.of().formatHex(
                verified.receipt().activityId()));
            System.out.println("Result code: " + verified.receipt().resultCode());
            System.out.println("Amount: " + verified.receipt().amount());
            System.out.println("Fee charged: " + verified.receipt().feeCharged());
            
            if (verified.receipt().resultCode() == 0) {
                System.out.println("Receipt verification successful");
            } else {
                System.out.println("Activity failed with code: " + 
                    verified.receipt().resultCode());
            }
        } catch (PlatformSdkException e) {
            System.err.println("Verification failed: " + e.getMessage());
            System.exit(1);
        }
    }

    private static String sampleReceiptHex() {
        return "0".repeat(200);
    }
}
