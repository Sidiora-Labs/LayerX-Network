use crate::settings::{FetcherSettings, S3FetcherSettings};
use cron::Schedule;
use s3::{creds::Credentials, Bucket, Region};
use smart_contract_verifier::{Fetcher, FileValidator, ListFetcher, S3Fetcher, Version};
use std::{path::PathBuf, str::FromStr, sync::Arc};

pub fn versions_to_sorted_string<Version: Ord + ToString>(
    mut versions: Vec<Version>,
) -> Vec<String> {
    // sort in descending order
    versions.sort_by(|x, y| x.cmp(y).reverse());
    versions.into_iter().map(|v| v.to_string()).collect()
}

pub async fn initialize_fetcher<Ver: Version>(
    fetcher_settings: FetcherSettings,
    compilers_dir: PathBuf,
    schedule: Schedule,
    validator: Option<Arc<dyn FileValidator<Ver>>>,
) -> anyhow::Result<Arc<dyn Fetcher<Version = Ver>>>
where
    <Ver as FromStr>::Err: std::fmt::Display,
{
    let fetcher: Arc<dyn Fetcher<Version = Ver>> = match fetcher_settings {
        FetcherSettings::List(list_settings) => Arc::new(
            ListFetcher::new(
                list_settings.list_url,
                compilers_dir,
                Some(schedule),
                validator,
            )
            .await?,
        ),
        FetcherSettings::S3(s3_settings) => Arc::new(
            S3Fetcher::new(
                new_bucket(&s3_settings)?,
                compilers_dir,
                Some(schedule),
                validator,
            )
            .await?,
        ),
    };

    Ok(fetcher)
}

fn new_region(region: Option<String>, endpoint: Option<String>) -> Option<Region> {
    let region = region.unwrap_or_default();
    if let Some(endpoint) = endpoint {
        return Some(Region::Custom { region, endpoint });
    }

    // try to match with AWS regions, fail otherwise
    let region = Region::from_str(&region).ok()?;
    match region {
        Region::Custom {
            region: _,
            endpoint: _,
        } => None,
        region => Some(region),
    }
}

fn new_bucket(settings: &S3FetcherSettings) -> anyhow::Result<Arc<Bucket>> {
    let region = new_region(settings.region.clone(), settings.endpoint.clone())
        .ok_or_else(|| anyhow::anyhow!("got invalid region/endpoint settings"))?;
    let bucket = Arc::new(Bucket::new(
        &settings.bucket,
        region,
        Credentials::new(
            settings.access_key.as_deref(),
            settings.secret_key.as_deref(),
            None,
            None,
            None,
        )?,
    )?);
    Ok(bucket)
}

macro_rules! process_solo_verification_request_conversion {
    ($maybe_verification_request:expr) => {
        match $maybe_verification_request {
            Ok(request) => request,
            Err(err @ smart_contract_verifier::RequestParseError::InvalidContent(_)) => {
                let response = $crate::types::VerifyResponseWrapper::err(err).into_inner();
                tracing::info!(response=?response, "request processed");
                return Ok(Response::new(response));
            }
            Err(err @ smart_contract_verifier::RequestParseError::BadRequest(_)) => {
                tracing::info!(err=%err, "bad request");
                return Err(tonic::Status::invalid_argument(err.to_string()));
            }
        }
    };
}
pub(crate) use process_solo_verification_request_conversion;

macro_rules! process_batch_verification_request_conversion {
    ($maybe_verification_request:expr) => {
        match $maybe_verification_request {
            Ok(request) => request,
            Err(err @ smart_contract_verifier::RequestParseError::InvalidContent(_)) => {
                let response = $crate::types::batch_verification::compilation_error(err.to_string());
                tracing::info!(response=?response, "request processed");
                return Ok(Response::new(response));
            }
            Err(err @ smart_contract_verifier::RequestParseError::BadRequest(_)) => {
                tracing::info!(err=%err, "bad request");
                return Err(tonic::Status::invalid_argument(err.to_string()));
            }
        }
    };
}
pub(crate) use process_batch_verification_request_conversion;
