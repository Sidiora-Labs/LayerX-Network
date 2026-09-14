use super::{checked, Result};
use layerx_agentd::human::{serve_one, HumanPeer};
use layerx_agentd::human_runtime::{RemoteHumanAuthority, UnifiedAgentOwner};
use layerx_client::lni::transport::{FrameTransport, Limits, TransportError};
use layerx_human_service::server::agent_runtime::{AgentOwnerInstall, AgentRuntime};
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::Duration;

struct Stream(UnixStream);
impl FrameTransport for Stream {
    fn send(&mut self, bytes: &[u8]) -> std::result::Result<(), TransportError> {
        layerx_client::lni::framing::write_frame(&mut self.0, bytes, 1_048_576)
    }
    fn receive(&mut self) -> std::result::Result<Vec<u8>, TransportError> {
        layerx_client::lni::framing::read_frame(&mut self.0, 1_048_576)
    }
}

pub fn install(
    path: &Path,
    owner: &mut UnifiedAgentOwner<RemoteHumanAuthority>,
    peer: &HumanPeer,
    registry: layerx_types::payload::ModuleRegistry,
    request: &AgentOwnerInstall,
) -> Result<()> {
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    let result = std::thread::scope(|scope| {
        let server = scope.spawn(|| -> std::result::Result<(), String> {
            let mut run = || -> Result<()> {
                let (stream, _) = listener.accept()?;
                let credentials = rustix::net::sockopt::socket_peercred(&stream)?;
                assert_eq!(credentials.uid.as_raw(), peer.uid);
                assert_eq!(credentials.gid.as_raw(), 4021);
                stream.set_read_timeout(Some(Duration::from_secs(15)))?;
                stream.set_write_timeout(Some(Duration::from_secs(15)))?;
                let transport = HumanPeer {
                    uid: peer.uid,
                    principal: "owner".to_owned(),
                    tenant: "native-managed-limit".to_owned(),
                    subject: None,
                };
                checked(serve_one(&mut Stream(stream), &transport, owner))
            };
            run().map_err(|error| error.to_string())
        });
        let runtime = checked(AgentRuntime::new(
            path,
            Limits {
                maximum_frame_bytes: 1_048_576,
                maximum_connections: 1,
                maximum_streams: 1,
                maximum_queued_bytes: 1_048_576,
                deadline: Duration::from_secs(15),
            },
            registry,
        ))?;
        let subject = peer
            .subject
            .as_ref()
            .ok_or("actual provider subject missing")?;
        let mut runtime = checked(runtime.for_subject(
            &checked(layerx_human_service::store::PrincipalId::new(
                &peer.principal,
            ))?,
            &checked(layerx_types::ids::Did::new(subject.owner.as_bytes()))?,
            &checked(layerx_types::account::AccountId::parse(&subject.account))?,
            subject.asset,
        ))?;
        let installed = runtime.owner_install(
            8700,
            super::creation::SESSION_ACTION,
            checked(request.body_digest())?,
            request,
        );
        server
            .join()
            .map_err(|_| "owner listener panicked")?
            .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;
        let installed = checked(installed)?;
        assert_eq!(installed.session_id, super::creation::SESSION_ACTION);
        assert!(installed.generation > 0);
        Ok(())
    });
    std::fs::remove_file(path)?;
    result
}
