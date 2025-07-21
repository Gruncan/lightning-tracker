use crate::state_log;
use base64::Engine;
use native_tls::{TlsConnector, TlsStream};
use rand::Rng;
use std::any::Any;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::str::Utf8Error;
use std::sync::{Arc, Mutex};
use std::u8;
use url::Url;
use web_socket_states::*;

use crate::prelude::*;

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
    state: Arc<Mutex<InternalWebSocketState>>,
    marker: std::marker::PhantomData<S>,
}

pub trait AnyWebSocketClientState: Send {
    fn as_any(&self) -> &dyn Any;

    fn as_any_mut(&mut self) -> &mut dyn Any;

}


impl<T> AnyWebSocketClientState for WebSocketClient<T>
where
    T: WebSocketClientState + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    // fn as_current_state<S: 'static>(&mut self) -> &mut S {
    //     self.as_any_mut().downcast_mut::<S>().unwrap()
    // }
}

impl<S: WebSocketClientState> Clone for WebSocketClient<S> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            marker: self.marker,
        }
    }
}


impl WebSocketClient<Uninitialised> {

    pub fn new(url: &str) -> WebSocketResult<WebSocketClient<Disconnected>> {
        let parsed_url = Url::parse(url)?;

        Ok(WebSocketClient {
            state: Arc::new(Mutex::new(InternalWebSocketState::new(parsed_url))),
            marker: std::marker::PhantomData,
        })
    }
}

impl WebSocketClient<Disconnected> {

    pub fn connect(self) -> WebSocketStateResult<Connected> {
        Err(self.transition())
    }

    pub fn detect_requirements(self, web_url: Option<&Url>) -> WebSocketStateResult<Detected> {
        if let Some(web_url) = web_url {
            match Self::probe_server_requirements(web_url) {
                Ok(headers ) => {
                    let mut mutex = self.state.lock().unwrap();
                    (*mutex).detected_headers = headers;
                },
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
        let mutex = self.state.lock().unwrap();
        let host = mutex.url.host_str().unwrap();
        let port = mutex.url.port().unwrap_or(443);
        let path = mutex.url.path();

        let connector = TlsConnector::new()?;
        let stream = TcpStream::connect((host, port))?;
        let mut stream = connector.connect(host, stream)?;


        let mut rng = rand::thread_rng();
        let key_bytes: [u8; 16] = rng.r#gen();
        let key = base64::prelude::BASE64_STANDARD.encode(key_bytes);

        let mut request = format!(
            "GET {}{} HTTP/1.1\r\n\
             Host: {}\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Key: {}\r\n\
             Sec-WebSocket-Version: 13\r\n",
            path, "", host, key
        );

        for (name, value) in &mutex.detected_headers {
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

        drop(mutex);
        {
            let mut mutex = self.state.lock().unwrap();
            (*mutex).stream = Some(stream);
        }

        if response.contains("101 Switching Protocols") {
            println!("Handshake successful");
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


impl WebSocketClient<Connected> {

    pub fn send(&mut self, message: &str) -> WebSocketResult<()> {
        let frame = self.create_frame(0x1, message.as_bytes())?;
        let mut mutex = self.state.lock().unwrap();
        if let Some(ref mut stream) = mutex.stream {
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

    fn check_valid_utf8(payload: &[u8], i: u32) -> Result<(u32, u8), Utf8Error> {
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
                Ok(s) =>{
                    let chars = s.chars().map(|c| c as u32).collect::<Vec<u32>>();
                    if chars.len() != 1 {
                        panic!("This should never happen!");
                    }
                    return Ok((chars[0], offset as u8));
                },
                Err(_) => continue,
            }
        }
        Err(std::str::from_utf8(&[]).unwrap_err())

    }

    pub fn convert_to_valid_utf8(payload: &[u8]) -> Result<Vec<u32>, Utf8Error> {
        let payload_len = payload.len() as u32;
        let mut converted: Vec<u32> = Vec::with_capacity(payload_len as usize);
        let mut i : u32 = 0;
        while i < payload_len {
            let (unicode, offset) = Self::check_valid_utf8(payload, i)?;
            converted.push(unicode);
            i += offset as u32;
        }
        Ok(converted)

    }

    pub fn read_message(&mut self) -> WebSocketResult<String> {
        let mut mutex = self.state.lock().unwrap();
        if let Some(ref mut stream) = mutex.stream {
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
                    let decoded = Self::lzw_decode(&converted)?;
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

    pub(crate) fn lzw_decode(data: &[u32]) -> Result<Vec<char>, Box<dyn Error>> {
        let mut dict : HashMap<u32, Vec<u32>> = HashMap::new();
        let size: u32 = 256;
        let mut dyn_size = size;

        let mut first = vec!(data[0]);
        let mut dyn_first = first.clone();
        let mut result: Vec<u32> = vec!(data[0]);


        for index in 1..data.len() {
            let value = data[index];
            let new_value : Vec<u32> = if size > value {
                vec!(data[index])
            }else if let Some(v) = dict.get(&(value)){
                v.clone()
            } else{
                let mut new_first =  first.clone();
                new_first.extend_from_slice(&dyn_first);
                new_first
            };
            result.extend_from_slice(&new_value);
            dyn_first = vec![new_value[0]];
            let mut new_first = first.clone();
            new_first.extend_from_slice(&dyn_first);
            dict.insert(dyn_size, new_first);
            dyn_size += 1;
            first = new_value;
        }

        Ok(result.iter().map(|&c| char::from_u32(c).unwrap()).collect())
    }

    pub fn close(self) -> WebSocketClient<Disconnected> {
        {
            let mut mutex = self.state.lock().unwrap();
            let stream = mutex.stream.as_mut();
            if let Some(stream) = stream {
                stream.shutdown().expect("Failed unexpected to shutdown tcp stream");
            }
        }
        self.transition()
    }
}


impl<S> WebSocketClient<S>
where
    S: WebSocketClientState,
{

    fn transition<T>(self) -> WebSocketClient<T> where T: WebSocketClientState{
        state_log!("Transitioning from {} to {}",
            std::any::type_name::<S>(),
            std::any::type_name::<T>());
        WebSocketClient {
            state: self.state,
            marker: std::marker::PhantomData,
        }
    }
}


pub trait WebSocketClientState: Send {
    type FallbackState;

}

#[derive(Debug)]
pub(crate) struct WebSocketError<'a> {
    message: &'a str
}

impl<'a> WebSocketError<'a> {
    pub(crate) fn new(message: &'a str) -> Self {
        Self { message }
    }
}

impl Display for WebSocketError<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
       write!(f, "{}", self.message)
    }
}

impl Error for WebSocketError<'_> {

}


pub(crate) mod web_socket_states {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log;

    #[test]
    fn lzw_decode_test() -> Result<(), Box<dyn Error>> {
        let bytes : [u8; 2064] = [123, 34, 116, 105, 109, 101, 34, 58, 49, 55, 53, 50, 53, 49, 52, 56, 52, 54, 48, 50, 51, 57, 52, 196, 138, 48, 48, 44, 34, 108, 97, 116, 196, 134, 51, 48, 46, 52, 196, 152, 196, 139, 196, 154, 108, 111, 110, 196, 134, 45, 57, 50, 46, 196, 138, 196, 163, 51, 49, 196, 154, 97, 108, 196, 158, 58, 196, 153, 34, 112, 111, 108, 196, 134, 196, 184, 109, 100, 115, 196, 134, 49, 49, 54, 56, 56, 196, 154, 109, 99, 103, 197, 130, 52, 52, 196, 154, 115, 116, 196, 157, 117, 197, 129, 196, 135, 196, 154, 114, 101, 103, 105, 196, 168, 196, 189, 197, 143, 105, 197, 139, 58, 91, 196, 128, 197, 144, 97, 196, 134, 50, 56, 54, 55, 196, 154, 196, 130, 196, 132, 196, 134, 52, 55, 54, 197, 166, 197, 142, 196, 155, 196, 157, 196, 159, 196, 161, 196, 145, 55, 196, 152, 197, 178, 196, 167, 196, 169, 58, 196, 171, 49, 46, 48, 53, 51, 51, 56, 51, 196, 179, 196, 181, 197, 173, 54, 197, 143, 197, 145, 116, 197, 147, 197, 130, 50, 125, 44, 197, 162, 197, 145, 197, 165, 198, 130, 196, 184, 197, 171, 196, 133, 58, 56, 197, 131, 196, 148, 197, 186, 197, 180, 58, 51, 196, 173, 53, 197, 141, 197, 133, 53, 196, 166, 197, 155, 197, 189, 196, 172, 198, 128, 48, 196, 176, 198, 138, 34, 196, 180, 196, 182, 197, 141, 198, 139, 197, 146, 197, 148, 49, 198, 144, 198, 146, 34, 197, 163, 197, 165, 49, 196, 152, 197, 170, 196, 131, 198, 153, 57, 51, 55, 198, 131, 197, 135, 197, 179, 196, 182, 196, 160, 46, 196, 141, 53, 197, 134, 197, 186, 198, 168, 196, 171, 53, 196, 162, 49, 196, 160, 55, 196, 178, 198, 175, 198, 136, 58, 52, 198, 134, 198, 185, 198, 140, 198, 142, 196, 135, 198, 183, 198, 147, 197, 164, 196, 135, 196, 141, 50, 198, 190, 197, 172, 58, 57, 54, 199, 139, 196, 140, 196, 166, 198, 159, 199, 136, 198, 129, 198, 132, 48, 199, 133, 197, 187, 196, 170, 57, 199, 144, 52, 196, 149, 54, 57, 199, 133, 198, 176, 197, 173, 199, 164, 199, 155, 198, 180, 197, 130, 48, 198, 145, 199, 160, 196, 134, 56, 48, 197, 169, 196, 129, 198, 191, 200, 129, 49, 199, 139, 54, 199, 154, 196, 156, 196, 182, 50, 57, 46, 199, 131, 196, 177, 53, 198, 174, 199, 179, 198, 169, 199, 144, 199, 170, 200, 142, 199, 190, 199, 188, 199, 152, 199, 190, 197, 163, 198, 141, 198, 181, 200, 130, 198, 184, 198, 186, 196, 135, 56, 50, 197, 178, 198, 152, 197, 130, 196, 177, 48, 196, 141, 198, 158, 200, 145, 200, 147, 56, 198, 163, 55, 50, 199, 149, 200, 153, 196, 171, 54, 196, 174, 50, 55, 196, 150, 199, 187, 199, 151, 57, 200, 136, 200, 162, 199, 157, 198, 182, 200, 131, 199, 191, 197, 130, 197, 184, 199, 154, 200, 172, 196, 135, 199, 185, 199, 153, 57, 200, 184, 199, 173, 196, 161, 199, 153, 198, 172, 197, 176, 198, 167, 197, 188, 196, 171, 55, 200, 148, 56, 199, 131, 198, 135, 200, 145, 54, 199, 149, 201, 132, 198, 181, 199, 159, 201, 136, 58, 200, 183, 197, 175, 199, 165, 198, 153, 200, 170, 196, 148, 201, 149, 199, 172, 199, 135, 52, 196, 174, 57, 196, 172, 54, 200, 152, 199, 142, 197, 167, 200, 188, 55, 200, 134, 57, 201, 158, 197, 173, 198, 130, 198, 179, 200, 163, 198, 143, 201, 135, 200, 167, 197, 132, 201, 190, 200, 137, 199, 166, 200, 170, 196, 138, 200, 140, 200, 136, 200, 144, 196, 159, 201, 176, 200, 180, 196, 177, 200, 140, 201, 151, 196, 170, 201, 183, 199, 131, 196, 163, 53, 198, 166, 199, 150, 200, 145, 196, 137, 201, 191, 201, 133, 201, 164, 200, 167, 55, 201, 178, 201, 169, 197, 165, 200, 142, 196, 172, 200, 151, 201, 174, 196, 159, 199, 144, 200, 182, 199, 185, 196, 165, 196, 155, 201, 182, 200, 187, 197, 134, 197, 133, 201, 156, 201, 188, 201, 166, 198, 178, 199, 191, 202, 128, 196, 135, 200, 165, 200, 132, 199, 162, 201, 185, 202, 162, 198, 160, 196, 146, 201, 156, 197, 166, 202, 167, 198, 160, 201, 176, 48, 196, 149, 199, 148, 202, 152, 200, 185, 198, 133, 46, 199, 129, 48, 199, 169, 198, 174, 200, 159, 199, 130, 197, 178, 201, 162, 200, 129, 202, 130, 198, 148, 199, 162, 198, 155, 202, 189, 196, 160, 201, 144, 203, 128, 203, 130, 51, 199, 144, 51, 197, 141, 199, 186, 199, 190, 203, 137, 201, 176, 197, 184, 199, 139, 198, 131, 202, 179, 200, 189, 201, 161, 199, 156, 197, 148, 202, 158, 203, 150, 200, 146, 199, 181, 203, 153, 196, 160, 196, 163, 200, 142, 203, 157, 200, 147, 198, 172, 198, 182, 202, 160, 202, 146, 198, 169, 201, 176, 199, 139, 200, 146, 198, 161, 203, 169, 200, 190, 202, 156, 197, 148, 52, 203, 149, 199, 161, 200, 146, 196, 163, 203, 153, 197, 176, 197, 168, 202, 155, 199, 134, 197, 173, 196, 161, 197, 132, 203, 191, 200, 143, 199, 142, 198, 170, 196, 145, 200, 135, 198, 163, 203, 169, 199, 129, 204, 131, 202, 129, 200, 166, 203, 150, 202, 160, 199, 149, 201, 140, 203, 145, 52, 198, 188, 203, 163, 199, 173, 56, 46, 50, 200, 183, 48, 203, 158, 203, 187, 45, 196, 143, 46, 198, 155, 199, 131, 199, 186, 203, 169, 200, 170, 204, 155, 202, 184, 204, 134, 197, 181, 50, 198, 151, 200, 138, 198, 160, 200, 190, 199, 169, 203, 152, 204, 142, 198, 160, 204, 167, 204, 169, 197, 131, 201, 144, 204, 173, 204, 175, 204, 177, 51, 203, 143, 199, 151, 204, 169, 201, 131, 203, 172, 204, 156, 202, 186, 196, 141, 196, 148, 203, 153, 200, 190, 202, 143, 201, 145, 199, 135, 205, 131, 200, 183, 197, 131, 203, 136, 201, 182, 202, 142, 196, 136, 199, 153, 205, 139, 200, 145, 202, 145, 202, 182, 202, 157, 204, 184, 58, 197, 133, 201, 187, 202, 134, 198, 153, 199, 130, 201, 180, 54, 204, 163, 203, 157, 205, 153, 197, 131, 199, 178, 205, 157, 204, 176, 205, 159, 205, 138, 204, 180, 202, 152, 203, 147, 201, 166, 205, 166, 196, 139, 201, 160, 203, 153, 200, 180, 203, 155, 204, 165, 199, 135, 200, 187, 202, 151, 196, 147, 202, 133, 203, 137, 198, 162, 196, 147, 57, 204, 171, 202, 179, 199, 130, 205, 142, 200, 128, 199, 152, 205, 166, 200, 140, 205, 147, 205, 170, 196, 159, 197, 167, 203, 158, 198, 182, 203, 130, 196, 163, 196, 162, 197, 133, 199, 183, 205, 179, 201, 152, 206, 139, 46, 199, 185, 201, 180, 202, 151, 203, 169, 196, 141, 204, 182, 204, 133, 204, 157, 199, 161, 196, 136, 198, 189, 206, 149, 199, 152, 196, 152, 198, 129, 206, 173, 202, 140, 206, 175, 205, 181, 199, 186, 197, 174, 205, 135, 200, 147, 197, 176, 199, 146, 205, 163, 200, 159, 196, 147, 203, 146, 205, 143, 206, 145, 206, 170, 197, 165, 197, 167, 200, 171, 204, 188, 196, 163, 199, 153, 197, 141, 206, 130, 202, 168, 198, 128, 198, 132, 205, 172, 205, 135, 196, 161, 198, 133, 197, 175, 196, 172, 204, 180, 199, 133, 205, 186, 206, 169, 202, 186, 200, 170, 197, 134, 202, 189, 204, 163, 55, 199, 131, 196, 143, 203, 130, 204, 169, 199, 137, 199, 153, 203, 176, 204, 173, 198, 188, 197, 191, 204, 171, 196, 139, 197, 132, 202, 179, 49, 199, 181, 206, 191, 206, 144, 201, 134, 207, 130, 201, 166, 199, 139, 204, 187, 199, 166, 52, 196, 139, 202, 151, 199, 168, 206, 154, 197, 191, 199, 131, 197, 175, 203, 141, 204, 173, 199, 129, 206, 162, 196, 146, 198, 132, 205, 161, 197, 165, 201, 130, 206, 168, 206, 146, 199, 131, 202, 152, 201, 140, 199, 153, 207, 179, 205, 163, 206, 179, 204, 163, 196, 174, 199, 177, 205, 190, 202, 173, 201, 152, 197, 134, 199, 137, 199, 148, 197, 174, 201, 128, 196, 182, 49, 200, 134, 208, 135, 207, 177, 200, 189, 203, 133, 207, 155, 207, 182, 205, 174, 199, 183, 207, 186, 199, 145, 197, 133, 204, 130, 208, 148, 202, 147, 201, 154, 206, 147, 200, 142, 199, 154, 206, 189, 206, 148, 207, 150, 206, 146, 208, 156, 200, 136, 208, 139, 53, 198, 172, 199, 130, 205, 151, 199, 189, 206, 156, 199, 146, 198, 172, 207, 191, 196, 173, 199, 130, 199, 177, 207, 182, 204, 153, 196, 184, 205, 186, 203, 174, 206, 171, 196, 136, 208, 138, 207, 134, 196, 140, 196, 147, 199, 146, 206, 154, 196, 173, 196, 144, 198, 163, 207, 147, 208, 170, 198, 169, 197, 191, 201, 156, 199, 168, 196, 144, 204, 129, 199, 154, 209, 134, 205, 188, 53, 203, 177, 206, 174, 196, 150, 200, 174, 208, 169, 208, 143, 197, 191, 202, 177, 200, 151, 199, 141, 206, 160, 204, 167, 196, 152, 199, 146, 204, 172, 202, 153, 196, 134, 198, 163, 203, 171, 206, 144, 207, 151, 201, 165, 207, 172, 206, 148, 208, 139, 197, 168, 198, 133, 196, 142, 203, 157, 200, 187, 198, 172, 199, 168, 202, 166, 209, 148, 45, 202, 160, 198, 128, 203, 128, 199, 169, 207, 171, 208, 134, 205, 164, 204, 132, 205, 188, 200, 146, 204, 160, 207, 134, 199, 148, 199, 177, 209, 163, 198, 159, 206, 155, 197, 134, 200, 190, 210, 135, 203, 137, 51, 204, 176, 197, 134, 199, 168, 205, 169, 206, 189, 208, 165, 210, 136, 197, 173, 209, 157, 206, 139, 207, 155, 207, 157, 197, 184, 209, 147, 206, 179, 203, 158, 204, 176, 201, 178, 197, 174, 200, 184, 199, 142, 198, 188, 200, 187, 200, 189, 207, 182, 199, 177, 204, 180, 198, 129, 208, 158, 207, 152, 198, 163, 201, 139, 207, 134, 201, 144, 201, 130, 206, 140, 205, 129, 208, 144, 199, 183, 209, 137, 201, 181, 208, 149, 210, 151, 200, 182, 209, 171, 208, 132, 200, 168, 207, 174, 202, 183, 209, 179, 202, 131, 207, 172, 205, 169, 208, 139, 196, 172, 204, 171, 197, 184, 208, 166, 210, 190, 207, 157, 211, 128, 202, 147, 211, 130, 204, 186, 207, 168, 207, 171, 205, 128, 209, 156, 207, 177, 207, 172, 210, 157, 201, 140, 208, 183, 202, 165, 204, 152, 205, 129, 196, 148, 206, 162, 207, 182, 56, 209, 159, 200, 185, 198, 188, 203, 165, 198, 132, 196, 172, 200, 158, 199, 151, 196, 136, 202, 188, 210, 158, 207, 129, 207, 152, 199, 153, 210, 183, 199, 166, 209, 140, 196, 142, 209, 173, 208, 143, 201, 176, 198, 188, 207, 132, 211, 148, 198, 169, 210, 151, 204, 170, 198, 161, 200, 136, 203, 144, 207, 157, 210, 180, 201, 165, 50, 197, 168, 198, 174, 211, 159, 197, 132, 203, 162, 208, 186, 204, 189, 203, 139, 208, 141, 203, 163, 199, 142, 201, 185, 200, 188, 197, 134, 211, 162, 200, 159, 208, 156, 209, 155, 207, 128, 211, 137, 203, 175, 196, 151, 202, 189, 196, 138, 203, 140, 209, 131, 211, 163, 200, 147, 212, 141, 201, 185, 206, 159, 196, 170, 212, 149, 201, 130, 199, 181, 201, 168, 209, 174, 198, 160, 203, 129, 211, 178, 212, 157, 206, 171, 208, 183, 209, 138, 211, 183, 205, 138, 201, 156, 206, 183, 210, 188, 203, 165, 196, 142, 200, 135, 202, 139, 204, 148, 197, 191, 202, 151, 198, 157, 212, 132, 205, 140, 202, 161, 212, 177, 205, 188, 201, 148, 212, 160, 209, 161, 207, 136, 209, 187, 200, 148, 212, 188, 199, 153, 207, 165, 199, 177, 204, 168, 199, 148, 196, 143, 208, 175, 211, 175, 212, 138, 208, 135, 93, 196, 154, 100, 101, 196, 156, 121, 197, 173, 199, 137, 201, 151, 99, 197, 156, 199, 134, 213, 162, 196, 183, 125];
        let utf8bytes = WebSocketClient::convert_to_valid_utf8(&bytes)?;
        log!("{:?}", bytes);
        log!("{:?}", utf8bytes);
        let result = WebSocketClient::lzw_decode(&utf8bytes)?;
        let result_string: String = result.iter().collect();
        log!("{}", result_string);

        Ok(())

    }
}