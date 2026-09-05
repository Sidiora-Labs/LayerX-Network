package com.sidiora.layerx.sdk;

import java.util.Map;

final class ProgramLifecycleRoutes {
    private ProgramLifecycleRoutes() {}
    static final Map<String, Integer> ORDINALS = Map.of(
        "program.deploy", 1,
        "program.upgrade", 2,
        "program.wind-down", 7);
    static final Map<String, String> PATHS = Map.of(
        "program.deploy", "/v1/programs/deploy",
        "program.upgrade", "/v1/programs/upgrade",
        "program.wind-down", "/v1/programs/wind-down");
}
