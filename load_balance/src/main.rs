use std::{
    hash::{DefaultHasher, Hash, Hasher},
    str::FromStr,
    sync::{atomic::AtomicUsize, Arc},
};

use axum::{
    body::Body,
    extract::{Request, State},
    handler::Handler,
    http::{
        uri::{Authority, Scheme},
        StatusCode, Uri,
    },
    response::IntoResponse,
};
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client},
    rt::TokioExecutor,
};
use tokio::net::TcpListener;

struct RoundRobin {
    addrs: Vec<&'static str>,
    req_counter: Arc<AtomicUsize>,
}

trait LoadBalancer {
    fn next_server(&self, req: &Request) -> String;
}

struct RinhaAccountBalance {
    addrs: Vec<&'static str>,
}

impl LoadBalancer for RinhaAccountBalance {
    fn next_server(&self, req: &Request) -> String {
        let path = req.uri().path();
        let hash = {
            let mut hasher = DefaultHasher::new();
            path.hash(&mut hasher);
            hasher.finish() as usize
        };
        self.addrs[hash % self.addrs.len()].to_string()
    }
}

impl LoadBalancer for RoundRobin {
    fn next_server(&self, _req: &Request) -> String {
        let count = self
            .req_counter
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.addrs[count % self.addrs.len()].to_string()
    }
}

#[derive(Clone)]
struct AppState {
    http_client: Client<HttpConnector, Body>,
    load_balance: Arc<dyn LoadBalancer + Send + Sync>,
}

#[tokio::main]
async fn main() {
    let litenner = TcpListener::bind("0.0.0.0:9999").await.unwrap();
    let addrs = vec!["api01:9998", "api02:9997"];
    let http_client = Client::builder(TokioExecutor::new())
        .http2_only(true)
        .build_http::<Body>();
    let req_counter = Arc::new(AtomicUsize::new(0));
    let round_robin = RoundRobin {
        addrs: addrs.clone(),
        req_counter: req_counter.clone(),
    };
    let _fixed_load_balance = RinhaAccountBalance {
        addrs: addrs.clone(),
    };
    let app_state = AppState {
        http_client,
        load_balance: Arc::new(round_robin),
    };
    let app = proxy.with_state(app_state);
    axum::serve(litenner, app).await.unwrap();
}

async fn proxy(
    State(AppState {
        http_client,
        load_balance,
    }): State<AppState>,
    mut req: Request,
) -> impl IntoResponse {
    let addr = load_balance.next_server(&req);
    *req.uri_mut() = {
        let uri = req.uri();
        let mut parts = uri.clone().into_parts();
        parts.authority = Authority::from_str(&addr.as_str()).ok();
        parts.scheme = Some(Scheme::HTTP);
        Uri::from_parts(parts).unwrap()
    };

    match http_client.request(req).await {
        Ok(res) => Ok(res),
        Err(_) => Err(StatusCode::BAD_GATEWAY),
    }
}
