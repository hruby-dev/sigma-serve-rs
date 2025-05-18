use std::{
    env, fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::Arc,
};

fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080")?;
    println!("sigma-serve-rs started gooning files on http://127.0.0.1:8080");

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <root_directory>", args[0]);
        std::process::exit(1);
    }
    let root_dir = Arc::new(args[1].clone());

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let root_dir = Arc::clone(&root_dir);
                std::thread::spawn(move || {
                    if let Err(e) = handle_client(stream, &root_dir) {
                        eprintln!("Error handling client: {}", e);
                    }
                });
            }
            Err(e) => eprintln!("Connection failed: {}", e),
        }
    }
    Ok(())
}

fn handle_client(mut stream: TcpStream, root_dir: &str) -> std::io::Result<()> {
    let buf_reader = BufReader::new(&stream);
    let request_line = match buf_reader.lines().next().transpose()? {
        Some(line) => line,
        None => return Ok(()),
    };

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    if method != "GET" {
        return send_response(&mut stream, "HTTP/1.1 405 METHOD NOT ALLOWED", "Method Not Allowed");
    }

    let requested = if path == "/" {
        "index.html".to_string()
    } else {
        format!("{}.html", path.trim_start_matches('/'))
    };

    if requested.contains("..") {
        return send_response(&mut stream, "HTTP/1.1 403 FORBIDDEN", "Forbidden");
    }

    let full_path = format!("{}/{}", root_dir, requested);

    match fs::read_to_string(&full_path) {
        Ok(contents) => send_response(&mut stream, "HTTP/1.1 200 OK", &contents),
        Err(_) => {
            let fallback_path = format!("{}/404.html", root_dir);
            let fallback = fs::read_to_string(&fallback_path).unwrap_or_else(|_| "404 Not Found".to_string());
            send_response(&mut stream, "HTTP/1.1 404 NOT FOUND", &fallback)
        }
    }
}

fn send_response(stream: &mut TcpStream, status_line: &str, body: &str) -> std::io::Result<()> {
    let response = format!(
        "{status_line}\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}
