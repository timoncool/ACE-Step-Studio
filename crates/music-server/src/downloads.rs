//! Resumable downloads for the optional native extras.
//!
//! Every optional capability - the writing assistant, the karaoke aligner -
//! fetches the same way: a pinned URL, a `.part` file that survives an
//! interrupted run, and a zip that unpacks into its own directory so two
//! runtimes never mix. Nothing here starts on its own; a download happens only
//! when the user asks for it.
//!
//! How much a file weighs is asked of the server serving it, never assumed -
//! see `sizes` for what assuming it cost.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// What a downloadable asset is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// Model weights.
    Model,
    /// Executables and their libraries, delivered as a zip.
    Runtime,
}

#[derive(Debug, Clone, Copy)]
pub struct Asset {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: AssetKind,
    pub url: &'static str,
    /// Where the file lands, relative to the capability's root.
    pub relative_path: &'static str,
    /// About what it weighs, for showing a size before anything is fetched.
    /// The real one comes from the server; see `sizes`.
    pub bytes: u64,
    /// Unpacked into this sub-directory of `runtime/` when set.
    pub unzip_into: Option<&'static str>,
    /// A file name prefix that proves this asset is unpacked. Without it a
    /// companion archive would look installed as soon as its neighbour was,
    /// because both land in the same directory.
    pub marker: &'static str,
    /// When set, only these files are taken out of the archive, by name, over
    /// range requests - the rest is never downloaded. NVIDIA's libraries come
    /// in archives several times larger than the parts anyone uses.
    pub pick: &'static [&'static str],
    /// Roughly how much VRAM the asset wants; informational only.
    pub vram_gb: Option<u32>,
    pub note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: AssetKind,
    pub bytes: u64,
    pub vram_gb: Option<u32>,
    pub note: &'static str,
    pub installed: bool,
    /// Bytes already on disk, so a resumed download reports honestly.
    pub downloaded_bytes: u64,
}

/// What a whole set calls itself while it downloads. A capability is usually
/// several files - a runtime, a provider, the weights - and they arrive as one
/// thing, so they report as one thing.
pub const SET: &str = "set";

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub asset_id: String,
    /// Which panel asked for this. The karaoke recogniser and the separator
    /// share one downloader - they share the ONNX runtime and the CUDA
    /// libraries on disk, so they must - and without this the separator's panel
    /// showed the recogniser's download as its own: its button disappeared and
    /// a percentage of somebody else's gigabytes appeared in its place.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub done: bool,
    pub error: Option<String>,
}

/// Owns one capability's download directory.
pub struct Downloader {
    root: PathBuf,
    http: reqwest::Client,
    progress: Arc<Mutex<Option<DownloadProgress>>>,
    /// Raised to stop whatever is downloading. What has already arrived stays
    /// on disk with its manifest, so pressing this is a pause as much as a
    /// cancel: the next attempt carries on from the last finished piece.
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl Downloader {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            http: crate::sizes::client(),
            progress: Arc::new(Mutex::new(None)),
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_of(&self, asset: &Asset) -> PathBuf {
        self.root.join(asset.relative_path)
    }

    pub fn runtime_dir(&self, flavour: &str) -> PathBuf {
        self.root.join("runtime").join(flavour)
    }

    /// Where an asset's picked files land. Without a flavour they land in the
    /// root itself, which is how the engine's CUDA libraries end up beside
    /// `mm-server.exe` - the only place Windows looks without being told.
    pub fn picked_into(&self, asset: &Asset) -> PathBuf {
        match asset.unzip_into {
            Some(flavour) => self.runtime_dir(flavour),
            None => self.root.clone(),
        }
    }

    /// A model is installed when its file is there; a runtime when its own
    /// marker file is present in the directory it unpacks into.
    pub fn is_installed(&self, asset: &Asset) -> bool {
        if !asset.pick.is_empty() {
            let destination = self.picked_into(asset);
            return asset.pick.iter().all(|name| destination.join(name).is_file());
        }
        if let Some(flavour) = asset.unzip_into {
            let marker = asset.marker.to_ascii_lowercase();
            return fs::read_dir(self.runtime_dir(flavour))
                .map(|entries| {
                    entries
                        .flatten()
                        .any(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase().starts_with(&marker))
                })
                .unwrap_or(false);
        }
        // Presence, not size. A file only appears here after being downloaded
        // into `.part` and checked against the length the server itself
        // declared, so it being here is the proof that it arrived whole. Judging
        // it by a figure compiled into the studio meant that re-uploading a file
        // on someone else's repository - a routine thing, and none of our
        // business - made it permanently un-installable.
        fs::metadata(self.path_of(asset)).map(|meta| meta.len() > 0).unwrap_or(false)
    }

    pub fn partial_bytes(&self, asset: &Asset) -> u64 {
        fs::metadata(self.path_of(asset).with_extension("part")).map(|meta| meta.len()).unwrap_or(0)
    }

    pub fn status_of(&self, assets: &[Asset]) -> Vec<AssetStatus> {
        // Ask the servers what these actually weigh. The answers land in time
        // for the next poll a second from now; nothing waits on them.
        crate::sizes::ask(&self.http, assets.iter().map(|asset| asset.url.to_string()).collect());
        assets
            .iter()
            .map(|asset| AssetStatus {
                id: asset.id,
                label: asset.label,
                kind: asset.kind,
                bytes: crate::sizes::or_listed(asset.url, asset.bytes),
                vram_gb: asset.vram_gb,
                note: asset.note,
                installed: self.is_installed(asset),
                downloaded_bytes: if self.is_installed(asset) { crate::sizes::or_listed(asset.url, asset.bytes) } else { self.partial_bytes(asset) },
            })
            .collect()
    }

    /// Deletes an installed asset, whatever shape it took on disk.
    ///
    /// A single file is a file; a runtime is a directory of unpacked libraries;
    /// a picked set is the named files inside one. Anything half-downloaded goes
    /// with it - a `.part` occupies exactly as much disk as a finished file.
    pub fn remove(&self, asset: &Asset) -> Result<u64> {
        // A free function rather than a closure: the running total is read while
        // the directory walk below is still adding to it.
        fn drop_file(path: PathBuf) -> Result<u64> {
            match fs::metadata(&path) {
                Ok(meta) => {
                    let size = meta.len();
                    fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
                    Ok(size)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
                Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
            }
        }

        let mut freed = 0u64;

        if !asset.pick.is_empty() {
            let destination = self.root.join("runtime").join(asset.unzip_into.unwrap_or("."));
            for name in asset.pick {
                freed += drop_file(destination.join(name))?;
            }
        } else if let Some(flavour) = asset.unzip_into {
            let directory = self.runtime_dir(flavour);
            if let Ok(entries) = fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    if let Ok(meta) = entry.metadata() {
                        if meta.is_file() {
                            freed += meta.len();
                        }
                    }
                }
            }
            match fs::remove_dir_all(&directory) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).with_context(|| format!("remove {}", directory.display())),
            }
        } else {
            freed += drop_file(self.path_of(asset))?;
        }

        freed += drop_file(self.path_of(asset).with_extension("part"))?;
        Ok(freed)
    }

    pub async fn active(&self) -> Option<DownloadProgress> {
        self.progress.lock().await.clone()
    }

    /// Stops the running download.
    pub fn cancel(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// The running download, but only if this panel started it.
    pub async fn active_for(&self, scope: &str) -> Option<DownloadProgress> {
        self.active().await.filter(|progress| progress.scope.as_deref() == Some(scope))
    }

    /// Installs a whole set, several files at a time.
    ///
    /// One at a time was the reason a recogniser took as long as its slowest
    /// file and the panel showed whichever download happened to be current -
    /// the progress is one figure per downloader, so a queue of eight made it
    /// jump between them. Four at once saturates the line, and the set reports
    /// as one download: how much of everything it needs has arrived.
    ///
    /// That reporting is not decoration. The panel decides there is a download
    /// running by looking at this one figure, so a set that downloaded without
    /// touching it looked like nothing had happened at all: the request
    /// succeeded, the files arrived, and the interface redrew the same row with
    /// the same download button. That is what "the button does nothing" was.
    pub async fn install_all(&self, scope: &str, assets: &[&'static Asset]) -> Result<()> {
        const AT_ONCE: usize = 4;
        let pending: Vec<&'static Asset> = assets.iter().copied().filter(|asset| !self.is_installed(asset)).collect();
        if pending.is_empty() {
            return Ok(());
        }

        let total: u64 = pending.iter().map(|asset| asset.bytes).sum();
        let mut progress = self.progress.lock().await;
        // a second press before the first set finished would write the same
        // partial files twice
        if progress.as_ref().is_some_and(|active| !active.done && active.error.is_none()) {
            bail!("another download is already running");
        }
        self.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        *progress = Some(DownloadProgress {
            // A set of one is that one file, as far as anyone watching is
            // concerned; only a real set needs a name of its own.
            asset_id: if pending.len() == 1 { pending[0].id.to_string() } else { SET.to_string() },
            scope: Some(scope.to_string()),
            downloaded_bytes: pending.iter().map(|asset| self.partial_bytes(asset)).sum(),
            total_bytes: total,
            done: false,
            error: None,
        });
        drop(progress);

        let mut finished = 0u64;
        let mut failure = None;
        for batch in pending.chunks(AT_ONCE) {
            // One cell per download, summed by the reporter below: four
            // downloads writing to one counter is what made the bar jump
            // between files and reset.
            let cells: Vec<Arc<Mutex<Option<DownloadProgress>>>> = batch
                .iter()
                .map(|asset| {
                    Arc::new(Mutex::new(Some(DownloadProgress {
                        asset_id: asset.id.to_string(),
                        scope: Some(scope.to_string()),
                        downloaded_bytes: self.partial_bytes(asset),
                        total_bytes: asset.bytes,
                        done: false,
                        error: None,
                    })))
                })
                .collect();
            let reporter = self.publish_sum(finished, total, cells.clone());
            let outcomes =
                futures_util::future::join_all(batch.iter().zip(&cells).map(|(asset, cell)| self.fetch_now(asset, cell))).await;
            reporter.abort();
            finished += batch.iter().map(|asset| asset.bytes).sum::<u64>();
            if let Some(error) = outcomes.into_iter().find_map(Result::err) {
                failure = Some(error);
                break;
            }
        }

        let outcome = match failure {
            Some(error) if self.cancel.load(std::sync::atomic::Ordering::Relaxed) => {
                let _ = error;
                Err(anyhow::anyhow!("cancelled"))
            }
            Some(error) => Err(error),
            None => match assets.iter().find(|asset| !self.is_installed(asset)) {
                Some(asset) => Err(anyhow::anyhow!("{} finished downloading but its files are not on disk", asset.label)),
                None => Ok(()),
            },
        };
        if let Some(active) = self.progress.lock().await.as_mut() {
            active.done = true;
            active.error = outcome.as_ref().err().map(|error| error.to_string());
            if outcome.is_ok() {
                active.downloaded_bytes = total;
            }
        }
        outcome
    }

    /// Reports what a running batch has fetched, as one figure for the set.
    ///
    /// Aborted by the caller when the batch ends, which is why it may loop for
    /// ever: it has nothing to decide, only something to add up.
    fn publish_sum(&self, finished: u64, total: u64, cells: Vec<Arc<Mutex<Option<DownloadProgress>>>>) -> tokio::task::JoinHandle<()> {
        let progress = self.progress.clone();
        tokio::spawn(async move {
            loop {
                let mut written = finished;
                for cell in &cells {
                    if let Some(active) = cell.lock().await.as_ref() {
                        written += active.downloaded_bytes;
                    }
                }
                if let Some(active) = progress.lock().await.as_mut() {
                    active.downloaded_bytes = written.min(total);
                }
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            }
        })
    }

    /// Downloads one asset and waits for it, without the single-slot rule that
    /// `install` keeps for the panel's own buttons. Reports into the cell it is
    /// given rather than the downloader's own, because several of these run at
    /// the same time.
    async fn fetch_now(&self, asset: &'static Asset, cell: &Arc<Mutex<Option<DownloadProgress>>>) -> Result<()> {
        if !asset.pick.is_empty() {
            // The whole archive over the resumable downloader, then the wanted
            // files extracted locally. Reading named files straight out of a
            // remote zip over range requests was clever and fragile: a separate
            // client with no resume, no retry and no rate-limit handling, so a
            // slow or interrupted connection sat at nought bytes until a ten
            // minute timeout. NVIDIA's cuBLAS archive is almost all the two
            // libraries anyway - nineteen megabytes of difference bought a
            // download that survives a bad line.
            return download_and_extract_named(&self.http, asset, &self.picked_into(asset), cell, self.cancel.clone()).await;
        }
        let target = self.path_of(asset);
        fetch(&self.http, asset, &target, cell, self.cancel.clone()).await?;
        if let Some(flavour) = asset.unzip_into {
            extract_zip(&target, &self.root.join("runtime").join(flavour))?;
        }
        Ok(())
    }

    /// Starts one download in the background. Only one runs at a time, and an
    /// interrupted file resumes from what is already on disk.
    pub async fn install(&self, asset: &'static Asset) -> Result<()> {
        {
            let mut progress = self.progress.lock().await;
            if progress.as_ref().is_some_and(|active| !active.done && active.error.is_none()) {
                bail!("another download is already running");
            }
            *progress = Some(DownloadProgress {
                asset_id: asset.id.to_string(),
                scope: None,
                downloaded_bytes: self.partial_bytes(asset),
                total_bytes: asset.bytes,
                done: false,
                error: None,
            });
        }

        self.cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        let target = self.path_of(asset);
        let root = self.root.clone();
        let http = self.http.clone();
        let progress = self.progress.clone();
        let cancel = self.cancel.clone();
        tokio::spawn(async move {
            // A named-file asset: the whole archive over the resumable
            // downloader, then the wanted files extracted locally. The old path
            // read them straight out of a remote zip with a client that could
            // not resume or retry, so a bad connection sat at nought bytes.
            if !asset.pick.is_empty() {
                let destination = match asset.unzip_into {
                    Some(flavour) => root.join("runtime").join(flavour),
                    None => root.clone(),
                };
                let outcome = download_and_extract_named(&http, asset, &destination, &progress, cancel).await;
                let mut guard = progress.lock().await;
                if let Some(active) = guard.as_mut() {
                    active.done = true;
                    active.error = outcome.err().map(|error| error.to_string());
                }
                return;
            }
            let outcome = fetch(&http, asset, &target, &progress, cancel).await;
            let mut guard = progress.lock().await;
            let error = match outcome {
                Ok(()) => match asset.unzip_into {
                    Some(flavour) => extract_zip(&target, &root.join("runtime").join(flavour)).err().map(|error| error.to_string()),
                    None => None,
                },
                Err(error) => Some(error.to_string()),
            };
            if let Some(active) = guard.as_mut() {
                active.done = true;
                active.error = error;
            }
        });
        Ok(())
    }
}

async fn fetch(
    http: &reqwest::Client,
    asset: &Asset,
    target: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if fs::metadata(target).map(|meta| meta.len() > 0).unwrap_or(false) {
        return Ok(());
    }

    // How big it is and whether it can be fetched in pieces, asked of the
    // server rather than assumed - both answers come from one ranged byte.
    let plan = crate::chunked::probe(http, asset.url).await?;
    if plan.total > 0 {
        crate::sizes::learn(asset.url, plan.total);
    }

    let part = target.with_extension("part");
    let written = Arc::new(std::sync::atomic::AtomicU64::new(0));
    // The panel reads one figure; the pieces write to another, from four tasks
    // at once. This copies the second into the first while they work.
    let reporter = {
        let (written, progress) = (written.clone(), progress.clone());
        tokio::spawn(async move {
            loop {
                let value = written.load(std::sync::atomic::Ordering::Relaxed);
                if let Some(active) = progress.lock().await.as_mut() {
                    active.downloaded_bytes = value;
                }
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        })
    };
    let outcome = crate::chunked::fetch(http, asset.url, &part, plan, written.clone(), cancel).await;
    reporter.abort();
    outcome?;
    if let Some(active) = progress.lock().await.as_mut() {
        active.downloaded_bytes = written.load(std::sync::atomic::Ordering::Relaxed);
    }

    let size = fs::metadata(&part)?.len();
    if plan.total > 0 && size != plan.total {
        bail!("{} has {size} bytes, the server said {}", asset.label, plan.total);
    }
    fs::rename(&part, target).with_context(|| format!("publish {}", target.display()))?;
    Ok(())
}

/// Unpacks a release zip. Entries are flattened into `destination` because the
/// releases nest their binaries one or two directories deep.
/// Downloads a whole archive with the resumable downloader and extracts the
/// asset's named files, reporting progress into the shared cell.
async fn download_and_extract_named(
    http: &reqwest::Client,
    asset: &'static Asset,
    destination: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    if asset.pick.iter().all(|name| destination.join(name).is_file()) {
        return Ok(());
    }
    let archive = destination.join(format!("{}.part-archive", asset.id));
    let plan = crate::chunked::probe(http, asset.url).await?;
    let written = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let reporter = {
        let (written, progress) = (written.clone(), progress.clone());
        tokio::spawn(async move {
            loop {
                let value = written.load(std::sync::atomic::Ordering::Relaxed);
                if let Some(active) = progress.lock().await.as_mut() {
                    active.downloaded_bytes = value;
                }
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        })
    };
    let outcome = crate::chunked::fetch(http, asset.url, &archive, plan, written, cancel).await;
    reporter.abort();
    outcome?;
    let (archive_path, wanted, destination) = (archive.clone(), asset.pick, destination.to_path_buf());
    let extracted = tokio::task::spawn_blocking(move || extract_named_local(&archive_path, wanted, &destination))
        .await
        .map_err(|error| anyhow::anyhow!("extraction task failed: {error}"))?;
    fs::remove_file(&archive).ok();
    extracted
}

/// Pulls named files out of a local archive, by base name, flattened into the
/// destination. The archive stays where it is; the caller deletes it.
fn extract_named_local(archive: &Path, wanted: &[&str], destination: &Path) -> Result<()> {
    let file = fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).with_context(|| format!("read {}", archive.display()))?;
    let mut found: Vec<String> = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).context("read zip entry")?;
        let Some(name) = entry.enclosed_name().and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_string())) else {
            continue;
        };
        if !wanted.iter().any(|candidate| candidate.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let target = destination.join(&name);
        let partial = target.with_extension("part");
        let mut out = fs::File::create(&partial).with_context(|| format!("create {}", partial.display()))?;
        io::copy(&mut entry, &mut out).with_context(|| format!("extract {name}"))?;
        drop(out);
        fs::rename(&partial, &target).with_context(|| format!("publish {}", target.display()))?;
        found.push(name);
    }
    if found.len() != wanted.len() {
        let missing: Vec<&str> = wanted.iter().copied().filter(|name| !found.iter().any(|got| got.eq_ignore_ascii_case(name))).collect();
        bail!("{} did not contain {}", archive.display(), missing.join(", "));
    }
    Ok(())
}

pub fn extract_zip(archive: &Path, destination: &Path) -> Result<()> {
    // Packages ship every architecture they support; only this one belongs
    // here, and flattening the rest would overwrite it with an ARM build.
    const FOREIGN: [&str; 4] = ["arm64", "win-x86", "linux", "osx"];
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    let file = fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).with_context(|| format!("read {}", archive.display()))?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).context("read zip entry")?;
        if entry.is_dir() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else { continue };
        let inside = path.to_string_lossy().to_ascii_lowercase();
        if FOREIGN.iter().any(|other| inside.contains(other)) {
            continue;
        }
        let Some(name) = path.file_name().map(|name| name.to_owned()) else {
            continue;
        };
        let target = destination.join(name);
        let mut out = fs::File::create(&target).with_context(|| format!("create {}", target.display()))?;
        io::copy(&mut entry, &mut out).with_context(|| format!("extract {}", target.display()))?;
    }
    fs::remove_file(archive).ok();
    Ok(())
}

/// The first executable named `name` in the given flavour directories, in
/// preference order - a GPU build before a CPU one.
pub fn locate_binary(root: &Path, flavours: &[&str], name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    for flavour in flavours {
        let candidate = root.join("runtime").join(flavour).join(&file);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let direct = root.join("runtime").join(&file);
    direct.is_file().then_some(direct)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset() -> Asset {
        Asset {
            id: "test-model",
            label: "Test model",
            kind: AssetKind::Model,
            url: "https://example.invalid/model.bin",
            relative_path: "models/model.bin",
            bytes: 64,
            unzip_into: None,
            marker: "",
            pick: &[],
            vram_gb: None,
            note: "",
        }
    }

    #[test]
    fn a_partial_file_is_reported_but_never_counts_as_installed() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let entry = asset();
        fs::create_dir_all(downloader.path_of(&entry).parent().unwrap()).unwrap();
        fs::write(downloader.path_of(&entry).with_extension("part"), vec![0u8; 20]).unwrap();

        assert!(!downloader.is_installed(&entry));
        assert_eq!(downloader.partial_bytes(&entry), 20);

        fs::write(downloader.path_of(&entry), vec![0u8; 64]).unwrap();
        assert!(downloader.is_installed(&entry));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_runtime_is_installed_only_when_its_own_marker_is_present() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let runtime = Asset {
            id: "test-runtime",
            kind: AssetKind::Runtime,
            relative_path: "runtime/test.zip",
            unzip_into: Some("cuda"),
            marker: "whisper-cli",
            ..asset()
        };
        fs::create_dir_all(downloader.runtime_dir("cuda")).unwrap();
        fs::write(downloader.runtime_dir("cuda").join("ggml.dll"), b"x").unwrap();
        assert!(!downloader.is_installed(&runtime));

        fs::write(downloader.runtime_dir("cuda").join("whisper-cli.exe"), b"x").unwrap();
        assert!(downloader.is_installed(&runtime));
        fs::remove_dir_all(&root).ok();
    }
}
