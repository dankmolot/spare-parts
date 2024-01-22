use std::{collections::HashMap, net::TcpListener, sync::Arc};
use anyhow::Result;
use encoding::Encoding;
use futures_lite::AsyncReadExt;
use axum::{extract::{Query, State}, routing::get, Json};
use serde::Serialize;
use smol::Async;
use tower_http::trace::{self, TraceLayer};
use tracing::{info, Level};

#[derive(Debug, Serialize, Clone)]
struct Record {
    sn: String,
    name: String,
    warehouse1: String,
    warehouse2: String,
    warehouse3: String,
    warehouse4: String,
    warehouse5: String,
    price: String,
    product_type: String,
    price_vat: String,
}

struct AppState {
    data: Vec<Arc<Record>>,
}

async fn load_data() -> Result<Vec<Arc<Record>>> {
    let mut file = smol::fs::File::open("LE.txt").await?;
    let mut buf: Vec<u8> = Vec::new();
    file.read_to_end(&mut buf).await?;
    let data = smol::unblock(move || encoding::all::ISO_8859_4.decode(&buf, encoding::DecoderTrap::Strict)).await
        .map_err(|e| anyhow::anyhow!(e))?;

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(false)
        .from_reader(data.as_bytes());

    let mut records: Vec<Arc<Record>> = Vec::new();
    for result in reader.records() {
        let record = result?;
        records.push(Arc::new(Record {
            sn: record[0].to_string(),
            name: record[1].to_string(),
            warehouse1: record[2].to_string(),
            warehouse2: record[3].to_string(),
            warehouse3: record[4].to_string(),
            warehouse4: record[5].to_string(),
            warehouse5: record[6].to_string(),
            price: record[8].to_string(),
            product_type: record[9].to_string(),  
            price_vat: record[10].to_string(),
        }))
    }
    Ok(records)
}

#[axum::debug_handler]
async fn spare_parts_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<String, String> {
    smol::unblock(move || {
        let mut data: Vec<Arc<Record>> = state.data.clone();

        if let Some(sort) = params.get("sort").cloned().as_mut() {
            let mut reverse = false;
            if sort.starts_with("-") {
                sort.remove(0);
                reverse = true;
            }

            match sort.as_str() {
                "name" => data.sort_by(|a, b| a.name.cmp(&b.name)),
                "sn" => data.sort_by(|a, b| a.sn.cmp(&b.sn)),
                "price" => data.sort_by(|a, b| a.price.cmp(&b.price)),
                "price_vat" => data.sort_by(|a, b| a.price_vat.cmp(&b.price_vat)),
                "product_type" => data.sort_by(|a, b| a.product_type.cmp(&b.product_type)),
                _ => (),
            }

            if reverse {
                data.reverse();
            }
        }

        if let Some(name) = params.get("name") {
            data = data.iter().filter(|r| r.name.contains(name)).cloned().collect();
        }
        if let Some(sn) = params.get("sn") {
            data = data.iter().filter(|r| r.sn.contains(sn)).cloned().collect();
        }

        if let Some(page) = params.get("page").and_then(|v| v.parse::<i64>().ok()) {
            let page_size = 30;
            let start = (page - 1) * page_size;
            let end = std::cmp::min(start + page_size, data.len() as i64);
            if start >= 0 && end <= data.len() as i64 {
                data = data[start as usize..end as usize].to_vec();
            } else {
                data = Vec::new();
            }
        }

        serde_json::to_string(&data)
            .map_err(|e| e.to_string())
    }).await
}

smol_macros::main! {
    async fn main(ex: &Arc<smol_macros::Executor<'_>>) -> Result<()> {
        tracing_subscriber::fmt()
            .with_target(false)
            .compact()
            .init();

        let data = load_data().await?;

        let state = Arc::new(AppState {data});
    
        let app = axum::Router::new()
            .route("/", axum::routing::get(|| async { "Hello, World!" }))
            .route("/spare-parts", get(spare_parts_handler))
            .layer(TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO))
            )
            .with_state(state);
    
        // Create a `smol`-based TCP listener.
        let listener = Async::<TcpListener>::bind(([127, 0, 0, 1], 1337)).unwrap();
        info!("listening on {}", listener.get_ref().local_addr().unwrap());
    
        smol_axum::serve(ex.clone(), listener, app).await
            .map_err(|e| anyhow::anyhow!(e))
    }
}
