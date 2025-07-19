use crate::web_socket::web_socket_states::{Connected, Disconnected};
use crate::web_socket::{AnyWebSocketClientState, WebSocketClient, WebSocketError};
use std::error::Error;
use std::fmt::{Display, Formatter};
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
    websocket_state: Box<dyn AnyWebSocketClientState>,
    is_connected: bool,
}


impl LightningTracker {

    fn new(host: LightningTrackerHost) -> Result<Self, Box<dyn Error>> {
        let url = URL.replace("{}", host.to_string().as_str());
        let ws_client = WebSocketClient::new(&url)?;
        Ok(LightningTracker {
            websocket_state: Box::new(ws_client),
            is_connected: false,
        })
    }


    async fn open_connection(mut self) -> Result<(), Box<dyn Error>> {
        let request_url = Url::parse("https://www.blitzortung.org/en/live_lightning_maps.php")?;
        let any_state = self.websocket_state.as_any_mut();
        let current_state = any_state.downcast_mut::<WebSocketClient<Disconnected>>().unwrap();
        let copy_state = current_state.clone();
        if let Ok(detected) = copy_state.detect_requirements(Some(&request_url)) {
            if let Ok(connected) = detected.connect(){
                self.websocket_state = Box::new(connected);
                self.is_connected = true;
            }else{
                return Err(Box::new(WebSocketError::new("Failed to connect requirements of server")));
            }
        } else {
            return Err(Box::new(WebSocketError::new("Failed to detect requirements of server")));
        }

        Ok(())
    }

    async fn close_connection(mut self) -> Result<(), Box<dyn Error>> {
        let any_state = self.websocket_state.as_any_mut();
        let current_state = any_state.downcast_mut::<WebSocketClient<Connected>>().unwrap();
        let copy_state = current_state.clone();
        self.websocket_state = Box::new(copy_state.close());
        self.is_connected = false;
        Ok(())
    }
}

pub(crate) mod logger {

    #[macro_export]
    macro_rules! debug_log {
        ($($arg:tt)*) => {
            #[cfg(debug_assertions)]
            {
                //let now = Local::now().format("%Y-%m-%d %H:%M:%S");
                let label = format!("[DEBUG] {}", format!($($arg)*)).bold().yellow();
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

