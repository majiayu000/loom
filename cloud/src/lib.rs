use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
};
use jsonwebtoken::{DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::{
    io::Read,
    path::{Component, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

pub const MAX_UPLOAD: usize = 10 * 1024 * 1024;
#[derive(Clone)]
pub struct App {
    pub db: PgPool,
    pub storage: PathBuf,
    pub issuer: String,
    pub audience: String,
    pub jwks: Arc<JwkSet>,
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
async fn auth(
    State(a): State<App>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response> {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Bearer token required"))?;
    let h = decode_header(token).map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid token"))?;
    if !matches!(
        h.alg,
        jsonwebtoken::Algorithm::RS256 | jsonwebtoken::Algorithm::ES256
    ) {
        return Err(err(StatusCode::UNAUTHORIZED, "Unsupported token algorithm"));
    }
    let jwk = a
        .jwks
        .find(h.kid.as_deref().unwrap_or(""))
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Unknown signing key"))?;
    let key = DecodingKey::from_jwk(jwk)
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid signing key"))?;
    let mut v = Validation::new(h.alg);
    v.set_issuer(&[&a.issuer]);
    v.set_audience(&[&a.audience]);
    v.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    let identity = decode::<Identity>(token, &key, &v)
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid or expired token"))?
        .claims;
    if identity.sub.is_empty() {
        return Err(err(StatusCode::UNAUTHORIZED, "Missing subject"));
    }
    req.extensions_mut().insert(identity);
    Ok(next.run(req).await)
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
async fn member(a: &App, t: Uuid, u: &str) -> Result<String> {
    sqlx::query_scalar("SELECT owner_user_id FROM teams JOIN memberships ON teams.id=memberships.team_id WHERE teams.id=$1 AND user_id=$2").bind(t).bind(u).fetch_optional(&a.db).await?.ok_or_else(||err(StatusCode::NOT_FOUND,"Team not found"))
}
async fn locked<'a>(a: &'a App, t: Uuid, u: &str) -> Result<(Transaction<'a, Postgres>, String)> {
    let mut tx = a.db.begin().await?;
    let owner: String =
        sqlx::query_scalar("SELECT owner_user_id FROM teams WHERE id=$1 FOR UPDATE")
            .bind(t)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "Team not found"))?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM memberships WHERE team_id=$1 AND user_id=$2)",
    )
    .bind(t)
    .bind(u)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(err(StatusCode::NOT_FOUND, "Team not found"));
    }
    Ok((tx, owner))
}
fn owner(o: &str, u: &str) -> Result<()> {
    if o != u {
        Err(err(StatusCode::FORBIDDEN, "Owner required"))
    } else {
        Ok(())
    }
}
async fn event(
    tx: &mut Transaction<'_, Postgres>,
    t: Uuid,
    u: &str,
    action: &str,
    id: Uuid,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO team_events(id,team_id,actor_id,action,subject_id) VALUES($1,$2,$3,$4,$5)",
    )
    .bind(Uuid::new_v4())
    .bind(t)
    .bind(u)
    .bind(action)
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
async fn teams(State(a): State<App>, axum::Extension(u): User) -> Result<Json<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',t.id,'name',t.name,'owner_user_id',t.owner_user_id) FROM teams t JOIN memberships m ON m.team_id=t.id WHERE m.user_id=$1 ORDER BY t.created_at,t.id").bind(u.sub).fetch_all(&a.db).await?;
    Ok(ok(json!({"teams":rows})))
}
fn required(v: &Value, k: &str) -> Result<String> {
    let s = v[k]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 8192)
        .ok_or_else(|| bad(&format!("Invalid {k}")))?;
    Ok(s.to_owned())
}
fn key(h: &HeaderMap) -> Result<String> {
    h.get("idempotency-key")
        .and_then(|s| s.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= 200)
        .map(str::to_owned)
        .ok_or_else(|| bad("Idempotency-Key required"))
}
async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    u: &str,
    t: Uuid,
    op: &str,
    k: &str,
    d: &str,
) -> Result<Option<Value>> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{u}:{t}:{op}:{k}"))
        .execute(&mut **tx)
        .await?;
    let r=sqlx::query("SELECT request_digest,result FROM idempotency_requests WHERE actor_id=$1 AND team_id=$2 AND operation=$3 AND key=$4 AND expires_at>now()").bind(u).bind(t).bind(op).bind(k).fetch_optional(&mut **tx).await?;
    if let Some(r) = r {
        if r.get::<String, _>(0) != d {
            return Err(err(
                StatusCode::CONFLICT,
                "Idempotency key reused with different content",
            ));
        }
        return Ok(Some(r.get(1)));
    }
    Ok(None)
}
async fn remember(
    tx: &mut Transaction<'_, Postgres>,
    u: &str,
    t: Uuid,
    op: &str,
    k: &str,
    d: &str,
    v: &Value,
) -> Result<()> {
    sqlx::query("INSERT INTO idempotency_requests(actor_id,team_id,operation,key,request_digest,result) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(actor_id,team_id,operation,key) DO UPDATE SET request_digest=EXCLUDED.request_digest,result=EXCLUDED.result,expires_at=EXCLUDED.expires_at").bind(u).bind(t).bind(op).bind(k).bind(d).bind(v).execute(&mut **tx).await?;
    Ok(())
}
async fn create_team(
    State(a): State<App>,
    axum::Extension(u): User,
    h: HeaderMap,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let name = required(&v, "name")?;
    let k = key(&h)?;
    let d = hash(v.to_string().as_bytes());
    let mut tx = a.db.begin().await?;
    if let Some(v) = replay(&mut tx, &u.sub, Uuid::nil(), "create-team", &k, &d).await? {
        return Ok(ok(v));
    }
    let t = Uuid::new_v4();
    sqlx::query("INSERT INTO teams(id,name,owner_user_id) VALUES($1,$2,$3)")
        .bind(t)
        .bind(&name)
        .bind(&u.sub)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships(team_id,user_id) VALUES($1,$2)")
        .bind(t)
        .bind(&u.sub)
        .execute(&mut *tx)
        .await?;
    event(&mut tx, t, &u.sub, "team.created", t).await?;
    let v = json!({"team":{"id":t,"name":name,"owner_user_id":u.sub}});
    remember(&mut tx, &u.sub, Uuid::nil(), "create-team", &k, &d, &v).await?;
    tx.commit().await?;
    Ok(ok(v))
}
async fn members(
    State(a): State<App>,
    axum::Extension(u): User,
    Path(t): Path<Uuid>,
) -> Result<Json<Value>> {
    member(&a, t, &u.sub).await?;
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(m) FROM memberships m WHERE team_id=$1 ORDER BY joined_at,user_id",
    )
    .bind(t)
    .fetch_all(&a.db)
    .await?;
    Ok(ok(json!({"members":rows})))
}
async fn remove_member(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, target)): Path<(Uuid, String)>,
) -> Result<Json<Value>> {
    let (mut tx, o) = locked(&a, t, &u.sub).await?;
    if target == o {
        return Err(err(StatusCode::CONFLICT, "Transfer ownership first"));
    }
    if target != u.sub {
        owner(&o, &u.sub)?
    }
    sqlx::query("UPDATE skills SET maintainer_id=$3,revision=revision+1,updated_at=now() WHERE team_id=$1 AND maintainer_id=$2").bind(t).bind(&target).bind(o).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM memberships WHERE team_id=$1 AND user_id=$2")
        .bind(t)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    event(&mut tx, t, &u.sub, "member.removed", t).await?;
    tx.commit().await?;
    Ok(ok(json!({})))
}
async fn transfer_owner(
    State(a): State<App>,
    axum::Extension(u): User,
    Path(t): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let target = required(&v, "user_id")?;
    let (mut tx, o) = locked(&a, t, &u.sub).await?;
    owner(&o, &u.sub)?;
    sqlx::query("UPDATE teams SET owner_user_id=$2 WHERE id=$1")
        .bind(t)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    event(&mut tx, t, &u.sub, "owner.transferred", t).await?;
    tx.commit().await?;
    Ok(ok(json!({})))
}
async fn invite(
    State(a): State<App>,
    axum::Extension(u): User,
    Path(t): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let email = required(&v, "email")?.trim().to_lowercase();
    if !email.contains('@') {
        return Err(bad("Invalid email"));
    }
    let (mut tx, o) = locked(&a, t, &u.sub).await?;
    owner(&o, &u.sub)?;
    let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO invitations(id,team_id,email,token_hash,expires_at,created_by) VALUES($1,$2,$3,$4,now()+interval '7 days',$5)").bind(id).bind(t).bind(email).bind(hash(token.as_bytes())).bind(&u.sub).execute(&mut *tx).await?;
    event(&mut tx, t, &u.sub, "invitation.created", id).await?;
    tx.commit().await?;
    Ok(ok(
        json!({"invitation":{"id":id,"token":token,"expires_in_seconds":604800}}),
    ))
}
async fn revoke(
    State(a): State<App>,
    axum::Extension(u): User,
    Path((t, id)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    let (mut tx, o) = locked(&a, t, &u.sub).await?;
    owner(&o, &u.sub)?;
    sqlx::query("UPDATE invitations SET revoked_at=now() WHERE team_id=$1 AND id=$2 AND accepted_at IS NULL").bind(t).bind(id).execute(&mut *tx).await?;
    event(&mut tx, t, &u.sub, "invitation.revoked", id).await?;
    tx.commit().await?;
    Ok(ok(json!({})))
}
async fn accept(
    State(a): State<App>,
    axum::Extension(u): User,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    if !u.email_verified {
        return Err(err(StatusCode::FORBIDDEN, "Verified email required"));
    }
    let token = required(&v, "token")?;
    let mut tx = a.db.begin().await?;
    let r=sqlx::query("SELECT id,team_id,email FROM invitations WHERE token_hash=$1 AND accepted_at IS NULL AND revoked_at IS NULL AND expires_at>now() FOR UPDATE").bind(hash(token.as_bytes())).fetch_optional(&mut *tx).await?.ok_or_else(||bad("Invitation invalid, used or expired"))?;
    let email: String = r.get("email");
    if u.email.as_deref().map(|e| e.trim().to_lowercase()) != Some(email) {
        return Err(err(
            StatusCode::FORBIDDEN,
            "Invitation email does not match",
        ));
    }
    let t: Uuid = r.get("team_id");
    let id: Uuid = r.get("id");
    sqlx::query("INSERT INTO memberships(team_id,user_id) VALUES($1,$2) ON CONFLICT DO NOTHING")
        .bind(t)
        .bind(&u.sub)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE invitations SET accepted_at=now() WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    event(&mut tx, t, &u.sub, "invitation.accepted", id).await?;
    tx.commit().await?;
    Ok(ok(json!({"team_id":t})))
}
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
mod tests {
    use super::*;
    fn pack(path: &str, kind: tar::EntryType, content: &[u8]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut b = tar::Builder::new(gz);
        let mut h = tar::Header::new_gnu();
        h.set_size(content.len() as u64);
        h.set_mode(0o644);
        h.set_entry_type(kind);
        h.set_cksum();
        b.append_data(&mut h, path, content).unwrap();
        b.into_inner().unwrap().finish().unwrap()
    }
    #[test]
    fn archive_boundary() {
        assert!(validate_archive(&pack("SKILL.md", tar::EntryType::Regular, b"hello")).is_ok());
        for p in [".env", ".git/config", "nested/SKILL.md"] {
            assert!(validate_archive(&pack(p, tar::EntryType::Regular, b"secret")).is_err());
        }
        assert!(validate_archive(&pack("SKILL.md", tar::EntryType::Symlink, b"")).is_err());
        assert!(validate_archive(&vec![0; MAX_UPLOAD + 1]).is_err());
    }
    #[tokio::test]
    async fn postgres_team_and_invitation_isolation() {
        let url = std::env::var("LOOM_TEST_DATABASE_URL")
            .expect("Set LOOM_TEST_DATABASE_URL to a disposable PostgreSQL database");
        let db = PgPool::connect(&url).await.unwrap();
        sqlx::migrate!("./migrations").run(&db).await.unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let a = App {
            db,
            storage: tmp.path().into(),
            issuer: "https://test.invalid".into(),
            audience: "test".into(),
            jwks: Arc::new(JwkSet { keys: vec![] }),
        };
        let alice = Identity {
            sub: Uuid::new_v4().to_string(),
            email: Some("alice@example.test".into()),
            email_verified: true,
        };
        let bob = Identity {
            sub: Uuid::new_v4().to_string(),
            email: Some("bob@example.test".into()),
            email_verified: true,
        };
        let mut h = HeaderMap::new();
        h.insert(
            "idempotency-key",
            Uuid::new_v4().to_string().parse().unwrap(),
        );
        let v = json!({"name":"Test team"});
        let first = create_team(
            State(a.clone()),
            axum::Extension(alice.clone()),
            h.clone(),
            Json(v.clone()),
        )
        .await
        .unwrap()
        .0["data"]
            .clone();
        let retry = create_team(
            State(a.clone()),
            axum::Extension(alice.clone()),
            h.clone(),
            Json(v),
        )
        .await
        .unwrap()
        .0["data"]
            .clone();
        assert_eq!(first, retry);
        assert_eq!(
            create_team(
                State(a.clone()),
                axum::Extension(alice.clone()),
                h,
                Json(json!({"name":"other"}))
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::CONFLICT
        );
        let t = first["team"]["id"].as_str().unwrap().parse().unwrap();
        assert!(member(&a, t, &bob.sub).await.is_err());
        let invitation = invite(
            State(a.clone()),
            axum::Extension(alice.clone()),
            Path(t),
            Json(json!({"email":"bob@example.test"})),
        )
        .await
        .unwrap()
        .0["data"]["invitation"]
            .clone();
        let token = json!({"token":invitation["token"]});
        assert_eq!(
            accept(
                State(a.clone()),
                axum::Extension(alice.clone()),
                Json(token.clone())
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::FORBIDDEN
        );
        let _ = accept(
            State(a.clone()),
            axum::Extension(bob.clone()),
            Json(token.clone()),
        )
        .await
        .unwrap();
        assert!(
            accept(State(a.clone()), axum::Extension(bob.clone()), Json(token))
                .await
                .is_err()
        );
        assert!(member(&a, t, &bob.sub).await.is_ok());
        assert!(
            remove_member(
                State(a.clone()),
                axum::Extension(alice.clone()),
                Path((t, alice.sub.clone()))
            )
            .await
            .is_err()
        );
        let _ = remove_member(
            State(a.clone()),
            axum::Extension(alice.clone()),
            Path((t, bob.sub.clone())),
        )
        .await
        .unwrap();
        assert!(member(&a, t, &bob.sub).await.is_err());
        use tower::ServiceExt;
        let response = router(a.clone())
            .oneshot(
                Request::builder()
                    .uri(format!("/v1/teams/{t}/members"))
                    .header("x-user-id", &alice.sub)
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let (r1, r2) = tokio::join!(
            transfer_owner(
                State(a.clone()),
                axum::Extension(alice.clone()),
                Path(t),
                Json(json!({"user_id":bob.sub}))
            ),
            remove_member(
                State(a.clone()),
                axum::Extension(alice.clone()),
                Path((t, bob.sub.clone()))
            )
        );
        assert!(r1.is_err());
        assert!(r2.is_ok());
    }
    #[tokio::test]
    async fn postgres_publish_concurrency_and_private_download() {
        use http_body_util::BodyExt;
        use tower::ServiceExt;
        let db = PgPool::connect(
            &std::env::var("LOOM_TEST_DATABASE_URL").expect("disposable PostgreSQL required"),
        )
        .await
        .unwrap();
        sqlx::migrate!("./migrations").run(&db).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let a = App {
            db,
            storage: dir.path().into(),
            issuer: "test".into(),
            audience: "test".into(),
            jwks: Arc::new(JwkSet { keys: vec![] }),
        };
        let u = Identity {
            sub: Uuid::new_v4().to_string(),
            email: None,
            email_verified: false,
        };
        let mut h = HeaderMap::new();
        h.insert(
            "idempotency-key",
            Uuid::new_v4().to_string().parse().unwrap(),
        );
        let t: Uuid = create_team(
            State(a.clone()),
            axum::Extension(u.clone()),
            h,
            Json(json!({"name":"Publisher"})),
        )
        .await
        .unwrap()
        .0["data"]["team"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        // Identity injection exists only within this test; the production router always verifies JWTs.
        let app = Router::new()
            .route("/v1/teams/{team}/skills", post(publish_first))
            .route(
                "/v1/teams/{team}/skills/{skill}/versions",
                post(publish_next),
            )
            .layer(axum::Extension(u.clone()))
            .with_state(a.clone());
        fn request(uri: String, metadata: Value, k: &str) -> Request<axum::body::Body> {
            let mut b=format!("--BOUNDARY\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{metadata}\r\n--BOUNDARY\r\nContent-Disposition: form-data; name=\"artifact\"; filename=\"skill.tar.gz\"\r\nContent-Type: application/gzip\r\n\r\n").into_bytes();
            b.extend(pack("SKILL.md", tar::EntryType::Regular, b"# Skill"));
            b.extend(b"\r\n--BOUNDARY--\r\n");
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "multipart/form-data; boundary=BOUNDARY")
                .header("idempotency-key", k)
                .body(axum::body::Body::from(b))
                .unwrap()
        }
        let metadata = json!({"slug":"review","title":"Review","description":"Useful review","example":"Review changes","version":"0.1.0","release_notes":"First"});
        let r = app
            .clone()
            .oneshot(request(
                format!("/v1/teams/{t}/skills"),
                metadata.clone(),
                "first",
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v: Value =
            serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        let sid: Uuid = v["data"]["skill_id"].as_str().unwrap().parse().unwrap();
        let vid: Uuid = v["data"]["version"]["id"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let r = app
            .clone()
            .oneshot(request(format!("/v1/teams/{t}/skills"), metadata, "first"))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let next = json!({"version":"0.2.0","release_notes":"Second","expected_recommended_version_id":vid});
        let other = json!({"version":"0.3.0","release_notes":"Third","expected_recommended_version_id":vid});
        let (x, y) = tokio::join!(
            app.clone().oneshot(request(
                format!("/v1/teams/{t}/skills/{sid}/versions"),
                next,
                "second"
            )),
            app.oneshot(request(
                format!("/v1/teams/{t}/skills/{sid}/versions"),
                other,
                "third"
            ))
        );
        let statuses = [x.unwrap().status(), y.unwrap().status()];
        assert!(statuses.contains(&StatusCode::OK));
        assert!(statuses.contains(&StatusCode::CONFLICT));
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM skill_versions WHERE skill_id=$1")
            .bind(sid)
            .fetch_one(&a.db)
            .await
            .unwrap();
        assert_eq!(n, 2);
        assert!(
            artifact(
                State(a.clone()),
                axum::Extension(u.clone()),
                Path((t, sid, vid))
            )
            .await
            .is_ok()
        );
        let outsider = Identity {
            sub: Uuid::new_v4().to_string(),
            email: None,
            email_verified: false,
        };
        assert_eq!(
            artifact(
                State(a.clone()),
                axum::Extension(outsider),
                Path((t, sid, vid))
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::NOT_FOUND
        );
        assert!(version(&a, Uuid::new_v4(), sid, vid).await.is_err());
        let stored = version(&a, t, sid, vid).await.unwrap();
        tokio::fs::write(
            a.storage.join(stored["artifact_key"].as_str().unwrap()),
            b"tampered",
        )
        .await
        .unwrap();
        assert_eq!(
            artifact(State(a), axum::Extension(u), Path((t, sid, vid)))
                .await
                .unwrap_err()
                .0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
