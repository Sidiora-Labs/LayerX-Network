use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::ArtifactManifestEntry;

const GIT_SCHEME: &str = "git+";
const SIMPLE_SCHEME: &str = "simple+";
const TIMEOUT: Duration = Duration::from_secs(300);
const MAX_ARTIFACT_BYTES: u64 = 1 << 30;
const MAX_INDEX_BYTES: u64 = 1 << 24;

/// Fetches the bytes the registry serves for one manifest entry into
/// `destination`.
///
/// Locations are resolved by scheme: `https://` is downloaded as is,
/// `simple+https://` names a PEP 503 project page whose link to the artifact
/// file is followed, and `git+https://<repository>#<tag>` archives the tagged
/// tree the way the Swift package manager consumes it.
///
/// # Errors
///
/// Fails when the location cannot be resolved or downloaded, when the registry
/// answers with anything but HTTP 200, or when the bytes cannot be written.
pub fn fetch(entry: &ArtifactManifestEntry, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    let location = entry.location.as_str();
    if let Some(repository) = location.strip_prefix(GIT_SCHEME) {
        return fetch_git_archive(repository, destination);
    }
    if let Some(index) = location.strip_prefix(SIMPLE_SCHEME) {
        let url = simple_index_link(index, &entry.artifact)?;
        return download(&url, destination);
    }
    if location.starts_with("https://") {
        return download(location, destination);
    }
    Err(format!("location {location} uses no fetchable scheme"))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into()
}

fn get(url: &str, limit: u64, what: &str) -> Result<Vec<u8>, String> {
    let mut response = agent()
        .get(url)
        .call()
        .map_err(|error| format!("GET {url}: {error}"))?;
    let status = response.status();
    if status != 200 {
        return Err(format!(
            "GET {url} answered HTTP {}, not 200",
            status.as_u16()
        ));
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("GET {url}: {error}"))?;
    if bytes.len() as u64 > limit {
        return Err(format!("GET {url}: {what} exceeds {limit} bytes"));
    }
    Ok(bytes)
}

fn download(url: &str, destination: &Path) -> Result<(), String> {
    let bytes = get(url, MAX_ARTIFACT_BYTES, "artifact")?;
    fs::write(destination, bytes)
        .map_err(|error| format!("write {}: {error}", destination.display()))
}

fn simple_index_link(index: &str, artifact: &str) -> Result<String, String> {
    let page = get(index, MAX_INDEX_BYTES, "project page")?;
    let page = String::from_utf8(page)
        .map_err(|error| format!("GET {index}: project page is not UTF-8: {error}"))?;
    let mut links = Vec::new();
    let mut rest = page.as_str();
    while let Some(start) = rest.find("href=\"") {
        let after = &rest[start + "href=\"".len()..];
        let Some(end) = after.find('"') else {
            break;
        };
        links.push(after[..end].replace("&amp;", "&"));
        rest = &after[end..];
    }
    let found = links.iter().find(|href| {
        let path = href.split(['#', '?']).next().unwrap_or_default();
        path == artifact || path.ends_with(&format!("/{artifact}"))
    });
    let Some(href) = found else {
        return Err(format!(
            "project page {index} links no file named {artifact}"
        ));
    };
    resolve(index, href)
}

fn resolve(base: &str, href: &str) -> Result<String, String> {
    if href.starts_with("https://") || href.starts_with("http://") {
        return Ok(href.to_owned());
    }
    let (scheme, remainder) = base
        .split_once("://")
        .ok_or_else(|| format!("index {base} has no scheme"))?;
    let (host, path) = remainder.split_once('/').unwrap_or((remainder, ""));
    if let Some(absolute) = href.strip_prefix('/') {
        return Ok(format!("{scheme}://{host}/{absolute}"));
    }
    let directory = path.rsplit_once('/').map_or("", |(directory, _)| directory);
    let mut segments = directory
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for segment in href.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            segment => segments.push(segment.to_owned()),
        }
    }
    Ok(format!("{scheme}://{host}/{}", segments.join("/")))
}

fn fetch_git_archive(repository: &str, destination: &Path) -> Result<(), String> {
    let (url, tag) = repository
        .split_once('#')
        .filter(|(url, tag)| !url.is_empty() && !tag.is_empty())
        .ok_or_else(|| {
            format!("git location {GIT_SCHEME}{repository} must name the tag after `#`")
        })?;
    let checkout = PathBuf::from(format!("{}.checkout", destination.display()));
    if checkout.exists() {
        fs::remove_dir_all(&checkout)
            .map_err(|error| format!("remove {}: {error}", checkout.display()))?;
    }
    run(
        Command::new("git")
            .args(["clone", "--quiet", "--depth", "1", "--branch", tag, url])
            .arg(&checkout),
        &format!("clone {url} at {tag}"),
    )?;
    run(
        Command::new("git")
            .arg("-C")
            .arg(&checkout)
            .args(["archive", "--format=tar", "--output"])
            .arg(destination)
            .arg("HEAD"),
        &format!("archive {url} at {tag}"),
    )?;
    fs::remove_dir_all(&checkout).map_err(|error| format!("remove {}: {error}", checkout.display()))
}

fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let output = command
        .output()
        .map_err(|error| format!("{what}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{what}: git exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
