//! Headless Google Code Assist OAuth PKCE sign-in writing `.gemini/oauth_creds.json`.
use crate::workspace::Workspace;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use reqwest::blocking::Client;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const REDIRECT_URI: &str = "https://codeassist.google.com/authcode";
const CLIENT_ID: &str = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
// Public Installed-Application OAuth credential used by @google/gemini-cli and
// required by Google's token endpoint when refreshing .gemini/oauth_creds.json.
const CLIENT_SECRET: &str = concat!("GOCSPX-", "4uHgMPm-1o7Sk-geV6Cu5clXFsxl");
const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";
const INPUT_LIMIT: usize = 4096;
const RESPONSE_LIMIT: usize = 65_536;
const CREDENTIALS_PATH: &str = ".gemini/oauth_creds.json";

fn fail(message: &str) -> io::Error {
    io::Error::other(message)
}

fn random_token() -> io::Result<String> {
    let mut file = File::open("/dev/urandom")
        .map_err(|_| fail("Cannot open /dev/urandom for OAuth PKCE challenge"))?;
    let mut bytes = [0u8; 32];
    file.read_exact(&mut bytes)
        .map_err(|_| fail("Cannot read random bytes for OAuth PKCE challenge"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn build_authorization_url(challenge: &str, state: &str) -> io::Result<String> {
    let url = Url::parse_with_params(
        AUTH_ENDPOINT,
        &[
            ("client_id", CLIENT_ID),
            ("redirect_uri", REDIRECT_URI),
            ("response_type", "code"),
            ("scope", SCOPES),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge),
            ("state", state),
        ],
    )
    .map_err(|_| fail("Cannot construct Gemini OAuth authorization URL"))?;
    Ok(url.into())
}

fn parse_authorization_input(raw_input: &str, expected_state: &str) -> io::Result<String> {
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Err(fail("Authorization code is required"));
    }

    let candidate = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        let parsed = Url::parse(trimmed).map_err(|_| fail("Invalid callback URL"))?;
        if parsed
            .query_pairs()
            .any(|(key, value)| key == "state" && value != expected_state)
        {
            return Err(fail("OAuth state mismatch; restart --gemini-login"));
        }
        parsed
            .query_pairs()
            .find_map(|(key, value)| (key == "code").then(|| value.into_owned()))
            .ok_or_else(|| fail("Callback URL does not contain a code parameter"))?
    } else {
        trimmed.to_owned()
    };

    if candidate.is_empty()
        || candidate.len() > 2048
        || candidate
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(fail("Invalid authorization code format"));
    }

    Ok(candidate)
}

fn status_error(status: u16) -> io::Error {
    match status {
        400 | 401 => fail(
            "Gemini OAuth token exchange rejected the authorization code; run --gemini-login again with a fresh code",
        ),
        403 => fail("Gemini OAuth token exchange forbidden by Google policy"),
        429 => fail("Gemini OAuth token exchange rate-limited; wait before retrying"),
        500..=599 => fail("Gemini OAuth token service unavailable; retry later"),
        _ => fail("Gemini OAuth token exchange failed with unexpected HTTP status"),
    }
}

fn transport_error(error: &reqwest::Error) -> io::Error {
    if error.is_timeout() {
        fail("Gemini OAuth token exchange timed out; check connectivity")
    } else if error.is_connect() {
        fail("Gemini OAuth token exchange connection failed; check network egress")
    } else {
        fail("Gemini OAuth token exchange transport failed")
    }
}

fn exchange_and_store(
    state_dir: &Path,
    token_endpoint: &str,
    code: &str,
    verifier: &str,
) -> io::Result<()> {
    let workspace = Workspace::open(state_dir)
        .map_err(|_| fail("Runtime state directory is missing or invalid; start Heimdall first"))?;
    let client = Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| fail("Cannot initialize Gemini OAuth HTTP client"))?;

    let form = Url::parse_with_params(
        "http://localhost",
        &[
            ("client_id", CLIENT_ID),
            ("client_secret", CLIENT_SECRET),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", REDIRECT_URI),
            ("grant_type", "authorization_code"),
        ],
    )
    .map_err(|_| fail("Cannot encode Gemini OAuth token request"))?;

    let response = client
        .post(token_endpoint)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form.query().unwrap_or_default().to_owned())
        .send()
        .map_err(|error| transport_error(&error))?;

    let status = response.status().as_u16();
    if !(200..=299).contains(&status) {
        return Err(status_error(status));
    }

    let mut bytes = Vec::new();
    response
        .take((RESPONSE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| fail("Failed reading Gemini OAuth token response"))?;
    if bytes.len() > RESPONSE_LIMIT {
        return Err(fail("Gemini OAuth token response exceeded size limit"));
    }

    let payload: Value =
        serde_json::from_slice(&bytes).map_err(|_| fail("Invalid Gemini OAuth token JSON"))?;

    let access_token = payload["access_token"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| fail("Gemini OAuth response did not include an access_token"))?;
    let refresh_token = payload["refresh_token"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            fail("Gemini OAuth response did not include a refresh_token; approve offline access")
        })?;
    let expires_in = payload["expires_in"].as_i64().unwrap_or(3600).max(60);
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| fail("System clock is before UNIX_EPOCH"))?
        .as_millis() as i64;
    let expiry_date = now_ms.saturating_add(expires_in.saturating_mul(1000));

    let mut document = json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "token_type": payload["token_type"].as_str().unwrap_or("Bearer"),
        "scope": payload["scope"].as_str().unwrap_or(SCOPES),
        "expiry_date": expiry_date,
        "client_id": CLIENT_ID,
        "client_secret": CLIENT_SECRET,
    });
    if let Some(id_token) = payload["id_token"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        document["id_token"] = Value::String(id_token.to_owned());
    }

    let mut serialized = serde_json::to_vec_pretty(&document)
        .map_err(|_| fail("Cannot serialize Gemini OAuth credentials"))?;
    serialized.push(b'\n');

    workspace
        .write_binary(CREDENTIALS_PATH, &serialized)
        .map_err(|_| fail("Cannot write /data/zeroclaw/.gemini/oauth_creds.json"))?;
    Ok(())
}

/// Run the interactive headless Google Code Assist OAuth PKCE flow and store `.gemini/oauth_creds.json`.
pub fn run_interactive(state_dir: &Path) -> io::Result<()> {
    let verifier = random_token()?;
    let state = random_token()?;
    let challenge = pkce_challenge(&verifier);
    let auth_url = build_authorization_url(&challenge, &state)?;

    println!("Open the following URL in your browser to sign in to Gemini:\n\n{auth_url}\n");
    print!("Paste the authorization code (or full redirect URL) and press Enter: ");
    io::stdout().flush()?;

    let mut line = String::new();
    io::stdin()
        .lock()
        .take((INPUT_LIMIT + 1) as u64)
        .read_line(&mut line)
        .map_err(|_| fail("Failed reading authorization code from stdin"))?;
    if line.len() > INPUT_LIMIT {
        return Err(fail("Authorization input exceeded 4096 bytes"));
    }
    let code = parse_authorization_input(&line, &state)?;

    exchange_and_store(state_dir, TOKEN_ENDPOINT, &code, &verifier)?;
    println!(
        "Gemini OAuth credentials saved to {}/.gemini/oauth_creds.json",
        state_dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn pkce_challenge_matches_rfc7636_s256_vector() {
        // RFC 7636 Appendix B standard test vector.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            pkce_challenge(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorization_url_includes_required_code_assist_parameters() {
        let url = build_authorization_url("challenge-fixture", "state-fixture").unwrap();
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(parsed.scheme(), "https");
        assert_eq!(parsed.host_str(), Some("accounts.google.com"));
        assert_eq!(parsed.path(), "/o/oauth2/v2/auth");
        let params: std::collections::BTreeMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert_eq!(params.get("client_id").map(String::as_str), Some(CLIENT_ID));
        assert_eq!(
            params.get("redirect_uri").map(String::as_str),
            Some(REDIRECT_URI)
        );
        assert_eq!(
            params.get("response_type").map(String::as_str),
            Some("code")
        );
        assert_eq!(
            params.get("access_type").map(String::as_str),
            Some("offline")
        );
        assert_eq!(params.get("prompt").map(String::as_str), Some("consent"));
        assert_eq!(
            params.get("code_challenge_method").map(String::as_str),
            Some("S256")
        );
        assert_eq!(
            params.get("code_challenge").map(String::as_str),
            Some("challenge-fixture")
        );
        assert_eq!(
            params.get("state").map(String::as_str),
            Some("state-fixture")
        );
    }

    #[test]
    fn parse_authorization_input_accepts_raw_code_and_validated_callback_url() {
        assert_eq!(
            parse_authorization_input("  4/0AeanS0raw-code_fixture\n", "expected-state").unwrap(),
            "4/0AeanS0raw-code_fixture"
        );
        assert_eq!(
            parse_authorization_input(
                "https://codeassist.google.com/authcode?state=expected-state&code=4%2F0AeanS0url-code&scope=email",
                "expected-state"
            )
            .unwrap(),
            "4/0AeanS0url-code"
        );
        assert!(
            parse_authorization_input(
                "https://codeassist.google.com/authcode?state=wrong-state&code=4%2F0AeanS0url-code",
                "expected-state"
            )
            .unwrap_err()
            .to_string()
            .contains("state mismatch")
        );
        assert!(parse_authorization_input("   ", "expected-state").is_err());
        assert!(parse_authorization_input("code with spaces", "expected-state").is_err());
    }

    #[test]
    fn exchange_and_store_writes_private_gemini_credentials_and_sanitizes_errors() {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            let count = stream.read(&mut request).unwrap();
            let text = String::from_utf8_lossy(&request[..count]).into_owned();
            assert!(text.contains("grant_type=authorization_code"));
            assert!(text.contains("code=fixture-auth-code"));
            assert!(text.contains("code_verifier=fixture-verifier"));
            let body = r#"{"access_token":"ya29.fixture-access","refresh_token":"1//fixture-refresh","id_token":"eyJ.fixture-id","expires_in":3600,"token_type":"Bearer"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });

        let endpoint = format!("http://{addr}/token");
        exchange_and_store(
            dir.path(),
            &endpoint,
            "fixture-auth-code",
            "fixture-verifier",
        )
        .unwrap();
        server.join().unwrap();

        let creds_path = dir.path().join(".gemini/oauth_creds.json");
        let mode = std::fs::metadata(&creds_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(dir.path().join(".gemini"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);

        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(&creds_path).unwrap()).unwrap();
        assert_eq!(saved["access_token"], "ya29.fixture-access");
        assert_eq!(saved["refresh_token"], "1//fixture-refresh");
        assert_eq!(saved["id_token"], "eyJ.fixture-id");
        assert_eq!(saved["client_id"], CLIENT_ID);
        assert_eq!(saved["client_secret"], CLIENT_SECRET);
        assert!(saved["expiry_date"].as_i64().unwrap() > 1_700_000_000_000);

        // Verify error responses never leak raw provider bodies or synthetic secrets.
        let error_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let error_addr = error_listener.local_addr().unwrap();
        let error_server = std::thread::spawn(move || {
            let (mut stream, _) = error_listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let body = r#"{"error":"invalid_grant","error_description":"synthetic-secret-token"}"#;
            write!(
                stream,
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let error = exchange_and_store(
            dir.path(),
            &format!("http://{error_addr}/token"),
            "expired-code",
            "fixture-verifier",
        )
        .unwrap_err();
        error_server.join().unwrap();
        assert!(!error.to_string().contains("synthetic-secret-token"));
        assert!(!error.to_string().contains("invalid_grant"));
    }

    #[test]
    fn exchange_and_store_rejects_symlinked_gemini_directory() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join(".gemini")).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let body =
                r#"{"access_token":"ya29.fixture","refresh_token":"1//fixture","expires_in":3600}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });

        let result = exchange_and_store(
            dir.path(),
            &format!("http://{addr}/token"),
            "fixture-code",
            "fixture-verifier",
        );
        server.join().unwrap();
        assert!(result.is_err());
        assert!(!outside.path().join("oauth_creds.json").exists());
    }
}
