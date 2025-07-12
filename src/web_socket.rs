use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::io::{Read, Write};
use std::net::TcpStream;
use native_tls::{TlsConnector, TlsStream};
use rand::Rng;
use url::Url;
use web_socket_states::*;



type WebSocketResult<T> = Result<T, Box<dyn Error>>;
type WebSocketStateFail<S: WebSocketClientState> = Result<WebSocketClient<S>, WebSocketClient<S::FallbackState>>;

struct InternalWebSocketState {
    stream: Option<TlsStream<TcpStream>>,
    url: Url,
    detected_headers: HashMap<String, String>,
}

impl InternalWebSocketState {
    fn new(url: Url) -> InternalWebSocketState {
        Self {
            stream: None,
            url,
            detected_headers: HashMap::new(),
        }
    }
}


struct WebSocketClient<S: WebSocketClientState> {

    state: Box<InternalWebSocketState>,
    marker: std::marker::PhantomData<S>,
}


impl WebSocketClient<Uninitialised> {

    pub fn new(url: &str) -> WebSocketResult<WebSocketClient<Disconnected>> {
        let parsed_url = Url::parse(url)?;

        Ok(WebSocketClient {
            state: Box::new(InternalWebSocketState::new(parsed_url)),
            marker: std::marker::PhantomData,
        })
    }
}

impl WebSocketClient<Disconnected> {

    pub fn connect(self) -> WebSocketStateFail<Connected> {
        Err(self)
    }

    pub fn detect_requirements(mut self, web_url: Option<&Url>) -> WebSocketStateFail<Detected> {
        if let Some(web_url) = web_url {
            match Self::probe_server_requirements(web_url) {
                Ok(headers ) => self.state.detected_headers = headers,
                Err(_) => return Err(self),
            }
            Ok(self as WebSocketClient<Detected>)
        }else{
            Err(self)
        }

    }

    fn probe_server_requirements(web_url: &Url) -> WebSocketResult<HashMap<String, String>> {
        let mut headers = HashMap::new();
        let host= web_url.host_str().unwrap();
        let port = web_url.port().unwrap_or(80);

        let connector = TlsConnector::new()?;
        let stream = TcpStream::connect((host, port))?;
        let mut stream = connector.connect(web_url.as_str(), stream)?;

        let request = format!(
            "GET / HTTP/1.1\r\n\
             Host: {}\r\n\
             User-Agent: Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36\r\n\
             Accept: text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8\r\n\
             Accept-Language: en-US,en;q=0.5\r\n\
             Accept-Encoding: gzip, deflate\r\n\
             Connection: close\r\n\r\n",
            host
        );

        stream.write_all(request.as_bytes())?;

        let mut response = String::new();
        let mut buffer = [0; 4096];

        while let Ok(size) = stream.read(&mut buffer) {
            if size == 0 {
                break;
            }
            response.push_str(&String::from_utf8_lossy(&buffer[..size]));
            if response.len() > 1000 {
                break;
            }
        }

        if response.contains("Set-Cookie:") {
            for line in response.lines() {
                if line.starts_with("Set-Cookie:") {
                    let cookie = line.trim_start_matches("Set-Cookie:").trim();
                    if let Some(cookie_value) = cookie.split(';').next() {
                        headers.insert("Cookie".to_string(), cookie_value.to_string());
                    }
                }
            }
        }

        if response.contains("401") || response.contains("403") {
            println!("Server requires authentication");
        }

        Ok(headers)
    }
}



pub trait WebSocketClientState {
    type FallbackState;

}

#[derive(Debug)]
struct WebSocketError {

}

impl Display for WebSocketError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        todo!()
    }
}

impl Error for WebSocketError {

}


mod web_socket_states {
    use crate::web_socket::WebSocketClientState;

    pub enum Uninitialised {}
    pub enum Disconnected {}
    pub enum Detected {}
    pub enum Connected {}

    impl WebSocketClientState for Uninitialised {
        type FallbackState = Uninitialised;
    }

    impl WebSocketClientState for Disconnected {
        type FallbackState = Disconnected;
    }

    impl WebSocketClientState for Detected {
        type FallbackState = Disconnected;
    }

    impl WebSocketClientState for Connected {
        type FallbackState = Disconnected;
    }
}