use std::sync::Arc;

use axum::{
    Router,
    extract::{Query, State, WebSocketUpgrade, ws},
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::config::MiConfig;
use crate::events::Envelope;
use crate::llm::gemini::GeminiProvider;
use crate::llm::types::Message;
use crate::secrets;

use futures::StreamExt;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<MiConfig>>,
    pub token: String,
    pub session_id: String,
}

#[derive(Deserialize)]
pub struct WsQuery {
    token: Option<String>,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let provided = query.token.unwrap_or_default();
    if provided != state.token {
        return (axum::http::StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: ws::WebSocket, state: AppState) {
    while let Some(Ok(msg)) = socket.recv().await {
        let ws::Message::Text(text) = msg else {
            continue;
        };
        let Ok(envelope) = serde_json::from_str::<Envelope>(&text) else {
            let err = Envelope::new(
                "error",
                &state.session_id,
                serde_json::json!({"level": "error", "message": "invalid JSON"}),
            );
            let _ = socket
                .send(ws::Message::Text(serde_json::to_string(&err).unwrap().into()))
                .await;
            continue;
        };

        if envelope.event_type == "user.message" {
            let user_text = envelope.data["text"].as_str().unwrap_or("").to_string();
            let config = state.config.read().await;
            let model = config.active_model().to_string();
            drop(config);

            let api_key = secrets::get_gemini_key().ok().flatten().unwrap_or_default();
            if api_key.is_empty() {
                let notice = Envelope::new(
                    "error",
                    &state.session_id,
                    serde_json::json!({"level": "error", "message": "No Gemini API key. Run: mi auth set-gemini-key"}),
                );
                let _ = socket
                    .send(ws::Message::Text(serde_json::to_string(&notice).unwrap().into()))
                    .await;
                continue;
            }

            let provider = GeminiProvider::new(api_key);
            let messages = vec![Message::user(&user_text)];
            let turn_id = uuid::Uuid::new_v4().to_string();

            let mut stream = provider.stream(&messages, &model, None, None);
            let mut full_text = String::new();

            while let Some(result) = stream.next().await {
                match result {
                    Ok(chunk) => {
                        if chunk.done {
                            break;
                        }
                        if !chunk.text.is_empty() {
                            full_text.push_str(&chunk.text);
                            let delta = Envelope::new(
                                "assistant.delta",
                                &state.session_id,
                                serde_json::json!({"turn_id": turn_id, "text": chunk.text}),
                            );
                            let _ = socket
                                .send(ws::Message::Text(
                                    serde_json::to_string(&delta).unwrap().into(),
                                ))
                                .await;
                        }
                    }
                    Err(e) => {
                        let err = Envelope::new(
                            "error",
                            &state.session_id,
                            serde_json::json!({"level": "error", "message": e.to_string()}),
                        );
                        let _ = socket
                            .send(ws::Message::Text(serde_json::to_string(&err).unwrap().into()))
                            .await;
                        break;
                    }
                }
            }

            let done = Envelope::new(
                "assistant.done",
                &state.session_id,
                serde_json::json!({"turn_id": turn_id, "text": full_text}),
            );
            let _ = socket
                .send(ws::Message::Text(serde_json::to_string(&done).unwrap().into()))
                .await;
        }
    }
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state)
}

pub fn generate_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_token_is_uuid() {
        let token = generate_token();
        assert_eq!(token.len(), 36);
        assert!(uuid::Uuid::parse_str(&token).is_ok());
    }
}
