//! Cross-phase contracts exercised through the real per-message handlers.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    clippy::missing_assert_message,
    clippy::missing_docs_in_private_items,
    clippy::large_futures,
    clippy::future_not_send,
    reason = "test fixtures and assertions"
)]

use praxis_proto::envoy::service::{
    common::v3::HeaderValue,
    ext_proc::v3::{
        HeaderMap as WireHeaders, HttpBody, HttpHeaders, body_mutation, processing_response::Response as Reply,
    },
};

use super::*;
use crate::handlers::{handle_request_body, handle_request_headers, handle_response_headers, handle_trailers};

fn headers(eos: bool) -> HttpHeaders {
    HttpHeaders {
        headers: Some(WireHeaders {
            headers: vec![
                HeaderValue {
                    key: ":method".into(),
                    value: "POST".into(),
                    ..Default::default()
                },
                HeaderValue {
                    key: ":path".into(),
                    value: "/v1/chat/completions".into(),
                    ..Default::default()
                },
                HeaderValue {
                    key: ":status".into(),
                    value: "200".into(),
                    ..Default::default()
                },
            ],
        }),
        end_of_stream: eos,
    }
}

#[tokio::test]
async fn fds_body_resolver_observes_early_pending_headers_once_after_interleaving() {
    let pipeline = pending_headers_pipeline("once");
    for trailers in [false, true] {
        for interleave in [false, true] {
            let mut state = StreamState::new();
            state.protocol_config.request_body_mode = BodyMode::FullDuplexStreamed;
            state.protocol_config.response_body_mode = BodyMode::Streamed;
            assert!(
                handle_request_headers(&pipeline, headers(false), &mut state)
                    .await
                    .unwrap()
                    .is_empty()
            );
            let partial = HttpBody {
                body: br#"{"model":"publishers/"#.to_vec(),
                end_of_stream: false,
            };
            assert!(
                handle_request_body(&pipeline, partial, &mut state)
                    .await
                    .unwrap()
                    .is_empty()
            );
            if interleave {
                let replies = handle_response_headers(&pipeline, headers(false), &mut state)
                    .await
                    .unwrap();
                assert!(matches!(replies[0].response, Some(Reply::ResponseHeaders(_))));
            }
            let last = HttpBody {
                body: br#"acme/models/llama-3"}"#.to_vec(),
                end_of_stream: !trailers,
            };
            let mut replies = handle_request_body(&pipeline, last, &mut state).await.unwrap();
            if trailers {
                assert!(replies.is_empty());
                replies = handle_trailers(&pipeline, &mut state, true).await.unwrap();
            }
            let Some(Reply::RequestHeaders(header)) = &replies[0].response else {
                panic!("{replies:?}")
            };
            let mutation = header.response.as_ref().unwrap().header_mutation.as_ref().unwrap();
            for (name, value) in [("x-model", "publishers/acme/models/llama-3"), ("x-append", "once")] {
                let values: Vec<_> = mutation
                    .set_headers
                    .iter()
                    .filter_map(|entry| entry.header.as_ref())
                    .filter(|entry| entry.key.eq_ignore_ascii_case(name))
                    .map(|entry| entry.value.as_str())
                    .collect();
                assert_eq!(values, [value], "deferred headers must be emitted once");
            }
            let Some(Reply::RequestBody(body)) = &replies[1].response else {
                panic!("{replies:?}")
            };
            let Some(body_mutation::Mutation::StreamedResponse(body)) =
                &body.response.as_ref().unwrap().body_mutation.as_ref().unwrap().mutation
            else {
                panic!("FDS body required")
            };
            let document: serde_json::Value = serde_json::from_slice(&body.body).unwrap();
            assert_eq!(document["model"], "llama-3", "body hook must see early pending X-Model");
            assert_eq!(body.end_of_stream, !trailers);
            assert!(state.deferred_request_header_mutation.is_none());
            assert!(
                state.pending_request_mutations.take().is_none(),
                "request mutations must be consumed once"
            );
            assert_eq!(replies.len(), if trailers { 3 } else { 2 });
        }
    }
}

#[tokio::test]
async fn fds_pending_request_mutations_reject_capture_before_consumption() {
    let pipeline = pending_headers_pipeline("once");
    let overwrite = pending_headers_pipeline("twice");
    let mut state = StreamState::new();
    state.protocol_config.request_body_mode = BodyMode::FullDuplexStreamed;
    handle_request_headers(&pipeline, headers(false), &mut state)
        .await
        .unwrap();

    let error = run_request_header_filters_early(&overwrite, &mut state, MutationDelivery::DeferSilent)
        .await
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::Internal);
    assert_eq!(
        error.message(),
        "pending request mutations already present before capture"
    );

    let request = adapter::envoy_headers_to_request(&[]);
    let mut ctx = adapter::build_filter_context(&pipeline, &request, false);
    state.pending_request_mutations.take().unwrap().restore(&mut ctx);
    let values: Vec<_> = ctx
        .extra_request_headers
        .iter()
        .filter(|(name, _)| name == "x-append")
        .map(|(_, value)| value.as_str())
        .collect();
    assert_eq!(values, ["once"], "failed capture must preserve the original operations");
    assert!(
        state.pending_request_mutations.take().is_none(),
        "request mutations cannot be restored twice"
    );
}

// -----------------------------------------------------------------------------
// Test Utilities
// -----------------------------------------------------------------------------

fn pending_headers_pipeline(append_value: &str) -> std::sync::Arc<FilterPipeline> {
    let cfg: crate::config::ExtProcConfig = serde_yaml::from_str(&format!(
        "
filter_chains:
  - name: main
    filters:
      - filter: headers
        request_set:
          - name: X-Model
            value: publishers/acme/models/llama-3
        request_add:
          - name: x-append
            value: {append_value}
      - filter: llmisvc_model_provider_resolver
"
    ))
    .unwrap();
    crate::config::build_pipeline(&cfg, &praxis_ai_filters::build_ai_registry()).unwrap()
}
