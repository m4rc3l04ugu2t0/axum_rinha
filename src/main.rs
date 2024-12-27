use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use db::Db;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{HashMap, VecDeque},
    env,
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

struct Account {
    balance: i64,
    limit: i64,
    transaction: RingBuffer<Transaction>,
    db: Db<(i64, Transaction), 128>,
}

impl<A> FromIterator<A> for RingBuffer<A> {
    fn from_iter<T: IntoIterator<Item = A>>(iter: T) -> Self {
        let mut ring_buffer = Self::with_capacity(10);
        for item in iter.into_iter() {
            ring_buffer.push(item);
        }
        ring_buffer
    }
}

impl Account {
    pub fn with_db(
        path: impl AsRef<std::path::Path>,
        limit: i64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut db = Db::<(i64, Transaction), 128>::from_path(path)?;

        let mut transaction = db.rows().collect::<Vec<_>>();

        let balance = transaction
            .last()
            .map(|(balance, _)| *balance)
            .unwrap_or_default();

        transaction.reverse();

        Ok(Self {
            limit,
            balance,
            transaction: transaction.into_iter().map(|(_, t)| t).collect(),
            db,
        })
    }

    pub fn transact(&mut self, transaction: Transaction) -> Result<(), &'static str> {
        let balance = match transaction.kind {
            TransactionType::Credit => self.balance + transaction.value,
            TransactionType::Debit => {
                if self.balance + self.limit >= transaction.value {
                    self.balance - transaction.value
                } else {
                    return Err("Not enough balance");
                }
            }
        };
        self.db
            .insert((balance, transaction.clone()))
            .map_err(|_| "Failed to persist data")?;
        self.balance = balance;
        self.transaction.push(transaction);
        Ok(())
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
    value: i64,
    kind: TransactionType,
    description: Description,
    #[serde(with = "time::serde::rfc3339", default = "OffsetDateTime::now_utc")]
    created_at: OffsetDateTime,
}

#[allow(unused)]
#[derive(Clone, Deserialize)]
struct TransactionPay {
    value: i64,
    kind: TransactionType,
    description: String,
}

type AppState = Arc<HashMap<u8, RwLock<Account>>>;

#[tokio::main]
async fn main() {
    let port = env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);

    let accounts = HashMap::<u8, RwLock<Account>>::from_iter([
        (
            1,
            RwLock::new(Account::with_db("Account-1", 100_000).unwrap()),
        ),
        (
            2,
            RwLock::new(Account::with_db("Account-2", 80_000).unwrap()),
        ),
        (
            3,
            RwLock::new(Account::with_db("Account-3", 1_000_000).unwrap()),
        ),
        (
            4,
            RwLock::new(Account::with_db("Account-4", 10_000_000).unwrap()),
        ),
        (
            5,
            RwLock::new(Account::with_db("Account-5", 500_000).unwrap()),
        ),
    ]);

    let app = Router::new()
        .route("/clients/:id/transaction", post(create_transaction))
        .route("/clients/:id/extract", get(view_account))
        .with_state(Arc::new(accounts));

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .unwrap();
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

            match account.transact(transaction) {
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
