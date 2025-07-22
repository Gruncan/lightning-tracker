use std::error::Error;
use url::Url;
use lightning_tracker::web_socket::{WebSocketClient, WebSocketResult};
use lightning_tracker::*;

//
// fn test_main_old() -> WebSocketResult<()> {
//     let request_url = Url::parse("https://www.blitzortung.org/en/live_lightning_maps.php")?;
//     let state_machine = WebSocketClient::new("wss://ws7.blitzortung.org/")?;
//     let requirements = state_machine.detect_requirements(Some(&request_url));
//     match requirements {
//         Ok(r) => {
//             let connected = r.connect();
//             if let Ok(mut c) = connected {
//                 c.send("{\"a\":111}")?;
//                 loop {
//                     match c.read_message() {
//                         Ok(message) => {
//                             println!("Received: {}", message);
//                         }
//                         Err(e) => {
//                             eprintln!("{:?}", e);
//                             break;
//                         }
//                     }
//                 }
//             } else {
//                 eprintln!("Unable to connect to web socket!");
//                 return Err(Error::last_os_error().into())
//             }
//             Ok(())
//         },
//         Err(d) => {
//             eprintln!("Failed to find requirements");
//             Err(Error::last_os_error().into())
//         },
//     }
// }




#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>>{
    let mut lt = LightningTracker::new(LightningTrackerHost::WS7)?;
    lt.open_connection().await?;
    let mut data_stream = lt.receive().await?;

    while let Some(data) = data_stream.next().await {
        println!("[{}] {:.6} {:.6}", data.get_date_time(), data.lat, data.lon);
    }

    Ok(())
}
