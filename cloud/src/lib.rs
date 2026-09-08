use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
mod auth;
mod teams;
pub use auth::JwksCache;
use auth::auth;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::{
    io::Read,
    path::{Component, PathBuf},
    sync::Arc,
};
use teams::*;
use uuid::Uuid;

pub const MAX_UPLOAD: usize = 10 * 1024 * 1024;
#[derive(Clone)]
pub struct App {
    pub db: PgPool,
    pub storage: PathBuf,
    pub issuer: String,
    pub audience: String,
    pub jwks: Arc<JwksCache>,
}
#[derive(Clone, Deserialize)]
struct Identity {
    sub: String,
    email: Option<String>,
    #[serde(default)]
    email_verified: bool,
}
#[derive(Debug)]
pub struct Error(StatusCode, String);
type Result<T> = std::result::Result<T, Error>;
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        (
            self.0,
            Json(json!({"error":{"message":self.1},"request_id":Uuid::new_v4()})),
        )
            .into_response()
    }
}
impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        if e.as_database_error()
            .is_some_and(|e| e.is_unique_violation() || e.is_foreign_key_violation())
        {
            err(StatusCode::CONFLICT, "Conflicting resource state")
        } else {
            eprintln!("database request failed");
            err(StatusCode::INTERNAL_SERVER_ERROR, "Database request failed")
        }
    }
}
fn err(s: StatusCode, m: &str) -> Error {
    Error(s, m.into())
}
fn bad(m: &str) -> Error {
    err(StatusCode::BAD_REQUEST, m)
}
fn ok(v: Value) -> Json<Value> {
    Json(json!({"data":v,"request_id":Uuid::new_v4()}))
}
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
pub fn router(a: App) -> Router {
    Router::new()
        .route("/v1/me/teams", get(teams))
        .route("/v1/teams", post(create_team))
        .route("/v1/teams/{team}/members", get(members))
        .route("/v1/teams/{team}/members/{user}", delete(remove_member))
        .route("/v1/teams/{team}/transfer-owner", post(transfer_owner))
        .route("/v1/teams/{team}/invitations", post(invite))
        .route("/v1/teams/{team}/invitations/{id}", delete(revoke))
        .route("/v1/invitations/accept", post(accept))
        .route("/v1/teams/{team}/skills", get(skills).post(publish_first))
        .route(
            "/v1/teams/{team}/skills/{skill}",
            get(detail).patch(edit_skill),
        )
        .route(
            "/v1/teams/{team}/skills/{skill}/versions",
            get(versions).post(publish_next),
        )
        .route(
            "/v1/teams/{team}/skills/{skill}/recommendation",
            put(recommend),
        )
        .route(
            "/v1/teams/{team}/skills/{skill}/versions/{version}",
            get(version_detail),
        )
        .route(
            "/v1/teams/{team}/skills/{skill}/versions/{version}/artifact",
            get(artifact),
        )
        .route(
            "/v1/teams/{team}/skills/{skill}/versions/{version}/files",
            get(files),
        )
        .route(
            "/v1/teams/{team}/skills/{skill}/versions/{version}/file",
            get(file),
        )
        .layer(middleware::from_fn_with_state(a.clone(), auth))
        .route("/v1/health", get(health))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD + 128 * 1024))
        .with_state(a)
        .layer(middleware::from_fn(envelope_errors))
}
async fn envelope_errors(req: Request<axum::body::Body>, next: Next) -> Response {
    let response = next.run(req).await;
    if response.status().is_client_error() || response.status().is_server_error() {
        if response
            .headers()
            .get("content-type")
            .is_none_or(|v| v != "application/json")
        {
            return err(
                response.status(),
                response
                    .status()
                    .canonical_reason()
                    .unwrap_or("Request failed"),
            )
            .into_response();
        }
    }
    response
}

async fn health(State(a): State<App>) -> Result<Json<Value>> {
    sqlx::query("SELECT 1").execute(&a.db).await?;
    Ok(ok(json!({"status":"ready"})))
}
type User = axum::Extension<Identity>;
#[derive(Deserialize)]
struct Listing {
    q: Option<String>,
    cursor: Option<Uuid>,
    limit: Option<i64>,
    #[serde(default)]
    archived: bool,
}
async fn skills(
    State(a): State<App>,
    axum::Extension(u): User,
    Path(t): Path<Uuid>,
    Query(q): Query<Listing>,
) -> Result<Json<Value>> {
    member(&a, t, &u.sub).await?;
    let n = q.limit.unwrap_or(50);
    if !(1..=100).contains(&n) {
        return Err(bad("limit must be 1..100"));
    }
    if let Some(c) = q.cursor {
        if !sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM skills WHERE team_id=$1 AND id=$2)",
        )
        .bind(t)
        .bind(c)
        .fetch_one(&a.db)
        .await?
        {
            return Err(bad("Invalid cursor"));
        }
    }
    let mut rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(s) || jsonb_build_object('recommended_version',v.version) FROM skills s LEFT JOIN skill_versions v ON v.id=s.recommended_version_id WHERE s.team_id=$1 AND ($2 OR s.archived_at IS NULL) AND ($3::text IS NULL OR s.title ILIKE '%'||$3||'%' OR s.description ILIKE '%'||$3||'%') AND ($4::uuid IS NULL OR (s.updated_at,s.id)<(SELECT updated_at,id FROM skills WHERE team_id=$1 AND id=$4)) ORDER BY s.updated_at DESC,s.id DESC LIMIT $5").bind(t).bind(q.archived).bind(q.q).bind(q.cursor).bind(n+1).fetch_all(&a.db).await?;
    let next = if rows.len() > n as usize {
        rows.truncate(n as usize);
        rows.last().map(|r| r["id"].clone())
    } else {
        None
    };
    Ok(ok(json!({"skills":rows,"next_cursor":next})))
}
async fn skill(a: &App, t: Uuid, s: Uuid) -> Result<Value> {
    sqlx::query_scalar("SELECT to_jsonb(s) || jsonb_build_object('recommended_version',v.version) FROM skills s LEFT JOIN skill_versions v ON v.id=s.recommended_version_id WHERE s.team_id=$1 AND s.id=$2").bind(t).bind(s).fetch_optional(&a.db).await?.ok_or_else(||err(StatusCode::NOT_FOUND,"Skill not found"))
}
async fn detail(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, s)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    member(&a, t, &u.sub).await?;
    Ok(ok(json!({"skill":skill(&a,t,s).await?})))
}
async fn versions(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, s)): Path<(Uuid, Uuid)>,
    Query(q): Query<Listing>,
) -> Result<Json<Value>> {
    member(&a, t, &u.sub).await?;
    skill(&a, t, s).await?;
    let n = q.limit.unwrap_or(50);
    if !(1..=100).contains(&n) {
        return Err(bad("limit must be 1..100"));
    }
    if let Some(c) = q.cursor {
        version(&a, t, s, c)
            .await
            .map_err(|_| bad("Invalid cursor"))?;
    }
    let mut rows:Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(v)-'artifact_key' FROM skill_versions v WHERE team_id=$1 AND skill_id=$2 AND ($3::uuid IS NULL OR (created_at,id)<(SELECT created_at,id FROM skill_versions WHERE team_id=$1 AND skill_id=$2 AND id=$3)) ORDER BY created_at DESC,id DESC LIMIT $4").bind(t).bind(s).bind(q.cursor).bind(n+1).fetch_all(&a.db).await?;
    let next = if rows.len() > n as usize {
        rows.truncate(n as usize);
        rows.last().map(|r| r["id"].clone())
    } else {
        None
    };
    Ok(ok(json!({"versions":rows,"next_cursor":next})))
}
async fn manageable(
    tx: &mut Transaction<'_, Postgres>,
    t: Uuid,
    s: Uuid,
    u: &str,
    o: &str,
) -> Result<Value> {
    let v: Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM skills s WHERE team_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(t)
    .bind(s)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Skill not found"))?;
    if o != u && v["maintainer_id"] != u {
        return Err(err(StatusCode::FORBIDDEN, "Maintainer required"));
    }
    Ok(v)
}
async fn edit_skill(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, s)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let revision = h
        .get("if-match")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim_matches('"').parse::<i64>().ok())
        .ok_or_else(|| bad("If-Match revision required"))?;
    let (mut tx, o) = locked(&a, t, &u.sub).await?;
    let old = manageable(&mut tx, t, s, &u.sub, &o).await?;
    if old["revision"] != revision {
        return Err(err(StatusCode::CONFLICT, "Skill changed; reload"));
    }
    let maintainer = v.get("maintainer_id").and_then(Value::as_str);
    if maintainer.is_some() {
        owner(&o, &u.sub)?
    }
    for k in ["title", "description", "example"] {
        if v.get(k).is_some() {
            required(&v, k)?;
        }
    }
    if v.get("archived").is_some() && !v["archived"].is_boolean() {
        return Err(bad("archived must be boolean"));
    }
    sqlx::query("UPDATE skills SET title=COALESCE($3,title),description=COALESCE($4,description),example=COALESCE($5,example),maintainer_id=COALESCE($6,maintainer_id),archived_at=CASE WHEN $7::bool IS NULL THEN archived_at WHEN $7 THEN now() ELSE NULL END,revision=revision+1,updated_at=now() WHERE team_id=$1 AND id=$2").bind(t).bind(s).bind(v["title"].as_str()).bind(v["description"].as_str()).bind(v["example"].as_str()).bind(maintainer).bind(v["archived"].as_bool()).execute(&mut *tx).await?;
    event(&mut tx, t, &u.sub, "skill.edited", s).await?;
    tx.commit().await?;
    Ok(ok(json!({"skill":skill(&a,t,s).await?})))
}
async fn recommend(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, s)): Path<(Uuid, Uuid)>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let id: Uuid = required(&v, "version_id")?
        .parse()
        .map_err(|_| bad("Invalid version_id"))?;
    let (mut tx, o) = locked(&a, t, &u.sub).await?;
    let old = manageable(&mut tx, t, s, &u.sub, &o).await?;
    if !v
        .get("expected_version_id")
        .is_some_and(|x| x == &old["recommended_version_id"])
    {
        return Err(err(StatusCode::CONFLICT, "Recommendation changed"));
    }
    sqlx::query("UPDATE skills SET recommended_version_id=$3,revision=revision+1,updated_at=now() WHERE team_id=$1 AND id=$2").bind(t).bind(s).bind(id).execute(&mut *tx).await?;
    event(&mut tx, t, &u.sub, "skill.recommended", s).await?;
    tx.commit().await?;
    Ok(ok(json!({"skill":skill(&a,t,s).await?})))
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    path: String,
    size: u64,
}
pub fn validate_archive(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > MAX_UPLOAD {
        return Err(err(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Compressed archive exceeds 10 MiB",
        ));
    }
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(decoder.take(52 * 1024 * 1024));
    let mut total = 0u64;
    let mut seen = std::collections::HashSet::new();
    let mut manifest = Vec::new();
    for entry in archive.entries().map_err(|_| bad("Invalid tar.gz"))? {
        let mut e = entry.map_err(|_| bad("Invalid archive entry"))?;
        let p = e
            .path()
            .map_err(|_| bad("Invalid archive path"))?
            .into_owned();
        let name = p
            .to_str()
            .ok_or_else(|| bad("Paths must be UTF-8"))?
            .to_owned();
        if name.contains('\\')
            || p.is_absolute()
            || p.components().any(|c| !matches!(c, Component::Normal(_)))
            || p.components().any(|c| {
                let s = c.as_os_str().to_string_lossy();
                s == ".git" || s == ".env" || s.starts_with(".env.")
            })
        {
            return Err(bad("Unsafe or private archive path"));
        }
        if seen.len() >= 2000 {
            return Err(err(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Too many archive entries",
            ));
        }
        if !seen.insert(name.clone()) {
            return Err(bad("Duplicate archive path"));
        }
        let ty = e.header().entry_type();
        if !ty.is_file() && !ty.is_dir() {
            return Err(bad("Links and special entries are forbidden"));
        }
        if ty.is_dir() {
            continue;
        }
        let size = e.size();
        total = total
            .checked_add(size)
            .ok_or_else(|| bad("Archive size overflow"))?;
        if total > 50 * 1024 * 1024 || manifest.len() >= 1000 {
            return Err(err(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Expanded archive exceeds limits",
            ));
        }
        let copied =
            std::io::copy(&mut e, &mut std::io::sink()).map_err(|_| bad("Truncated archive"))?;
        if copied != size {
            return Err(bad("Truncated archive"));
        }
        manifest.push(Manifest { path: name, size });
    }
    if !manifest.iter().any(|f| f.path == "SKILL.md") {
        return Err(bad("Root SKILL.md required"));
    }
    serde_json::to_value(manifest).map_err(|_| bad("Invalid manifest"))
}
async fn publish_first(
    State(a): State<App>,
    axum::Extension(u): User,
    Path(t): Path<Uuid>,
    h: HeaderMap,
    m: Multipart,
) -> Result<Json<Value>> {
    publish(a, u, t, None, h, m).await
}
async fn publish_next(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, s)): Path<(Uuid, Uuid)>,
    h: HeaderMap,
    m: Multipart,
) -> Result<Json<Value>> {
    publish(a, u, t, Some(s), h, m).await
}
async fn publish(
    a: App,
    u: Identity,
    t: Uuid,
    s: Option<Uuid>,
    h: HeaderMap,
    mut m: Multipart,
) -> Result<Json<Value>> {
    member(&a, t, &u.sub).await?;
    let k = key(&h)?;
    let mut metadata = None;
    let mut artifact = None;
    while let Some(f) = m
        .next_field()
        .await
        .map_err(|_| bad("Invalid multipart body"))?
    {
        match f.name() {
            Some("metadata") if metadata.is_none() => {
                let b = f.bytes().await.map_err(|_| bad("Invalid metadata"))?;
                if b.len() > 65536 {
                    return Err(bad("Metadata too large"));
                }
                metadata = Some(
                    serde_json::from_slice::<Value>(&b)
                        .map_err(|_| bad("Invalid metadata JSON"))?,
                );
            }
            Some("artifact") if artifact.is_none() => {
                let b = f.bytes().await.map_err(|_| bad("Invalid artifact"))?;
                artifact = Some(b);
            }
            _ => return Err(bad("Unexpected or duplicate multipart field")),
        }
    }
    let v = metadata.ok_or_else(|| bad("metadata required"))?;
    let b = artifact.ok_or_else(|| bad("artifact required"))?;
    let label = required(&v, "version")?;
    semver::Version::parse(&label).map_err(|_| bad("Semantic version required"))?;
    let notes = required(&v, "release_notes")?;
    let digest = hash(&b);
    if let Some(expected) = v["sha256"].as_str() {
        if expected != digest {
            return Err(bad("Artifact digest mismatch"));
        }
    }
    let bytes = b.to_vec();
    let manifest = tokio::task::spawn_blocking(move || validate_archive(&bytes))
        .await
        .map_err(|_| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Archive validation failed",
            )
        })??;
    let request_digest = hash(format!("{}:{digest}", v).as_bytes());
    let op = format!("publish:{}", s.map(|s| s.to_string()).unwrap_or_default());
    let (mut tx, o) = locked(&a, t, &u.sub).await?;
    if let Some(v) = replay(&mut tx, &u.sub, t, &op, &k, &request_digest).await? {
        return Ok(ok(v));
    }
    let sid = if let Some(s) = s {
        let old = manageable(&mut tx, t, s, &u.sub, &o).await?;
        if !old["archived_at"].is_null() {
            return Err(err(StatusCode::CONFLICT, "Skill archived"));
        }
        s
    } else {
        let slug = required(&v, "slug")?;
        if slug.len() > 100
            || !slug
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        {
            return Err(bad("Invalid slug"));
        }
        if let Some(id) =
            sqlx::query_scalar::<_, Uuid>("SELECT id FROM skills WHERE team_id=$1 AND slug=$2")
                .bind(t)
                .bind(&slug)
                .fetch_optional(&mut *tx)
                .await?
        {
            manageable(&mut tx, t, id, &u.sub, &o).await?;
            id
        } else {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO skills(id,team_id,slug,title,description,example,maintainer_id) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(id).bind(t).bind(slug).bind(required(&v,"title")?).bind(required(&v,"description")?).bind(required(&v,"example")?).bind(&u.sub).execute(&mut *tx).await?;
            id
        }
    };
    if let Some(existing)=sqlx::query_scalar::<_,Value>("SELECT to_jsonb(v)-'artifact_key' FROM skill_versions v WHERE team_id=$1 AND skill_id=$2 AND version=$3").bind(t).bind(sid).bind(&label).fetch_optional(&mut *tx).await?{if existing["sha256"]!=digest{return Err(err(StatusCode::CONFLICT,"Version already has different artifact"))}let result=json!({"skill_id":sid,"version":existing});remember(&mut tx,&u.sub,t,&op,&k,&request_digest,&result).await?;tx.commit().await?;return Ok(ok(result))}
    let current: Option<Uuid> =
        sqlx::query_scalar("SELECT recommended_version_id FROM skills WHERE id=$1 AND team_id=$2")
            .bind(sid)
            .bind(t)
            .fetch_one(&mut *tx)
            .await?;
    if current.is_some()
        && (s.is_none() || v.get("expected_recommended_version_id") != Some(&json!(current)))
    {
        return Err(err(
            StatusCode::CONFLICT,
            "Recommendation changed; reload and confirm",
        ));
    }
    let id = Uuid::new_v4();
    let object = id.to_string();
    let path = a.storage.join(&object);
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Artifact storage failed"))?;
    use tokio::io::AsyncWriteExt;
    file.write_all(&b)
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Artifact write failed"))?;
    file.sync_all().await.map_err(|_| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Artifact durability failed",
        )
    })?;
    let version:Value=sqlx::query_scalar("INSERT INTO skill_versions(id,team_id,skill_id,version,artifact_key,sha256,size_bytes,file_manifest,release_notes,published_by) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) RETURNING to_jsonb(skill_versions)-'artifact_key'").bind(id).bind(t).bind(sid).bind(label).bind(object).bind(digest).bind(b.len() as i64).bind(manifest).bind(notes).bind(&u.sub).fetch_one(&mut *tx).await?;
    sqlx::query("UPDATE skills SET recommended_version_id=$3,updated_at=now(),revision=revision+1 WHERE team_id=$1 AND id=$2").bind(t).bind(sid).bind(id).execute(&mut *tx).await?;
    event(&mut tx, t, &u.sub, "version.published", id).await?;
    let result = json!({"skill_id":sid,"version":version});
    remember(&mut tx, &u.sub, t, &op, &k, &request_digest, &result).await?;
    tx.commit().await?;
    Ok(ok(result))
}
async fn version(a: &App, t: Uuid, s: Uuid, v: Uuid) -> Result<Value> {
    sqlx::query_scalar(
        "SELECT to_jsonb(v) FROM skill_versions v WHERE team_id=$1 AND skill_id=$2 AND id=$3",
    )
    .bind(t)
    .bind(s)
    .bind(v)
    .fetch_optional(&a.db)
    .await?
    .ok_or_else(|| err(StatusCode::NOT_FOUND, "Version not found"))
}
async fn bytes(a: &App, v: &Value) -> Result<Vec<u8>> {
    let key = v["artifact_key"]
        .as_str()
        .ok_or_else(|| err(StatusCode::INTERNAL_SERVER_ERROR, "Missing artifact key"))?;
    Uuid::parse_str(key)
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Invalid artifact key"))?;
    let b = tokio::fs::read(a.storage.join(key))
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Artifact unavailable"))?;
    if hash(&b) != v["sha256"] {
        return Err(err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Artifact integrity failure",
        ));
    }
    Ok(b)
}
async fn version_detail(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, s, v)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Value>> {
    member(&a, t, &u.sub).await?;
    let mut row = version(&a, t, s, v).await?;
    row.as_object_mut()
        .ok_or_else(|| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid version metadata",
            )
        })?
        .remove("artifact_key");
    Ok(ok(json!({"version":row})))
}
async fn artifact(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, s, v)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Response> {
    member(&a, t, &u.sub).await?;
    let v = version(&a, t, s, v).await?;
    let b = bytes(&a, &v).await?;
    Ok((
        [
            ("content-type", "application/gzip"),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        b,
    )
        .into_response())
}
async fn files(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, s, v)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<Json<Value>> {
    member(&a, t, &u.sub).await?;
    let v = version(&a, t, s, v).await?;
    Ok(ok(json!({"files":v["file_manifest"]})))
}
#[derive(Deserialize)]
struct FileQuery {
    path: String,
}
async fn file(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, s, v)): Path<(Uuid, Uuid, Uuid)>,
    Query(q): Query<FileQuery>,
) -> Result<Json<Value>> {
    member(&a, t, &u.sub).await?;
    let v = version(&a, t, s, v).await?;
    let entry = v["file_manifest"]
        .as_array()
        .and_then(|m| m.iter().find(|f| f["path"] == q.path))
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "File not found"))?;
    if entry["size"].as_u64().unwrap_or(u64::MAX) > 256 * 1024 {
        return Ok(ok(json!({"file":entry,"text":null,"preview":"too_large"})));
    }
    let b = bytes(&a, &v).await?;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(b.as_slice()));
    for e in ar
        .entries()
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Artifact damaged"))?
    {
        let mut e = e.map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Artifact damaged"))?;
        if e.path().map_err(|_| bad("Invalid path"))?.to_str() == Some(&q.path) {
            let mut b = Vec::new();
            e.read_to_end(&mut b)
                .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "Artifact read failed"))?;
            return Ok(ok(json!({"file":entry,"text":String::from_utf8(b).ok()})));
        }
    }
    Err(err(StatusCode::INTERNAL_SERVER_ERROR, "Manifest mismatch"))
}
#[cfg(test)]
mod tests;
