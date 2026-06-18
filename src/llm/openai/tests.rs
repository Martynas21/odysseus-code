use super::*;
use crate::llm::message::ChatMessage;
use crate::llm::{ChatRequest, Provider, StreamEvent};
use futures_util::StreamExt;

fn req() -> ChatRequest {
    ChatRequest {
        model: "m".into(),
        messages: vec![ChatMessage::user("hi")],
        tools: vec![],
        temperature: 0.0,
        max_tokens: 16,
        think: true,
    }
}

#[tokio::test]
async fn chat_stream_yields_text_then_done() {
    let mut server = mockito::Server::new_async().await;
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
                data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
                data: [DONE]\n\n";
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let provider = OpenAiProvider::new(server.url(), "");
    let mut stream = provider.chat_stream(req()).await.unwrap();
    let mut events = Vec::new();
    while let Some(ev) = stream.next().await {
        events.push(ev.unwrap());
    }
    assert_eq!(
        events.first(),
        Some(&StreamEvent::TextDelta("Hello".into()))
    );
    assert_eq!(events.last(), Some(&StreamEvent::Done));
}

#[tokio::test]
async fn chat_stream_maps_401() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(401)
        .with_body("nope")
        .create_async()
        .await;
    let provider = OpenAiProvider::new(server.url(), "");
    // Note: the Ok variant (BoxStream) is not Debug, so `unwrap_err()` won't
    // compile here; match on the result instead.
    let err = match provider.chat_stream(req()).await {
        Ok(_) => panic!("expected error"),
        Err(e) => e,
    };
    assert!(matches!(err, ProviderError::Unauthorized));
}
