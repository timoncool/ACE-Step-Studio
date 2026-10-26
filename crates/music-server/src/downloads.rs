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
    /// When set, only these files are taken out of the archive, by name, once
    /// the whole archive has arrived; the rest is not unpacked. NVIDIA's
    /// libraries come in archives several times larger than the parts anyone uses.
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
    /// The program (yue2, dub...) it was taken from in this run instead of being downloaded.
    pub taken_from: Option<String>,
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
#[derive(Clone)]
pub struct Downloader {
    root: PathBuf,
    /// Where runtimes unpack, one folder per flavour: `root/runtime` unless
    /// several capabilities share one build of a tool.
    runtime_root: PathBuf,
    http: reqwest::Client,
    progress: Arc<Mutex<Option<DownloadProgress>>>,
    /// Raised to stop whatever is downloading. What has already arrived stays
    /// on disk with its manifest, so pressing this is a pause as much as a
    /// cancel: the next attempt carries on from the last finished piece.
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

impl Downloader {
    pub fn new(root: PathBuf) -> Self {
        let runtime_root = root.join("runtime");
        Self::with_runtimes(root, runtime_root)
    }

    /// A downloader whose runtimes unpack under `runtime_root`, where another
    /// capability's downloader finds the same build.
    pub fn with_runtimes(root: PathBuf, runtime_root: PathBuf) -> Self {
        Self {
            root,
            runtime_root,
            http: crate::net::download_client(),
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
        self.runtime_root.join(flavour)
    }

    /// Where an asset's picked files land. Without a flavour they land in the
    /// root itself, which is how the engine's CUDA libraries end up beside
    /// the engine's executable - the only place Windows looks without being told.
    pub fn picked_into(&self, asset: &Asset) -> PathBuf {
        match asset.unzip_into {
            Some(flavour) => self.runtime_dir(flavour),
            None => self.root.clone(),
        }
    }

    /// Installed when its files are there and, for a runtime, its record names
    /// today's pin (`install_record`), or its files are today's pin byte for byte
    /// whichever address put them there; a runtime from before records counts as
    /// in `take_over`. A model counts by its file: its address can move to
    /// another revision of the same weights without them being fetched again.
    pub fn is_installed(&self, asset: &Asset) -> bool {
        if !self.present(asset) {
            return false;
        }
        if asset.kind == AssetKind::Model {
            return true;
        }
        let record = self.record_of(asset);
        match crate::install_record::matches(&record, asset.url, &self.root) {
            Some(true) => true,
            Some(false) => self.holds_pinned_bytes(asset, &record),
            None => self.take_over(asset, &record) || self.holds_pinned_bytes(asset, &record),
        }
    }

    /// Today's pin already on disk byte for byte, from whichever address: the files a pick takes out of its
    /// package by the hashes the package lists, or a single file by the pin's own. Its record is then written for
    /// today's pin, so the next look reads the record.
    fn holds_pinned_bytes(&self, asset: &Asset, record: &Path) -> bool {
        if crate::install_record::under_way(record) {
            return false;
        }
        let files: Vec<PathBuf> = if !asset.pick.is_empty() {
            let destination = self.picked_into(asset);
            let mut files = Vec::new();
            for entry in asset.pick {
                let name = picked_file_name(entry);
                let path = destination.join(name);
                match crate::pin_sha::picked(asset.url, name) {
                    Some(sha256) if crate::pin_sha::holds(&path, sha256) => files.push(path),
                    _ => return false,
                }
            }
            files
        } else if asset.unzip_into.is_none() {
            let path = self.path_of(asset);
            match crate::pin_sha::sha256(asset.url) {
                Some(sha256) if crate::pin_sha::holds(&path, sha256) => vec![path],
                _ => return false,
            }
        } else {
            return false;
        };
        if let Err(error) = crate::install_record::write(record, asset.url, "", &files, &self.root) {
            studio_log!("[ERROR] {}: the record of today's files already on disk was not written: {error:#}", asset.label);
        }
        true
    }

    /// What an earlier release left of `asset` for today's pin to replace: `Some(Some(url))` when its record names
    /// the URL it came from, `Some(None)` when its files lie there without a record of it; `None` when it is today's
    /// or not there. A model counts by its file, so it has no earlier release.
    pub fn earlier(&self, asset: &Asset) -> Option<Option<String>> {
        if asset.kind == AssetKind::Model || self.is_installed(asset) {
            return None;
        }
        if let Some(record) = crate::install_record::read(&self.record_of(asset)) {
            if record.url != asset.url {
                return Some(Some(record.url));
            }
        }
        self.present(asset).then_some(None)
    }

    /// Where the record of an asset's download lies: beside its file or archive.
    pub fn record_of(&self, asset: &Asset) -> PathBuf {
        crate::install_record::record_path(&self.path_of(asset))
    }

    /// A download from before records existed. A model counts by its presence;
    /// a runtime only when the file its pin names has the pinned size - a single
    /// file its own, an archive its key file - or when the `.pin` an earlier
    /// development build left names today's URL, and its record is then written.
    fn take_over(&self, asset: &Asset, record: &Path) -> bool {
        if asset.kind == AssetKind::Model {
            return true;
        }
        let key = if asset.pick.is_empty() && asset.unzip_into.is_none() {
            Some((self.path_of(asset), asset.bytes))
        } else {
            crate::install_record::key_file(asset.url).map(|(file, size)| (self.picked_into(asset).join(file), size))
        };
        // the .pin a development build of this version wrote names the URL it came from
        let earlier_pin = if asset.pick.is_empty() && asset.unzip_into.is_none() {
            let target = self.path_of(asset);
            let name = target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            target.with_file_name(format!("{name}.pin"))
        } else {
            self.picked_into(asset).join(format!(".pin-{}", asset.id))
        };
        let path = if pin_names(&earlier_pin, asset.url) {
            key.map(|(path, _)| path).filter(|path| path.is_file()).or_else(|| Some(self.path_of(asset)).filter(|path| path.is_file()))
        } else {
            key.filter(|(path, size)| fs::metadata(path).is_ok_and(|meta| meta.len() == *size)).map(|(path, _)| path)
        };
        let Some(path) = path else {
            return false;
        };
        if let Err(error) = crate::install_record::write(record, asset.url, "", &[path], &self.root) {
            studio_log!("[ERROR] {}: the record of the download already on disk was not written: {error:#}", asset.label);
        }
        true
    }

    /// The files of the asset are on disk, whichever pin put them there.
    fn present(&self, asset: &Asset) -> bool {
        if !asset.pick.is_empty() {
            let destination = self.picked_into(asset);
            return asset.pick.iter().all(|name| destination.join(picked_file_name(name)).is_file());
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

    /// What the assets still need on the disk: the bytes their partial files do
    /// not hold yet and, for an archive, twice its size while it is unpacked.
    fn space_needed(&self, assets: &[&'static Asset]) -> u64 {
        assets
            .iter()
            .filter(|asset| !self.is_installed(asset))
            .map(|asset| {
                let partial = if asset.pick.is_empty() {
                    self.partial_bytes(asset)
                } else {
                    fs::metadata(self.picked_into(asset).join(format!("{}.part-archive", asset.id))).map(|meta| meta.len()).unwrap_or(0)
                };
                let rest = asset.bytes - partial.min(asset.bytes);
                if asset.unzip_into.is_some() || !asset.pick.is_empty() { rest + 2 * asset.bytes } else { rest }
            })
            .sum()
    }

    /// Refuses a download before it starts when the volume of the downloads
    /// has less room than the assets need.
    fn ensure_space(&self, assets: &[&'static Asset]) -> Result<()> {
        let need = self.space_needed(assets);
        let Some(folder) = self.root.ancestors().find(|path| path.is_dir()) else { return Ok(()) };
        let free = match fs2::available_space(folder) {
            Ok(free) => free,
            Err(error) => {
                studio_log!("[ERROR] the free space of {} is unknown ({error}); the download goes ahead unchecked", folder.display());
                return Ok(());
            }
        };
        if free < need {
            bail!("not enough space: {:.1} GB needed, {:.1} GB free ({})", need as f64 / 1e9, free as f64 / 1e9, self.root.display());
        }
        Ok(())
    }

    pub fn status_of(&self, assets: &[Asset]) -> Vec<AssetStatus> {
        // Ask the servers what these actually weigh. The answers land in time
        // for the next poll a second from now; nothing waits on them.
        crate::sizes::ask(&self.http, assets.iter().map(|asset| asset.url.to_string()).collect());
        self.link_from_neighbors(
            assets.iter().copied().filter(|asset| asset.kind == AssetKind::Model && asset.pick.is_empty() && asset.unzip_into.is_none() && !self.is_installed(asset)).collect(),
        );
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
                taken_from: taken_from(asset.id),
            })
            .collect()
    }

    /// Links in what another program already has off the async workers: the walk of its folders, and the wait for a
    /// file another caller is taking, must not hold a status poll. What is found shows from the next poll.
    fn link_from_neighbors(&self, assets: Vec<Asset>) {
        static LINKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        struct Linking;
        impl Drop for Linking {
            fn drop(&mut self) {
                LINKING.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        if assets.is_empty() || LINKING.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        let downloader = self.clone();
        let work = move || {
            let _linking = Linking;
            for asset in &assets {
                downloader.link_from_neighbor(asset);
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(work)),
            Err(_) => work(),
        }
    }

    /// A model another program of timoncool's already has (the same name and the size the server gives) is linked
    /// in without asking: a hard link costs no disk and no download. A copy waits for the person to ask (`fetch`).
    fn link_from_neighbor(&self, asset: &Asset) {
        if self.is_installed(asset) {
            return;
        }
        let target = self.path_of(asset);
        let (Some(bytes), Some(name)) = (crate::sizes::known(asset.url), target.file_name()) else { return };
        if let Some((source, app)) = studio_hub_client::neighbors::find_file(name, bytes, target.parent().and_then(Path::file_name)) {
            match studio_hub_client::neighbors::take_file(&source, &target, false, |staged| crate::pin_sha::accepts(asset.url, staged)) {
                Ok(()) => note_taken(asset.id, &target, &app),
                Err(error) => report_take_failure(&target, &app, &error),
            }
        }
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
            match fs::symlink_metadata(&path) {
                Ok(meta) => {
                    // A library picked by its loader name is a link; the
                    // versioned file it names is what takes the disk.
                    let size = match fs::read_link(&path) {
                        Ok(target) if meta.file_type().is_symlink() => match path.parent() {
                            Some(directory) => drop_file(directory.join(target))?,
                            None => 0,
                        },
                        _ => meta.len(),
                    };
                    fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
                    Ok(size)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
                Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
            }
        }

        let mut freed = 0u64;

        if !asset.pick.is_empty() {
            let destination = self.runtime_root.join(asset.unzip_into.unwrap_or("."));
            for name in asset.pick {
                freed += drop_file(destination.join(picked_file_name(name)))?;
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
        drop_file(self.record_of(asset))?;
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

    /// The panel's set failed after its files arrived: the step that completes it did not, and the panel shows the
    /// reason as it shows a failed download.
    pub async fn fail(&self, scope: &str, error: String) {
        let mut progress = self.progress.lock().await;
        let mut failed = progress.clone().filter(|active| active.scope.as_deref() == Some(scope)).unwrap_or(DownloadProgress {
            asset_id: SET.to_string(),
            scope: Some(scope.to_string()),
            downloaded_bytes: 0,
            total_bytes: 0,
            done: true,
            error: None,
        });
        failed.done = true;
        failed.error = Some(error);
        *progress = Some(failed);
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
        self.ensure_space(&pending)?;

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
        let record = self.record_of(asset);
        if !asset.pick.is_empty() {
            // The whole archive over the resumable downloader, then the wanted
            // files extracted locally. Reading named files straight out of a
            // remote zip over range requests was clever and fragile: a separate
            // client with no resume, no retry and no rate-limit handling, so a
            // slow or interrupted connection sat at nought bytes until a ten
            // minute timeout. NVIDIA's cuBLAS archive is almost all the two
            // libraries anyway - nineteen megabytes of difference bought a
            // download that survives a bad line.
            let (sha256, files) = download_and_extract_named(&self.http, asset, &self.picked_into(asset), cell, self.cancel.clone(), &record, &self.root).await?;
            return crate::install_record::write(&record, asset.url, &sha256, &files, &self.root);
        }
        let target = self.path_of(asset);
        fetch(&self.http, asset, &target, cell, self.cancel.clone()).await?;
        let sha256 = crate::pin_sha::check(asset.url, &target)?;
        replace_earlier(&record, &self.root, &target)?;
        let files = match asset.unzip_into {
            Some(flavour) => extract_archive(&target, &self.runtime_dir(flavour))?,
            None => vec![target.clone()],
        };
        crate::install_record::write(&record, asset.url, &sha256, &files, &self.root)
    }

    /// Starts one download in the background. Only one runs at a time, and an
    /// interrupted file resumes from what is already on disk.
    pub async fn install(&self, asset: &'static Asset) -> Result<()> {
        self.ensure_space(&[asset])?;
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
        let record = self.record_of(asset);
        let root = self.root.clone();
        let runtime_root = self.runtime_root.clone();
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
                    Some(flavour) => runtime_root.join(flavour),
                    None => root.clone(),
                };
                let outcome = download_and_extract_named(&http, asset, &destination, &progress, cancel, &record, &root)
                    .await
                    .and_then(|(sha256, files)| crate::install_record::write(&record, asset.url, &sha256, &files, &root));
                let mut guard = progress.lock().await;
                if let Some(active) = guard.as_mut() {
                    active.done = true;
                    active.error = outcome.err().map(|error| error.to_string());
                }
                return;
            }
            let outcome = fetch(&http, asset, &target, &progress, cancel).await.and_then(|()| {
                let sha256 = crate::pin_sha::check(asset.url, &target)?;
                replace_earlier(&record, &root, &target)?;
                let files = match asset.unzip_into {
                    Some(flavour) => extract_archive(&target, &runtime_root.join(flavour))?,
                    None => vec![target.clone()],
                };
                crate::install_record::write(&record, asset.url, &sha256, &files, &root)
            });
            let mut guard = progress.lock().await;
            let error = outcome.err().map(|error| error.to_string());
            if let Some(active) = guard.as_mut() {
                active.done = true;
                active.error = error;
            }
        });
        Ok(())
    }
}

/// Whether the `.pin` file of a development build of this version, at `pin`,
/// names `url`. The file goes either way: the install record takes its place.
pub(crate) fn pin_names(pin: &Path, url: &str) -> bool {
    let Ok(text) = fs::read_to_string(pin) else { return false };
    if let Err(error) = fs::remove_file(pin) {
        studio_log!("[ERROR] the earlier pin {} was not removed: {error}", pin.display());
    }
    text.trim() == url
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
    if holds_todays(asset.kind == AssetKind::Model, asset.url, target).await {
        return Ok(());
    }
    // an earlier pin's file of this name waits beside it, and comes back when today's does not arrive
    let earlier = PathBuf::from(format!("{}.earlier", target.display()));
    let set_aside = target.is_file();
    if set_aside {
        fs::rename(target, &earlier).with_context(|| format!("set {} aside", target.display()))?;
    }
    let mut outcome = fetch_absent(http, asset, target, progress, cancel).await;
    if set_aside && outcome.is_ok() {
        let (url, file) = (asset.url, target.to_path_buf());
        outcome = tokio::task::spawn_blocking(move || crate::pin_sha::check(url, &file).map(|_| ()))
            .await
            .unwrap_or_else(|error| Err(anyhow::anyhow!("the check of {} failed: {error}", target.display())));
    }
    if set_aside {
        if outcome.is_ok() {
            fs::remove_file(&earlier).ok();
        } else if !target.exists() {
            if let Err(error) = fs::rename(&earlier, target) {
                studio_log!("[ERROR] {} was not put back after the download failed: {error}", target.display());
            }
        }
    }
    outcome
}

/// `fetch` once nothing lies at `target`: from another program that has the file, or downloaded into `.part`.
async fn fetch_absent(
    http: &reqwest::Client,
    asset: &Asset,
    target: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
) -> Result<()> {
    // How big it is and whether it can be fetched in pieces, asked of the
    // server rather than assumed - both answers come from one ranged byte.
    let url = crate::net::model_url(asset.url);
    let plan = crate::chunked::probe(http, &url).await?;
    if plan.total > 0 {
        crate::sizes::learn(asset.url, plan.total);
        // another program of timoncool's has it: taken from there, by copy too when it is on another drive
        let (to, total, pinned) = (target.to_path_buf(), plan.total, asset.url);
        let taken = tokio::task::spawn_blocking(move || {
            let (source, app) = to.file_name().and_then(|name| studio_hub_client::neighbors::find_file(name, total, to.parent().and_then(Path::file_name)))?;
            Some((studio_hub_client::neighbors::take_file(&source, &to, true, |staged| crate::pin_sha::accepts(pinned, staged)), app))
        })
        .await
        .ok()
        .flatten();
        match taken {
            Some((Ok(()), app)) => {
                note_taken(asset.id, target, &app);
                return Ok(());
            }
            Some((Err(error), app)) => report_take_failure(target, &app, &error),
            None => {}
        }
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
    let outcome = crate::chunked::fetch(http, &url, &part, plan, written.clone(), cancel).await;
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

/// Says once per file why a neighbour's copy could not be taken: the setup screen asks every second.
pub(crate) fn report_take_failure(target: &Path, app: &str, error: &io::Error) {
    // a link does not cross drives; installing takes such a file by copy
    if error.kind() == io::ErrorKind::CrossesDevices {
        return;
    }
    static SAID: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    let mut said = SAID.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if !said.iter().any(|known| known == target) {
        said.push(target.to_path_buf());
        studio_log!("[ERROR] {} was not taken from {app}: {error}", target.display());
    }
}

/// A model taken from another program instead of being downloaded: which program, and the file's name.
#[derive(Clone, Debug, Serialize)]
pub struct Taken {
    pub app: String,
    pub file: String,
}

/// What was taken from another program in this run, by asset or component id: the panels say "taken from ...".
fn taken() -> &'static std::sync::Mutex<std::collections::BTreeMap<String, Taken>> {
    static TAKEN: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeMap<String, Taken>>> = std::sync::OnceLock::new();
    TAKEN.get_or_init(Default::default)
}

pub(crate) fn note_taken(id: &str, file: &Path, app: &str) {
    studio_log!("[OK] {id} taken from {app} instead of downloading");
    let file = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    taken().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id.to_string(), Taken { app: app.to_string(), file });
}

pub(crate) fn taken_from(id: &str) -> Option<String> {
    taken().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(id).map(|taken| taken.app.clone())
}

/// Everything taken from another program in this run, for the setup screen.
pub(crate) fn taken_all() -> std::collections::BTreeMap<String, Taken> {
    taken().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}


/// Downloads a whole archive with the resumable downloader and extracts the
/// asset's named files, reporting progress into the shared cell. What an earlier
/// pin recorded at `record` goes only once the archive is on disk and checked.
async fn download_and_extract_named(
    http: &reqwest::Client,
    asset: &'static Asset,
    destination: &Path,
    progress: &Arc<Mutex<Option<DownloadProgress>>>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    record: &Path,
    root: &Path,
) -> Result<(String, Vec<PathBuf>)> {
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    let archive = destination.join(format!("{}.part-archive", asset.id));
    let url = crate::net::model_url(asset.url);
    let plan = crate::chunked::probe(http, &url).await?;
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
    let outcome = crate::chunked::fetch(http, &url, &archive, plan, written, cancel).await;
    reporter.abort();
    outcome?;
    let (archive_path, wanted, destination, pinned) = (archive.clone(), asset.pick, destination.to_path_buf(), asset.url);
    let (record, root) = (record.to_path_buf(), root.to_path_buf());
    let extracted = tokio::task::spawn_blocking(move || -> Result<(String, Vec<PathBuf>)> {
        let sha256 = crate::pin_sha::check(pinned, &archive_path)?;
        replace_earlier(&record, &root, &archive_path)?;
        Ok((sha256, extract_named_local(&archive_path, wanted, &destination)?))
    })
    .await
    .map_err(|error| anyhow::anyhow!("extraction task failed: {error}"))?;
    fs::remove_file(&archive).ok();
    extracted
}

/// Today's download of an asset is on disk and checked: its install is under way from here
/// (`install_record::begin`), and what an earlier pin put down goes - the files its record lists but `kept`, the
/// file the download itself replaced, then that record. Until this point a download cut off leaves the earlier
/// pin's files where they were.
pub(crate) fn replace_earlier(record: &Path, root: &Path, kept: &Path) -> Result<()> {
    crate::install_record::begin(record)?;
    crate::install_record::remove_earlier(record, root, kept)
}

/// Whether the file a download of `url` lands on already holds today's pin: a model by its presence, as models
/// count; a runtime only by the pinned hash, since an earlier pin's file of the same name stays until today's has
/// arrived.
pub(crate) async fn holds_todays(model: bool, url: &str, target: &Path) -> bool {
    if !fs::metadata(target).is_ok_and(|meta| meta.len() > 0) {
        return false;
    }
    if model {
        return true;
    }
    let Some(pinned) = crate::pin_sha::sha256(url) else {
        return false;
    };
    let target = target.to_path_buf();
    tokio::task::spawn_blocking(move || crate::install_record::sha256_of(&target).is_ok_and(|got| got == pinned)).await.unwrap_or(false)
}

fn extract_named_local(archive: &Path, wanted: &[&str], destination: &Path) -> Result<Vec<PathBuf>> {
    let format = archive_format(archive)?;
    let mut found: Vec<&str> = Vec::new();
    let mut written: Vec<PathBuf> = Vec::new();
    if format == ArchiveFormat::Zip {
        let file = fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?;
        let mut zip = zip::ZipArchive::new(file).with_context(|| format!("read {}", archive.display()))?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).context("read zip entry")?;
            let Some(path) = entry.enclosed_name().map(|path| path.to_string_lossy().replace('\\', "/")) else {
                continue;
            };
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let Some(candidate) = wanted.iter().copied().find(|candidate| {
                if candidate.contains('/') { candidate.eq_ignore_ascii_case(&path) } else { candidate.eq_ignore_ascii_case(&name) }
            }) else {
                continue;
            };
            let mode = entry.unix_mode();
            write_entry(&mut entry, &destination.join(&name), mode)?;
            written.push(destination.join(&name));
            found.push(candidate);
        }
    } else {
        // A library is asked for by the name the loader looks up,
        // `libcublas.so.12`; the archive carries it as the versioned file that
        // name links to, which lands under its own name with the link beside it.
        let mut links: Vec<(&str, String)> = Vec::new();
        for entry in open_tar(archive, format)?.entries().with_context(|| format!("read {}", archive.display()))? {
            let mut entry = entry.with_context(|| format!("read {}", archive.display()))?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry.path().context("read tar entry")?.to_string_lossy().replace('\\', "/");
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let Some(candidate) = wanted.iter().copied().find(|candidate| name == *candidate || name.starts_with(&format!("{candidate}."))) else {
                continue;
            };
            if found.contains(&candidate) {
                continue;
            }
            let mode = entry.header().mode().ok();
            write_entry(&mut entry, &destination.join(&name), mode)?;
            written.push(destination.join(&name));
            if name != candidate {
                links.push((candidate, name));
            }
            found.push(candidate);
        }
        for (candidate, name) in links {
            link_beside(&destination.join(candidate), &name)?;
            written.push(destination.join(candidate));
        }
    }
    let missing: Vec<&str> = wanted.iter().copied().filter(|candidate| !found.contains(candidate)).collect();
    if !missing.is_empty() {
        bail!("{} did not contain {}", archive.display(), missing.join(", "));
    }
    Ok(written)
}

/// The file a picked archive entry lands as: its base name.
pub fn picked_file_name(entry: &str) -> &str {
    entry.rsplit('/').next().unwrap_or(entry)
}

/// Unpacks a release archive, zip or tar. Entries are flattened into
/// `destination` because the releases nest their binaries one or two
/// directories deep.
pub fn extract_archive(archive: &Path, destination: &Path) -> Result<Vec<PathBuf>> {
    // Packages ship every architecture they support; only this one belongs
    // here, and flattening the rest would overwrite it with an ARM build.
    const FOREIGN: [&str; 4] = ["arm64", "win-x86", if cfg!(windows) { "linux" } else { "win-x64" }, "osx"];
    fs::create_dir_all(destination).with_context(|| format!("create {}", destination.display()))?;
    let mut written: Vec<PathBuf> = Vec::new();
    match archive_format(archive)? {
        ArchiveFormat::Zip => {
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
                let mode = entry.unix_mode();
                write_entry(&mut entry, &destination.join(&name), mode)?;
                written.push(destination.join(name));
            }
        }
        format => {
            // A tar carries the libraries and programs among headers and build
            // files; the loader's short names are links to the versioned file.
            let mut links: Vec<(String, String)> = Vec::new();
            for entry in open_tar(archive, format)?.entries().with_context(|| format!("read {}", archive.display()))? {
                let mut entry = entry.with_context(|| format!("read {}", archive.display()))?;
                let path = entry.path().context("read tar entry")?.to_string_lossy().replace('\\', "/");
                // a macOS package's debug symbols carry the library's own file name
                if path.contains(".dSYM/") {
                    continue;
                }
                let name = path.rsplit('/').next().unwrap_or(&path).to_string();
                let kind = entry.header().entry_type();
                let mode = entry.header().mode().ok();
                let executable = mode.is_some_and(|mode| mode & 0o111 != 0);
                if kind.is_symlink() && is_shared_library(&name) {
                    let target = entry.link_name().ok().flatten().map(|target| target.to_string_lossy().replace('\\', "/")).unwrap_or_default();
                    links.push((name, target.rsplit('/').next().unwrap_or(&target).to_string()));
                } else if kind.is_file() && (is_shared_library(&name) || executable) {
                    write_entry(&mut entry, &destination.join(&name), mode)?;
                    written.push(destination.join(&name));
                }
            }
            // A link may name another link, in whatever order the archive lists them.
            while !links.is_empty() {
                let (ready, waiting): (Vec<_>, Vec<_>) = links.into_iter().partition(|(_, target)| destination.join(target).is_file());
                if ready.is_empty() {
                    bail!("{} links {} to files it does not contain", archive.display(), waiting.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", "));
                }
                for (name, target) in ready {
                    link_beside(&destination.join(&name), &target)?;
                    written.push(destination.join(&name));
                }
                links = waiting;
            }
            // a library that comes by its full version alone (BSRoformer.cpp's processor and macOS builds) gets the
            // name the loader asks for beside it
            let unpacked: Vec<String> = written.iter().filter_map(|path| path.file_name()?.to_str().map(str::to_owned)).collect();
            for name in unpacked {
                let Some(short) = soname_of(&name) else { continue };
                let link = destination.join(&short);
                if !link.exists() {
                    link_beside(&link, &name)?;
                    written.push(link);
                }
            }
        }
    }
    fs::remove_file(archive).ok();
    written.sort();
    written.dedup();
    Ok(written)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveFormat {
    Zip,
    TarGz,
    TarXz,
}

/// An archive's format by its first bytes: the file on disk is named after the
/// download, not after what it is.
fn archive_format(archive: &Path) -> Result<ArchiveFormat> {
    let mut head = [0u8; 6];
    let read = io::Read::read(&mut fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?, &mut head)?;
    Ok(match &head[..read] {
        [0x1f, 0x8b, ..] => ArchiveFormat::TarGz,
        [0xfd, b'7', b'z', b'X', b'Z', 0x00] => ArchiveFormat::TarXz,
        _ => ArchiveFormat::Zip,
    })
}

fn open_tar(archive: &Path, format: ArchiveFormat) -> Result<tar::Archive<Box<dyn io::Read>>> {
    let file = io::BufReader::new(fs::File::open(archive).with_context(|| format!("open {}", archive.display()))?);
    let reader: Box<dyn io::Read> = match format {
        ArchiveFormat::TarGz => Box::new(flate2::read::GzDecoder::new(file)),
        _ => Box::new(lzma_rust2::XzReader::new(file, true)),
    };
    Ok(tar::Archive::new(reader))
}

/// `libcublas.so.12`, `libcublas.so.12.9.2.10`, `libonnxruntime.so`.
fn is_shared_library(name: &str) -> bool {
    name.ends_with(".so") || name.contains(".so.") || name.ends_with(".dylib")
}

/// The name the loader asks for of a library named by its full version: `libX.so.0.15.1` → `libX.so.0`,
/// `libX.0.15.1.dylib` → `libX.0.dylib`; None for a name without one (as Speech Studio's setup gives it).
fn soname_of(name: &str) -> Option<String> {
    let numeric = |parts: &[&str]| parts.len() >= 2 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    if let Some(at) = name.find(".so.") {
        let version: Vec<&str> = name[at + 4..].split('.').collect();
        return numeric(&version).then(|| format!("{}.so.{}", &name[..at], version[0]));
    }
    let stem = name.strip_suffix(".dylib")?;
    let mut parts: Vec<&str> = stem.split('.').collect();
    let base = parts.remove(0);
    let first = parts.iter().position(|part| part.bytes().all(|b| b.is_ascii_digit()))?;
    let (named, version) = parts.split_at(first);
    numeric(version).then(|| format!("{}.{}.dylib", std::iter::once(base).chain(named.iter().copied()).collect::<Vec<_>>().join("."), version[0]))
}

/// Writes an archive entry through a partial file, keeping its permission
/// bits where the system has them: a program unpacked without them does not run.
fn write_entry(entry: &mut impl io::Read, target: &Path, mode: Option<u32>) -> Result<()> {
    let partial = target.with_file_name(format!("{}.part", target.file_name().unwrap_or_default().to_string_lossy()));
    let mut out = fs::File::create(&partial).with_context(|| format!("create {}", partial.display()))?;
    io::copy(entry, &mut out).with_context(|| format!("extract {}", target.display()))?;
    drop(out);
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&partial, fs::Permissions::from_mode(mode & 0o777)).with_context(|| format!("set permissions of {}", partial.display()))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    fs::rename(&partial, target).with_context(|| format!("publish {}", target.display()))
}

/// Gives the file `target` beside `link` the name `link` as well; a copy where
/// there are no symbolic links.
fn link_beside(link: &Path, target: &str) -> Result<()> {
    match fs::remove_file(link) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("replace {}", link.display())),
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).with_context(|| format!("link {} to {target}", link.display()))?;
    #[cfg(not(unix))]
    fs::copy(link.with_file_name(target), link).with_context(|| format!("copy {target} to {}", link.display()))?;
    Ok(())
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
    fn a_download_counts_what_its_partial_file_holds_and_room_to_unpack_an_archive() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let model: &'static Asset = Box::leak(Box::new(Asset { bytes: 1000, ..asset() }));
        let archive: &'static Asset = Box::leak(Box::new(Asset { id: "test-archive", kind: AssetKind::Runtime, relative_path: "runtime/tool.zip", bytes: 1000, unzip_into: Some("tool"), marker: "tool", ..asset() }));
        fs::create_dir_all(downloader.path_of(model).parent().unwrap()).unwrap();
        fs::write(downloader.path_of(model).with_extension("part"), vec![0u8; 400]).unwrap();
        assert_eq!(downloader.space_needed(&[model]), 600);
        assert_eq!(downloader.space_needed(&[archive]), 3000);
        assert_eq!(downloader.space_needed(&[model, archive]), 3600);
        assert!(downloader.ensure_space(&[model, archive]).is_ok());

        let huge: &'static Asset = Box::leak(Box::new(Asset { id: "test-huge", relative_path: "models/huge.bin", bytes: 1 << 60, ..asset() }));
        let refused = downloader.ensure_space(&[huge]).unwrap_err().to_string();
        assert!(refused.starts_with("not enough space:"), "{refused}");
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

        let program = downloader.runtime_dir("cuda").join("whisper-cli.exe");
        fs::write(&program, b"x").unwrap();
        assert!(!downloader.is_installed(&runtime), "no record and no key file in the pin: fetched again");
        crate::install_record::write(&downloader.record_of(&runtime), runtime.url, "", &[program], &root).unwrap();
        assert!(downloader.is_installed(&runtime));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_runtime_of_an_earlier_pin_is_not_installed() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let runtime = Asset { id: "test-runtime", kind: AssetKind::Runtime, url: "https://example.invalid/engine-0.1.3.dll", relative_path: "models/engine/engine.dll", bytes: 5, ..asset() };
        let file = downloader.path_of(&runtime);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"0.1.1-build").unwrap();
        assert!(!downloader.is_installed(&runtime), "a file of another size than the pin's is an earlier build");
        let beside = file.with_file_name("engine-helper.dll");
        fs::write(&beside, b"helper").unwrap();
        let record = downloader.record_of(&runtime);
        crate::install_record::write(&record, "https://example.invalid/engine-0.1.1.dll", "", &[file.clone(), beside.clone()], &root).unwrap();
        assert!(!downloader.is_installed(&runtime), "recorded from the earlier pin");
        // today's download has replaced the file in place: the rest of the earlier pin and its record go
        replace_earlier(&record, &root, &file).unwrap();
        assert!(file.exists() && !beside.exists() && !record.exists());
        assert!(!downloader.is_installed(&runtime), "until its record is written the install is under way");
        fs::write(&file, b"0.1.3").unwrap();
        crate::install_record::write(&record, runtime.url, "", &[file.clone()], &root).unwrap();
        assert!(downloader.is_installed(&runtime));
        fs::remove_dir_all(&root).ok();
    }

    /// A download that does not arrive leaves an earlier pin's file where it was, and one that arrives takes its
    /// place; a runtime whose file already holds today's pin by its hash is not fetched again.
    #[tokio::test]
    async fn an_earlier_pin_stays_until_todays_download_arrives() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let runtime = Asset { id: "test-runtime", kind: AssetKind::Runtime, url: "http://127.0.0.1:9/engine-0.1.3.dll", relative_path: "models/engine/engine.dll", bytes: 5, ..asset() };
        let file = downloader.path_of(&runtime);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"0.1.1-build").unwrap();
        let cell = Arc::new(Mutex::new(None));
        assert!(fetch(&downloader.http, &runtime, &file, &cell, downloader.cancel.clone()).await.is_err(), "nothing answers on port 9");
        assert_eq!(fs::read(&file).unwrap(), b"0.1.1-build", "the earlier file is back in its place");
        assert!(!file.with_file_name("engine.dll.earlier").exists());
        assert!(!holds_todays(false, runtime.url, &file).await, "an unpinned runtime is never taken as today's by its name");
        assert!(holds_todays(true, runtime.url, &file).await, "a model counts by its file");
        fs::remove_dir_all(&root).ok();
    }

    /// A runtime a development build recorded with a `.pin` of today's URL is
    /// taken over with a record, and the pin goes; one of another URL is fetched again.
    #[test]
    fn an_earlier_pin_of_todays_url_becomes_the_record() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let runtime = Asset { id: "test-runtime", kind: AssetKind::Runtime, url: "https://example.invalid/engine-0.1.3.dll", relative_path: "models/engine/engine.dll", bytes: 5, ..asset() };
        let file = downloader.path_of(&runtime);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, b"a build of another size").unwrap();
        let pin = file.with_file_name("engine.dll.pin");
        fs::write(&pin, runtime.url).unwrap();
        assert!(downloader.is_installed(&runtime), "the pin names today's URL");
        assert!(downloader.record_of(&runtime).exists() && !pin.exists(), "the record takes the pin's place");

        fs::remove_file(downloader.record_of(&runtime)).unwrap();
        fs::write(&pin, "https://example.invalid/engine-0.1.1.dll").unwrap();
        assert!(!downloader.is_installed(&runtime), "the pin names an earlier URL");
        assert!(!pin.exists() && !downloader.record_of(&runtime).exists());

        let archive = Asset { id: "test-archive", kind: AssetKind::Runtime, url: "https://example.invalid/tool.zip", relative_path: "runtime/tool.zip", unzip_into: Some("tool"), marker: "tool.exe", ..asset() };
        fs::create_dir_all(downloader.runtime_dir("tool")).unwrap();
        fs::write(downloader.runtime_dir("tool").join("tool.exe"), b"x").unwrap();
        fs::write(downloader.path_of(&archive), b"zip").unwrap();
        fs::write(downloader.picked_into(&archive).join(".pin-test-archive"), archive.url).unwrap();
        assert!(downloader.is_installed(&archive), "an unpacked runtime the pin names");
        fs::remove_dir_all(&root).ok();
    }

    /// An install cut off after the archive's key file was unpacked and before
    /// its record was written: the status poll in between must not take the
    /// half-unpacked folder over as a download from before records.
    #[test]
    fn an_install_cut_off_before_its_record_is_not_installed() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let url = "https://github.com/microsoft/onnxruntime/releases/download/v1.30.0/onnxruntime-win-x64-1.30.0.zip";
        let runtime = Asset { id: "test-runtime", kind: AssetKind::Runtime, url, relative_path: "runtime/onnxruntime.zip", unzip_into: Some("onnx"), marker: "onnxruntime.dll", ..asset() };
        let (key, size) = crate::install_record::key_file(url).unwrap();
        let record = downloader.record_of(&runtime);
        replace_earlier(&record, &root, &downloader.path_of(&runtime)).unwrap();
        fs::create_dir_all(downloader.runtime_dir("onnx")).unwrap();
        let unpacked = downloader.runtime_dir("onnx").join(key);
        fs::File::create(&unpacked).unwrap().set_len(size).unwrap();
        assert!(!downloader.is_installed(&runtime), "half unpacked, counted as installed");
        assert!(!record.exists(), "half unpacked, recorded as installed");

        // the studio closed here; at the next start the folder is still not taken over
        let reopened = Downloader::new(root.clone());
        assert!(!reopened.is_installed(&runtime) && !record.exists());

        // the next install, its download on disk, marks itself again and ends with the record alone
        replace_earlier(&record, &root, &reopened.path_of(&runtime)).unwrap();
        fs::File::create(&unpacked).unwrap().set_len(size).unwrap();
        crate::install_record::write(&record, url, "abc", &[unpacked], &root).unwrap();
        assert!(reopened.is_installed(&runtime));
        let marks: Vec<_> = fs::read_dir(record.parent().unwrap()).unwrap().flatten().filter(|entry| entry.path().is_file()).map(|entry| entry.file_name()).collect();
        assert_eq!(marks, vec![record.file_name().unwrap().to_owned()], "only the record is left beside the archive");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn capabilities_sharing_a_runtime_root_find_one_unpacked_build() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let shared = root.join("hotstep");
        let midi = Downloader::with_runtimes(root.join("midi"), shared.clone());
        let training = Downloader::with_runtimes(root.join("training"), shared.clone());
        let tool = |id: &'static str, marker: &'static str| Asset { id, kind: AssetKind::Runtime, relative_path: "tools.zip", unzip_into: Some("hotstep-tools-1"), marker, ..asset() };
        let transcriber = tool("music-midi", "music-midi.exe");
        let trainer = tool("music-train", "music-train.exe");
        assert_eq!(midi.runtime_dir("hotstep-tools-1"), training.runtime_dir("hotstep-tools-1"));
        fs::create_dir_all(midi.runtime_dir("hotstep-tools-1")).unwrap();
        fs::create_dir_all(midi.root()).unwrap();
        fs::create_dir_all(training.root()).unwrap();
        for name in ["music-midi.exe", "music-train.exe", "ggml-cuda.dll"] {
            fs::write(shared.join("hotstep-tools-1").join(name), b"x").unwrap();
        }
        let unpacked = vec![shared.join("hotstep-tools-1").join("music-midi.exe"), shared.join("hotstep-tools-1").join("music-train.exe")];
        crate::install_record::write(&midi.record_of(&transcriber), transcriber.url, "", &unpacked, midi.root()).unwrap();
        crate::install_record::write(&training.record_of(&trainer), trainer.url, "", &unpacked, training.root()).unwrap();
        assert!(midi.is_installed(&transcriber) && training.is_installed(&trainer));
        assert_eq!(midi.path_of(&transcriber), root.join("midi").join("tools.zip"), "the archive downloads into the capability's own folder");
        fs::remove_dir_all(&root).ok();
    }


    /// A package carries one library for several platforms under the same
    /// name: a pick by path takes the entry it names, a bare name any entry.
    #[test]
    fn a_pick_by_path_takes_that_entry_and_not_its_namesakes() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(root.join("out")).unwrap();
        let archive = root.join("package.nupkg");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        for (path, body) in [("bin/x64-xbox/DirectML.dll", "xbox"), ("bin/x64-win/DirectML.dll", "win"), ("lib/readme.txt", "notes")] {
            zip.start_file(path, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();

        extract_named_local(&archive, &["bin/x64-win/DirectML.dll", "readme.txt"], &root.join("out")).unwrap();
        assert_eq!(fs::read_to_string(root.join("out").join("DirectML.dll")).unwrap(), "win");
        assert_eq!(fs::read_to_string(root.join("out").join("readme.txt")).unwrap(), "notes");
        assert!(extract_named_local(&archive, &["bin/arm64-win/DirectML.dll"], &root.join("out")).is_err());
        fs::remove_dir_all(&root).ok();
    }

    /// A Linux release archive as NVIDIA and Microsoft ship them: one versioned
    /// library, the loader's names as links to it, a program, and headers.
    fn linux_archive(root: &Path) -> PathBuf {
        let archive = root.join("package.part-archive");
        let encoder = flate2::write::GzEncoder::new(fs::File::create(&archive).unwrap(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(encoder);
        let mut add = |path: &str, mode: u32, body: &[u8]| {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            tar.append_data(&mut header, path, body).unwrap();
        };
        add("pkg/lib/libfoo.so.1.2.3", 0o755, b"library");
        add("pkg/bin/tool", 0o755, b"program");
        add("pkg/include/foo.h", 0o644, b"header");
        for (link, target) in [("pkg/lib/libfoo.so", "libfoo.so.1"), ("pkg/lib/libfoo.so.1", "libfoo.so.1.2.3")] {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            tar.append_link(&mut header, link, target).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
        archive
    }

    #[test]
    fn a_tar_unpacks_its_libraries_and_programs_with_the_loader_names() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let archive = linux_archive(&root);
        extract_archive(&archive, &root.join("out")).unwrap();
        for name in ["libfoo.so.1.2.3", "libfoo.so.1", "libfoo.so"] {
            assert_eq!(fs::read_to_string(root.join("out").join(name)).unwrap(), "library", "{name}");
        }
        assert_eq!(fs::read_to_string(root.join("out").join("tool")).unwrap(), "program");
        assert!(!root.join("out").join("foo.h").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(fs::metadata(root.join("out").join("tool")).unwrap().permissions().mode() & 0o111, 0);
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_library_picked_from_a_tar_by_its_loader_name_lands_with_that_name() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(root.join("out")).unwrap();
        let archive = linux_archive(&root);
        extract_named_local(&archive, &["libfoo.so.1"], &root.join("out")).unwrap();
        assert_eq!(fs::read_to_string(root.join("out").join("libfoo.so.1")).unwrap(), "library");
        assert!(root.join("out").join("libfoo.so.1.2.3").is_file());
        assert!(!root.join("out").join("tool").exists());
        assert!(extract_named_local(&archive, &["libbar.so.1"], &root.join("out")).is_err());
        fs::remove_dir_all(&root).ok();
    }

    /// A library that comes by its full version gets the name the loader asks for, on Linux and on macOS alike
    /// (the same rule as Speech Studio's setup).
    #[test]
    fn a_library_by_its_full_version_gets_the_name_the_loader_asks_for() {
        assert_eq!(soname_of("libggml-base.so.0.15.1").as_deref(), Some("libggml-base.so.0"));
        assert_eq!(soname_of("libggml-base.0.15.1.dylib").as_deref(), Some("libggml-base.0.dylib"));
        assert_eq!(soname_of("libllama-common.0.7.0.dylib").as_deref(), Some("libllama-common.0.dylib"));
        for name in ["libggml.0.dylib", "libqwen.dylib", "libggml.so.0", "libfoo.so", "bs_roformer-cli", "libonnxruntime.dylib"] {
            assert_eq!(soname_of(name), None, "{name}");
        }
    }

    /// BSRoformer.cpp's processor and macOS archives as v0.1.0 ships them: the program and the ggml
    /// libraries by their full versions, no links; the program asks for libggml.so.0 / @rpath/libggml.0.dylib.
    #[test]
    fn an_archive_of_full_version_libraries_unpacks_with_the_loader_names() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).unwrap();
        let archive = root.join("bsroformer.part-archive");
        let encoder = flate2::write::GzEncoder::new(fs::File::create(&archive).unwrap(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(encoder);
        for (path, mode) in [("bs_roformer-cli", 0o755), ("libggml.0.15.1.dylib", 0o644), ("libggml-metal.0.15.1.dylib", 0o644), ("libggml.so.0.15.1", 0o644)] {
            let mut header = tar::Header::new_gnu();
            header.set_size(path.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            tar.append_data(&mut header, path, path.as_bytes()).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
        let files = extract_archive(&archive, &root.join("out")).unwrap();
        for (short, full) in [("libggml.0.dylib", "libggml.0.15.1.dylib"), ("libggml-metal.0.dylib", "libggml-metal.0.15.1.dylib"), ("libggml.so.0", "libggml.so.0.15.1")] {
            assert_eq!(fs::read_to_string(root.join("out").join(short)).unwrap(), full, "{short}");
            assert!(files.contains(&root.join("out").join(short)), "{short} is recorded with the install");
        }
        assert!(root.join("out").join("bs_roformer-cli").is_file());
        fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn removing_a_picked_library_takes_the_file_its_name_links_to() {
        let root = std::env::temp_dir().join(format!("downloads-{}", uuid::Uuid::now_v7()));
        let downloader = Downloader::new(root.clone());
        let runtime = Asset { id: "test-runtime", kind: AssetKind::Runtime, unzip_into: Some("cuda"), pick: &["libfoo.so.1"], ..asset() };
        fs::create_dir_all(downloader.picked_into(&runtime)).unwrap();
        let archive = linux_archive(&root);
        let files = extract_named_local(&archive, runtime.pick, &downloader.picked_into(&runtime)).unwrap();
        crate::install_record::write(&downloader.record_of(&runtime), runtime.url, "", &files, &root).unwrap();
        assert!(downloader.is_installed(&runtime));
        assert_eq!(downloader.remove(&runtime).unwrap(), "library".len() as u64);
        assert!(!downloader.runtime_dir("cuda").join("libfoo.so.1.2.3").exists());
        assert!(!downloader.is_installed(&runtime));
        fs::remove_dir_all(&root).ok();
    }
}
