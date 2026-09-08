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
        jwks: Arc::new(JwksCache::fixture(jsonwebtoken::jwk::JwkSet {
            keys: vec![],
        })),
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
        jwks: Arc::new(JwksCache::fixture(jsonwebtoken::jwk::JwkSet {
            keys: vec![],
        })),
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
    let next =
        json!({"version":"0.2.0","release_notes":"Second","expected_recommended_version_id":vid});
    let other =
        json!({"version":"0.3.0","release_notes":"Third","expected_recommended_version_id":vid});
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
    let metadata = version_detail(
        State(a.clone()),
        axum::Extension(u.clone()),
        Path((t, sid, vid)),
    )
    .await
    .unwrap()
    .0;
    assert_eq!(metadata["data"]["version"]["id"], vid.to_string());
    assert_eq!(
        metadata["data"]["version"]["sha256"],
        hash(&pack("SKILL.md", tar::EntryType::Regular, b"# Skill"))
    );
    assert!(metadata["data"]["version"].get("artifact_key").is_none());
    assert_eq!(
        version_detail(
            State(a.clone()),
            axum::Extension(u.clone()),
            Path((Uuid::new_v4(), sid, vid))
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::NOT_FOUND
    );

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
