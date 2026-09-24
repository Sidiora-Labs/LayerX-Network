use serde::{Deserialize, Serialize};
use std::{borrow::Cow, fmt::Debug};
use url::Url;

/// Represents a specification for an API call that can be built into an HTTP request and sent.
/// New endpoints should implement this trait.
///
/// If the request succeeds, the call will resolve to a `Response`.
pub trait Endpoint {
    type Response: for<'a> Deserialize<'a> + Debug;

    /// The HTTP Method used for this endpoint (e.g. GET, PATCH, DELETE)
    fn method(&self) -> reqwest::Method;

    /// The relative URL path for this endpoint
    fn path(&self) -> String;

    /// The url-encoded query string associated with this endpoint. Defaults to `None`.
    ///
    /// Implementors should inline this.
    #[inline]
    fn query(&self) -> Option<String> {
        None
    }

    /// The set of headers to be sent with request. Defaults to `None`.
    ///
    /// Implementors should inline this.
    #[inline]
    fn headers(&self) -> Option<reqwest::header::HeaderMap> {
        None
    }

    /// The HTTP body associated with this endpoint. If not implemented, defaults to `None`.
    ///
    /// Implementors should inline this.
    #[inline]
    fn body(&self) -> Option<String> {
        None
    }

    /// Builds and returns a formatted full URL, including query, for the endpoint.
    ///
    /// Implementors should generally not override this.
    fn url(&self, base_url: &Url) -> Url {
        let mut url = base_url.join(&self.path()).unwrap();
        url.set_query(self.query().as_deref());
        url
    }

    /// If `body` is populated, indicates the body MIME type (defaults to JSON).
    ///
    /// Implementors generally do not need to override this.
    fn content_type(&self) -> Cow<'static, str> {
        Cow::Borrowed("application/json")
    }
}

/// A utility function for serializing parameters into a URL query string.
#[inline]
pub fn serialize_query<Q: Serialize>(q: &Q) -> Option<String> {
    serde_urlencoded::to_string(q).ok()
}
