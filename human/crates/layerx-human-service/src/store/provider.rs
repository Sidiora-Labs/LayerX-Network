use super::*;
use std::sync::Arc;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::io::Write as _;

pub trait PrincipalTenancyAuthority: std::fmt::Debug + Send + Sync {
    /// # Errors
    /// Refuses unavailable or unauthenticated provider bindings.
    fn tenant_for(&self, principal: &PrincipalId) -> Result<AgentTenantId, StoreError>;
}

impl PrincipalStore {
    /// # Errors
    /// Preserves pinned static tenancy checks and refuses changed provider bindings.
    pub fn open_with_authority(
        root: impl AsRef<Path>, retention: RetentionPolicy, tenancy_digest: TenancyDigest,
        authority: Arc<dyn PrincipalTenancyAuthority>,
    ) -> Result<Self, StoreError> {
        let mut store = Self::open(root, retention, tenancy_digest)?;
        store.provider = Some(authority);
        for principal in store.known_principals()? {
            store.resolve_tenant(&principal)?;
        }
        Ok(store)
    }

    /// # Errors
    /// Refuses invalid durable principal directories or unavailable provider state.
    pub fn known_principals(&self) -> Result<Vec<PrincipalId>, StoreError> {
        let mut principals: BTreeSet<_> = self.tenancy.principals().into_iter().collect();
        if self.provider.is_some() {
            verify_principals_tree(&self.root)?;
            let directory = self.root.join(PRINCIPALS_DIR);
            if directory.exists() {
                for entry in fs::read_dir(directory)? {
                    let entry = entry?;
                    let name = entry.file_name();
                    let name = name.to_str().ok_or(StoreError::InvalidPrincipal)?;
                    principals.insert(PrincipalId::new(name)?);
                }
            }
        }
        Ok(principals.into_iter().collect())
    }

    pub(super) fn resolve_tenant(&self, principal: &PrincipalId) -> Result<AgentTenantId, StoreError> {
        let configured = self.tenancy.tenant_for(principal);
        let Some(provider) = &self.provider else {
            return configured.cloned().map_err(StoreError::from);
        };
        let tenant = provider.tenant_for(principal)?;
        if configured.is_ok_and(|value| value != &tenant) {
            return Err(StoreError::Tenancy(TenancyError::DigestMismatch));
        }
        let directory = self.root.join(PRINCIPALS_DIR).join(principal.as_str());
        let path = directory.join("provider-binding");
        let bytes = tenant.as_str().as_bytes();
        fs::create_dir_all(&directory)?;
        let metadata = fs::symlink_metadata(&directory)?;
        if !metadata.is_dir() || metadata.uid() != rustix::process::geteuid().as_raw() {
            return Err(StoreError::Tenancy(TenancyError::DigestMismatch));
        }
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        if path.exists() {
            verify_binding_file(&path, bytes)?;
        } else {
            let pending = directory.join("provider-binding.pending");
            match fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&pending) {
                Ok(mut file) => { file.write_all(bytes)?; file.sync_all()?; }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => verify_binding_file(&pending, bytes)?,
                Err(error) => return Err(error.into()),
            }
            fs::rename(pending, path)?;
            fs::File::open(&directory)?.sync_all()?;
            fs::File::open(self.root.join(PRINCIPALS_DIR))?.sync_all()?;
        }
        Ok(tenant)
    }
}

fn verify_binding_file(path: &Path, expected: &[u8]) -> Result<(), StoreError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > 255
        || metadata.uid() != rustix::process::geteuid().as_raw() || metadata.mode() & 0o077 != 0
        || fs::read(path)? != expected
    {
        return Err(StoreError::Tenancy(TenancyError::DigestMismatch));
    }
    Ok(())
}
