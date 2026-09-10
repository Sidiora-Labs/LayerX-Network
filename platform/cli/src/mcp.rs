use std::io;
use std::path::Path;

use layerx_mcp::binding::Binding;

/// Serves the daemon-bound tool catalogue on standard input and output.
///
/// The served path holds no signing seed and no gateway credential: every tool call is
/// authorized by the agent daemon named in the binding document and executed against that
/// daemon's verified read surface.
pub fn serve(binding: &Path, read_only: bool) -> Result<(), String> {
    let mut declared = Binding::open(binding).map_err(|error| error.detail())?;
    if read_only {
        declared.restrict_to_read_only();
    }
    let mut session = declared.open_session().map_err(|error| error.detail())?;
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    session.serve(&mut reader, &mut writer)
}
