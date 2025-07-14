
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::str::Utf8Error;
use std::u8;
use native_tls::{TlsConnector, TlsStream};
use rand::Rng;
use url::Url;
use web_socket_states::*;



pub type WebSocketResult<T> = Result<T, Box<dyn Error>>;
type WebSocketStateResult<S: WebSocketClientState> = Result<WebSocketClient<S>, WebSocketClient<S::FallbackState>>;

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


pub struct WebSocketClient<S: WebSocketClientState> {

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

    pub fn connect(self) -> WebSocketStateResult<Connected> {
        Err(self.transition())
    }

    pub fn detect_requirements(mut self, web_url: Option<&Url>) -> WebSocketStateResult<Detected> {
        if let Some(web_url) = web_url {
            match Self::probe_server_requirements(web_url) {
                Ok(headers ) => self.state.detected_headers = headers,
                Err(e) => {
                    eprintln!("{}", e);
                    return Err(self);
                }
            }
            Ok(self.transition())
        }else{
            Err(self.transition())
        }

    }

    fn probe_server_requirements(web_url: &Url) -> WebSocketResult<HashMap<String, String>> {
        let mut headers = HashMap::new();
        let host= web_url.host_str().unwrap();
        let port = web_url.port().unwrap_or(80);

        // let connector = TlsConnector::new()?;
        let mut stream = TcpStream::connect((host, port))?;
        // let mut stream = connector.connect(web_url.as_str(), stream)?;

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


impl WebSocketClient<Detected> {

    pub fn connect(mut self) -> WebSocketStateResult<Connected> {
        match Self::internal_connect(&mut self) {
            Ok(_) => Ok(self.transition()),
            Err(_) => Err(self.transition()),
        }
    }

    fn internal_connect(&mut self) -> WebSocketResult<()> {
        let host = self.state.url.host_str().unwrap();
        let port = self.state.url.port().unwrap_or(443);
        let path = self.state.url.path();

        let connector = TlsConnector::new()?;
        let stream = TcpStream::connect((host, port))?;
        let mut stream = connector.connect(host, stream)?;


        let mut rng = rand::thread_rng();
        let key_bytes: [u8; 16] = rng.r#gen();
        let key = base64::encode(&key_bytes);

        let mut request = format!(
            "GET {}{} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n",
            path, "", host, key
        );

        for (name, value) in &self.state.detected_headers {
            request.push_str(&format!("{}: {}\r\n", name, value));
        }

        request.push_str("\r\n");

        stream.write_all(request.as_bytes())?;

        let mut response = String::new();
        let mut buffer = [0; 1024];

        loop {
            let n = stream.read(&mut buffer)?;
            response.push_str(&String::from_utf8_lossy(&buffer[..n]));
            if response.contains("\r\n\r\n") {
                break;
            }
        }
        self.state.stream = Some(stream);

        if response.contains("101 Switching Protocols") {
            println!("✓ Handshake successful");
            println!("Response: {}", response);
            Ok(())
        } else if response.contains("401") {
            Err("Authentication required".into())
        } else if response.contains("403") {
            Err("Access forbidden".into())
        } else {
            Err(format!("Handshake failed: {}", response.lines().next().unwrap_or("Unknown error")).into())
        }
    }
}

struct Utf8BufferReturn<'a> {
    string: &'a str,
    offset: u8
}


impl WebSocketClient<Connected> {


    pub fn send(&mut self, message: &str) -> WebSocketResult<()> {
        let frame = self.create_frame(0x1, message.as_bytes())?;
        if let Some(ref mut stream) = self.state.stream {
            stream.write_all(&frame)?;
        }
        Ok(())
    }

    fn create_frame(&self, opcode: u8, payload: &[u8]) -> WebSocketResult<Vec<u8>> {
        let mut frame = Vec::new();

        frame.push(0x80 | opcode);

        let payload_len = payload.len();
        if payload_len < 126 {
            frame.push(0x80 | payload_len as u8);
        } else if payload_len <= 65535 {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(payload_len as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(payload_len as u64).to_be_bytes());
        }

        let mut rng = rand::thread_rng();
        let mask: [u8; 4] = rng.r#gen();
        frame.extend_from_slice(&mask);

        for (i, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[i % 4]);
        }

        Ok(frame)
    }

    fn check_valid_utf8(payload: &[u8], i: u32) -> Result<(&str, u8), Utf8Error> {
        let start = i as usize;

        if start >= payload.len() {
            return Err(std::str::from_utf8(&[]).unwrap_err());
        }

        for offset in 1..=4 {
            let end = start + offset;

            if end > payload.len() {
                break;
            }

            match std::str::from_utf8(&payload[start..end]) {
                Ok(s) => return Ok((s, offset as u8)),
                Err(_) => continue,
            }
        }

        let result = std::str::from_utf8(&payload[start..start + 1]);
        match result {
            Ok(s) => Ok((s, 1)),
            Err(e) => Err(e),
        }
    }

    fn convert_to_valid_utf8(payload: &[u8]) -> Result<Vec<String>, Utf8Error> {
        let payload_len = payload.len() as u32;
        let mut converted: Vec<String> = Vec::with_capacity(payload_len as usize);
        let mut i : u32 = 0;
        while i < payload_len {
            let (string, offset) = Self::check_valid_utf8(payload, i)?;
            converted.push(string.to_string());
            i += offset as u32;
        }
        Ok(converted)

    }

    pub fn read_message(&mut self) -> WebSocketResult<String> {
        if let Some(ref mut stream) = self.state.stream {
            let mut header = [0u8; 2];
            stream.read_exact(&mut header)?;

            let fin = (header[0] & 0x80) != 0;
            let opcode = header[0] & 0x0F;
            let masked = (header[1] & 0x80) != 0;
            let mut payload_len = (header[1] & 0x7F) as usize;

            if payload_len == 126 {
                let mut len_bytes = [0u8; 2];
                stream.read_exact(&mut len_bytes)?;
                payload_len = u16::from_be_bytes(len_bytes) as usize;
            } else if payload_len == 127 {
                let mut len_bytes = [0u8; 8];
                stream.read_exact(&mut len_bytes)?;
                let len_64 = u64::from_be_bytes(len_bytes);
                if len_64 > usize::MAX as u64 {
                    return Err("Payload too large".into());
                }
                payload_len = len_64 as usize;
            }

            let mut mask = [0u8; 4];
            if masked {
                stream.read_exact(&mut mask)?;
            }

            let mut payload = vec![0u8; payload_len];
            stream.read_exact(&mut payload)?;

            let converted = Self::convert_to_valid_utf8(&payload)?;
            let mut updated = Vec::new();
            for string in converted {
                for int in string.as_str().chars().map(|c| c as u32).collect::<Vec<u32>>() {
                    updated.push(int);
                }
            }

            if masked {
                for (i, byte) in payload.iter_mut().enumerate() {
                    *byte ^= mask[i % 4];
                }
            }

            if !fin {
                return Err("Fragmented frames not supported".into());
            }

            match opcode {
                0x1 => {
                    let decoded = Self::lzw_decode(&updated)?;
                    let combined: String = decoded.iter().collect();
                    Ok(combined)
                },
                0x8 => {
                    Err("Connection closed".into())
                },
                _ => {
                    Err(format!("Unsupported opcode: {}", opcode).into())
                }
            }
        } else {
            Err("No connection available".into())
        }
    }

    pub fn lzw_decode(data: &[u32]) -> Result<Vec<char>, Box<dyn Error>> {
        let mut dict : HashMap<u16, Vec<u32>> = HashMap::new();
        let size: u16 = 256;
        let mut dyn_size = size;

        let mut first = data[0];
        let mut dyn_first = first;
        let mut result: Vec<u32> = vec![first];


        for index in 1..data.len() {
            let value = data[index];
            let new_value : Vec<u32> = if size > value as u16 {
                vec!(data[index])
            }else {
                if let Some(v) = dict.get(&(value as u16)){
                    v.clone()
                } else{
                    vec!(first, dyn_first)
                }
            };
            result.extend_from_slice(&new_value);
            dyn_first = new_value[0];
            dict.insert(dyn_size, vec!(first, dyn_first));
            dyn_size += 1;
            first = value;
        }

        Ok(result.iter().map(|&c| char::from_u32(c).unwrap()).collect())
    }
}


impl<S> WebSocketClient<S> where S: WebSocketClientState {

    fn transition<T>(self) -> WebSocketClient<T> where T: WebSocketClientState{
        println!("Transitioning from {} to {}",
            std::any::type_name::<S>(),
            std::any::type_name::<T>());
        WebSocketClient {
            state: self.state,
            marker: std::marker::PhantomData,
        }
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