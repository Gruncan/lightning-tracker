use crate::prelude::*;
use crate::web_socket::web_socket_states::{Connected, Disconnected};
use crate::web_socket::{AnyWebSocketClientState, WebSocketClient, WebSocketClientState, WebSocketError};
use std::any::Any;
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::mpsc::Receiver;
use std::sync::{mpsc, Arc, Mutex};
use url::Url;

pub/*(crate)*/ mod web_socket;
pub(crate) mod prelude;

static URL: &str = "wss://{}.blitzortung.org/";

#[derive(Debug)]
enum LightningTrackerHost {
    WS7,
}

impl Display for LightningTrackerHost {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

struct LightningTracker {
    websocket_state: Arc<Mutex<Box<dyn AnyWebSocketClientState>>>,
    is_connected: bool,
}


#[derive(Debug, Deserialize)]
struct LightningSignal {
    sta: u32,
    time: u64,
    lat: f64,
    lon: f64,
    alt: u16,
    status: u8,
}

#[derive(Debug, Deserialize)]
struct LightningData {
    time: u64,
    lat: f64,
    lon: f64,
    alt: f64,
    pol: f64,
    mds: u32,
    mcg: u32,
    status: u16,
    region: u8,
    sig: Vec<LightningSignal>,
    delay: f32,
    lonc: u32,
    latc: u32,
}

struct LightningStream {
    rx: Receiver<LightningData>,
}

impl LightningStream {
    pub(crate) fn new(rx: Receiver<LightningData>) -> Self {
        Self { rx }
    }

    pub async fn next(&mut self) -> Option<LightningData> {
        self.rx.recv().ok()
    }
}


impl LightningTracker {

    fn new(host: LightningTrackerHost) -> Result<Self, Box<dyn Error>> {
        let url = URL.replace("{}", host.to_string().as_str());
        let ws_client = WebSocketClient::new(&url)?;
        Ok(LightningTracker {
            websocket_state: Arc::new(Mutex::new(Box::new(ws_client))),
            is_connected: false,
        })
    }


    async fn open_connection(mut self) -> Result<(), Box<dyn Error>> {
        let request_url = Url::parse("https://www.blitzortung.org/en/live_lightning_maps.php")?;
        {
            let mut mutex = self.websocket_state.lock().unwrap();
            let any_state = mutex.as_any_mut();
            let current_state = any_state.downcast_mut::<WebSocketClient<Disconnected>>().unwrap();
            let copy_state = current_state.clone();
            if let Ok(detected) = copy_state.detect_requirements(Some(&request_url)) {
                if let Ok(connected) = detected.connect() {
                    *mutex = Box::new(connected);
                    self.is_connected = true;
                } else {
                    return Err(Box::new(WebSocketError::new("Failed to connect requirements of server")));
                }
            } else {
                return Err(Box::new(WebSocketError::new("Failed to detect requirements of server")));
            }
        }
        

        Ok(())
    }

    async fn close_connection(mut self) -> Result<(), Box<dyn Error>> {
        {
            let mut mutex = self.websocket_state.lock().unwrap();
            let any_state = mutex.as_any_mut();
            let current_state = any_state.downcast_mut::<WebSocketClient<Connected>>().unwrap();
            let copy_state = current_state.clone();
            *mutex = Box::new(copy_state.close());
            self.is_connected = false;
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<LightningStream, Box<dyn Error>> {
        if !self.is_connected {
            return Err(Box::new(WebSocketError::new("Not connected")));
        }

        let (tx, rx) = mpsc::channel::<LightningData>();
        let arc_state = Arc::clone(&self.websocket_state);

        std::thread::spawn(move || {
            let mut mutex = arc_state.lock().unwrap();
            let any_state = mutex.as_any_mut();
            let current_state = any_state.downcast_mut::<WebSocketClient<Connected>>().unwrap();
            current_state.send("{\"a\":111}").expect("Failed to send message to websocket");
            loop {
                match current_state.read_message() {
                    Ok(message) => {
                        if let Ok(lightning_data) = serde_json::from_str(message.as_str()) {
                            let lightning_data: LightningData = lightning_data;
                            tx.send(lightning_data).expect("Failed to send lightning data to channel");
                        }
                    },
                    Err(e) => {
                        println!("Failed to read message from websocket: {}", e);
                        break;
                    },
                }
            }
        });

        Ok(LightningStream::new(rx))
    }
}

pub(crate) mod logger {

    #[macro_export]
    macro_rules! debug_log {
        ($($arg:tt)*) => {
            #[cfg(debug_assertions)]
            {
                //let now = Local::now().format("%Y-%m-%d %H:%M:%S");
                let label = format!("[DEBUG] {:?}", format!($($arg)*)).bold().yellow();
                println!("{}", label);
            }
        };
    }

    #[macro_export]
    macro_rules! state_log {
        ($($arg:tt)*) => {
            #[cfg(debug_assertions)]
            {
                //let now = Local::now().format("%Y-%m-%d %H:%M:%S");
                let label = format!("[STATE] {}", format!($($arg)*)).bold().blue();
                println!("{}", label);
            }
        };
    }


    #[macro_export]
    macro_rules! log {
        ($($arg:tt)*) => {
            //let now = Local::now().format("%Y-%m-%d %H:%M:%S");
            println!("{}", format!($($arg)*));
        };
    }
}

