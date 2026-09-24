use crate::{
    bidirectional_stream::{BidirectionalStream, Direction},
    channel::ChannelCentral,
    conn::ChannelConn,
    handler::ChannelHandler,
};
use actix_web::{Error, HttpRequest, HttpResponse, web};
use actix_ws::{AggregatedMessage, handle};
use futures_lite::StreamExt;
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::interval;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
pub struct ProtocolVersion {
    vsn: Option<String>,
}

pub fn configure_channel_websocket_route<T: ChannelHandler>(
    cfg: &mut web::ServiceConfig,
    channel: Arc<ChannelCentral<T>>,
) {
    cfg.app_data(web::Data::from(channel));
    cfg.route(
        "/socket/websocket",
        web::get().to(phoenix_channel_handler::<T>),
    );
}

pub async fn phoenix_channel_handler<T: ChannelHandler>(
    req: HttpRequest,
    body: web::Payload,
    version: web::Query<ProtocolVersion>,
    channel: web::Data<ChannelCentral<T>>,
) -> Result<HttpResponse, Error> {
    if let Some(vsn) = version.vsn.as_ref() {
        if vsn != "2.0.0" {
            return Ok(HttpResponse::BadRequest().body("unsupported protocol version"));
        }
    } else {
        return Ok(HttpResponse::BadRequest().body("protocol version is required"));
    }

    let (res, session, msg_stream) = handle(&req, body)?;

    let client_stream = msg_stream
        .aggregate_continuations()
        .max_continuation_size(2_usize.pow(20)); // 1MiB

    let mut last_heartbeat = Instant::now();
    let mut interval = interval(HEARTBEAT_INTERVAL);

    actix_web::rt::spawn(async move {
        let (client, receiver) = channel.build_client();

        let mut shared_stream = std::pin::pin!(BidirectionalStream {
            inbound: Some(client_stream),
            outbound: receiver,
        });

        let mut conn = ChannelConn::new(session, client);

        let close_reason = loop {
            tokio::select! {
                maybe_msg = shared_stream.next() => {
                    match maybe_msg {
                        Some(msg) => match msg{
                            Direction::Inbound(Ok(AggregatedMessage::Close(reason))) => {
                                break reason;
                            }
                            Direction::Inbound(Ok(msg)) => {
                                last_heartbeat = Instant::now();
                                handle_message(msg, &mut conn, &channel).await;
                            }
                            Direction::Outbound(msg) => {
                                if let Err(e) = conn.session().text(msg).await {
                                    tracing::warn!("outbound websocket error: {:?}", e);
                                    break None;
                                }
                            }
                            _ => {
                                break None;
                            }
                        }
                        // stream error or client terminated connection
                        None => { break None; }
                    }
                }
                _ = interval.tick() => {
                    if last_heartbeat.elapsed() > CLIENT_TIMEOUT {
                        break None;
                    }
                }
            }
        };

        let _ = conn.session.close(close_reason).await;
    });

    Ok(res)
}

async fn handle_message<T: ChannelHandler>(
    msg: AggregatedMessage,
    conn: &mut ChannelConn,
    channel: &ChannelCentral<T>,
) {
    let event = match conn.client().deserialize(msg) {
        Some(event) => event,
        None => return, // ignore invalid message
    };
    match (event.topic(), event.event()) {
        ("phoenix", "heartbeat") => {
            tracing::trace!("heartbeat");
            conn.client().reply_ok(&event, &()).await;
        }
        (_, "phx_join") => {
            channel.handler().join_channel(conn, event).await;
        }
        (_, "phx_leave") => {
            channel.handler().leave_channel(conn, event).await;
        }
        _ => {
            channel.handler().incoming_message(conn, event).await;
        }
    }
}
