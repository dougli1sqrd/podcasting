use std::{collections::HashMap, str::FromStr, sync::Arc};

use axum::{
    extract::{Path, State},
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
    current_user: Option<Uuid>,
    db: D,
}

fn routes() -> Router<Arc<Mutex<AppState<InMemoryStore>>>> {
    Router::new()
        .route("/", get(handler))
        .route("/users", post(add_user))
        .route("/users/:id", get(get_user))
        .route("/login", get(user_status))
        .route("/login/:id", post(login))
        .route("/podcast", post(subscribe_to_podcast))
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

async fn get_user<D: DB>(
    State(state): State<Arc<Mutex<AppState<D>>>>,
    Path(uid): Path<Uuid>,
) -> impl IntoResponse {
    let x = state.lock().await.db.get_user(uid);
    // (StatusCode::OK, Json(x))
    match x {
        Ok(u) => (StatusCode::OK, Json(Some(u))),
        Err(Error::NotFound) => (StatusCode::NOT_FOUND, Json(None)),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, Json(None)),
    }
}

async fn login<D: DB>(
    State(state): State<Arc<Mutex<AppState<D>>>>,
    Path(uid): Path<Uuid>,
) -> impl IntoResponse {
    let mut s = state.lock().await;
    match s.db.get_user(uid) {
        Ok(u) => {
            s.current_user = Some(u.id);
            StatusCode::OK
        }
        Err(_) => StatusCode::NOT_EXTENDED,
    }
}

#[derive(Serialize, Clone, Debug)]
struct UserStatus {
    user: Option<Uuid>,
    logged_in: bool,
}

async fn user_status<D: DB>(State(state): State<Arc<Mutex<AppState<D>>>>) -> impl IntoResponse {
    match &state.lock().await.current_user {
        Some(u) => Json(UserStatus {
            user: Some(u.clone()),
            logged_in: true,
        }),
        None => Json(UserStatus {
            user: None,
            logged_in: false,
        }),
    }
}

async fn subscribe_to_podcast<D: DB>(
    State(state): State<Arc<Mutex<AppState<D>>>>,
    Json(rss): Json<PodcastRSS>,
) -> impl IntoResponse {
    match Uri::from_str(&rss.rss) {
        Ok(url) => {
            let state = &mut state.lock().await;
            let logged_in = state.current_user.clone();
            let db = &mut state.db;

            match db.get_podcast(rss.rss) {
                Ok(p) => {
                    if let Some(u) = logged_in {
                        let subs = db.subscribe(u, p.rss);
                        match subs {
                            Ok(s) => (StatusCode::CREATED, Json(Some(s))),
                            Err(_) => (StatusCode::BAD_REQUEST, Json(None))
                        }
                    } else {
                        (StatusCode::NOT_FOUND, Json(None))
                    }
                },
                Err(Error::NotFound) => {
                    // Podcast not found, so let's create it
                    let (title, description) = parse_rss(url.to_string()).await;
                    match db.create_podcast(url.to_string(), title, description) {
                        Ok(p) => {
                            if let Some(u) = logged_in {
                                let subs = db.subscribe(u, p.rss);
                                match subs {
                                    Ok(s) => (StatusCode::CREATED, Json(Some(s))),
                                    Err(_) => (StatusCode::BAD_REQUEST, Json(None))
                                }
                            } else {
                                (StatusCode::NOT_FOUND, Json(None))
                            }
                        },
                        Err(_) => (StatusCode::BAD_REQUEST, Json(None))
                    }
                },
                Err(_) => (StatusCode::BAD_REQUEST, Json(None))
            }

        }
        // try with https://revolutionspodcast.libsyn.com/rss/
        Err(_) => (StatusCode::BAD_REQUEST, Json(None)),
    }
}

async fn parse_rss(rss_url: String) -> (String, String) {
    let resp = reqwest::get(rss_url)
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let xml = roxmltree::Document::parse(&resp).unwrap();
    let rss = xml
        .root()
        .children()
        .find(|n| n.tag_name().name() == "rss")
        .unwrap();
    let channel = rss
        .children()
        .find(|n| n.tag_name().name() == "channel")
        .unwrap();
    let title = channel
        .children()
        .find(|n| n.tag_name().name() == "title")
        .unwrap()
        .text()
        .unwrap();
    let description = channel
        .children()
        .find(|n| n.tag_name().name() == "description")
        .unwrap()
        .text()
        .unwrap();

    (title.to_string(), description.to_string())
}

#[derive(Serialize, Clone, Debug)]
struct User {
    name: String,
    id: Uuid,
    subscribed: Vec<String>,
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
    rss: String,
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

    fn get_podcast(&self, rss: String) -> Result<PodcastChannel, Error>;

    fn create_podcast(
        &mut self,
        rss: String,
        title: String,
        description: String,
    ) -> Result<PodcastChannel, Error>;

    fn subscribe(&mut self, user: Uuid, rss: String) -> Result<Vec<String>, Error>;
}

#[derive(Debug, Clone)]
struct InMemoryStore {
    users: HashMap<Uuid, User>,
    podcasts: HashMap<String, PodcastChannel>,
}

impl InMemoryStore {
    fn new() -> InMemoryStore {
        InMemoryStore {
            users: HashMap::new(),
            podcasts: HashMap::new(),
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
            subscribed: vec![],
        };
        let _ = self.users.insert(uuid, u.clone());
        Ok(u)
    }

    fn get_podcast(&self, rss: String) -> Result<PodcastChannel, Error> {
        self.podcasts.get(&rss).cloned().ok_or(Error::NotFound)
    }

    fn create_podcast(
        &mut self,
        rss: String,
        title: String,
        description: String,
    ) -> Result<PodcastChannel, Error> {
        let id = Uuid::new_v4();
        let p = PodcastChannel {
            rss: rss.clone(),
            name: title,
            description,
            id,
        };
        let _ = self.podcasts.insert(rss, p.clone());
        Ok(p)
    }

    fn subscribe(&mut self, user: Uuid, rss: String) -> Result<Vec<String>, Error> {
        let p = self.get_podcast(rss)?;
        let u = self.users.get_mut(&user).ok_or(Error::NotFound)?;
        if !u.subscribed.contains(&p.rss) {
            u.subscribed.push(p.rss.clone());
        }
        Ok(u.subscribed.clone())
    }
}
