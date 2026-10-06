use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use base64::{Engine, engine::general_purpose::STANDARD};
use host_link::{Credential, CredentialStore, Error, Machine, Pending, State};
use secrecy::ExposeSecret;
use serde_json::{Value, json};
use uuid::Uuid;

/// An exclusive credential lock is held throughout login, logout, or managed runtime.
#[derive(Debug)]
pub(crate) struct Store {
    directory: PathBuf,
    _lock: File,
}

impl Store {
    pub fn open(data: &Path) -> Result<Self, Error> {
        let directory = data.join("device");
        match std::fs::symlink_metadata(&directory) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => return Err(Error::Storage),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = std::fs::DirBuilder::new();
                builder.recursive(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&directory).map_err(|_| Error::Storage)?;
            }
            Err(_) => return Err(Error::Storage),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let parent = std::fs::metadata(&directory).map_err(|_| Error::Storage)?;
            if parent.mode() & 0o077 != 0 {
                return Err(Error::Storage);
            }
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| Error::Storage)?;
        }
        let lock_path = directory.join("credential.lock");
        regular(&lock_path)?;
        let mut options = private_options();
        let lock = options
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|_| Error::Storage)?;
        lock.try_lock().map_err(|_| Error::Conflict)?;
        regular(&directory.join(".env"))?;
        Ok(Self {
            directory,
            _lock: lock,
        })
    }

    pub fn load(&self) -> Result<Option<State>, Error> {
        let path = self.directory.join(".env");
        regular(&path)?;
        let file = match private_options().read(true).open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(Error::Storage),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if file
                .metadata()
                .map_err(|_| Error::Storage)?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err(Error::Storage);
            }
        }
        let mut text = String::new();
        file.take(64 * 1024 + 1)
            .read_to_string(&mut text)
            .map_err(|_| Error::Storage)?;
        if text.len() > 64 * 1024 {
            return Err(Error::Storage);
        }
        let encoded = text
            .trim()
            .strip_prefix("AIT_DEVICE_STATE=")
            .ok_or(Error::Storage)?;
        let bytes = STANDARD.decode(encoded).map_err(|_| Error::Storage)?;
        decode(&serde_json::from_slice::<Value>(&bytes).map_err(|_| Error::Storage)?).map(Some)
    }

    pub fn clear(&self) -> Result<(), Error> {
        let path = self.directory.join(".env");
        regular(&path)?;
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::Storage),
        }
        File::open(&self.directory)
            .and_then(|f| f.sync_all())
            .map_err(|_| Error::Storage)
    }
}

impl CredentialStore for Store {
    fn save(&self, state: &State) -> Result<(), Error> {
        let path = self.directory.join(".env");
        regular(&path)?;
        let temporary = self.directory.join(format!(".env-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut file = private_options()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|_| Error::Storage)?;
            let bytes = serde_json::to_vec(&encode(state)).map_err(|_| Error::Storage)?;
            writeln!(file, "AIT_DEVICE_STATE={}", STANDARD.encode(bytes))
                .map_err(|_| Error::Storage)?;
            file.sync_all().map_err(|_| Error::Storage)?;
            std::fs::rename(&temporary, &path).map_err(|_| Error::Storage)?;
            File::open(&self.directory)
                .and_then(|f| f.sync_all())
                .map_err(|_| Error::Storage)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
        // Atomically reject symlinks even if the path changes after inspection.
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
}

fn regular(path: &Path) -> Result<(), Error> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if !m.is_file() || m.file_type().is_symlink() => Err(Error::Storage),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Error::Storage),
    }
}

fn encode(state: &State) -> Value {
    let credential = state.credential.as_ref().map(|c| {
        json!({"refresh_token":c.refresh_token.expose_secret(),
        "refresh_expires_at":c.refresh_expires_at, "binding":c.binding})
    });
    let pending = state.pending.as_ref().map(|p| match p {
        Pending::Refresh(id) => json!({"kind":"refresh","request_id":id}),
        Pending::Enrollment { token, request_id } => {
            json!({"kind":"enrollment","request_id":request_id,"secret":token.expose_secret()})
        }
        Pending::Web {
            device_code,
            user_code,
            request_id,
            expires_at,
            interval,
        } => json!({"kind":"web","request_id":request_id,
            "secret":device_code.expose_secret(),"user_code":user_code,"expires_at":expires_at,"interval":interval}),
    });
    json!({"version":1,"machine":state.machine,"credential":credential,"pending":pending})
}

fn decode(value: &Value) -> Result<State, Error> {
    if value["version"] != 1 {
        return Err(Error::Storage);
    }
    let machine: Machine =
        serde_json::from_value(value["machine"].clone()).map_err(|_| Error::Storage)?;
    let credential = if value["credential"].is_null() {
        None
    } else {
        Some(Credential {
            refresh_token: secret(&value["credential"], "refresh_token")?,
            refresh_expires_at: serde_json::from_value(
                value["credential"]["refresh_expires_at"].clone(),
            )
            .map_err(|_| Error::Storage)?,
            binding: serde_json::from_value(value["credential"]["binding"].clone())
                .map_err(|_| Error::Storage)?,
        })
    };
    let p = &value["pending"];
    let pending = if p.is_null() {
        None
    } else {
        let request_id =
            serde_json::from_value(p["request_id"].clone()).map_err(|_| Error::Storage)?;
        Some(match p["kind"].as_str() {
            Some("refresh") => Pending::Refresh(request_id),
            Some("enrollment") => Pending::Enrollment {
                token: secret(p, "secret")?,
                request_id,
            },
            Some("web") => Pending::Web {
                device_code: secret(p, "secret")?,
                user_code: p["user_code"]
                    .as_str()
                    .filter(|s| {
                        s.len() == 14 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
                    })
                    .ok_or(Error::Storage)?
                    .to_owned(),
                request_id,
                expires_at: serde_json::from_value(p["expires_at"].clone())
                    .map_err(|_| Error::Storage)?,
                interval: p["interval"]
                    .as_u64()
                    .filter(|i| (5..=300).contains(i))
                    .ok_or(Error::Storage)?,
            },
            _ => return Err(Error::Storage),
        })
    };
    if machine.server_id.is_nil()
        || credential
            .as_ref()
            .is_some_and(|c| c.binding.server_id != machine.server_id)
    {
        return Err(Error::Storage);
    }
    Ok(State {
        machine,
        credential,
        pending,
    })
}

fn secret(value: &Value, key: &str) -> Result<secrecy::SecretString, Error> {
    let text = value[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 8192)
        .ok_or(Error::Storage)?;
    Ok(text.to_owned().into())
}

#[cfg(test)]
mod tests;
