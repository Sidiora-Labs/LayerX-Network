use serde_json::{json, Value};

pub struct CommandOutput {
    kind: &'static str,
    message: String,
    data: Value,
}

impl CommandOutput {
    pub fn new(kind: &'static str, message: impl Into<String>, data: Value) -> Self {
        Self {
            kind,
            message: message.into(),
            data,
        }
    }

    pub fn emit(self, machine: bool) -> Result<(), String> {
        if machine {
            let value = json!({
                "ok": true,
                "kind": self.kind,
                "message": self.message,
                "data": self.data,
            });
            println!(
                "{}",
                serde_json::to_string(&value)
                    .map_err(|error| format!("could not encode command output: {error}"))?
            );
        } else {
            println!("{}", self.message);
            if self.data != Value::Null {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&self.data)
                        .map_err(|error| format!("could not encode command output: {error}"))?
                );
            }
        }
        Ok(())
    }
}

pub fn emit_error(error: &str, machine: bool) {
    if machine {
        let value = json!({
            "ok": false,
            "error": {
                "code": machine_error_code(error),
                "detail": error,
            }
        });
        let encoded = serde_json::to_string(&value)
            .unwrap_or_else(|_| "{\"ok\":false,\"error\":{\"code\":\"command_failed\"}}".into());
        eprintln!("{encoded}");
    } else {
        eprintln!("layerx: {error}");
    }
}

fn machine_error_code(error: &str) -> &str {
    let Some((code, _)) = error.split_once(": ") else {
        return "command_failed";
    };
    if code.contains('_')
        && code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        code
    } else {
        "command_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::machine_error_code;

    #[test]
    fn typed_command_errors_keep_their_machine_code() {
        assert_eq!(
            machine_error_code("sequencer_seed_exists: already provisioned"),
            "sequencer_seed_exists"
        );
    }

    #[test]
    fn prose_errors_use_the_generic_machine_code() {
        assert_eq!(
            machine_error_code("could not contact the endpoint: refused"),
            "command_failed"
        );
        assert_eq!(
            machine_error_code("unauthorized: refused"),
            "command_failed"
        );
    }
}
