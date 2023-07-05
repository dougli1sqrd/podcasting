use std::{collections::HashMap, sync::{Arc}, str::FromStr};

use axum::{
    extract::{State, Path},
    http::{StatusCode, Uri},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
struct AppState<D: DB> {
    current_user: Option<User>,
    db: D,
}

fn routes() -> Router<Arc<Mutex<AppState<InMemoryStore>>>> {
    Router::new().route("/", get(handler))
        .route("/users", post(add_user))
        .route("/users/:id", get(get_user))
        .route("/login", get(user_status))
        .route("/login/:id", post(login))
        .route("/subscribe", post(subscribe_to_podcast))
}

#[tokio::main]
async fn main() {
    let state = AppState {
        db: InMemoryStore::new(),
        current_user: None,
    };
    // build our application with a route
    let routes = routes().with_state(Arc::new(Mutex::new(state)));

    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(routes.into_make_service())
        .await
        .unwrap();
}

async fn handler() -> Json<&'static str> {
    Json("hello world")
}

async fn add_user<D: DB>(
    State(state): State<Arc<Mutex<AppState<D>>>>,
    Json(payload): Json<CreateUser>,
) -> impl IntoResponse {
    
    let user = state.lock().await.db.create_user(payload).unwrap();
    // Presumably store somewhere?
    (StatusCode::CREATED, Json(user))
}

async fn get_user<D: DB>(State(state): State<Arc<Mutex<AppState<D>>>>, Path(uid): Path<Uuid>) -> impl IntoResponse {
    let x = state.lock().await.db.get_user(uid);
    // (StatusCode::OK, Json(x))
    match x {
        Ok(u) => (StatusCode::OK, Json(Some(u))),
        Err(Error::NotFound) => (StatusCode::NOT_FOUND, Json(None)),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(None))
    }
}

async fn login<D: DB>(State(state): State<Arc<Mutex<AppState<D>>>>, Path(uid): Path<Uuid>) -> impl IntoResponse {
    let mut s = state.lock().await;
    match s.db.get_user(uid) {
        Ok(u) => {
            s.current_user = Some(u);
            StatusCode::OK
        },
        Err(_) => StatusCode::NOT_EXTENDED
    }
}

#[derive(Serialize, Clone, Debug)]
struct UserStatus {
    user: Option<User>,
    logged_in: bool,
}

async fn user_status<D: DB>(State(state): State<Arc<Mutex<AppState<D>>>>) -> impl IntoResponse {
    match &state.lock().await.current_user {
        Some(u) => {
            Json(UserStatus {
                user: Some(u.clone()),
                logged_in: true
            })
        },
        None => Json(UserStatus { user: None, logged_in: false })
    }
}

async fn subscribe_to_podcast<D: DB>(State(_): State<Arc<Mutex<AppState<D>>>>, Json(rss): Json<PodcastRSS>) -> impl IntoResponse {
    match Uri::from_str(&rss.rss) {
        Ok(url) => {
            let resp = reqwest::get(url.to_string()).await.unwrap().text().await.unwrap();
            let xml = roxmltree::Document::parse(&resp).unwrap();
            let rss = xml.root().children().find(|n| n.tag_name().name() == "rss").unwrap();
            let channel = rss.children().find(|n| n.tag_name().name() == "channel").unwrap();
            let title = channel.children().find(|n| n.tag_name().name() == "title").unwrap().text().unwrap();
            let description = channel.children().find(|n| n.tag_name().name() == "description").unwrap().text().unwrap();
            (StatusCode::OK, Json(Some(PodcastChannel {
                id: Uuid::new_v4(),
                name: title.to_string(),
                description: description.to_string(),
                rss: url.to_string()
            })))
        },
        // try with https://revolutionspodcast.libsyn.com/rss/
        Err(_) => (StatusCode::BAD_REQUEST, Json(None))
    }
}

#[derive(Serialize, Clone, Debug)]
struct User {
    name: String,
    id: uuid::Uuid,
}

#[derive(Serialize, Clone, Debug)]
struct PodcastChannel {
    name: String,
    description: String,
    rss: String,
    id: Uuid,
}

#[derive(Debug, Deserialize, Clone)]
struct PodcastRSS {
    rss: String
}

#[derive(Deserialize)]
struct CreateUser {
    name: String,
}

#[derive(Debug)]
enum Error {
    NotFound,
    DbError,
}

trait DB {
    fn get_user(&self, id: Uuid) -> Result<User, Error>;

    fn create_user(&mut self, name: CreateUser) -> Result<User, Error>;
}

#[derive(Debug, Clone)]
struct InMemoryStore {
    users: HashMap<Uuid, User>,
}

impl InMemoryStore {
    fn new() -> InMemoryStore {
        InMemoryStore {
            users: HashMap::new(),
        }
    }
}

impl DB for InMemoryStore {
    fn get_user(&self, id: Uuid) -> Result<User, Error> {
        self.users.get(&id).cloned().ok_or(Error::NotFound)
    }

    fn create_user(&mut self, user: CreateUser) -> Result<User, Error> {
        let uuid = Uuid::new_v4();
        let u = User {
            name: user.name,
            id: uuid,
        };
        let _ = self.users.insert(uuid, u.clone());
        Ok(u)
    }
}
