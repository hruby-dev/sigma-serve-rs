use std::{
    fs,
    io::{self, BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
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

struct Request {
    pub path: String,
    pub raw_path: String,
    pub method: String,
}

struct Response {
    pub status_code: i32,
    pub status_message: String,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status_code: i32, status_message: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            status_code,
            status_message: status_message.into(),
            body,
        }
    }

    pub fn write(&self, stream: &mut TcpStream) -> io::Result<()> {
        stream.write_all(
            format!(
                "HTTP/1.1 {} {}\r\nContent-Length: {}\r\n\r\n",
                self.status_code,
                self.status_message,
                self.body.len()
            )
            .as_bytes(),
        )?;
        stream.write_all(&self.body)?;
        stream.flush()
    }
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

    loop {
        let Ok((mut stream, addr)) = listener.accept() else {
            continue;
        };

        let args = Arc::clone(&args);
        std::thread::spawn(move || {
            let request = match parse_request(&mut stream) {
                Ok(request) => request,
                Err(e) => {
                    match e.kind() {
                        io::ErrorKind::ConnectionReset => return, // can also mean not a HTTP connection (not a relevant error so not logged)
                        io::ErrorKind::InvalidData => return, // invalid UTF-8 (probably better to return a 400 Bad Request but eh)
                        _ => {
                            error!("request parser error: {e}");
                            return;
                        }
                    }
                }
            };

            let response = match prepare_response(&request, &args) {
                Ok(response) => response,
                Err(e) => {
                    error!("client handler error: {e}");
                    Response::new(
                        500,
                        "Internal Server Error",
                        "500 Internal Server Error".as_bytes().to_vec(),
                    )
                }
            };

            info!(
                "{:?} - \"{} {}\" - {}",
                addr.ip(),
                request.method,
                request.raw_path,
                response.status_code
            );

            let _ = response.write(&mut stream);
        });
    }
}

fn parse_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    let buf_reader = BufReader::new(stream);
    let request_line = match buf_reader.lines().next().transpose()? {
        Some(line) => line,
        None => return Err(io::ErrorKind::ConnectionReset.into()),
    };

    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    Ok(Request {
        path: urlencoding::decode(path)
            .map_err(|_| io::ErrorKind::InvalidData)?
            .to_string(),
        raw_path: path.to_string(),
        method: method.to_string(),
    })
}

fn prepare_response(request: &Request, args: &Args) -> std::io::Result<Response> {
    if request.method != "GET" {
        return Ok(Response::new(
            405,
            "Method Not Allowed",
            "405 Method Not Allowed".as_bytes().to_vec(),
        ));
    }

    let requested = if request.path == "/" {
        PathBuf::from("index.html")
    } else {
        let decoded =
            match urlencoding::decode(request.path.strip_prefix('/').unwrap_or(&request.path)) {
                Ok(decoded) => decoded,
                Err(_) => {
                    return Ok(Response::new(
                        400,
                        "Bad Request",
                        "400 Bad Request".as_bytes().to_vec(),
                    ));
                }
            };
        PathBuf::from(format!("{}{}", decoded, args.suffix))
    };

    let full_path = match fs::canonicalize(args.root.join(requested)) {
        Ok(full_path) => full_path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(not_found(args)),
        Err(e) => return Err(e),
    };

    if !full_path.starts_with(&args.root) {
        return Ok(not_found(args));
    }

    Ok(match fs::read(&full_path) {
        Ok(contents) => Response::new(200, "Ok", contents),
        Err(_) => not_found(args),
    })
}

fn not_found(args: &Args) -> Response {
    let fallback_path = args.root.join("404.html");
    let fallback =
        fs::read_to_string(&fallback_path).unwrap_or_else(|_| "404 Not Found".to_string());
    return Response::new(404, "Not Found", fallback.as_bytes().to_vec());
}
