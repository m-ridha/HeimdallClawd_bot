//! Typed-only transcription must dispatch Telegram voice updates to the agent.
use serde_json::json;
use std::time::Duration;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};
use zeroclaw_channels::orchestrator::build_channel_map_for_agent;
use zeroclaw_config::schema::Config;

#[tokio::test]
async fn typed_only_provider_delivers_transcribed_voice() {
    let endpoint = MockServer::start().await;
    let token = "123456:TEST_token_abcdefghijklmnop";
    let update = json!({"update_id":1,"message":{"message_id":1,"chat":{"id":1001,"type":"private"},"from":{"id":1001,"first_name":"Fixture"},"voice":{"file_id":"fixture-audio","duration":1}}});
    let mut too_long = update.clone();
    too_long["update_id"] = json!(0);
    too_long["message"]["voice"]["duration"] = json!(121);
    Mock::given(path(format!("/bot{token}/getUpdates")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"result":[too_long, update]})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&endpoint)
        .await;
    Mock::given(path(format!("/bot{token}/getUpdates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":[]})))
        .with_priority(2)
        .mount(&endpoint)
        .await;
    Mock::given(path(format!("/bot{token}/getFile")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"result":{"file_path":"voice/fixture.ogg"}})),
        )
        .expect(1)
        .mount(&endpoint)
        .await;
    Mock::given(path(format!("/file/bot{token}/voice/fixture.ogg")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"synthetic-audio"))
        .expect(1)
        .mount(&endpoint)
        .await;
    Mock::given(method("POST"))
        .and(path("/transcribe"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"text":"synthetic spoken message"})),
        )
        .expect(1)
        .mount(&endpoint)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let mut config: Config = serde_json::from_value(json!({
        "transcription":{"enabled":true,"max_duration_secs":120,"max_audio_bytes":8388608},
        "providers":{"transcription":{"local_whisper":{"fixture":{"uri":format!("{}/transcribe",endpoint.uri()),"timeout_secs":5}}}},
        "agents":{"primary":{"enabled":true,"channels":["telegram.home"],"transcription_provider":"local_whisper.fixture"}},
        "channels":{"telegram":{"home":{"enabled":true,"bot_token":token,"api_base_url":endpoint.uri(),"mention_only":false,"ack_reactions":false,"stream_mode":"partial"}}},
        "peer_groups":{"home":{"channel":"telegram.home","agents":["primary"],"external_peers":["1001"]}}
    })).unwrap();
    config.config_path = temp.path().join("config.toml");
    config.data_dir = temp.path().join("data");
    let channels = build_channel_map_for_agent(&config, "primary");
    let channel = channels
        .get("telegram.home")
        .expect("production factory must bind Telegram");
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let message = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            result = channel.listen(tx) => panic!("listener exited: {result:?}"),
            message = rx.recv() => message.unwrap(),
        }
    })
    .await
    .expect("voice update was silently skipped before transcription");
    assert_eq!(message.content, "[Voice] synthetic spoken message");
    endpoint.verify().await;
}

#[tokio::test]
async fn tts_voice_reply_delivers_synthesized_audio_to_telegram() {
    use std::sync::{Arc, Mutex};
    use tokio::sync::Notify;
    use zeroclaw_api::channel::SendMessage;

    let endpoint = MockServer::start().await;
    let token = "123456:TEST_token_abcdefghijklmnop";
    let update = json!({"update_id":1,"message":{"message_id":1,"chat":{"id":1001,"type":"private"},"from":{"id":1001,"first_name":"Fixture"},"voice":{"file_id":"fixture-audio","duration":1}}});
    Mock::given(path(format!("/bot{token}/getUpdates")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":[update]})),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&endpoint)
        .await;
    Mock::given(path(format!("/bot{token}/getUpdates")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":[]})))
        .with_priority(2)
        .mount(&endpoint)
        .await;
    Mock::given(path(format!("/bot{token}/getFile")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"result":{"file_path":"voice/fixture.ogg"}})),
        )
        .expect(1)
        .mount(&endpoint)
        .await;
    Mock::given(path(format!("/file/bot{token}/voice/fixture.ogg")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"synthetic-audio"))
        .expect(1)
        .mount(&endpoint)
        .await;
    Mock::given(method("POST"))
        .and(path("/transcribe"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"text":"synthetic spoken question"})),
        )
        .expect(1)
        .mount(&endpoint)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/audio/speech"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"OggS-synthetic-opus-audio"))
        .expect(1)
        .mount(&endpoint)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/sendMessage")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":{"message_id":10}})),
        )
        .expect(1..)
        .mount(&endpoint)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/editMessageText")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":{"message_id":10}})),
        )
        .mount(&endpoint)
        .await;

    let voice_delivered = Arc::new(Notify::new());
    let voice_delivered_signal = voice_delivered.clone();
    let captured_voice_request: Arc<Mutex<Option<(String, Vec<u8>)>>> = Arc::new(Mutex::new(None));
    let captured_voice_sink = captured_voice_request.clone();
    Mock::given(method("POST"))
        .and(path(format!("/bot{token}/sendVoice")))
        .respond_with(move |req: &wiremock::Request| {
            let content_type = req
                .headers
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            *captured_voice_sink.lock().unwrap() = Some((content_type, req.body.clone()));
            voice_delivered_signal.notify_one();
            ResponseTemplate::new(200).set_body_json(json!({"ok":true,"result":{"message_id":11}}))
        })
        .expect(1)
        .mount(&endpoint)
        .await;

    let temp = tempfile::tempdir().unwrap();
    let mut config: Config = serde_json::from_value(json!({
        "transcription":{"enabled":true,"max_duration_secs":120,"max_audio_bytes":8388608},
        "tts":{"enabled":true,"default_voice":"alloy","default_format":"opus","max_text_length":4096},
        "providers":{
            "transcription":{"local_whisper":{"fixture":{"uri":format!("{}/transcribe",endpoint.uri()),"timeout_secs":5}}},
            "tts":{"openai":{"fixture":{"api_key":"synthetic-tts-key","uri":format!("{}/v1/audio/speech",endpoint.uri()),"voice":"alloy","response_format":"opus"}}}
        },
        "agents":{"primary":{"enabled":true,"channels":["telegram.home"],"transcription_provider":"local_whisper.fixture","tts_provider":"openai.fixture"}},
        "channels":{"telegram":{"home":{"enabled":true,"bot_token":token,"api_base_url":endpoint.uri(),"mention_only":false,"ack_reactions":false,"stream_mode":"partial"}}},
        "peer_groups":{"home":{"channel":"telegram.home","agents":["primary"],"external_peers":["1001"]}}
    })).unwrap();
    config.config_path = temp.path().join("config.toml");
    config.data_dir = temp.path().join("data");
    let channels = build_channel_map_for_agent(&config, "primary");
    let channel = channels
        .get("telegram.home")
        .expect("production factory must bind Telegram");
    let (tx, mut rx) = tokio::sync::mpsc::channel(1);
    let message = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::select! {
            result = channel.listen(tx) => panic!("listener exited: {result:?}"),
            message = rx.recv() => message.unwrap(),
        }
    })
    .await
    .expect("voice update was silently skipped before transcription");
    assert_eq!(message.content, "[Voice] synthetic spoken question");

    let reply = "Here is a complete spoken reply with enough natural language words to pass the voice reply filter.";
    let draft_id = channel
        .send_draft(&SendMessage::new("...", "1001"))
        .await
        .unwrap()
        .expect("draft id");
    channel
        .finalize_draft("1001", &draft_id, reply, false)
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(3), voice_delivered.notified())
        .await
        .expect("Telegram sendVoice reply did not fire");
    let (content_type, body) = captured_voice_request
        .lock()
        .unwrap()
        .take()
        .expect("sendVoice request body was not captured");
    assert!(
        content_type.starts_with("multipart/form-data"),
        "expected multipart/form-data, got: {content_type}"
    );
    let boundary = content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("boundary="))
        .map(|value| value.trim_matches('"'))
        .expect("multipart/form-data header missing boundary");
    let delimiter = format!("--{boundary}").into_bytes();
    let mut chat_id_value = None;
    let mut voice_value = None;
    let mut offset = 0;
    while let Some(pos) = body[offset..]
        .windows(delimiter.len())
        .position(|window| window == delimiter.as_slice())
    {
        let part_start = offset + pos + delimiter.len();
        if body[part_start..].starts_with(b"--") {
            break;
        }
        let content_start = if body[part_start..].starts_with(b"\r\n") {
            part_start + 2
        } else {
            part_start
        };
        let next_pos = body[content_start..]
            .windows(delimiter.len())
            .position(|window| window == delimiter.as_slice())
            .expect("multipart part missing closing boundary");
        let part = &body[content_start..content_start + next_pos];
        let part = part.strip_suffix(b"\r\n").unwrap_or(part);
        let header_end = part
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("multipart part missing header terminator");
        let headers = String::from_utf8_lossy(&part[..header_end]);
        let value = &part[header_end + 4..];
        if headers.contains("name=\"chat_id\"") {
            chat_id_value = Some(value.to_vec());
        } else if headers.contains("name=\"voice\"") {
            voice_value = Some(value.to_vec());
        }
        offset = content_start + next_pos;
    }
    assert_eq!(
        chat_id_value.as_deref(),
        Some(b"1001".as_slice()),
        "sendVoice chat_id field must be exactly 1001"
    );
    assert_eq!(
        voice_value.as_deref(),
        Some(b"OggS-synthetic-opus-audio".as_slice()),
        "sendVoice voice field must contain the exact fixture bytes"
    );
    endpoint.verify().await;
}
