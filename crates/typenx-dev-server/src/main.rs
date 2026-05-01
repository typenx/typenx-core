use std::{
    env,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
};

fn main() -> std::io::Result<()> {
    let bind_addr = env::var("TYPENX_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_owned());
    let listener = TcpListener::bind(&bind_addr)?;
    println!("typenx-dev-server listening on http://{bind_addr}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(error) = handle_connection(stream) {
                    eprintln!("request failed: {error}");
                }
            }
            Err(error) => eprintln!("connection failed: {error}"),
        }
    }

    Ok(())
}

fn handle_connection(mut stream: TcpStream) -> std::io::Result<()> {
    let mut buffer = [0; 2048];
    let bytes_read = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    let (status, content_type, body) = match path {
        "/health" => (
            "200 OK",
            "application/json",
            r#"{"ok":true,"service":"typenx-dev-server"}"#.to_owned(),
        ),
        "/openapi.json" => ("200 OK", "application/json", openapi_placeholder()),
        _ => (
            "404 Not Found",
            "application/json",
            r#"{"message":"not found"}"#.to_owned(),
        ),
    };

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes())
}

fn openapi_placeholder() -> String {
    r#"{
  "openapi": "3.1.0",
  "info": {
    "title": "Typenx Core Dev Server",
    "version": "0.1.0"
  },
  "paths": {
    "/health": {
      "get": {
        "responses": {
          "200": {
            "description": "Dev server health response"
          }
        }
      }
    }
  }
}"#
    .to_owned()
}
