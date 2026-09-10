use super::{App, Identity, Result, err};
use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{
    DecodingKey, Validation, decode, decode_header,
    jwk::{Jwk, JwkSet},
};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const CACHE_TTL: Duration = Duration::from_secs(300);
const REFRESH_COOLDOWN: Duration = Duration::from_secs(30);
const MAX_JWKS_BYTES: usize = 1024 * 1024;

fn validate_jwks_url(url: &reqwest::Url) -> anyhow::Result<()> {
    let loopback = url
        .host_str()
        .and_then(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .is_some_and(|ip| ip.is_loopback());
    anyhow::ensure!(
        (url.scheme() == "https" || (url.scheme() == "http" && loopback))
            && url.username().is_empty()
            && url.password().is_none(),
        "JWKS URL must use HTTPS (HTTP is allowed only for loopback IPs), without credentials"
    );
    Ok(())
}

/// A single configured authority. Token-provided URLs never select a key source.
pub struct JwksCache {
    source: Source,
    state: Mutex<CacheState>,
}
struct CacheState {
    keys: JwkSet,
    loaded_at: Instant,
    attempted_at: Option<Instant>,
}
enum Source {
    Http {
        client: reqwest::Client,
        url: reqwest::Url,
    },
    #[cfg(test)]
    Fixture {
        next: std::sync::Mutex<JwkSet>,
        calls: std::sync::atomic::AtomicUsize,
    },
}
impl Source {
    async fn fetch(&self) -> anyhow::Result<JwkSet> {
        match self {
            Self::Http { client, url } => {
                let mut response = client.get(url.clone()).send().await?.error_for_status()?;
                anyhow::ensure!(
                    response
                        .content_length()
                        .is_none_or(|n| n <= MAX_JWKS_BYTES as u64),
                    "JWKS too large"
                );
                let mut bytes = Vec::new();
                while let Some(chunk) = response.chunk().await? {
                    anyhow::ensure!(
                        bytes.len() + chunk.len() <= MAX_JWKS_BYTES,
                        "JWKS too large"
                    );
                    bytes.extend_from_slice(&chunk);
                }
                let keys: JwkSet = serde_json::from_slice(&bytes)?;
                anyhow::ensure!(!keys.keys.is_empty(), "JWKS contains no keys");
                Ok(keys)
            }
            #[cfg(test)]
            Self::Fixture { next, calls } => {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tokio::task::yield_now().await;
                Ok(next.lock().unwrap().clone())
            }
        }
    }
}
impl JwksCache {
    pub async fn from_url(url: &str) -> anyhow::Result<Self> {
        let url = reqwest::Url::parse(url)?;
        validate_jwks_url(&url)?;
        // Never follow redirects, including redirects from a loopback authority.
        let source = Source::Http {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            url,
        };
        let keys = source.fetch().await?;
        Ok(Self {
            source,
            state: Mutex::new(CacheState {
                keys,
                loaded_at: Instant::now(),
                attempted_at: None,
            }),
        })
    }

    #[cfg(test)]
    pub(crate) fn fixture(keys: JwkSet) -> Self {
        Self {
            source: Source::Fixture {
                next: std::sync::Mutex::new(keys.clone()),
                calls: std::sync::atomic::AtomicUsize::new(0),
            },
            state: Mutex::new(CacheState {
                keys,
                loaded_at: Instant::now(),
                attempted_at: None,
            }),
        }
    }

    async fn key(&self, kid: &str) -> Result<Jwk> {
        // Holding one mutex through refresh coalesces concurrent unknown-kid requests.
        let mut state = self.state.lock().await;
        let stale = state.loaded_at.elapsed() >= CACHE_TTL;
        if !stale {
            if let Some(key) = state.keys.find(kid) {
                return Ok(key.clone());
            }
        }
        let cooldown = state
            .attempted_at
            .is_some_and(|at| at.elapsed() < REFRESH_COOLDOWN);
        if !cooldown {
            state.attempted_at = Some(Instant::now());
            let keys =
                self.source.fetch().await.map_err(|_| {
                    err(StatusCode::SERVICE_UNAVAILABLE, "Signing keys unavailable")
                })?;
            state.keys = keys;
            state.loaded_at = Instant::now();
        } else if stale {
            return Err(err(
                StatusCode::SERVICE_UNAVAILABLE,
                "Signing keys unavailable; retry later",
            ));
        }
        state
            .keys
            .find(kid)
            .cloned()
            .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Unknown signing key"))
    }
}

async fn verify(token: &str, issuer: &str, audience: &str, cache: &JwksCache) -> Result<Identity> {
    let header =
        decode_header(token).map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid token"))?;
    if !matches!(
        header.alg,
        jsonwebtoken::Algorithm::RS256 | jsonwebtoken::Algorithm::ES256
    ) {
        return Err(err(StatusCode::UNAUTHORIZED, "Unsupported token algorithm"));
    }
    let kid = header
        .kid
        .as_deref()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Signing key id required"))?;
    let jwk = cache.key(kid).await?;
    let key = DecodingKey::from_jwk(&jwk)
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid signing key"))?;
    let mut validation = Validation::new(header.alg);
    validation.validate_nbf = true;
    validation.leeway = 0;
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);
    validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
    let identity = decode::<Identity>(token, &key, &validation)
        .map_err(|_| err(StatusCode::UNAUTHORIZED, "Invalid or expired token"))?
        .claims;
    if identity.sub.is_empty() {
        return Err(err(StatusCode::UNAUTHORIZED, "Missing subject"));
    }
    Ok(identity)
}

pub(super) async fn auth(
    State(app): State<App>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response> {
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "Bearer token required"))?;
    let identity = verify(token, &app.issuer, &app.audience, &app.jwks).await?;
    req.extensions_mut().insert(identity);
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    #[test]
    fn jwks_transport_rejects_remote_http_and_credentials() {
        for raw in [
            "https://auth.example.test/jwks",
            "http://127.0.0.1:5574/jwks",
            "http://[::1]:5574/jwks",
        ] {
            assert!(super::validate_jwks_url(&reqwest::Url::parse(raw).unwrap()).is_ok());
        }
        for raw in [
            "http://auth.example.test/jwks",
            "http://127.0.0.1.example.test/jwks",
            "http://192.168.1.1/jwks",
            "http://user:pass@127.0.0.1/jwks",
            "ftp://127.0.0.1/jwks",
        ] {
            assert!(super::validate_jwks_url(&reqwest::Url::parse(raw).unwrap()).is_err());
        }
    }
    use super::*;
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use p256::{
        SecretKey,
        elliptic_curve::sec1::ToEncodedPoint,
        pkcs8::{EncodePrivateKey, LineEnding},
    };
    use serde_json::json;
    use std::sync::atomic::Ordering;

    fn signing_key(seed: u8, kid: &str) -> (EncodingKey, JwkSet) {
        // Deterministic test-only keys, constructed in memory; never production credentials.
        let key = SecretKey::from_slice(&[seed; 32]).unwrap();
        let public = key.public_key().to_encoded_point(false);
        let jwk = json!({"keys":[{"kty":"EC","crv":"P-256","alg":"ES256","use":"sig","kid":kid,"x":URL_SAFE_NO_PAD.encode(public.x().unwrap()),"y":URL_SAFE_NO_PAD.encode(public.y().unwrap())}]});
        (
            EncodingKey::from_ec_pem(key.to_pkcs8_pem(LineEnding::LF).unwrap().as_bytes()).unwrap(),
            serde_json::from_value(jwk).unwrap(),
        )
    }
    fn token(key: &EncodingKey, kid: &str, nbf: u64) -> String {
        let now = jsonwebtoken::get_current_timestamp();
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(kid.into());
        encode(&header, &json!({"iss":"https://issuer.test","aud":"loom","sub":"alice","exp":now+3600,"nbf":nbf}), key).unwrap()
    }
    #[tokio::test]
    async fn signed_future_nbf_rejected_and_current_token_accepted() {
        let (key, jwks) = signing_key(7, "first");
        let cache = JwksCache::fixture(jwks);
        let now = jsonwebtoken::get_current_timestamp();
        assert!(
            verify(
                &token(&key, "first", now - 1),
                "https://issuer.test",
                "loom",
                &cache
            )
            .await
            .is_ok()
        );
        let error = verify(
            &token(&key, "first", now + 600),
            "https://issuer.test",
            "loom",
            &cache,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.0, StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn rotation_refreshes_once_and_unknown_kids_are_coalesced() {
        let (_, old) = signing_key(7, "old");
        let (new_key, new) = signing_key(9, "new");
        let cache = JwksCache::fixture(old);
        if let Source::Fixture { next, .. } = &cache.source {
            *next.lock().unwrap() = new;
        }
        let signed = token(&new_key, "new", 0);
        let (a, b) = tokio::join!(
            verify(&signed, "https://issuer.test", "loom", &cache),
            verify(&signed, "https://issuer.test", "loom", &cache)
        );
        assert!(a.is_ok() && b.is_ok());
        for n in 0..20 {
            assert!(cache.key(&format!("attacker-{n}")).await.is_err());
        }
        if let Source::Fixture { calls, .. } = &cache.source {
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
        // TTL refresh also removes a formerly known key after authority revocation.
        {
            let mut state = cache.state.lock().await;
            state.loaded_at = Instant::now() - CACHE_TTL;
            state.attempted_at = Some(Instant::now() - REFRESH_COOLDOWN);
        }
        let (_, replacement) = signing_key(11, "replacement");
        if let Source::Fixture { next, .. } = &cache.source {
            *next.lock().unwrap() = replacement;
        }
        assert_eq!(
            cache.key("new").await.err().unwrap().0,
            StatusCode::UNAUTHORIZED
        );
        if let Source::Fixture { calls, .. } = &cache.source {
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }
    }
}
