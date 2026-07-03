use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use jsonwebtoken::{DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::SqlitePool;
use time::{Duration, format_description::StaticFormatDescription, macros::format_description};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: i64,
    pub iat: i64,
}

pub fn jwt_secret() -> String {
    std::env::var("JWT_SECRET").expect("JWT_SECRET must be set")
}

const FORMAT: StaticFormatDescription =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

pub fn sqlite_datetime_string_to_unix_timestamp(
    datetime_str: &str,
) -> Result<i64, time::error::Parse> {
    time::UtcDateTime::parse(datetime_str, &FORMAT).map(|dt| dt.unix_timestamp())
}

pub async fn logout(jar: CookieJar) -> (CookieJar, Json<serde_json::Value>) {
    let cookie = Cookie::build(("token", ""))
        .path("/")
        .http_only(true)
        .max_age(Duration::seconds(0))
        .build();

    (jar.add(cookie), Json(json!({ "success": true })))
}

pub async fn auth_middleware(
    State(db): State<SqlitePool>,
    cookie_jar: CookieJar,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let token = cookie_jar
        .get("token")
        .ok_or(StatusCode::UNAUTHORIZED)?
        .value();

    let secret = jwt_secret();

    let token_data = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let user_updated_at: String =
        sqlx::query_scalar("SELECT u.updated_at FROM users u WHERE u.username = ?")
            .bind(token_data.claims.sub)
            .fetch_one(&db)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let user_updated_at_parsed = sqlite_datetime_string_to_unix_timestamp(&user_updated_at)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    if user_updated_at_parsed > token_data.claims.iat {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}

pub async fn me() -> Json<serde_json::Value> {
    Json(json!({ "authenticated": true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::setup_db;
    use axum::body::Body;
    use axum::{Router, middleware, routing::get};
    use jsonwebtoken::{EncodingKey, Header, encode};
    use serial_test::serial;
    use time::{Duration, OffsetDateTime};
    use tower::ServiceExt;

    pub fn unix_timestamp_to_sqlite_datetime_string(
        timestamp: i64,
    ) -> Result<String, time::error::Error> {
        let datetime = time::UtcDateTime::from_unix_timestamp(timestamp)?;
        Ok(datetime.format(&FORMAT)?)
    }

    fn create_token_admin_issued_now_valid_one_day() -> String {
        let now = OffsetDateTime::now_utc();
        let claims = Claims {
            sub: "admin".into(),
            exp: (now + Duration::days(1)).unix_timestamp(),
            iat: now.unix_timestamp(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(jwt_secret().as_bytes()),
        )
        .unwrap()
    }

    pub async fn change_user_updated_at(db: SqlitePool, new_updated_at: i64) {
        sqlx::query("UPDATE users SET updated_at = ?")
            .bind(unix_timestamp_to_sqlite_datetime_string(new_updated_at).unwrap())
            .execute(&db)
            .await
            .unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn test_me_returns_authenticated() {
        let response = me().await;
        assert_eq!(response.0.get("authenticated"), Some(&json!(true)));
    }

    #[tokio::test]
    #[serial]
    async fn test_auth_middleware_no_cookie() {
        let db = setup_db().await;
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(db, auth_middleware));

        let res = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn test_auth_middleware_invalid_token() {
        let db = setup_db().await;
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(db, auth_middleware));

        let res = temp_env::async_with_vars([("JWT_SECRET", Some("test_secret"))], async {
            app.oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header("Cookie", "token=invalid.jwt.here")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        })
        .await;

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    #[serial]
    async fn test_auth_middleware_valid_token() {
        let db = setup_db().await;
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(db.clone(), auth_middleware));

        let res = temp_env::async_with_vars([("JWT_SECRET", Some("test_secret"))], async {
            let token = create_token_admin_issued_now_valid_one_day();
            app.oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header("Cookie", format!("token={}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
        })
        .await;

        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    #[serial]
    async fn test_auth_middleware_should_unauthorize_tokens_created_before_user_data_update() {
        let db = setup_db().await;
        let app = Router::new()
            .route("/", get(|| async { "ok" }))
            .layer(middleware::from_fn_with_state(db.clone(), auth_middleware));

        let res = temp_env::async_with_vars([("JWT_SECRET", Some("test_secret"))], async {
            let token = create_token_admin_issued_now_valid_one_day();
            change_user_updated_at(
                db.clone(),
                (OffsetDateTime::now_utc() + Duration::seconds(1)).unix_timestamp(),
            )
            .await;
            app.oneshot(
                axum::http::Request::builder()
                    .uri("/")
                    .header("Cookie", format!("token={}", token))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
        })
        .await
        .unwrap();

        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }
}
