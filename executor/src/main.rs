use heimdall_executor::{
    files::Workspace,
    linux, process,
    protocol::{BINARY_LIMIT, BODY_LIMIT, Job},
    server::{App, listen_address, router},
};
use std::{
    io::{self, Read, Write},
    path::Path,
};
fn helper() -> io::Result<()> {
    let mut input = String::new();
    io::stdin()
        .take((BODY_LIMIT + 1) as u64)
        .read_to_string(&mut input)?;
    if input.len() > BODY_LIMIT {
        return Err(io::Error::other("Input too large"));
    }
    let job: Job = serde_json::from_str(&input).map_err(io::Error::other)?;
    linux::restrict()?;
    if job.tool == "__probe" {
        linux::probe()?;
        println!("isolation:passed");
        return Ok(());
    }
    job.validate().map_err(io::Error::other)?;
    match job.tool.as_str() {
        "bash" => {
            use std::os::unix::process::CommandExt;
            Err(std::process::Command::new("/bin/bash")
                .args([
                    "--noprofile",
                    "--norc",
                    "-c",
                    job.arguments["command"].as_str().unwrap(),
                ])
                .exec())
        }
        "file_read_base64" => {
            use base64::Engine;
            let bytes = Workspace::open(Path::new("/data/workspace"))?
                .read_binary(job.arguments["path"].as_str().unwrap())?;
            if bytes.len() > BINARY_LIMIT {
                return Err(io::Error::other("Transfer exceeds 48 KiB"));
            }
            io::stdout().write_all(
                base64::engine::general_purpose::STANDARD
                    .encode(bytes)
                    .as_bytes(),
            )
        }
        "file_write_base64" => {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(job.arguments["content"].as_str().unwrap())
                .map_err(io::Error::other)?;
            if bytes.len() > BINARY_LIMIT {
                return Err(io::Error::other("Transfer exceeds 48 KiB"));
            }
            Workspace::open(Path::new("/data/workspace"))?
                .write_binary(job.arguments["path"].as_str().unwrap(), &bytes)?;
            println!("file written");
            Ok(())
        }
        "file_read" => io::stdout().write_all(
            &Workspace::open(Path::new("/data/workspace"))?
                .read(job.arguments["path"].as_str().unwrap())?,
        ),
        "file_write" => {
            Workspace::open(Path::new("/data/workspace"))?.write(
                job.arguments["path"].as_str().unwrap(),
                job.arguments["content"].as_str().unwrap(),
            )?;
            println!("file written");
            Ok(())
        }
        _ => Err(io::Error::other("Unknown tool")),
    }
}
fn main() -> io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--helper") {
        if let Err(error) = helper() {
            eprintln!("Tool failed: {error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    #[cfg(target_os = "linux")]
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(async {
            let token = std::env::var("EXECUTOR_TOKEN").map_err(io::Error::other)?;
            if token.len() < 32 {
                return Err(io::Error::other(
                    "Executor authentication secret is too short",
                ));
            }
            heimdall_executor::linux::report_workspace_storage();
            let verified = process::verify_platform().await;
            let ready = verified.is_ok();
            println!(
                "Linux isolation probe: {}",
                if ready {
                    "PASS"
                } else {
                    "FAIL; execution disabled"
                }
            );
            if let Err(error) = verified {
                println!("Probe diagnostic: {error}");
            }
            let port = std::env::var("PORT").unwrap_or_else(|_| "3889".into());
            let listener = tokio::net::TcpListener::bind(listen_address(&port)?).await?;
            println!("Executor listening on {}", listener.local_addr()?);
            axum::serve(listener, router(App::new(token, ready))).await
        })
}
