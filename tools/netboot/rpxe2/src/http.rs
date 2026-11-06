use std::path::PathBuf;

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use tokio_util::io::ReaderStream;

const ARCHIVE_NAME: &str = "TrueOS.tar.lz4";
const ARCHIVE_PATH: &str = "/TrueOS.tar.lz4";

/// Serve only the live-update archive, resolving the publication on each request.
pub fn router(source: PathBuf) -> Router {
    // A fallback avoids Axum's automatic HEAD support for GET routes and lets us
    // compare the encoded path and query exactly as pxe3.js does.
    Router::new().fallback(archive).with_state(source)
}

async fn archive(State(source): State<PathBuf>, request: Request) -> Response {
    if request.method() != Method::GET
        || request.uri().path_and_query().map(|value| value.as_str()) != Some(ARCHIVE_PATH)
    {
        return StatusCode::NOT_FOUND.into_response();
    }

    let Ok(directory) = tokio::fs::canonicalize(&source).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let Ok(file) = tokio::fs::File::open(directory.join(ARCHIVE_NAME)).await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    // Inspect the opened file so an atomic publication switch cannot mix one
    // generation's Content-Length with another generation's streamed bytes.
    let Ok(metadata) = file.metadata().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if !metadata.is_file() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    let mut response = Response::new(Body::from_stream(ReaderStream::new(file)));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, metadata.len().into());
    response
}

#[cfg(test)]
mod tests {
    use std::fs;

    use axum::{body::to_bytes, http::Request};
    use tower::ServiceExt;

    use super::*;

    async fn get(app: Router) -> Response {
        app.oneshot(
            Request::builder()
                .uri(ARCHIVE_PATH)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn archive_stream_has_exact_bytes_and_headers() {
        let source = tempfile::tempdir().unwrap();
        let bytes = b"\0TRUEOS archive\xff\n";
        fs::write(source.path().join(ARCHIVE_NAME), bytes).unwrap();

        let response = get(router(source.path().to_owned())).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "application/octet-stream"
        );
        assert_eq!(
            response.headers()[header::CONTENT_LENGTH],
            bytes.len().to_string()
        );
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX).await.unwrap(),
            bytes.as_slice()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn each_request_follows_the_current_publication_and_keeps_its_open_file() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        fs::write(first.join(ARCHIVE_NAME), b"old").unwrap();
        fs::write(second.join(ARCHIVE_NAME), b"new archive").unwrap();
        let current = root.path().join("current");
        symlink(&first, &current).unwrap();
        let app = router(current.clone());
        let first_response = get(app.clone()).await;

        let next = root.path().join("next");
        symlink(&second, &next).unwrap();
        fs::rename(next, current).unwrap();
        let second_response = get(app).await;

        assert_eq!(first_response.headers()[header::CONTENT_LENGTH], "3");
        assert_eq!(
            to_bytes(first_response.into_body(), usize::MAX)
                .await
                .unwrap(),
            b"old".as_slice()
        );
        assert_eq!(second_response.headers()[header::CONTENT_LENGTH], "11");
        assert_eq!(
            to_bytes(second_response.into_body(), usize::MAX)
                .await
                .unwrap(),
            b"new archive".as_slice()
        );
    }

    #[tokio::test]
    async fn unsupported_methods_paths_and_queries_return_404() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join(ARCHIVE_NAME), b"archive").unwrap();
        fs::write(source.path().join("secret"), b"private").unwrap();
        let app = router(source.path().to_owned());

        let requests = [
            (Method::HEAD, ARCHIVE_PATH),
            (Method::POST, ARCHIVE_PATH),
            (Method::PUT, ARCHIVE_PATH),
            (Method::DELETE, ARCHIVE_PATH),
            (Method::OPTIONS, ARCHIVE_PATH),
            (Method::GET, "/"),
            (Method::GET, "/secret"),
            (Method::GET, "/TRUEOS.tar.lz4"),
            (Method::GET, "/TrueOS%2Etar.lz4"),
            (Method::GET, "/TrueOS.tar.lz4/"),
            (Method::GET, "/TrueOS.tar.lz4?"),
            (Method::GET, "/TrueOS.tar.lz4?cache=1"),
        ];
        for (method, uri) in requests {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method.clone())
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {uri}");
        }
    }

    #[tokio::test]
    async fn missing_source_or_archive_and_non_regular_archive_return_503() {
        let source = tempfile::tempdir().unwrap();
        let app = router(source.path().to_owned());
        assert_eq!(
            get(app.clone()).await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );

        fs::create_dir(source.path().join(ARCHIVE_NAME)).unwrap();
        assert_eq!(get(app).await.status(), StatusCode::SERVICE_UNAVAILABLE);

        assert_eq!(
            get(router(source.path().join("missing"))).await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
