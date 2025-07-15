use std::cell::RefCell;
use std::error::Error;
use std::fmt::{format, Display, Formatter};
use url::Url;
use crate::web_socket::web_socket_states::Disconnected;
use crate::web_socket::{WebSocketClient, WebSocketError};

pub(crate) mod web_socket;
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
    websocket_state: WebSocketClient<Disconnected>,
}


impl LightningTracker {

    fn new(host: LightningTrackerHost) -> Result<Self, Box<dyn Error>> {
        let url = URL.replace("{}", host.to_string().as_str());
        let ws_client = WebSocketClient::new(&url)?;
        Ok(LightningTracker {
            websocket_state: ws_client
        })
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

