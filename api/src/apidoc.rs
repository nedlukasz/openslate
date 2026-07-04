use utoipa::OpenApi;
use crate::users;

#[derive(OpenApi)]
#[openapi(paths(users::change_password))]
pub struct ApiDoc;
