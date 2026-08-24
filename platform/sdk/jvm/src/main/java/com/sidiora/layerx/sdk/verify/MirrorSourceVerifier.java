package com.sidiora.layerx.sdk.verify;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.sidiora.layerx.sdk.GeneratedMirror;
import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.math.BigInteger;
import java.nio.file.Path;
import java.time.Duration;
import java.util.HashSet;
import java.util.HexFormat;
import java.util.List;
import java.util.concurrent.TimeUnit;

public final class MirrorSourceVerifier {
    public record Candidate(int source, byte[] commitment) {}
    public record Policy(GeneratedMirror.Policy kind, List<Candidate> candidates, int minimum) {}
    public record Verification(String level, BigInteger batchNumber, byte[] headerDigest,
        byte[] evidenceDigest, String sourceId, String target, String canonicalPosition,
        String provenance, BigInteger latestBatch, String batchLag, int failoverCount,
        int agreeingSources, String checkpointLevel) {}
    public static final class VerificationException extends Exception {
        private final String code;
        public VerificationException(String code) { super("mirror verification refused: " + code); this.code = code; }
        public String code() { return code; }
    }

    private static final int MAX_REQUEST = 40 * 1024 * 1024;
    private static final int MAX_RESPONSE = 1024 * 1024;
    private final Path executable;
    private final Path configuration;
    private final Duration timeout;
    private final ObjectMapper json = new ObjectMapper();

    public MirrorSourceVerifier(Path executable, Path configuration, Duration timeout) throws VerificationException {
        if (!executable.isAbsolute() || !configuration.isAbsolute() || timeout.compareTo(Duration.ofMillis(100)) < 0 || timeout.compareTo(Duration.ofSeconds(120)) > 0) throw new VerificationException("configuration");
        this.executable = executable; this.configuration = configuration; this.timeout = timeout;
    }
    public Verification receipt(BigInteger batch, Policy policy, byte[] receipt) throws VerificationException {
        ObjectNode evidence=json.createObjectNode().put("kind","receipt").put("canonical_hex",HexFormat.of().formatHex(receipt));return verify(batch,policy,evidence);
    }
    public Verification state(BigInteger batch, Policy policy, byte[] state, byte[] proof) throws VerificationException {
        ObjectNode evidence=json.createObjectNode().put("kind","state").put("canonical_hex",HexFormat.of().formatHex(state)).put("proof_hex",HexFormat.of().formatHex(proof));return verify(batch,policy,evidence);
    }
    private Verification verify(BigInteger batch, Policy policy, ObjectNode evidence) throws VerificationException {
        if(batch.signum()<=0||batch.bitLength()>64||policy.candidates().isEmpty()||policy.candidates().size()>GeneratedMirror.MAX_SOURCES)throw new VerificationException("configuration");
        ArrayNode candidates=json.createArrayNode();HashSet<Integer> seen=new HashSet<>();
        for(Candidate value:policy.candidates()){if(value.source()<0||!seen.add(value.source())||value.commitment().length!=32)throw new VerificationException("configuration");candidates.addObject().put("source",value.source()).put("commitment_hex",HexFormat.of().formatHex(value.commitment()));}
        ObjectNode wirePolicy=json.createObjectNode();
        switch(policy.kind()) {case EXACT->{if(candidates.size()!=1)throw new VerificationException("configuration");wirePolicy.put("kind","exact").set("candidate",candidates.get(0));}case ORDERED_PREFERENCE->wirePolicy.put("kind","ordered-preference").set("candidates",candidates);case AGREEMENT->{if(policy.minimum()<1||policy.minimum()>candidates.size())throw new VerificationException("configuration");wirePolicy.put("kind","agreement").put("minimum",policy.minimum()).set("candidates",candidates);}}
        ObjectNode request=json.createObjectNode().put("batch_number",batch.toString()).set("evidence",evidence);request.set("policy",wirePolicy);
        try { byte[] bytes=json.writeValueAsBytes(request);if(bytes.length>MAX_REQUEST)throw new VerificationException("bounds");Process process=new ProcessBuilder(executable.toString(),configuration.toString()).redirectError(ProcessBuilder.Redirect.DISCARD).start();process.getOutputStream().write(bytes);process.getOutputStream().close();if(!process.waitFor(timeout.toMillis(),TimeUnit.MILLISECONDS)){process.destroyForcibly();throw new VerificationException("unavailable");}ByteArrayOutputStream output=new ByteArrayOutputStream();process.getInputStream().transferTo(output);if(output.size()>MAX_RESPONSE)throw new VerificationException("bounds");JsonNode response=json.readTree(output.toByteArray());if(!response.path("ok").asBoolean(false))throw new VerificationException(response.path("error").asText("unavailable"));JsonNode value=response.path("verification");return new Verification(required(value,"level").asText(),unsignedText(required(value,"batchNumber")),digest(required(value,"headerDigest").asText()),digest(required(value,"evidenceDigest").asText()),required(value,"sourceId").asText(),required(value,"target").asText(),required(value,"canonicalPosition").asText(),required(value,"provenance").asText(),value.path("latestBatch").isTextual()?unsignedText(value.path("latestBatch")):null,required(value,"batchLag").asText(),required(value,"failoverCount").asInt(),required(value,"agreeingSources").asInt(),required(value,"checkpointLevel").asText());
        } catch(InterruptedException error){Thread.currentThread().interrupt();throw new VerificationException("unavailable");}
        catch(IOException error){throw new VerificationException("unavailable");}
    }
    private static JsonNode required(JsonNode value,String field)throws VerificationException{JsonNode result=value.get(field);if(result==null||result.isNull())throw new VerificationException("malformed");return result;}
    private static byte[] digest(String value)throws VerificationException{try{byte[] result=HexFormat.of().parseHex(value);if(result.length!=32)throw new VerificationException("malformed");return result;}catch(IllegalArgumentException error){throw new VerificationException("malformed");}}
    private static BigInteger unsignedText(JsonNode value)throws VerificationException{if(!value.isTextual()||!value.textValue().matches("[1-9][0-9]*"))throw new VerificationException("malformed");BigInteger result=new BigInteger(value.textValue());if(result.bitLength()>64)throw new VerificationException("malformed");return result;}
}
