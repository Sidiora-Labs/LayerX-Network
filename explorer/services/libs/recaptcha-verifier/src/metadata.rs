use crate::error::Error;
use tonic::metadata::MetadataMap;

pub const HEADER_RECAPTCHA_V2: &str = "recaptcha-v2-response";
pub const HEADER_RECAPTCHA_V3: &str = "recaptcha-v3-response";

pub fn extract_v2_token(metadata: &MetadataMap) -> Result<String, Error> {
    extract_token(metadata, HEADER_RECAPTCHA_V2)
}

pub fn extract_v3_token(metadata: &MetadataMap) -> Result<String, Error> {
    extract_token(metadata, HEADER_RECAPTCHA_V3)
}

pub fn extract_token(metadata: &MetadataMap, header_name: &str) -> Result<String, Error> {
    metadata
        .get(header_name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| Error::MissingToken {
            header: header_name.to_string(),
        })
}

pub fn extract_any_token(metadata: &MetadataMap) -> Result<(String, &'static str), Error> {
    let headers = [HEADER_RECAPTCHA_V3, HEADER_RECAPTCHA_V2];

    for header_name in headers {
        if let Ok(token) = extract_token(metadata, header_name) {
            return Ok((token, header_name));
        }
    }

    Err(Error::MissingToken {
        header: headers.join(" or "),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonic::metadata::AsciiMetadataValue;

    #[test]
    fn extract_v2_token_success() {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            HEADER_RECAPTCHA_V2,
            "test-v2-token".parse::<AsciiMetadataValue>().unwrap(),
        );

        let token = extract_v2_token(&metadata).unwrap();
        assert_eq!(token, "test-v2-token");
    }

    #[test]
    fn extract_v3_token_success() {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            HEADER_RECAPTCHA_V3,
            "test-v3-token".parse::<AsciiMetadataValue>().unwrap(),
        );

        let token = extract_v3_token(&metadata).unwrap();
        assert_eq!(token, "test-v3-token");
    }

    #[test]
    fn extract_token_missing() {
        let metadata = MetadataMap::new();
        let result = extract_v2_token(&metadata);
        assert!(result.is_err());

        match result.unwrap_err() {
            Error::MissingToken { header } => {
                assert_eq!(header, HEADER_RECAPTCHA_V2);
            }
            _ => panic!("Expected MissingToken error"),
        }
    }

    #[test]
    fn extract_custom_header() {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            "my-custom-recaptcha",
            "custom-token".parse::<AsciiMetadataValue>().unwrap(),
        );

        let token = extract_token(&metadata, "my-custom-recaptcha").unwrap();
        assert_eq!(token, "custom-token");
    }

    #[test]
    fn extract_any_token_prefers_v3() {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            HEADER_RECAPTCHA_V3,
            "v3-token".parse::<AsciiMetadataValue>().unwrap(),
        );
        metadata.insert(
            HEADER_RECAPTCHA_V2,
            "v2-token".parse::<AsciiMetadataValue>().unwrap(),
        );

        let (token, header) = extract_any_token(&metadata).unwrap();
        assert_eq!(token, "v3-token");
        assert_eq!(header, HEADER_RECAPTCHA_V3);
    }
}
