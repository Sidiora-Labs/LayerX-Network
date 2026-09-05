package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import java.math.BigInteger;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.HexFormat;
import org.junit.jupiter.api.Test;
import static com.sidiora.layerx.sdk.NativeCapabilitySet.*;
import static org.junit.jupiter.api.Assertions.*;

public final class NativeCapabilitySetTest {
    private static byte[] field(JsonNode value, String name) { return HexFormat.of().parseHex(value.get(name).asText()); }
    private static List<Capability> logicalGrants(JsonNode values) {
        var grants = new ArrayList<Capability>();
        for (JsonNode value : values) {
            Capability grant = switch (value.get("kind").asText()) {
                case "StorageRead" -> new StorageRead();
                case "StorageWrite" -> new StorageWrite();
                case "EmitEvent" -> new EmitEvent();
                case "Call" -> new Call(field(value, "program"));
                case "Transfer402" -> new Transfer402(field(value, "asset"), field(value, "to"), new BigInteger(value.get("maximum_amount").asText()));
                case "ProgramSpend" -> new ProgramSpend(field(value, "owner_program"), field(value, "seed"), field(value, "source_account"), field(value, "asset"), field(value, "to"), new BigInteger(value.get("maximum_amount").asText()));
                case "ReceiptRead" -> new ReceiptRead(field(value, "receipt_digest"));
                case "BalanceView" -> new BalanceView(field(value, "account"), field(value, "asset"), field(value, "receipt_digest"));
                case "SharedStorageRead" -> new SharedStorageRead();
                case "SharedStorageWrite" -> new SharedStorageWrite();
                default -> throw new IllegalArgumentException("Unknown fixture capability");
            };
            assertEquals(value.get("tag").intValue(), Byte.toUnsignedInt(encode(List.of(grant))[2]));
            grants.add(grant);
        }
        return grants;
    }
    private static void assertEncoding(List<Capability> grants, byte[] expected) {
        assertArrayEquals(expected, encode(grants));
        assertArrayEquals(expected, encode(decode(expected)));
    }
    @Test
    void runtimeFixtureBindsLogicalGrantsAndEscalations() throws Exception {
        Path path = Path.of(System.getProperty("layerx.repo.root", "../../.."), "platform/sdk/conformance/fixtures/native-program-capabilities-v2.json");
        JsonNode fixture = new ObjectMapper().readTree(Files.readString(path));
        assertEquals(10, fixture.get("capabilities").size());
        int[] order = {1, 2, 3, 4, 5, 9, 6, 10, 7, 8};
        for (int index = 0; index < order.length; index++) assertEquals(order[index], fixture.get("capabilities").get(index).get("tag").intValue());
        List<Capability> parent = logicalGrants(fixture.get("capabilities"));
        assertEncoding(parent, field(fixture, "canonical_hex"));
        List<Capability> requested = logicalGrants(fixture.get("narrowed_capabilities"));
        assertEncoding(requested, field(fixture, "narrowed_hex"));
        List<Capability> narrowed = narrow(parent, requested);
        assertEncoding(narrowed, field(fixture, "narrowed_hex"));
        assertTrue(fixture.get("equal_narrowing_accepted").booleanValue());
        assertEncoding(narrow(parent, parent), field(fixture, "canonical_hex"));
        assertEquals(3, fixture.get("escalation_cases").size());
        for (JsonNode escalation : fixture.get("escalation_cases")) {
            assertEquals("narrowed", escalation.get("parent").asText()); assertFalse(escalation.get("accepted").booleanValue());
            List<Capability> grants = logicalGrants(escalation.get("capabilities"));
            assertEncoding(grants, field(escalation, "canonical_hex"));
            assertThrows(IllegalArgumentException.class, () -> narrow(narrowed, grants), escalation.get("name").asText());
        }
    }

    private static byte[] identifier(int value) {
        byte[] bytes = new byte[32]; Arrays.fill(bytes, (byte)value); return bytes;
    }
    @Test
    void boundsAndNarrowingMatchRuntime() {
        byte[] owner = identifier(1), asset = identifier(2), to = identifier(3), receipt = identifier(4);
        byte[] seed = {0, (byte)255}; byte[] source = deriveProgramAccount(owner, seed);
        BigInteger maximum = BigInteger.ONE.shiftLeft(64);
        var spend = new ProgramSpend(owner, seed, source, asset, to, maximum);
        var view = new BalanceView(source, asset, receipt);
        List<Capability> parent = List.of(new SharedStorageWrite(), new SharedStorageRead(), view,
            new ReceiptRead(receipt), spend, new Transfer402(asset, to, maximum), new Call(owner),
            new EmitEvent(), new StorageWrite(), new StorageRead());
        byte[] encoded = encode(parent);
        assertArrayEquals(new byte[]{0, 10, 1, 2, 3}, Arrays.copyOf(encoded, 5));
        assertEquals(2, narrow(parent, List.of(new ProgramSpend(owner, seed, source, asset, to, maximum.subtract(BigInteger.ONE)), view)).size());
        assertThrows(IllegalArgumentException.class, () -> narrow(parent, List.of(new ProgramSpend(owner, seed, source, asset, to, maximum.add(BigInteger.ONE)))));
        var changedView = new BalanceView(source, asset, identifier(5));
        assertThrows(IllegalArgumentException.class, () -> narrow(parent, List.of(changedView)));
        assertThrows(IllegalArgumentException.class, () -> encode(List.of(view, changedView)));
        assertThrows(IllegalArgumentException.class, () -> narrow(List.of(), List.of(new StorageRead())));
        assertThrows(IllegalArgumentException.class, () -> encode(List.of(new Transfer402(asset, to, BigInteger.ZERO))));
        assertThrows(IllegalArgumentException.class, () -> encode(List.of(new Transfer402(asset, to, BigInteger.ONE.shiftLeft(128)))));
        assertThrows(IllegalArgumentException.class, () -> encode(List.of(new Call(new byte[32]))));
        assertThrows(IllegalArgumentException.class, () -> encode(List.of(new ProgramSpend(owner, seed, identifier(9), asset, to, maximum))));
        assertThrows(IllegalArgumentException.class, () -> encode(List.of(new ProgramSpend(owner, new byte[129], source, asset, to, maximum))));
        for (int length = 0; length < encoded.length; length++) {
            byte[] prefix = Arrays.copyOf(encoded, length);
            assertThrows(IllegalArgumentException.class, () -> decode(prefix));
        }
        assertThrows(IllegalArgumentException.class, () -> decode(Arrays.copyOf(encoded, encoded.length + 1)));
        for (byte[] malformed : List.of(new byte[]{0, 2, 2, 1}, new byte[]{0, 2, 1, 1}, new byte[]{0, 1, 11}, new byte[]{0, (byte)239})) {
            assertThrows(IllegalArgumentException.class, () -> decode(malformed));
        }
        var views = new ArrayList<Capability>();
        for (int index = 1; index <= 33; index++) views.add(new BalanceView(identifier(index), asset, receipt));
        assertDoesNotThrow(() -> encode(views.subList(0, 32)));
        assertThrows(IllegalArgumentException.class, () -> encode(views));
        var full = new ArrayList<Capability>();
        for (int index = 0; index <= MAXIMUM_GRANTS; index++) {
            byte[] fullSeed = new byte[128]; fullSeed[0] = (byte)(index >>> 8); fullSeed[1] = (byte)index;
            full.add(new ProgramSpend(owner, fullSeed, deriveProgramAccount(owner, fullSeed), asset, to, maximum));
        }
        assertEquals(MAXIMUM_BYTES, encode(full.subList(0, MAXIMUM_GRANTS)).length);
        assertThrows(IllegalArgumentException.class, () -> encode(full));
    }
}
