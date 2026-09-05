package layerx

var programLifecycleOrdinals = map[string]uint16{
	"program.deploy":    1,
	"program.upgrade":   2,
	"program.wind-down": 7,
}

var programLifecyclePaths = map[string]string{
	"program.deploy":    "/v1/programs/deploy",
	"program.upgrade":   "/v1/programs/upgrade",
	"program.wind-down": "/v1/programs/wind-down",
}
