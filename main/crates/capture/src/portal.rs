//! ScreenCast portal negotiation. Shows the GNOME monitor picker,
//! then hands back a PipeWire fd plus per-stream geometry used for
//! rect mapping. Pops a system dialog; never call from unit tests.

use std::os::fd::OwnedFd;

use ashpd::desktop::screencast::{
    CursorMode, OpenPipeWireRemoteOptions, Screencast, SelectSourcesOptions, SourceType,
};
use ashpd::desktop::PersistMode;
use ashpd::desktop::Session;
use peek_core::PeekError;

/// One chosen monitor stream, in compositor logical coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorStream {
    pub node_id: u32,
    pub position: Option<(i32, i32)>,
    pub size: Option<(i32, i32)>,
}

/// Live portal session. Keep alive for the whole recording; dropping
/// it does not stop the GStreamer pipeline, so call [`CastSession::close`].
pub struct CastSession {
    session: Option<Session<Screencast>>,
    fd: OwnedFd,
    pub streams: Vec<MonitorStream>,
}

pub struct CastOutcome {
    pub session: CastSession,
    pub restore_token: Option<String>,
}

fn cap<T>(r: Result<T, ashpd::Error>, what: &str) -> Result<T, PeekError> {
    r.map_err(|e| PeekError::Capture(format!("{what}: {e}")))
}

/// Run the GNOME picker on first use, then restore the saved monitor
/// choice silently on later runs (portal restore token persisted by the
/// caller). A stale token falls back to a fresh dialog automatically.
pub async fn start_monitor_cast(restore_token: Option<&str>) -> Result<CastOutcome, PeekError> {
    match try_cast(restore_token).await {
        Ok(outcome) => Ok(outcome),
        Err(_) if restore_token.is_some() => {
            eprintln!("[peek] saved monitor choice expired, asking again…");
            try_cast(None).await
        }
        Err(e) => Err(e),
    }
}

async fn try_cast(restore_token: Option<&str>) -> Result<CastOutcome, PeekError> {
    let proxy = cap(Screencast::new().await, "open screencast portal")?;
    let session = cap(
        proxy.create_session(Default::default()).await,
        "create portal session",
    )?;
    let mut select = SelectSourcesOptions::default()
        .set_cursor_mode(CursorMode::Embedded)
        .set_sources(Some(SourceType::Monitor.into()))
        .set_multiple(false)
        .set_persist_mode(PersistMode::ExplicitlyRevoked);
    if let Some(token) = restore_token {
        select = select.set_restore_token(token);
    }
    cap(
        proxy.select_sources(&session, select).await,
        "select portal sources",
    )?;
    let response = cap(
        proxy.start(&session, None, Default::default()).await,
        "start portal session",
    )?;
    let streams = cap(response.response(), "read portal response")?;
    let list: Vec<MonitorStream> = streams
        .streams()
        .iter()
        .map(|s| MonitorStream {
            node_id: s.pipe_wire_node_id(),
            position: s.position(),
            size: s.size(),
        })
        .collect();
    if list.is_empty() {
        return Err(PeekError::Capture(
            "portal returned no streams (dialog cancelled?)".into(),
        ));
    }
    let restore = streams.restore_token().map(str::to_string);
    let options = OpenPipeWireRemoteOptions::default();
    let fd = cap(
        proxy.open_pipe_wire_remote(&session, options).await,
        "open pipewire remote",
    )?;
    Ok(CastOutcome {
        session: CastSession {
            session: Some(session),
            fd,
            streams: list,
        },
        restore_token: restore,
    })
}

impl CastSession {
    pub fn fd(&self) -> &OwnedFd {
        &self.fd
    }

    /// Idempotent: safe to call on every path (normal end, early error).
    /// A second call is a no-op, so the caller can close as soon as
    /// capture ends and still have a fallback close for error paths.
    pub async fn close(&mut self) -> Result<(), PeekError> {
        let Some(session) = self.session.take() else {
            return Ok(());
        };
        cap(session.close().await, "close portal session")?;
        Ok(())
    }
}
