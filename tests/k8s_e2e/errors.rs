use crate::fixtures::{REQUEST_TIMEOUT, chat_completion, ensure_gateway_ready, gateway_url, http_client};

#[tokio::test]
async fn invalid_api_key_rejected() {
    ensure_gateway_ready().await;
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .expect("failed to build HTTP client");
    let url = format!("{}/v1/chat/completions", gateway_url());

    let resp = client
        .post(&url)
        .header("Authorization", "Bearer wrong-key")
        .json(&serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .expect("request failed");

    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn malformed_json_rejected() {
    ensure_gateway_ready().await;
    let client = http_client();
    let url = format!("{}/v1/chat/completions", gateway_url());

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .body("this is not json")
        .send()
        .await
        .expect("request failed");

    assert!(
        resp.status().is_client_error(),
        "malformed JSON should return 4xx, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn empty_messages_handled() {
    ensure_gateway_ready().await;
    let resp = chat_completion("gpt-4", "").await;

    let status = resp.status();
    assert!(
        status == 200 || status.is_client_error(),
        "expected 200 or 4xx, got {status}"
    );
}
