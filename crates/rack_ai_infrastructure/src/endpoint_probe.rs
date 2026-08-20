#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointProbe;

impl EndpointProbe {
    pub fn check_models(&self, endpoint: &str) -> Result<bool, String> {
        let url = format!("{endpoint}/models");
        let response = ureq::get(&url).call().map_err(|error| error.to_string())?;
        Ok(response.status().as_u16() >= 200 && response.status().as_u16() < 300)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::EndpointProbe;

    #[test]
    fn reports_success_for_http_ok() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        });
        let probe = EndpointProbe;
        let ok = probe
            .check_models(format!("http://{address}/v1").as_str())
            .unwrap();
        assert!(ok);
    }
}
