use heddle::tools::types::ExecOptions;
use heddle::tools::web_fetch::create_web_fetch_tool;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn fetches_url_and_returns_text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/plain")
                .set_body_string("Hello world"),
        )
        .mount(&server)
        .await;

    let tool = create_web_fetch_tool();
    let result = tool
        .execute(json!({ "url": server.uri() }), ExecOptions::default())
        .await;
    assert_eq!(result, "Hello world");
}

#[tokio::test]
async fn renders_html_via_html2text() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(
                "<html><body><h1>Title</h1><p>Content</p></body></html>"
                    .as_bytes()
                    .to_vec(),
                "text/html",
            ),
        )
        .mount(&server)
        .await;

    let tool = create_web_fetch_tool();
    let result = tool
        .execute(json!({ "url": server.uri() }), ExecOptions::default())
        .await;
    // html2text preserves both pieces of visible text; exact formatting varies.
    assert!(result.contains("Title"), "got: {result}");
    assert!(result.contains("Content"), "got: {result}");
    // Tags should be gone.
    assert!(!result.contains("<h1>"), "got: {result}");
    assert!(!result.contains("<body>"), "got: {result}");
}

#[tokio::test]
async fn truncates_long_responses_at_50k() {
    let long = "x".repeat(60_000);
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/plain")
                .set_body_string(long),
        )
        .mount(&server)
        .await;

    let tool = create_web_fetch_tool();
    let result = tool
        .execute(json!({ "url": server.uri() }), ExecOptions::default())
        .await;
    assert_eq!(result.len(), 50_000);
}

#[tokio::test]
async fn returns_error_for_non_200() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not Found"))
        .mount(&server)
        .await;

    let tool = create_web_fetch_tool();
    let result = tool
        .execute(
            json!({ "url": format!("{}/missing", server.uri()) }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Error"), "got: {result}");
    assert!(result.contains("404"), "got: {result}");
}

#[tokio::test]
async fn returns_error_for_non_text_content_type() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .set_body_bytes(vec![0u8, 1, 2]),
        )
        .mount(&server)
        .await;

    let tool = create_web_fetch_tool();
    let result = tool
        .execute(json!({ "url": server.uri() }), ExecOptions::default())
        .await;
    assert!(result.contains("Error"), "got: {result}");
    assert!(result.contains("Non-text content type"), "got: {result}");
}

#[tokio::test]
async fn returns_error_on_network_failure() {
    let tool = create_web_fetch_tool();
    // Connection refused on closed local port.
    let result = tool
        .execute(
            json!({ "url": "http://127.0.0.1:1" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Error"), "got: {result}");
}

#[tokio::test]
async fn returns_error_for_invalid_url_scheme() {
    let tool = create_web_fetch_tool();
    let result = tool
        .execute(
            json!({ "url": "ftp://example.com/file" }),
            ExecOptions::default(),
        )
        .await;
    assert!(result.contains("Error"), "got: {result}");
    assert!(result.contains("http"), "got: {result}");
}
