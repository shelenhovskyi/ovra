use std::time::Duration;

use ovra_core::Core;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream};

use super::{Ended, Hello, HelloLine, IDLE, LINE_MAX, Line, Lines, PING, serve};

fn hello() -> Hello {
    Hello {
        version: "1.2.3".into(),
        protocol: 1,
        os: "linux".into(),
        no_sandbox: None,
    }
}

/// A served core and the client's ends of its stdin and stdout.
fn start() -> (
    tokio::task::JoinHandle<Ended>,
    DuplexStream,
    tokio::io::Lines<BufReader<DuplexStream>>,
) {
    let (client_in, server_in) = tokio::io::duplex(1 << 16);
    let (server_out, client_out) = tokio::io::duplex(1 << 16);
    let core = Core::builder("test").build();
    let task =
        tokio::spawn(
            async move { serve(core, BufReader::new(server_in), server_out, &hello()).await },
        );
    (task, client_in, BufReader::new(client_out).lines())
}

async fn line(lines: &mut tokio::io::Lines<BufReader<DuplexStream>>) -> String {
    lines
        .next_line()
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| panic!("no line"))
}

#[tokio::test]
async fn hello_comes_first_then_ready_then_answers() {
    let (task, mut input, mut output) = start();
    assert_eq!(
        line(&mut output).await,
        r#"{"hello":{"version":"1.2.3","protocol":1,"os":"linux"}}"#
    );
    assert!(line(&mut output).await.contains("info.ready"));
    input
        .write_all(b"{\"ping\":{}}\n")
        .await
        .unwrap_or_default();
    input
        .write_all(b"{\"id\":7,\"method\":\"projects.list\"}\n")
        .await
        .unwrap_or_default();
    // The ping gets no answer: the next line answers id 7.
    assert!(line(&mut output).await.starts_with(r#"{"id":7,"#));
    drop(input);
    assert_eq!(task.await.ok(), Some(Ended::Closed));
}

#[tokio::test]
async fn a_line_that_is_too_long_or_not_text_ends_it() {
    let (task, mut input, _output) = start();
    let long = vec![b'x'; LINE_MAX + 2];
    let _ = input.write_all(&long).await;
    assert_eq!(task.await.ok(), Some(Ended::BadLine));

    let (task, mut input, _output) = start();
    input.write_all(b"\xff\xfe\n").await.unwrap_or_default();
    assert_eq!(task.await.ok(), Some(Ended::BadLine));
}

#[tokio::test(start_paused = true)]
async fn a_quiet_client_is_given_up_on_and_pings_keep_it() {
    let (task, mut input, mut output) = start();
    line(&mut output).await;
    line(&mut output).await;
    for _ in 0..3 {
        tokio::time::sleep(IDLE - Duration::from_secs(5)).await;
        input
            .write_all(format!("{PING}\n").as_bytes())
            .await
            .unwrap_or_default();
    }
    assert!(!task.is_finished());
    tokio::time::sleep(IDLE + Duration::from_secs(1)).await;
    assert_eq!(task.await.ok(), Some(Ended::Idle));
}

#[tokio::test(start_paused = true)]
async fn pings_sent_during_a_slow_write_still_count() {
    use tokio::io::AsyncReadExt;
    // Room for the hello and ready lines, not for the long answer below.
    let (client_in, server_in) = tokio::io::duplex(1 << 16);
    let (server_out, mut client_out) = tokio::io::duplex(512);
    let core = Core::builder("test").build();
    let task =
        tokio::spawn(
            async move { serve(core, BufReader::new(server_in), server_out, &hello()).await },
        );
    let mut input = client_in;
    // An unknown method answers with a long error that does not fit.
    input
        .write_all(b"{\"id\":1,\"method\":\"nope.nope\"}\n")
        .await
        .unwrap_or_default();
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_secs(14)).await;
        input
            .write_all(format!("{PING}\n").as_bytes())
            .await
            .unwrap_or_default();
    }
    // 56 s later the client reads again; the answer finally goes out.
    let mut seen = Vec::new();
    let mut chunk = vec![0; 1 << 16];
    while !String::from_utf8_lossy(&seen).contains("\"id\":1,") {
        let n = client_out.read(&mut chunk).await.unwrap_or_default();
        seen.extend_from_slice(&chunk[..n]);
    }
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(!task.is_finished());
    drop(input);
    assert_eq!(task.await.ok(), Some(Ended::Closed));
}

#[tokio::test]
async fn the_laptop_reads_the_hello_back_and_lines_keep_their_limit() {
    let (_task, _input, mut output) = start();
    let first = line(&mut output).await;
    let read: Result<HelloLine, _> = serde_json::from_str(&first);
    assert_eq!(read.ok().map(|l| l.hello), Some(hello()));

    let mut lines = Lines::with_limit(BufReader::new(&b"abcd\nabcdef\n"[..]), 4);
    assert_eq!(lines.next().await, Line::Text("abcd".into()));
    assert_eq!(lines.next().await, Line::Bad);
}
