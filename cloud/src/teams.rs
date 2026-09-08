use super::*;

pub(super) async fn member(a: &App, t: Uuid, u: &str) -> Result<String> {
    sqlx::query_scalar("SELECT owner_user_id FROM teams JOIN memberships ON teams.id=memberships.team_id WHERE teams.id=$1 AND user_id=$2").bind(t).bind(u).fetch_optional(&a.db).await?.ok_or_else(||err(StatusCode::NOT_FOUND,"Team not found"))
}
pub(super) async fn locked<'a>(
    a: &'a App,
    t: Uuid,
    u: &str,
) -> Result<(Transaction<'a, Postgres>, String)> {
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
pub(super) fn owner(o: &str, u: &str) -> Result<()> {
    if o != u {
        Err(err(StatusCode::FORBIDDEN, "Owner required"))
    } else {
        Ok(())
    }
}
pub(super) async fn event(
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
pub(super) async fn teams(State(a): State<App>, axum::Extension(u): User) -> Result<Json<Value>> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT jsonb_build_object('id',t.id,'name',t.name,'owner_user_id',t.owner_user_id) FROM teams t JOIN memberships m ON m.team_id=t.id WHERE m.user_id=$1 ORDER BY t.created_at,t.id").bind(u.sub).fetch_all(&a.db).await?;
    Ok(ok(json!({"teams":rows})))
}
pub(super) fn required(v: &Value, k: &str) -> Result<String> {
    let s = v[k]
        .as_str()
        .filter(|s| !s.trim().is_empty() && s.len() <= 8192)
        .ok_or_else(|| bad(&format!("Invalid {k}")))?;
    Ok(s.to_owned())
}
pub(super) fn key(h: &HeaderMap) -> Result<String> {
    h.get("idempotency-key")
        .and_then(|s| s.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= 200)
        .map(str::to_owned)
        .ok_or_else(|| bad("Idempotency-Key required"))
}
pub(super) async fn replay(
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
pub(super) async fn remember(
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
pub(super) async fn create_team(
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
pub(super) async fn members(
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
pub(super) async fn remove_member(
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
pub(super) async fn transfer_owner(
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
pub(super) async fn invite(
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
pub(super) async fn revoke(
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
pub(super) async fn accept(
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
