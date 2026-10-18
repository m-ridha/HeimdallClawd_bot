use std::process::Stdio;
use zeroclaw_tools::text_browser::heimdall_browser_output;
#[tokio::test]
async fn browser_output_is_bounded_during_reading() {
    let child = tokio::process::Command::new("/usr/bin/head")
        .args(["-c", "500001", "/dev/zero"])
        .env_clear()
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    assert!(heimdall_browser_output(child).await.is_err());
    let child = tokio::process::Command::new("/usr/bin/printf")
        .arg("public-browser-result")
        .env_clear()
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let output = heimdall_browser_output(child).await.unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"public-browser-result");
}
