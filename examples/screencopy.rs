use anyhow::*;
use futures::StreamExt;
#[allow(unused_imports)]
use log::{debug, error, info, warn};
use shaderlock::{
    screencopy::ScreencopyHandler,
    window_manager::{Event, WindowManager},
};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let mut wm = WindowManager::new()?;
    wm.run(|conn, qh, mut state, events| async move {
        debug!("awaiting events");
        let event = events.next().await.context("events stream was closed")?;
        debug!("got event: {:?}", event);
        match event {
            Event::NewOutput(output) => {
                let frame_handle = state
                    .access(|s| {
                        debug!("capture frame on output: {:?}", output);
                        let res = s.screencopy_state().capture_output(&output, &qh);
                        conn.flush()?;
                        res
                    })?
                    .await??;
                debug!("capture complete, getting buffer data");
                let frame = state.access(|s| s.get_buffer_data(frame_handle));
                debug!("got buffer data");
                drop(frame);
                Ok(())
            }
            _ => unimplemented!(),
        }
    })
    .await
}
