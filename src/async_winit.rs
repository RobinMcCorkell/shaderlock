use std::thread;

use anyhow::*;
#[allow(unused_imports)]
use log::{debug, error, info, warn};
use winit::event::*;
use winit::event_loop::*;

pub fn run<Item>(
    init: impl FnOnce() -> EventLoop<()> + Send + 'static,
    mut preprocess: impl FnMut(Event<()>, &ActiveEventLoop) -> Option<Item> + Send + 'static,
) -> tokio::sync::mpsc::Receiver<Item>
where
    Item: Send + 'static,
{
    let (tx, rx) = tokio::sync::mpsc::channel(0);

    thread::spawn(move || {
        let event_loop = init();
        event_loop
            .run(move |event, event_loop| {
                if let Some(item) = preprocess(event, event_loop) {
                    match tx.blocking_send(item) {
                        Result::Ok(()) => (),
                        Result::Err(tokio::sync::mpsc::error::SendError(_)) => {
                            // Receiver is closed which means we should exit.
                            info!("send error, exiting event loop");
                            event_loop.exit();
                        }
                    };
                    debug!("sent event");
                }
            })
            .unwrap();
    });

    rx
}
