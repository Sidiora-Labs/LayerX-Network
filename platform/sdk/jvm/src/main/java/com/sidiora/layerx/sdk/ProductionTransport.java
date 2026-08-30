package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JavaType;
import com.fasterxml.jackson.databind.node.ObjectNode;
import java.util.Objects;
import java.util.concurrent.CompletionStage;

public interface ProductionTransport {
    record Call(SchemaTypes.Operation operation, ObjectNode request,
                SchemaTypes.PathParameters pathParameters, IdempotencyKey idempotencyKey) {
        public Call {
            Objects.requireNonNull(operation, "operation");
            request = request == null ? com.fasterxml.jackson.databind.node.JsonNodeFactory.instance.objectNode()
                : request.deepCopy();
            pathParameters = pathParameters == null ? SchemaTypes.PathParameters.none() : pathParameters;
        }
    }
    record ProgramsCall(String operation, ObjectNode request,
                SchemaTypes.PathParameters pathParameters, IdempotencyKey idempotencyKey) {
        public ProgramsCall {
            Objects.requireNonNull(operation, "operation");
            request = request == null ? com.fasterxml.jackson.databind.node.JsonNodeFactory.instance.objectNode()
                : request.deepCopy();
            pathParameters = pathParameters == null ? SchemaTypes.PathParameters.none() : pathParameters;
        }
    }
    <T> CompletionStage<T> call(Call call, JavaType responseType);

    default <T> CompletionStage<T> callPrograms(ProgramsCall call, JavaType responseType) {
        return java.util.concurrent.CompletableFuture.failedFuture(new PlatformSdkException(
            PlatformSdkException.Code.UNAVAILABLE_CAPABILITY, PlatformSdkException.Retry.NEVER,
            null, null, null));
    }
}
