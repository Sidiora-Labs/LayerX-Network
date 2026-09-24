use std::{path::PathBuf, sync::Arc};

use actix_files::NamedFile;
use actix_web::{web::get, HttpRequest, Result};

async fn serve_swagger_from(path: Arc<PathBuf>, _req: HttpRequest) -> Result<NamedFile> {
    Ok(NamedFile::open(path.as_ref())?)
}

pub fn route_swagger(
    service_config: &mut actix_web::web::ServiceConfig,
    swagger_file_path: PathBuf,
    route: &str,
) {
    let path = Arc::new(swagger_file_path);
    let serve_swagger = move |req: HttpRequest| serve_swagger_from(path.clone(), req);
    service_config.configure(|config| {
        config.route(route, get().to(serve_swagger));
    });
}

pub fn default_swagger_path_from_service_name(service_name: &str) -> PathBuf {
    let options = [
        PathBuf::from(format!(
            "./{service_name}-proto/swagger/{service_name}.swagger.yaml",
        )),
        PathBuf::from(format!(
            "./{service_name}-proto/swagger/v1/{service_name}.swagger.yaml",
        )),
        PathBuf::from(format!(
            "../{service_name}-proto/swagger/{service_name}.swagger.yaml",
        )),
        PathBuf::from(format!(
            "../{service_name}-proto/swagger/v1/{service_name}.swagger.yaml",
        )),
    ];

    for option in options.iter() {
        if option.exists() {
            return option.clone();
        }
    }
    options[0].clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_swagger_path_from_service_name() {
        assert_eq!(
            default_swagger_path_from_service_name("stats"),
            PathBuf::from("./stats-proto/swagger/stats.swagger.yaml")
        );
    }
}
