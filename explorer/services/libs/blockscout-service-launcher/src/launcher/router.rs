pub trait HttpRouter {
    fn register_routes(&self, service_config: &mut actix_web::web::ServiceConfig);
}

impl<T: HttpRouter> HttpRouter for Option<T> {
    fn register_routes(&self, service_config: &mut actix_web::web::ServiceConfig) {
        if let Some(router) = self {
            router.register_routes(service_config)
        }
    }
}

pub fn configure_router(
    router: &impl HttpRouter,
    base_path: Option<String>,
) -> impl FnOnce(&mut actix_web::web::ServiceConfig) + '_ {
    |service_config| {
        if let Some(base_path) = base_path {
            service_config.service(
                actix_web::web::scope(&base_path)
                    .configure(|inner_config| router.register_routes(inner_config)),
            );
        } else {
            router.register_routes(service_config);
        }
    }
}
