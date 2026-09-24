mustache_to_handlebars \
    ~/poa/blockscout-rs/libs/blockscout-client/rust-templates \
    -out_dir ~/poa/blockscout-rs/libs/blockscout-client/rust-templates-handlebars \
    -handlebars_if_tags="
        # Type checks
        isEnum isInteger isLong isString isNumber isBoolean isArray isMap isModel \
        isByteArray isDate isDateTime isBinary isFile isUuid isPrimitiveType \
        isAnyType isDeepObject isUnboundedInteger isShort isDouble isFloat \
        isReadOnly isNullable isRequired isKeyInHeader isKeyInQuery isKeyInCookie \
        isPathParam isQueryParam isHeaderParam isCookieParam isFormParam isBodyParam \
        isResponseFile is2xx is3xx is4xx is5xx isDefault isBasic isBasicBearer \
        isBasicBasic isAlias isOAuth isApiKey isHttpSignature isMultipart \
        isHttpBasicMethods isBearerMethods isApiKeyMethods isOAuthMethods \
        isHttpSignatureMethods \
        # Feature flags
        supportMiddleware supportAsync supportMultipleResponses supportTokenSource \
        reqwestTrait reqwest hyper hyper0x mockall topLevelApiClient useBonBuilder \
        useNose serdeWith withAWSV4Signature \
        # Configuration
        oneOf.isEmpty avoidBoxedModels vendorExtensions.x-rust-has-byte-array \
        vendorExtensions.x-group-parameters \
        # State checks
        hasAuthMethods hasRequiredVars hasVars hasMore hasItems hasProduces \
        hasConsumes hasFormParams hasHeaderParams hasBodyParam hasUUIDs \
        hasRequired hasValidation hasDiscriminatorWithNonEmptyMapping \
        # Properties
        complexType produces consumes minItems maxItems notes getUniqueItems \
        minProperties pattern maxLength exclusiveMinimum maximum multipleOf \
        returnTypeIsPrimitive minLength required additionalPropertiesIsAnyType \
        arrayModelType recursionLimit \
        infoName returnType defaultValue asyncio tornado additionalPropertiesType \
        infoUrl exclusiveMaximum minimum nameInSnakeCase \
        collectionFormat bearerFormat summary vendorExtensions.x-regex maxProperties" \
    -handlebars_each_tags="
        # Collections
        models apis vars requiredVars optionalVars allParams requiredParams \
        optionalParams headerParams queryParams pathParams bodyParams formParams \
        responses servers scopes headers imports mappedModels enumVars enumValues \
        uniqueItems \
        # Schema components
        allOf oneOf anyOf composedSchemas.oneOf \
        # Extensions
        vendorExtensions vendorExtensions.x-auth-id-alias vendorExtensions.x-modifiers \
        x-mapped-models \
        # Other
        getComposedSchemas authMethods variables operation" \
    -handlebars_with_tags="
        # URLs and paths
        homePageUrl documentationUrl repositoryUrl publishRustRegistry \
        # Lambda specific
        lambdaVersion lambda.lifetimeName \
        # Schema properties
        items additionalProperties model apiInfo operations allowableValues \
        discriminator \
        # Parameters
        allParams.[0] baseName description \
        # Variables
        appName appDescription appDescriptionWithNewLines version infoEmail \
        httpUserAgent licenseInfo baseType \
        "