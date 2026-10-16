//! Typed-only transcription must dispatch Telegram voice updates to the agent.
use serde_json::json;
use std::{sync::Arc, time::Duration};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};
use zeroclaw_api::channel::Channel;
use zeroclaw_channels::telegram::TelegramChannel;
use zeroclaw_config::schema::Config;

#[tokio::test]
async fn typed_only_provider_delivers_transcribed_voice() {
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
            ResponseTemplate::new(200).set_body_json(json!({"text":"synthetic spoken message"})),
        )
        .expect(1)
        .mount(&endpoint)
        .await;
    let config: Config = serde_json::from_value(json!({"transcription":{"enabled":true,"max_duration_secs":120,"max_audio_bytes":8388608},"providers":{"transcription":{"local_whisper":{"fixture":{"uri":format!("{}/transcribe",endpoint.uri()),"timeout_secs":5}}}}})).unwrap();
    let channel = TelegramChannel::new(
        token.into(),
        "home",
        Arc::new(|| vec!["1001".into()]),
        false,
    )
    .with_ack_reactions(false)
    .with_api_base(endpoint.uri())
    .with_transcription_for_provider(&config, "local_whisper.fixture");
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
