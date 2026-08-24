use std::io;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::thread;

pub fn run_from_args(arguments: &[String]) -> Result<i32, String> {
    let socket_path = arguments.first().ok_or("expected socket path")?;
    let port_text = arguments.get(1).ok_or("expected listen port")?;
    if arguments.len() != 2 {
        return Err("expected exactly two bridge arguments".to_string());
    }
    let listen_port = port_text
        .parse::<u16>()
        .map_err(|error| format!("invalid listen port {}: {}", port_text, error))?;
    run(Path::new(socket_path), listen_port)?;
    Ok(0)
}

pub fn run(socket_path: &Path, listen_port: u16) -> Result<(), String> {
    let listener =
        TcpListener::bind(("127.0.0.1", listen_port)).map_err(|error| error.to_string())?;
    loop {
        let (client, _) = listener.accept().map_err(|error| error.to_string())?;
        let socket_path = socket_path.to_path_buf();
        thread::spawn(move || {
            let _ = bridge_client(client, &socket_path);
        });
    }
}

fn bridge_client(client: TcpStream, socket_path: &Path) -> Result<(), String> {
    let upstream = UnixStream::connect(socket_path).map_err(|error| error.to_string())?;
    let mut client_read = client.try_clone().map_err(|error| error.to_string())?;
    let mut client_write = client;
    let mut upstream_read = upstream.try_clone().map_err(|error| error.to_string())?;
    let mut upstream_write = upstream;
    let left = thread::spawn(move || -> Result<(), String> {
        io::copy(&mut client_read, &mut upstream_write).map_err(|error| error.to_string())?;
        upstream_write
            .shutdown(Shutdown::Write)
            .map_err(|error| error.to_string())?;
        Ok(())
    });
    io::copy(&mut upstream_read, &mut client_write).map_err(|error| error.to_string())?;
    client_write
        .shutdown(Shutdown::Write)
        .map_err(|error| error.to_string())?;
    left.join()
        .map_err(|_| "bridge thread panicked".to_string())??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::bridge_client;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::net::UnixListener;
    use std::path::PathBuf;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn bridges_bidirectional_bytes_between_tcp_and_unix_streams() {
        let root = temp_root();
        let socket_path = root.join("bridge.sock");
        let unix_listener = UnixListener::bind(&socket_path).unwrap();
        let tcp_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = tcp_listener.local_addr().unwrap().port();

        let server = thread::spawn(move || {
            let (client, _) = tcp_listener.accept().unwrap();
            bridge_client(client, &socket_path).unwrap();
        });
        let upstream = thread::spawn(move || {
            let (mut stream, _) = unix_listener.accept().unwrap();
            let mut request = [0_u8; 5];
            stream.read_exact(&mut request).unwrap();
            assert_eq!(&request, b"hello");
            stream.write_all(b"world").unwrap();
        });

        let mut tcp_client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        tcp_client.write_all(b"hello").unwrap();
        tcp_client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = String::new();
        tcp_client.read_to_string(&mut response).unwrap();

        assert_eq!(response, "world");
        upstream.join().unwrap();
        server.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rack-ai-sandbox-bridge-{nanos}"));
        fs::create_dir_all(&root).unwrap();
        root
    }
}
