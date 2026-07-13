use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::Duration,
};

pub const TIMEOUT_TEST_CLIENT_LIMIT: Duration = Duration::from_millis(100);
pub const TIMEOUT_TEST_RESPONSE_DELAY: Duration = Duration::from_secs(1);

pub fn serve_once(status: u16, headers: &[(&str, &str)], body: &str) -> String {
    serve_once_after(Duration::ZERO, status, headers, body)
}

pub fn serve_once_after(
    delay: Duration,
    status: u16,
    headers: &[(&str, &str)],
    body: &str,
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("mock HTTP listener should bind");
    let address = listener
        .local_addr()
        .expect("mock listener should have an address");
    let headers = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();
    let body = body.to_owned();

    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut request = [0_u8; 4096];
        let _ = stream.read(&mut request);
        thread::sleep(delay);
        let reason = match status {
            200 => "OK",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            429 => "Too Many Requests",
            _ => "Test Response",
        };
        let response = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
    });

    format!("http://{address}")
}
