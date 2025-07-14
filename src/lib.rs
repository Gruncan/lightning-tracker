use std::io::Error;
use url::Url;
use crate::web_socket::{WebSocketClient, WebSocketClientState, WebSocketResult};

mod web_socket;



fn test_main() -> WebSocketResult<()> {
    let request_url = Url::parse("https://www.blitzortung.org/en/live_lightning_maps.php")?;
    let mut state_machine = WebSocketClient::new("wss://ws7.blitzortung.org/")?;
    let requirements = state_machine.detect_requirements(Some(&request_url));
    match requirements {
        Ok(r) => {
            let connected = r.connect();
            if let Ok(c) = connected {
                println!("Connected");
            } else {
                eprintln!("Unable to connect to web socket!");
                return Err(Error::last_os_error().into())
            }
            Ok(())
        },
        Err(d) => {
            eprintln!("Failed to find requirements");
            Err(Error::last_os_error().into())
        },
    }
}


#[cfg(test)]
mod tests {
    use crate::web_socket::WebSocketClient;
    use super::*;
    
    #[test]
    fn it_works() {
        match test_main() {
            Ok(_) => println!("Works"),
            Err(e) => eprintln!("Error: {}", e),
        }
    }
}
