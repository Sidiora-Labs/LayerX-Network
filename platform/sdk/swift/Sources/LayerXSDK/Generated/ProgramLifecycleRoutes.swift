enum ProgramLifecycleRoutes {
    static let ordinals: [String: UInt16] = [
        "program.deploy": 1,
        "program.upgrade": 2,
        "program.wind-down": 7,
    ]
    static let paths: [String: String] = [
        "program.deploy": "/v1/programs/deploy",
        "program.upgrade": "/v1/programs/upgrade",
        "program.wind-down": "/v1/programs/wind-down",
    ]
}
