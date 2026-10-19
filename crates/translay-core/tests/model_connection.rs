use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};
use translay_core::{
    model_config::{ModelBackend, ModelConfig},
    model_transport::ModelClient,
};

#[test]
fn connection_test_uses_real_http_auth_payload_and_shared_failure_messages() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for status in [401, 200] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 2048];
                let count = stream.read(&mut chunk).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..index]);
                    let length: usize = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse().unwrap())
                        })
                        .unwrap();
                    if bytes.len() >= index + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(bytes).unwrap());
            let body = if status == 401 {
                r#"{"error":{"message":"fake-fixture-secret-must-not-echo"}}"#
            } else {
                r#"{"choices":[{"message":{"content":"OK"}}]}"#
            };
            write!(stream, "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
        requests
    });
    let mut config = ModelConfig::default();
    config.backend = ModelBackend::Api;
    config.api.base_url = format!("http://{address}");
    config.api.model = "fixture-model".into();
    let client = ModelClient::with_client(reqwest::Client::builder().no_proxy().build().unwrap());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (failed, recovered) = runtime.block_on(async {
        let failed = client
            .test_connection_with_config(config.clone(), Some("fake-fixture-secret".into()))
            .await;
        let recovered = client
            .test_connection_with_config(config, Some("fake-fixture-secret".into()))
            .await;
        (failed, recovered)
    });
    assert!(!failed.success);
    assert!(failed.message.contains("检查 API Key"));
    assert!(!failed.message.contains("must-not-echo"));
    assert!(recovered.success);
    let requests = server.join().unwrap();
    for request in requests {
        assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
        let (headers, body) = request.split_once("\r\n\r\n").unwrap();
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: bearer fake-fixture-secret")
        );
        let payload: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(payload["model"], "fixture-model");
        assert_eq!(payload["stream"], false);
        assert_eq!(payload["messages"][0]["content"], "Reply with OK only.");
    }
}
