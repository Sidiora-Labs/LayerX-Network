use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use layerx_types::clock::{Clock, ClockError, ClockReading};
use layerx_types::clock_protocol::{self, Request, RESPONSE_BYTES};

#[derive(Clone, Debug)]
pub struct ClockBinding {
    pub socket: PathBuf,
    pub process: u32,
    pub user: u32,
}

impl ClockBinding {
    /// # Errors
    /// Refuses missing or malformed explicit supervisor bindings.
    pub fn from_environment() -> Result<Self, ClockError> {
        Ok(Self {
            socket: std::env::var_os("LAYERX_RUNTIME_CLOCK_SOCKET")
                .ok_or(ClockError::Unavailable)?
                .into(),
            process: number("LAYERX_RUNTIME_CLOCK_PID")?,
            user: number("LAYERX_RUNTIME_CLOCK_UID")?,
        })
    }

    fn connect(&self) -> Result<UnixStream, ClockError> {
        if !self.socket.is_absolute() || self.process == 0 {
            return Err(ClockError::Invalid);
        }
        let parent = self.socket.parent().ok_or(ClockError::Invalid)?;
        let directory = fs::symlink_metadata(parent).map_err(unavailable)?;
        let socket = fs::symlink_metadata(&self.socket).map_err(unavailable)?;
        if !directory.is_dir()
            || directory.uid() != self.user
            || directory.mode() & 0o077 != 0
            || !socket.file_type().is_socket()
            || socket.uid() != self.user
            || socket.mode() & 0o077 != 0
        {
            return Err(ClockError::Invalid);
        }
        let descriptor = rustix::net::socket_with(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::STREAM,
            rustix::net::SocketFlags::CLOEXEC | rustix::net::SocketFlags::NONBLOCK,
            None,
        )
        .map_err(unavailable)?;
        let address = rustix::net::SocketAddrUnix::new(&self.socket).map_err(unavailable)?;
        rustix::net::connect(&descriptor, &address).map_err(unavailable)?;
        let stream = UnixStream::from(descriptor);
        let peer = rustix::net::sockopt::socket_peercred(&stream).map_err(unavailable)?;
        if peer.uid.as_raw() != self.user
            || u32::try_from(peer.pid.as_raw_nonzero().get()).ok() != Some(self.process)
        {
            return Err(ClockError::Invalid);
        }
        stream.set_nonblocking(false).map_err(unavailable)?;
        Ok(stream)
    }
}

#[derive(Default)]
struct Observations {
    counter: u64,
    previous: Option<ClockReading>,
}

pub struct RuntimeClock {
    binding: ClockBinding,
    observations: Mutex<Observations>,
    active: AtomicUsize,
}

struct Permit<'a>(&'a AtomicUsize);

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

impl RuntimeClock {
    /// # Errors
    /// Refuses inaccessible private sockets, incorrect peer identity and invalid observations.
    pub fn connect(binding: ClockBinding) -> Result<Arc<Self>, ClockError> {
        let clock = Arc::new(Self {
            binding,
            observations: Mutex::new(Observations::default()),
            active: AtomicUsize::new(0),
        });
        clock.sample(Duration::from_secs(1))?;
        Ok(clock)
    }

    /// # Errors
    /// Refuses missing or malformed explicit supervisor bindings.
    pub fn from_environment() -> Result<Arc<Self>, ClockError> {
        Self::connect(ClockBinding::from_environment()?)
    }

    /// # Errors
    /// Refuses excessive waits, lost authority and malformed or regressing observations.
    pub fn wait(&self, duration: Duration) -> Result<ClockReading, ClockError> {
        let wait_nanoseconds =
            u64::try_from(duration.as_nanos()).map_err(|_| ClockError::Overflow)?;
        if wait_nanoseconds > clock_protocol::MAX_WAIT_NANOSECONDS {
            return Err(ClockError::Invalid);
        }
        self.exchange(
            wait_nanoseconds,
            duration
                .checked_add(Duration::from_secs(1))
                .ok_or(ClockError::Overflow)?,
        )
    }

    fn exchange(
        &self,
        wait_nanoseconds: u64,
        budget: Duration,
    ) -> Result<ClockReading, ClockError> {
        if budget.is_zero() {
            return Err(ClockError::Unavailable);
        }
        self.active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < 64).then_some(active + 1)
            })
            .map_err(unavailable)?;
        let _permit = Permit(&self.active);
        let counter = {
            let mut state = self.observations.lock().map_err(unavailable)?;
            state.counter = state.counter.checked_add(1).ok_or(ClockError::Overflow)?;
            state.counter
        };
        let request = Request {
            counter,
            wait_nanoseconds,
        }
        .encode()?;
        let mut stream = self.binding.connect()?;
        stream.set_read_timeout(Some(budget)).map_err(unavailable)?;
        stream
            .set_write_timeout(Some(Duration::from_secs(1).min(budget)))
            .map_err(unavailable)?;
        let cancellation = stream.try_clone().map_err(unavailable)?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("runtime-clock-read".into())
            .spawn(move || {
                let result = transact(&mut stream, &request, counter);
                let _ = sender.send(result);
            })
            .map_err(unavailable)?;
        let result = receiver.recv_timeout(budget).map_err(unavailable);
        let _ = cancellation.shutdown(Shutdown::Both);
        worker.join().map_err(unavailable)?;
        let reading = result??;
        let mut state = self.observations.lock().map_err(unavailable)?;
        if let Some(previous) = state.previous {
            reading.follows(previous)?;
        }
        state.previous = Some(reading);
        Ok(reading)
    }
}

impl Clock for RuntimeClock {
    fn sample(&self, transport_budget: Duration) -> Result<ClockReading, ClockError> {
        self.exchange(0, transport_budget.min(Duration::from_secs(1)))
    }
}

fn transact(
    stream: &mut UnixStream,
    request: &[u8],
    counter: u64,
) -> Result<ClockReading, ClockError> {
    stream.write_all(request).map_err(unavailable)?;
    stream.shutdown(Shutdown::Write).map_err(unavailable)?;
    let mut response = [0; RESPONSE_BYTES];
    stream.read_exact(&mut response).map_err(unavailable)?;
    let mut trailing = [0];
    if stream.read(&mut trailing).map_err(unavailable)? != 0 {
        return Err(ClockError::Invalid);
    }
    clock_protocol::decode_response(&response, counter)
}

fn number(name: &str) -> Result<u32, ClockError> {
    let value = std::env::var(name).map_err(unavailable)?;
    let number: u32 = value.parse().map_err(unavailable)?;
    if number.to_string() != value {
        return Err(ClockError::Invalid);
    }
    Ok(number)
}

fn unavailable<T>(_: T) -> ClockError {
    ClockError::Unavailable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_clock_and_regressing_readings_refuse() -> Result<(), ClockError> {
        let clock = RuntimeClock::from_environment()?;
        let first = clock.sample(Duration::from_secs(1))?;
        let next = clock.wait(Duration::from_millis(2))?;
        assert!(next.follows(first).is_ok());
        let mut regressed = next;
        regressed.monotonic_nanoseconds = first.monotonic_nanoseconds;
        assert_eq!(regressed.follows(next), Err(ClockError::Regression));
        let mut missing = ClockBinding::from_environment()?;
        missing.socket = missing.socket.with_extension("absent");
        assert!(RuntimeClock::connect(missing).is_err());
        assert!(clock.sample(Duration::from_secs(1)).is_ok());
        Ok(())
    }

    #[test]
    fn exhausted_client_counter_refuses_without_disabling_authority() -> Result<(), ClockError> {
        let client = RuntimeClock::from_environment()?;
        client.observations.lock().map_err(unavailable)?.counter = u64::MAX;
        assert_eq!(
            client.sample(Duration::from_secs(1)),
            Err(ClockError::Overflow)
        );
        assert!(RuntimeClock::from_environment()?
            .sample(Duration::from_secs(1))
            .is_ok());
        Ok(())
    }

    #[test]
    fn mismatched_client_generation_refuses_without_disabling_authority() -> Result<(), ClockError>
    {
        let client = RuntimeClock::from_environment()?;
        let mut previous = client.sample(Duration::from_secs(1))?;
        previous.generation[0] ^= 1;
        client.observations.lock().map_err(unavailable)?.previous = Some(previous);
        assert_eq!(
            client.sample(Duration::from_secs(1)),
            Err(ClockError::Regression)
        );
        assert!(RuntimeClock::from_environment()?
            .sample(Duration::from_secs(1))
            .is_ok());
        Ok(())
    }
}
