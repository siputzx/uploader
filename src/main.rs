use axum::{
    extract::{DefaultBodyLimit, Multipart, Query, State, Path as AxumPath},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    collections::HashMap,
    env,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{
    fs::{self, File},
    io::{AsyncWriteExt, BufWriter},
    sync::RwLock,
    time::sleep,
};
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
struct Config {
    secret_key: String,
    upload_dir: String,
    max_file_size: usize,
    file_lifetime: u64,
    buffer_size: usize,
    bind_addr: String,
    base_url: String,
    workers: usize,
}

impl Config {
    fn from_env() -> Self {
        Self {
            secret_key: env::var("SPTZX_SECRET_KEY")
                .unwrap_or_else(|_| "sptzx-change-me-in-production".to_string()),
            upload_dir: env::var("SPTZX_UPLOAD_DIR")
                .unwrap_or_else(|_| "./uploads".to_string()),
            max_file_size: env::var("SPTZX_MAX_FILE_SIZE")
                .unwrap_or_else(|_| "536870912".to_string())
                .parse()
                .unwrap_or(536870912),
            file_lifetime: env::var("SPTZX_FILE_LIFETIME")
                .unwrap_or_else(|_| "300".to_string())
                .parse()
                .unwrap_or(300),
            buffer_size: env::var("SPTZX_BUFFER_SIZE")
                .unwrap_or_else(|_| "2097152".to_string())
                .parse()
                .unwrap_or(2097152),
            bind_addr: env::var("SPTZX_BIND_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
            base_url: env::var("SPTZX_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:3000".to_string()),
            workers: env::var("SPTZX_WORKERS")
                .unwrap_or_else(|_| "16".to_string())
                .parse()
                .unwrap_or(16),
        }
    }
}

#[derive(Debug, Clone)]
struct AppState {
    file_registry: Arc<RwLock<HashMap<String, FileMetadata>>>,
    config: Arc<Config>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileMetadata {
    file_id: String,
    original_name: String,
    disk_path: String,
    mime_type: String,
    size: u64,
    uploaded_at: i64,
    owner: String,
}

#[derive(Debug, Deserialize)]
struct SignedUrlParams {
    #[serde(rename = "sz-version")]
    version: String,
    #[serde(rename = "sz-owner")]
    owner: String,
    #[serde(rename = "sz-date")]
    date: String,
    #[serde(rename = "sz-expires")]
    expires: String,
    #[serde(rename = "sz-region")]
    region: String,
    #[serde(rename = "sz-mode")]
    mode: String,
    #[serde(rename = "sz-type")]
    file_type: String,
    #[serde(rename = "sz-id")]
    id: String,
    #[serde(rename = "sz-nonce")]
    nonce: String,
    #[serde(rename = "sz-signature")]
    signature: String,
}

#[derive(Debug, Serialize)]
struct UploadResponse {
    id: String,
    name: String,
    size: u64,
    mime: String,
    view: String,
    download: String,
    ttl: u64,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .compact()
        .init();

    let config = Arc::new(Config::from_env());

    fs::create_dir_all(&config.upload_dir).await?;

    let state = AppState {
        file_registry: Arc::new(RwLock::new(HashMap::new())),
        config: config.clone(),
    };

    let app = Router::new()
        .route("/", get(health_check))
        .route("/upload", post(upload_handler))
        .route("/file/:id", get(serve_file))
        .layer(DefaultBodyLimit::max(config.max_file_size))
        .layer(TraceLayer::new_for_http())
        .with_state(state.clone());

    tokio::spawn(cleanup_expired_files(state.clone()));

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    info!("🚀 Sptzx listening on {} | Workers: {} | Buffer: {}MB | Max: {}MB | TTL: {}s", 
        config.bind_addr, 
        config.workers,
        config.buffer_size / 1024 / 1024,
        config.max_file_size / 1024 / 1024,
        config.file_lifetime
    );
    
    axum::serve(listener, app)
        .tcp_nodelay(true)
        .await?;

    Ok(())
}

async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status":"ok"}))
}

async fn upload_handler(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, (StatusCode, Json<ErrorResponse>)> {
    let file_id = Uuid::new_v4().to_string();
    let mut original_filename = String::from("unknown");
    let mut total_size: u64 = 0;
    
    let disk_path = PathBuf::from(&state.config.upload_dir).join(format!("{}.bin", file_id));

    let file = File::create(&disk_path).await.map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "file_create_failed".to_string() }))
    })?;

    let mut writer = BufWriter::with_capacity(state.config.buffer_size, file);

    while let Some(field) = multipart.next_field().await.map_err(|_| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "invalid_multipart".to_string() }))
    })? {
        if let Some(name) = field.file_name() {
            original_filename = sanitize_filename(name);
        }

        let mut stream = field;
        while let Some(chunk) = stream.next().await {
            let data = chunk.map_err(|_| {
                (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "chunk_read_failed".to_string() }))
            })?;

            total_size += data.len() as u64;

            if total_size > state.config.max_file_size as u64 {
                let _ = fs::remove_file(&disk_path).await;
                return Err((StatusCode::PAYLOAD_TOO_LARGE, Json(ErrorResponse { error: "file_too_large".to_string() })));
            }

            writer.write_all(&data).await.map_err(|_| {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "write_failed".to_string() }))
            })?;
        }
    }

    writer.flush().await.map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "flush_failed".to_string() }))
    })?;

    let mime_type = mime_guess::from_path(&original_filename).first_or_octet_stream().to_string();

    let metadata = FileMetadata {
        file_id: file_id.clone(),
        original_name: original_filename.clone(),
        disk_path: disk_path.to_string_lossy().to_string(),
        mime_type: mime_type.clone(),
        size: total_size,
        uploaded_at: Utc::now().timestamp(),
        owner: "default".to_string(),
    };

    state.file_registry.write().await.insert(file_id.clone(), metadata.clone());

    info!("✅ {} | {} | {}", original_filename, total_size, mime_type);

    let view_url = generate_signed_url(&file_id, "inline", &metadata, &state.config);
    let download_url = generate_signed_url(&file_id, "attachment", &metadata, &state.config);

    let state_clone = state.clone();
    let file_id_clone = file_id.clone();
    tokio::spawn(async move {
        sleep(Duration::from_secs(state_clone.config.file_lifetime)).await;
        delete_file(&state_clone, &file_id_clone).await;
    });

    Ok(Json(UploadResponse {
        id: file_id,
        name: original_filename,
        size: total_size,
        mime: mime_type,
        view: view_url,
        download: download_url,
        ttl: state.config.file_lifetime,
    }))
}

async fn serve_file(
    State(state): State<AppState>,
    AxumPath(file_id): AxumPath<String>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let signed_params = parse_signed_params(&params)?;

    if !verify_signature(&signed_params, &state.config) {
        warn!("⚠️ invalid_sig | {}", file_id);
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: "invalid_signature".to_string() })));
    }

    let expires_timestamp = signed_params.expires.parse::<i64>().map_err(|_| {
        (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: "invalid_expires".to_string() }))
    })?;

    if Utc::now().timestamp() > expires_timestamp {
        warn!("⚠️ expired | {}", file_id);
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: "link_expired".to_string() })));
    }

    if signed_params.id != file_id {
        return Err((StatusCode::FORBIDDEN, Json(ErrorResponse { error: "id_mismatch".to_string() })));
    }

    let registry = state.file_registry.read().await;
    let metadata = registry.get(&file_id).ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(ErrorResponse { error: "file_not_found".to_string() }))
    })?;

    let file_content = fs::read(&metadata.disk_path).await.map_err(|_| {
        error!("❌ read_failed | {}", file_id);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: "read_failed".to_string() }))
    })?;

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, metadata.mime_type.parse().unwrap());

    let disposition = if is_viewable_mime(&metadata.mime_type) && signed_params.mode == "inline" {
        format!("inline; filename=\"{}\"", metadata.original_name)
    } else {
        format!("attachment; filename=\"{}\"", metadata.original_name)
    };
    headers.insert(header::CONTENT_DISPOSITION, disposition.parse().unwrap());
    headers.insert(header::CONTENT_LENGTH, metadata.size.to_string().parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "public, max-age=300".parse().unwrap());
    headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());

    info!("📤 {} | {}", metadata.original_name, metadata.mime_type);

    Ok((StatusCode::OK, headers, file_content).into_response())
}

fn generate_signed_url(file_id: &str, mode: &str, metadata: &FileMetadata, config: &Config) -> String {
    let version = "v1";
    let owner = &metadata.owner;
    let date = Utc::now().format("%Y%m%d").to_string();
    let expires = (Utc::now().timestamp() + config.file_lifetime as i64).to_string();
    let region = "global";
    let file_type = &metadata.mime_type;
    let nonce = Uuid::new_v4().to_string();

    let string_to_sign = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        version, owner, date, expires, region, mode, file_type, file_id, nonce
    );

    let signature = compute_hmac(&string_to_sign, &config.secret_key);

    format!(
        "{}/file/{}?sz-version={}&sz-owner={}&sz-date={}&sz-expires={}&sz-region={}&sz-mode={}&sz-type={}&sz-id={}&sz-nonce={}&sz-signature={}",
        config.base_url, file_id, version, owner, date, expires, region, mode, file_type, file_id, nonce, signature
    )
}

fn compute_hmac(data: &str, secret: &str) -> String {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(data.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn verify_signature(params: &SignedUrlParams, config: &Config) -> bool {
    let string_to_sign = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
        params.version, params.owner, params.date, params.expires,
        params.region, params.mode, params.file_type, params.id, params.nonce
    );
    compute_hmac(&string_to_sign, &config.secret_key) == params.signature
}

fn parse_signed_params(
    params: &HashMap<String, String>,
) -> Result<SignedUrlParams, (StatusCode, Json<ErrorResponse>)> {
    let get_param = |key: &str| {
        params.get(key).cloned().ok_or_else(|| {
            (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: format!("missing_{}", key) }))
        })
    };

    Ok(SignedUrlParams {
        version: get_param("sz-version")?,
        owner: get_param("sz-owner")?,
        date: get_param("sz-date")?,
        expires: get_param("sz-expires")?,
        region: get_param("sz-region")?,
        mode: get_param("sz-mode")?,
        file_type: get_param("sz-type")?,
        id: get_param("sz-id")?,
        nonce: get_param("sz-nonce")?,
        signature: get_param("sz-signature")?,
    })
}

fn is_viewable_mime(mime_type: &str) -> bool {
    mime_type.starts_with("image/") || mime_type.starts_with("video/") || mime_type.starts_with("audio/")
}

fn sanitize_filename(filename: &str) -> String {
    filename.chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .take(255)
        .collect()
}

async fn delete_file(state: &AppState, file_id: &str) {
    let mut registry = state.file_registry.write().await;
    if let Some(metadata) = registry.remove(file_id) {
        match fs::remove_file(&metadata.disk_path).await {
            Ok(_) => info!("🗑️ {} | {}", metadata.original_name, file_id),
            Err(e) => error!("❌ delete_failed | {} | {}", file_id, e),
        }
    }
}

async fn cleanup_expired_files(state: AppState) {
    let mut interval = tokio::time::interval(Duration::from_secs(60));
    loop {
        interval.tick().await;
        let now = Utc::now().timestamp();
        let to_delete: Vec<String> = {
            let registry = state.file_registry.read().await;
            registry.iter()
                .filter(|(_, m)| now - m.uploaded_at > state.config.file_lifetime as i64)
                .map(|(id, _)| id.clone())
                .collect()
        };
        for file_id in to_delete {
            delete_file(&state, &file_id).await;
        }
    }
}
