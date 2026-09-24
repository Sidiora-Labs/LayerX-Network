# Build-time environment variables

These variables are passed to the app during the image build process. They cannot be re-defined during the run-time.

| Variable | Type | Description | Optional | Example value |
| --- | --- | --- | --- | --- |
| NEXT_PUBLIC_GIT_COMMIT_SHA | `string` | SHA of the latest commit in the branch from which image is built | false | `29d0613e` |
| NEXT_PUBLIC_GIT_TAG | `string` | Git tag of the latest commit in the branch from which image is built | true | `v1.0.0` |
| NEXT_OPEN_TELEMETRY_ENABLED | `boolean` | Enables OpenTelemetry SDK | true | `true` |
| NEXT_PUBLIC_ICON_SPRITE_HASH | `string` | Hash post-fix of the SVG sprite file (generated automatically during the sprite build) | `08be4b10` | `true` |
