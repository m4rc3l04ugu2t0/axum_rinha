use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::RwLock;

#[derive(Deserialize, Serialize, Clone)]
#[serde(try_from = "String")]
struct Description(String);

impl TryFrom<String> for Description {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty() || value.len() > 10 {
            Err("Inavlid description.")
        } else {
            Ok(Self(value))
        }
    }
}

#[derive(Serialize, Clone)]
struct RingBuffer<T>(VecDeque<T>);

impl<T> Default for RingBuffer<T> {
    fn default() -> Self {
        Self::with_capacity(10)
    }
}

impl<T> RingBuffer<T> {
    fn with_capacity(capacity: usize) -> Self {
        Self(VecDeque::with_capacity(capacity))
    }

    fn push(&mut self, item: T) {
        if self.0.len() == self.0.capacity() {
            self.0.pop_back();
            self.0.push_front(item);
        } else {
            self.0.push_front(item);
        }
    }
}

#[derive(Default, Clone)]
struct Account {
    balance: i32,
    limit: i32,
    transaction: RingBuffer<Transaction>,
}

impl Account {
    pub fn with_limit(limit: i32) -> Self {
        Self {
            limit,
            ..Default::default()
        }
    }

    pub fn transect(&mut self, transaction: Transaction) -> Result<(), &'static str> {
        match transaction.kind {
            TransactionType::Credit => {
                self.balance += transaction.value;
                self.transaction.push(transaction);
                Ok(())
            }
            TransactionType::Debit => {
                if self.balance + self.limit >= transaction.value {
                    self.balance -= transaction.value;
                    self.transaction.push(transaction);
                    Ok(())
                } else {
                    Err("Not enough balance")
                }
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
enum TransactionType {
    #[serde(rename = "c")]
    Credit,
    #[serde(rename = "d")]
    Debit,
}

#[derive(Clone, Serialize, Deserialize)]
struct Transaction {
    value: i32,
    kind: TransactionType,
    description: Description,
    #[serde(with = "time::serde::rfc3339", default = "OffsetDateTime::now_utc")]
    created_at: OffsetDateTime,
}

#[derive(Clone, Deserialize)]
struct TransactionPay {
    value: i32,
    kind: TransactionType,
    description: String,
}

type AppState = Arc<HashMap<u8, RwLock<Account>>>;

#[tokio::main]
async fn main() {
    let accounts = HashMap::<u8, RwLock<Account>>::from_iter([
        (1, RwLock::new(Account::with_limit(100_000))),
        (2, RwLock::new(Account::with_limit(80_000))),
        (3, RwLock::new(Account::with_limit(1_000_000))),
        (4, RwLock::new(Account::with_limit(10_000_000))),
        (5, RwLock::new(Account::with_limit(500_000))),
    ]);

    let app = Router::new()
        .route("/clients/:id/transaction", post(create_transaction))
        .route("/clients/:id/extract", get(view_account))
        .with_state(Arc::new(accounts));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn create_transaction(
    Path(account_id): Path<u8>,
    State(accounts): State<AppState>,
    Json(transaction): Json<Transaction>,
) -> impl IntoResponse {
    match accounts.get(&account_id) {
        Some(account) => {
            let mut account = account.write().await;

            match account.transect(transaction) {
                Ok(()) => Ok(Json(json!({
                    "limit": account.limit,
                    "balance": account.balance
                }))),
                Err(_) => Err(StatusCode::UNPROCESSABLE_ENTITY),
            }
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn view_account(
    Path(account_id): Path<u8>,
    State(accounts): State<AppState>,
) -> impl IntoResponse {
    match accounts.get(&account_id) {
        Some(account) => {
            let account = account.read().await;
            Ok(Json(json!({
                "saldo": {
                    "total": account.balance,
                    "data_extrato": OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
                    "limite": account.limit
                },
                    "ultimas_transacoes": account.transaction,
            })))
        }
        None => Err(StatusCode::NOT_FOUND),
    }
}
