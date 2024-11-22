use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
	str,
};
use tokio::sync::mpsc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

struct AppState {
    user_map: Mutex<HashMap<String, mpsc::Sender<String>>>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=trace", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let user_map = Mutex::new(HashMap::new());

    let app_state = Arc::new(AppState { user_map });

    let app = Router::new()
        .route("/", get(index))
        .route("/websocket", get(websocket_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

async fn websocket(stream: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = stream.split();

    let mut username = String::new();

    let (tx, mut rx) = mpsc::channel(100);

    while let Some(Ok(message)) = receiver.next().await {
        if let Message::Text(name) = message {
    
            check_username(&state, &mut username, &name, tx);

            if !username.is_empty() {
                break;
            } else {
    
                let _ = sender
                    .send(Message::Text(String::from("Username already taken.")))
                    .await;
                return;
            }
        }
    }

    let msg = format!("{username} joined.");
    tracing::debug!("{msg}");
    let _ = sender.send(Message::Text(msg)).await.is_err();

    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {

            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {

		    let (user, message) = split_msg(&text);
                    let clone_user_map = state.user_map.lock().unwrap().clone();


                    if let Some(tx) = clone_user_map.get(&user) {
                        let _ = tx.send(format!("{username}: {message}")).await;
                    }
        	}
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    };

}

fn check_username(state: &AppState, string: &mut String, name: &str, tx: mpsc::Sender<String>) {
    let mut user_map = state.user_map.lock().unwrap();

    if !user_map.contains_key(name) {
        user_map.insert(name.to_owned(), tx);
        string.push_str(name);
    }
}

fn split_msg(str: &str) -> (String, String) {
    let mut user = String::new();
    let mut msg = String::new();
    
    if str.chars().next() == Some('@') {
        let parts: Vec<&str> = str[1..].splitn(2, ' ').collect();
        if parts.len() > 0 {
            user = parts[0].to_string();
        }
        if parts.len() > 1 {
            msg = parts[1].to_string();
        }
    }
    (user, msg)
}

async fn index() -> Html<&'static str> {
    Html(std::include_str!("../chat.html"))
}

