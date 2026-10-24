//! Headless Google Code Assist OAuth PKCE sign-in and onboarding writing `.gemini/oauth_creds.json`.
use crate::workspace::Workspace;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use reqwest::blocking::{Client, Response};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const CODE_ASSIST_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const REDIRECT_URI: &str = "https://codeassist.google.com/authcode";
const CLIENT_ID: &str = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
// Public Installed-Application OAuth credential used by @google/gemini-cli and
// required by Google's token endpoint when refreshing .gemini/oauth_creds.json.
const CLIENT_SECRET: &str = concat!("GOCSPX-", "4uHgMPm-1o7Sk-geV6Cu5clXFsxl");
const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";
const OAUTH_PROJECT_ENV: &str = "ZEROCLAW_providers__models__gemini__default__oauth_project";
const INPUT_LIMIT: usize = 4096;
const RESPONSE_LIMIT: usize = 65_536;
const ONBOARD_POLL_ATTEMPTS: usize = 12;
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

fn validate_project_seed(raw: Option<&str>) -> io::Result<Option<String>> {
    let Some(value) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > 128
        || value.chars().all(|ch| ch.is_ascii_digit())
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err(fail(
            "Invalid ZEROCLAW_providers__models__gemini__default__oauth_project; use a string Google Cloud Project ID",
        ));
    }
    Ok(Some(value.to_owned()))
}

fn status_error(status: u16, stage: &str) -> io::Error {
    match status {
        400 | 401 => fail(&format!(
            "Gemini {stage} rejected the credentials or request; run --gemini-login again"
        )),
        403 => fail(&format!("Gemini {stage} forbidden by Google policy")),
        429 => fail(&format!(
            "Gemini {stage} rate-limited; wait before retrying"
        )),
        500..=599 => fail(&format!("Gemini {stage} service unavailable; retry later")),
        _ => fail(&format!(
            "Gemini {stage} failed with unexpected HTTP status"
        )),
    }
}

fn transport_error(error: &reqwest::Error, stage: &str) -> io::Error {
    if error.is_timeout() {
        fail(&format!("Gemini {stage} timed out; check connectivity"))
    } else if error.is_connect() {
        fail(&format!(
            "Gemini {stage} connection failed; check network egress"
        ))
    } else {
        fail(&format!("Gemini {stage} transport failed"))
    }
}

fn read_json_response(response: Response, stage: &str) -> io::Result<Value> {
    let status = response.status().as_u16();
    if !(200..=299).contains(&status) {
        return Err(status_error(status, stage));
    }

    let mut bytes = Vec::new();
    response
        .take((RESPONSE_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| fail(&format!("Failed reading Gemini {stage} response")))?;
    if bytes.len() > RESPONSE_LIMIT {
        return Err(fail(&format!(
            "Gemini {stage} response exceeded size limit"
        )));
    }

    serde_json::from_slice(&bytes).map_err(|_| fail(&format!("Invalid Gemini {stage} JSON")))
}

fn extract_project_ref(field: &Value) -> Option<String> {
    field
        .as_str()
        .or_else(|| field["id"].as_str())
        .or_else(|| field["projectId"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn extract_project_id(payload: &Value) -> Option<String> {
    extract_project_ref(&payload["cloudaicompanionProject"])
        .or_else(|| extract_project_ref(&payload["currentCloudaicompanionProject"]))
}

fn valid_operation_name(name: &str) -> bool {
    name.starts_with("operations/")
        && name.len() <= 256
        && !name.contains("..")
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_'))
}

fn check_operation_error(lro: &Value) -> io::Result<()> {
    if let Some(err) = lro.get("error").filter(|err| !err.is_null()) {
        let code = err["code"].as_i64().unwrap_or(0);
        return Err(fail(&format!(
            "Gemini Code Assist onboarding failed with error code {code}; check project permissions and billing"
        )));
    }
    Ok(())
}

fn ensure_code_assist_ready(
    client: &Client,
    code_assist_base: &str,
    access_token: &str,
    project_seed: Option<&str>,
    poll_interval: Duration,
) -> io::Result<()> {
    let load_url = format!("{code_assist_base}:loadCodeAssist");
    let load_response = client
        .post(&load_url)
        .bearer_auth(access_token)
        .json(&json!({
            "cloudaicompanionProject": project_seed,
            "metadata": {
                "ideType": "GEMINI_CLI",
                "platform": "PLATFORM_UNSPECIFIED",
                "pluginType": "GEMINI",
                "duetProject": project_seed,
            }
        }))
        .send()
        .map_err(|error| transport_error(&error, "Code Assist project check"))?;
    let load_payload = read_json_response(load_response, "Code Assist project check")?;

    if extract_project_id(&load_payload).is_some() {
        return Ok(());
    }
    if load_payload.get("currentTier").is_some() {
        if project_seed.is_some() {
            return Ok(());
        }
        return Err(fail(
            "Gemini account requires a Google Cloud Project ID; set ZEROCLAW_providers__models__gemini__default__oauth_project in Railway Variables, redeploy, and re-run --gemini-login",
        ));
    }

    let allowed_tiers = load_payload["allowedTiers"].as_array();
    if allowed_tiers.is_none_or(Vec::is_empty)
        && load_payload["ineligibleTiers"]
            .as_array()
            .is_some_and(|tiers| !tiers.is_empty())
    {
        return Err(fail(
            "Gemini account is ineligible or requires account verification in Google Cloud before headless login",
        ));
    }

    let selected_tier = allowed_tiers.and_then(|tiers| {
        tiers
            .iter()
            .find(|tier| tier["isDefault"].as_bool() == Some(true))
            .or_else(|| tiers.first())
    });
    let tier_id = selected_tier
        .and_then(|tier| tier["id"].as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or("legacy-tier");
    let requires_user_project = selected_tier
        .and_then(|tier| tier["userDefinedCloudaicompanionProject"].as_bool())
        .unwrap_or(tier_id != "free-tier");

    if tier_id != "free-tier" && requires_user_project && project_seed.is_none() {
        return Err(fail(
            "Gemini account requires a Google Cloud Project ID; set ZEROCLAW_providers__models__gemini__default__oauth_project in Railway Variables, redeploy, and re-run --gemini-login",
        ));
    }

    let onboard_body = if tier_id == "free-tier" {
        json!({
            "tierId": tier_id,
            "metadata": {
                "ideType": "GEMINI_CLI",
                "platform": "PLATFORM_UNSPECIFIED",
                "pluginType": "GEMINI",
            }
        })
    } else {
        json!({
            "tierId": tier_id,
            "cloudaicompanionProject": project_seed,
            "metadata": {
                "ideType": "GEMINI_CLI",
                "platform": "PLATFORM_UNSPECIFIED",
                "pluginType": "GEMINI",
                "duetProject": project_seed,
            }
        })
    };

    let onboard_url = format!("{code_assist_base}:onboardUser");
    let onboard_response = client
        .post(&onboard_url)
        .bearer_auth(access_token)
        .json(&onboard_body)
        .send()
        .map_err(|error| transport_error(&error, "Code Assist onboarding"))?;
    let mut lro = read_json_response(onboard_response, "Code Assist onboarding")?;
    check_operation_error(&lro)?;

    let mut attempts = 0;
    while !lro["done"].as_bool().unwrap_or(false) {
        if attempts >= ONBOARD_POLL_ATTEMPTS {
            return Err(fail(
                "Gemini Code Assist onboarding timed out; retry --gemini-login",
            ));
        }
        attempts += 1;
        let name = lro["name"]
            .as_str()
            .filter(|name| valid_operation_name(name))
            .ok_or_else(|| fail("Gemini Code Assist onboarding returned an invalid operation"))?;
        std::thread::sleep(poll_interval);
        let poll_url = format!("{code_assist_base}/{name}");
        let poll_response = client
            .get(&poll_url)
            .bearer_auth(access_token)
            .send()
            .map_err(|error| transport_error(&error, "Code Assist onboarding poll"))?;
        lro = read_json_response(poll_response, "Code Assist onboarding poll")?;
        check_operation_error(&lro)?;
    }

    check_operation_error(&lro)?;

    if extract_project_id(&lro["response"]).is_some() || project_seed.is_some() {
        return Ok(());
    }

    Err(fail(
        "Gemini account requires a Google Cloud Project ID; set ZEROCLAW_providers__models__gemini__default__oauth_project in Railway Variables, redeploy, and re-run --gemini-login",
    ))
}

fn exchange_and_store(
    state_dir: &Path,
    token_endpoint: &str,
    code_assist_base: &str,
    code: &str,
    verifier: &str,
    project_seed: Option<&str>,
    poll_interval: Duration,
) -> io::Result<()> {
    let project_seed = validate_project_seed(project_seed)?;
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
        .map_err(|error| transport_error(&error, "OAuth token exchange"))?;

    let payload = read_json_response(response, "OAuth token exchange")?;

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

    ensure_code_assist_ready(
        &client,
        code_assist_base,
        access_token,
        project_seed.as_deref(),
        poll_interval,
    )?;

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
    let project_seed = match std::env::var(OAUTH_PROJECT_ENV) {
        Ok(value) => validate_project_seed(Some(&value))?,
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(fail(
                "Invalid ZEROCLAW_providers__models__gemini__default__oauth_project",
            ));
        }
    };
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

    exchange_and_store(
        state_dir,
        TOKEN_ENDPOINT,
        CODE_ASSIST_BASE,
        &code,
        &verifier,
        project_seed.as_deref(),
        Duration::from_secs(2),
    )?;
    println!(
        "Gemini OAuth credentials saved to {}/.gemini/oauth_creds.json",
        state_dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::fs::PermissionsExt;

    fn read_http_request(stream: &mut TcpStream) -> (String, String) {
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 1024];
        let header_end = loop {
            assert!(buffer.len() <= 8192, "HTTP request headers exceeded limit");
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0, "unexpected EOF before HTTP headers completed");
            buffer.extend_from_slice(&chunk[..count]);
            if let Some(pos) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break pos + 4;
            }
        };
        let headers = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        assert!(content_length <= 8192, "HTTP request body exceeded limit");
        let total_expected = header_end + content_length;
        while buffer.len() < total_expected {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0, "unexpected EOF while reading HTTP request body");
            buffer.extend_from_slice(&chunk[..count]);
        }
        let body = String::from_utf8_lossy(&buffer[header_end..total_expected]).into_owned();
        (headers, body)
    }

    fn write_http_response(stream: &mut TcpStream, status: &str, body: &str) {
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        stream.flush().unwrap();
    }

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
            let (mut token_stream, _) = listener.accept().unwrap();
            let (headers, body) = read_http_request(&mut token_stream);
            assert!(headers.starts_with("POST /token HTTP/1.1\r\n"));
            assert!(body.contains("grant_type=authorization_code"));
            assert!(body.contains("code=fixture-auth-code"));
            assert!(body.contains("code_verifier=fixture-verifier"));
            write_http_response(
                &mut token_stream,
                "200 OK",
                r#"{"access_token":"ya29.fixture-access","refresh_token":"1//fixture-refresh","id_token":"eyJ.fixture-id","expires_in":3600,"token_type":"Bearer"}"#,
            );

            let (mut load_stream, _) = listener.accept().unwrap();
            let (load_headers, load_body) = read_http_request(&mut load_stream);
            assert!(load_headers.starts_with("POST /v1internal:loadCodeAssist HTTP/1.1\r\n"));
            assert!(
                load_headers
                    .to_ascii_lowercase()
                    .contains("authorization: bearer ya29.fixture-access")
            );
            assert!(load_body.contains(r#""ideType":"GEMINI_CLI""#));
            write_http_response(
                &mut load_stream,
                "200 OK",
                r#"{"currentTier":{"id":"free-tier"},"cloudaicompanionProject":"gen-lang-client-fixture"}"#,
            );
        });

        let result = exchange_and_store(
            dir.path(),
            &format!("http://{addr}/token"),
            &format!("http://{addr}/v1internal"),
            "fixture-auth-code",
            "fixture-verifier",
            None,
            Duration::from_millis(10),
        );
        server.join().unwrap();
        result.unwrap();

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
            let _ = read_http_request(&mut stream);
            write_http_response(
                &mut stream,
                "400 Bad Request",
                r#"{"error":"invalid_grant","error_description":"synthetic-secret-token"}"#,
            );
        });
        let error_result = exchange_and_store(
            dir.path(),
            &format!("http://{error_addr}/token"),
            &format!("http://{error_addr}/v1internal"),
            "expired-code",
            "fixture-verifier",
            None,
            Duration::from_millis(10),
        );
        error_server.join().unwrap();
        let error = error_result.unwrap_err();
        assert!(!error.to_string().contains("synthetic-secret-token"));
        assert!(!error.to_string().contains("invalid_grant"));
    }

    #[test]
    fn first_time_account_onboards_and_project_required_account_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut token_stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut token_stream);
            write_http_response(
                &mut token_stream,
                "200 OK",
                r#"{"access_token":"ya29.first-time","refresh_token":"1//first-time","expires_in":3600}"#,
            );

            let (mut load_stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut load_stream);
            write_http_response(
                &mut load_stream,
                "200 OK",
                r#"{"allowedTiers":[{"id":"free-tier","isDefault":true}]}"#,
            );

            let (mut onboard_stream, _) = listener.accept().unwrap();
            let (onboard_headers, onboard_body) = read_http_request(&mut onboard_stream);
            assert!(onboard_headers.starts_with("POST /v1internal:onboardUser HTTP/1.1\r\n"));
            assert!(onboard_body.contains(r#""tierId":"free-tier""#));
            assert!(!onboard_body.contains("cloudaicompanionProject"));
            write_http_response(
                &mut onboard_stream,
                "200 OK",
                r#"{"name":"operations/onboard-123","done":false}"#,
            );

            let (mut poll_stream, _) = listener.accept().unwrap();
            let (poll_headers, _) = read_http_request(&mut poll_stream);
            assert!(
                poll_headers.starts_with("GET /v1internal/operations/onboard-123 HTTP/1.1\r\n")
            );
            write_http_response(
                &mut poll_stream,
                "200 OK",
                r#"{"name":"operations/onboard-123","done":true,"response":{"cloudaicompanionProject":{"id":"provisioned-free-project"}}}"#,
            );
        });

        let result = exchange_and_store(
            dir.path(),
            &format!("http://{addr}/token"),
            &format!("http://{addr}/v1internal"),
            "fixture-code",
            "fixture-verifier",
            None,
            Duration::from_millis(10),
        );
        server.join().unwrap();
        result.unwrap();
        assert!(dir.path().join(".gemini/oauth_creds.json").is_file());

        // Account that requires an explicit Google Cloud Project ID fails before writing credentials.
        let standard_dir = tempfile::tempdir().unwrap();
        let standard_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let standard_addr = standard_listener.local_addr().unwrap();
        let standard_server = std::thread::spawn(move || {
            let (mut token_stream, _) = standard_listener.accept().unwrap();
            let _ = read_http_request(&mut token_stream);
            write_http_response(
                &mut token_stream,
                "200 OK",
                r#"{"access_token":"ya29.standard","refresh_token":"1//standard","expires_in":3600}"#,
            );

            let (mut load_stream, _) = standard_listener.accept().unwrap();
            let _ = read_http_request(&mut load_stream);
            write_http_response(
                &mut load_stream,
                "200 OK",
                r#"{"currentTier":{"id":"standard-tier"}}"#,
            );
        });

        let standard_result = exchange_and_store(
            standard_dir.path(),
            &format!("http://{standard_addr}/token"),
            &format!("http://{standard_addr}/v1internal"),
            "fixture-code",
            "fixture-verifier",
            None,
            Duration::from_millis(10),
        );
        standard_server.join().unwrap();
        let error = standard_result.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("ZEROCLAW_providers__models__gemini__default__oauth_project")
        );
        assert!(
            !standard_dir
                .path()
                .join(".gemini/oauth_creds.json")
                .exists()
        );
        assert!(validate_project_seed(Some("123456789")).is_err());
        assert_eq!(
            validate_project_seed(Some("my-gcp-project-123")).unwrap(),
            Some("my-gcp-project-123".into())
        );
    }

    #[test]
    fn onboarding_terminal_error_with_configured_project_preserves_existing_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let gemini_dir = dir.path().join(".gemini");
        std::fs::create_dir(&gemini_dir).unwrap();
        let creds_path = gemini_dir.join("oauth_creds.json");
        std::fs::write(&creds_path, b"retained-existing-credential").unwrap();

        // Immediate terminal error on onboardUser.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut token_stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut token_stream);
            write_http_response(
                &mut token_stream,
                "200 OK",
                r#"{"access_token":"ya29.err1","refresh_token":"1//err1","expires_in":3600}"#,
            );

            let (mut load_stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut load_stream);
            write_http_response(
                &mut load_stream,
                "200 OK",
                r#"{"allowedTiers":[{"id":"standard-tier","userDefinedCloudaicompanionProject":true,"isDefault":true}]}"#,
            );

            let (mut onboard_stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut onboard_stream);
            write_http_response(
                &mut onboard_stream,
                "200 OK",
                r#"{"done":true,"error":{"code":7,"message":"permission denied"}}"#,
            );
        });

        let result = exchange_and_store(
            dir.path(),
            &format!("http://{addr}/token"),
            &format!("http://{addr}/v1internal"),
            "fixture-code",
            "fixture-verifier",
            Some("my-gcp-project"),
            Duration::from_millis(10),
        );
        server.join().unwrap();
        let error = result.unwrap_err();
        assert!(error.to_string().contains("error code 7"));
        assert_eq!(
            std::fs::read(&creds_path).unwrap(),
            b"retained-existing-credential"
        );

        // Polled terminal error on onboardUser.
        let poll_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let poll_addr = poll_listener.local_addr().unwrap();
        let poll_server = std::thread::spawn(move || {
            let (mut token_stream, _) = poll_listener.accept().unwrap();
            let _ = read_http_request(&mut token_stream);
            write_http_response(
                &mut token_stream,
                "200 OK",
                r#"{"access_token":"ya29.err2","refresh_token":"1//err2","expires_in":3600}"#,
            );

            let (mut load_stream, _) = poll_listener.accept().unwrap();
            let _ = read_http_request(&mut load_stream);
            write_http_response(
                &mut load_stream,
                "200 OK",
                r#"{"allowedTiers":[{"id":"standard-tier","userDefinedCloudaicompanionProject":true,"isDefault":true}]}"#,
            );

            let (mut onboard_stream, _) = poll_listener.accept().unwrap();
            let _ = read_http_request(&mut onboard_stream);
            write_http_response(
                &mut onboard_stream,
                "200 OK",
                r#"{"name":"operations/onboard-fail","done":false}"#,
            );

            let (mut poll_stream, _) = poll_listener.accept().unwrap();
            let _ = read_http_request(&mut poll_stream);
            write_http_response(
                &mut poll_stream,
                "200 OK",
                r#"{"name":"operations/onboard-fail","done":true,"error":{"code":9,"message":"resource exhausted"}}"#,
            );
        });

        let poll_result = exchange_and_store(
            dir.path(),
            &format!("http://{poll_addr}/token"),
            &format!("http://{poll_addr}/v1internal"),
            "fixture-code",
            "fixture-verifier",
            Some("my-gcp-project"),
            Duration::from_millis(10),
        );
        poll_server.join().unwrap();
        let poll_error = poll_result.unwrap_err();
        assert!(poll_error.to_string().contains("error code 9"));
        assert_eq!(
            std::fs::read(&creds_path).unwrap(),
            b"retained-existing-credential"
        );
    }

    #[test]
    fn exchange_and_store_rejects_symlinked_gemini_directory() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join(".gemini")).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut token_stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut token_stream);
            write_http_response(
                &mut token_stream,
                "200 OK",
                r#"{"access_token":"ya29.fixture","refresh_token":"1//fixture","expires_in":3600}"#,
            );

            let (mut load_stream, _) = listener.accept().unwrap();
            let _ = read_http_request(&mut load_stream);
            write_http_response(
                &mut load_stream,
                "200 OK",
                r#"{"currentTier":{"id":"free-tier"},"cloudaicompanionProject":"gen-lang-client-fixture"}"#,
            );
        });

        let result = exchange_and_store(
            dir.path(),
            &format!("http://{addr}/token"),
            &format!("http://{addr}/v1internal"),
            "fixture-code",
            "fixture-verifier",
            None,
            Duration::from_millis(10),
        );
        server.join().unwrap();
        assert!(result.is_err());
        assert!(!outside.path().join("oauth_creds.json").exists());
    }
}
