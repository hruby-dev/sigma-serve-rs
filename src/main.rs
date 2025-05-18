use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::{PathBuf},
    sync::Arc,
};

use clap::Parser;
use log::{error, info};

#[derive(Parser)]
pub struct Args {
    /// which directory or file to serve
    root: PathBuf,

    #[arg(short, long, default_value = "localhost:8080", id = "ADDRESS:PORT")]
    /// what address and port pair to listen on
    bind: String,

    /// suffix to append to requested file names
    #[arg(short, long, default_value = ".html")]
    suffix: String,
}

fn main() -> std::io::Result<()> {
    let _ = simplelog::TermLogger::init(
        if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        },
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    );

    let mut args = Args::parse();
    args.root = fs::canonicalize(&args.root)?;
    let args = Arc::new(args);

    let listener = TcpListener::bind(&args.bind)?;
    info!("sigma-serve-rs started serving files on {}", args.bind);

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let args = Arc::clone(&args);
                std::thread::spawn(move || {
                    if let Err(e) = handle_client(stream, &args) {
                        error!("client handler error: {}", e);
                    }
                });
            }
            Err(e) => error!("failed to accept connection: {}", e),
        }
    }
    Ok(())
}

fn handle_client(mut stream: TcpStream, args: &Args) -> std::io::Result<()> {
    let buf_reader = BufReader::new(&stream);
    let request_line = match buf_reader.lines().next().transpose()? {
        Some(line) => line,
        None => return Ok(()),
    };

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    if method != "GET" {
        return send_response(
            &mut stream,
            "HTTP/1.1 405 METHOD NOT ALLOWED",
            "Method Not Allowed",
        );
    }

    let requested = if path == "/" {
        PathBuf::from("index.html")
    } else {
        PathBuf::from(format!("{}{}", path.trim_start_matches('/'), args.suffix))
    };

    let full_path = match fs::canonicalize(args.root.join(requested)) {
        Ok(full_path) => full_path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return send_not_found(&mut stream, args),
        Err(e) => return Err(e),
    };

    if !full_path.starts_with(&args.root) {
        return send_response(&mut stream, "HTTP/1.1 403 FORBIDDEN", "Forbidden");
    }

    match fs::read_to_string(&full_path) {
        Ok(contents) => send_response(&mut stream, "HTTP/1.1 200 OK", &contents),
        Err(_) => send_not_found(&mut stream, args),
    }
}

fn send_not_found(stream: &mut TcpStream, args: &Args) -> std::io::Result<()> {
    let fallback_path = args.root.join("404.html");
    let fallback =
        fs::read_to_string(&fallback_path).unwrap_or_else(|_| "404 Not Found".to_string());
    send_response(stream, "HTTP/1.1 404 NOT FOUND", &fallback)
}

fn send_response(stream: &mut TcpStream, status_line: &str, body: &str) -> std::io::Result<()> {
    let response = format!(
        "{status_line}\r\nContent-Length: {}\r\nContent-Type: text/html\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}
