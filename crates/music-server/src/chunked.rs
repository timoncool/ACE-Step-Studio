//! Fetching a large file over several connections at once.
//!
//! One connection per file is what the studio used to do, and on a twelve
//! gigabyte model that is one TCP stream deciding how long the user waits -
//! usually a small fraction of the line. Hugging Face serves ranges, so the
//! file is cut into pieces and several are fetched at a time.
//!
//! The shape of this is taken from Dub Studio, which learned it the hard way,
//! and the awkward parts of it are the point:
//!
//! * Four connections for the whole job, not four per file and not sixteen.
//!   Xet-CAS - where the alternative quantisations live - drops connections
//!   under heavier parallelism, and a dropped range leaves a hole in a file
//!   whose size is already correct, so nothing notices until the model fails
//!   to load.
//! * A range is only accepted when the server answers 206 and sends exactly
//!   the bytes asked for. Anything else is retried, up to eight times, with the
//!   progress of the failed attempt subtracted again.
//! * A piece counts as done only after its bytes are on the disk - `sync_data`
//!   first, then its offset is appended to the manifest beside the file. The
//!   other order survives a normal exit and loses to a power cut: the manifest
//!   would promise data that never landed, and the resumed download would skip
//!   a hole.
//!
//! Where a server does not serve ranges, this falls back to reading the whole
//! body in order, which is what the studio did everywhere before.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;

/// The smallest piece worth a connection of its own.
const PIECE_MIN: u64 = 64 * 1024 * 1024;

/// How often a piece notes in the manifest how far it has come: what a download closed halfway fetches again.
const CHECKPOINT: u64 = 16 * 1024 * 1024;

/// The pieces a file of `total` bytes is fetched in, the first and the last byte of each: one per connection, fewer
/// for a file too small to be worth it. Hugging Face counts every request for a model file as a download of it, so
/// a file is a handful of requests whatever its size, and a dropped connection asks only for the rest of its piece.
fn pieces_of(total: u64) -> Vec<(u64, u64)> {
    if total == 0 {
        return Vec::new();
    }
    let count = total.div_ceil(PIECE_MIN).clamp(1, SLOTS as u64);
    let size = total.div_ceil(count);
    (0..count).map(|index| (index * size, ((index + 1) * size).min(total) - 1)).filter(|(start, _)| *start < total).collect()
}

/// Connections for the whole studio - not per file and not per panel.
///
/// Four is not shyness. Xet-CAS starts dropping connections above that, and a
/// dropped range is a hole in a file of exactly the right size. Four per file
/// looked fine until two panels downloaded at once: eight connections, and the
/// second download sat at nought bytes while the first ran. Hence one budget,
/// held by whoever is fetching a piece right now.
const SLOTS: usize = 4;

fn connections() -> &'static tokio::sync::Semaphore {
    static BUDGET: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    BUDGET.get_or_init(|| tokio::sync::Semaphore::new(SLOTS))
}

/// Attempts in a row that bring nothing before a piece gives up. Transient drops are normal on a twelve gigabyte
/// file, and an attempt that brought bytes starts the count again.
const RETRIES: u32 = 8;

/// How long a download may spend waiting to be let in before it gives up.
///
/// "Too many requests" is not a failure, it is a queue, and it must not count
/// against the attempts above: eight retries four hundred milliseconds apart
/// gave up after fourteen seconds and told the user their download was broken.
/// Hugging Face's limits clear in minutes, so the download waits minutes.
const RATE_LIMIT_BUDGET: std::time::Duration = std::time::Duration::from_secs(20 * 60);

/// Seconds the studio is currently waiting for a server to let it in, so a
/// download that is queued rather than broken can say so. A bar at nought per
/// cent with nothing under it is indistinguishable from a hang.
static WAITING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How long the download is being kept waiting, if it is.
pub fn waiting_for_server() -> Option<u64> {
    match WAITING.load(Ordering::Relaxed) {
        0 => None,
        seconds => Some(seconds),
    }
}

/// What Hugging Face says about the window we are in.
///
/// It does not send `Retry-After`. It implements the IETF RateLimit header
/// draft instead: `RateLimit: "resolvers";r=2871;t=143` - requests left in this
/// window, and seconds until it resets. Waiting on a guess when the server has
/// told you the number is how a download sits for fifteen seconds and walks
/// straight back into the same refusal.
fn window(response: &reqwest::Response) -> Option<Window> {
    let header = response.headers().get("ratelimit")?.to_str().ok()?;
    let field = |name: &str| {
        header
            .split(';')
            .filter_map(|part| part.trim().split_once('='))
            .find(|(key, _)| key.trim() == name)
            .and_then(|(_, value)| value.trim().parse::<u64>().ok())
    };
    Some(Window { remaining: field("r")?, resets_in: field("t").unwrap_or(300) })
}

#[derive(Debug, Clone, Copy)]
struct Window {
    remaining: u64,
    resets_in: u64,
}

/// Below this many requests left in the window, a download stops asking and
/// waits for the window to turn over. Hitting nought means every request -
/// including the ones already in flight - comes back 429, which is much more
/// expensive than pausing for a minute.
const KEEP_IN_RESERVE: u64 = 25;

/// How long to wait when the window is spent, or when the server refused
/// without saying why.
fn asked_to_wait(response: &reqwest::Response) -> std::time::Duration {
    let seconds = window(response).map(|window| window.resets_in).unwrap_or(30);
    std::time::Duration::from_secs(seconds.clamp(1, 310))
}

/// Whether a status is the server asking for patience rather than refusing.
fn is_busy(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// What the server will let us do with this file.
#[derive(Debug, Clone, Copy)]
pub struct Plan {
    pub total: u64,
    pub ranged: bool,
}

/// Asks for one byte, which answers both questions at once: how big the file
/// is, and whether ranges work at all. A HEAD answers the first only, and some
/// CDNs answer it differently from the GET that follows.
pub async fn probe(http: &reqwest::Client, url: &str) -> Result<(Plan, String)> {
    let mut last = String::new();
    let mut unreachable = 0;
    let mut attempts = 0;
    for attempt in 0..RETRIES {
        attempts = attempt + 1;
        match probe_once(http, url).await {
            Ok(plan) => return Ok((plan, url.to_owned())),
            Err(error) if error.is::<Refused>() => return Err(error),
            Err(error) => {
                last = format!("{error:#}");
                if cannot_connect(&error) {
                    unreachable += 1;
                    if unreachable == CONNECT_RETRIES {
                        break;
                    }
                }
            }
        }
        // Hugging Face answers 429 when it has had enough for the moment, and
        // a busy hour is not a broken download. Without this the whole install
        // died on the first refusal, with nothing to retry it.
        tokio::time::sleep(std::time::Duration::from_millis(500 << attempt.min(7))).await;
    }
    // Hugging Face could not be reached from this network at all - the top setup
    // failure in telemetry, a network behind the Great Firewall among them. The
    // HF-Mirror usually answers where it does not, so fall back to it once before
    // giving up; a non-HF url mirrors to itself and drops straight through.
    // Through a proxy whose address takes no connection the mirror is behind the same dead proxy: the proxy is named
    // instead. A proxy that takes the connection and cannot reach the host is alive, and the mirror is tried through it.
    let proxy = match crate::net::route_for(url).filter(|_| unreachable == CONNECT_RETRIES) {
        Some(route) if !route.takes_connections().await => Some(route),
        _ => None,
    };
    if unreachable == CONNECT_RETRIES && proxy.is_none() {
        let mirror = crate::net::mirror_url(url);
        if mirror != url {
            if let Ok(found) = Box::pin(probe(http, &mirror)).await {
                return Ok(found);
            }
        }
    }
    Err(anyhow!("{}", start_failure(url, attempts, unreachable == CONNECT_RETRIES, &last, proxy.as_ref())))
}

/// Attempts at a host that cannot be connected to at all - no route, no DNS
/// answer, a refused or timed-out connection - before the download says so.
/// Each costs up to the connect timeout, and a blocked host stays blocked.
const CONNECT_RETRIES: u32 = 3;

/// Whether the request never reached the server: a connection that failed, not
/// an answer and not a dropped transfer.
fn cannot_connect(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.downcast_ref::<reqwest::Error>().is_some_and(|error| error.is_connect()))
}

/// Why a download could not start. The host is named on its own, so it
/// survives the statistics' removal of addresses: which server a network
/// cannot reach is the whole question.
fn start_failure(url: &str, attempts: u32, unreachable: bool, last: &str, proxy: Option<&crate::net::ProxyRoute>) -> String {
    let host = reqwest::Url::parse(url).ok().and_then(|parsed| parsed.host_str().map(str::to_owned)).unwrap_or_default();
    if let Some(proxy) = proxy.filter(|_| unreachable) {
        return format!("could not connect to {host} ({attempts} attempts) for {url} through a proxy: {last}. {}", proxy.did_not_connect());
    }
    let mirror = if host == "huggingface.co" {
        ". Hugging Face did not answer from this network: Settings - Providers - Proxy can take models from the HF-Mirror instead"
    } else {
        ""
    };
    if unreachable {
        format!("could not connect to {host} ({attempts} attempts) for {url}: {last}{mirror}")
    } else {
        format!("could not start {url} from {host} after {attempts} attempts: {last}{mirror}")
    }
}

/// Sends one request, waiting out a server that is only busy.
///
/// The wait comes from the server's own `Retry-After` where it gives one. This
/// is where "too many requests" stops being an error: the caller sees either a
/// real answer or a real failure, never a queue.
async fn send(http: &reqwest::Client, url: &str, range: Option<String>) -> Result<reqwest::Response> {
    let mut waited = std::time::Duration::ZERO;
    loop {
        let _slot = connections().acquire().await.context("wait for a connection")?;
        let mut request = http.get(url);
        if let Some(range) = &range {
            request = request.header(reqwest::header::RANGE, range);
        }
        let response = request.send().await.with_context(|| format!("request {url}"))?;
        if !is_busy(response.status()) {
            // Slow down before the wall, not after it. The window is five
            // minutes long and shared by every request from this machine, so a
            // download that spends the last of it earns a refusal for
            // everything else the studio is doing.
            if let Some(window) = window(&response) {
                if window.remaining < KEEP_IN_RESERVE {
                    let pause = std::time::Duration::from_secs(window.resets_in.clamp(1, 310));
                    drop(_slot);
                    WAITING.store(pause.as_secs(), Ordering::Relaxed);
                    tokio::time::sleep(pause).await;
                    WAITING.store(0, Ordering::Relaxed);
                }
            }
            return Ok(response);
        }
        let pause = asked_to_wait(&response);
        if waited + pause > RATE_LIMIT_BUDGET {
            bail!("{url} answered {} for {} minutes", response.status(), waited.as_secs() / 60);
        }
        waited += pause;
        // The permit is dropped before sleeping, so a server that is turning
        // us away does not also hold the studio's connections shut.
        drop(_slot);
        WAITING.store(pause.as_secs().max(1), Ordering::Relaxed);
        tokio::time::sleep(pause).await;
        WAITING.store(0, Ordering::Relaxed);
    }
}

async fn probe_once(http: &reqwest::Client, url: &str) -> Result<Plan> {
    let response = send(http, url, Some("bytes=0-0".into())).await?;
    if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let total = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit('/').next())
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(0);
        if total > 0 {
            return Ok(Plan { total, ranged: true });
        }
    }
    if response.status().is_client_error() {
        return Err(Refused(format!("{url} answered {}", response.status())).into());
    }
    if !response.status().is_success() {
        bail!("{url} answered {}", response.status());
    }
    Ok(Plan { total: response.content_length().unwrap_or(0), ranged: false })
}

/// A server that said no (a 4xx; "too many requests" is waited out in `send`): asking again gets the same answer.
#[derive(Debug)]
struct Refused(String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

/// The manifest beside the part file: how far each piece of it has come.
pub fn manifest_path(part: &Path) -> PathBuf {
    let mut name = part.as_os_str().to_os_string();
    name.push(".pieces");
    PathBuf::from(name)
}

/// The manifest of the sixteen megabyte pieces earlier versions fetched a file in.
fn earlier_manifest_path(part: &Path) -> PathBuf {
    let mut name = part.as_os_str().to_os_string();
    name.push(".done");
    PathBuf::from(name)
}

/// How much of a download lies in its part file: what its manifest says of each piece, the part file being reserved
/// at the whole size; a part file without a manifest (a server without ranges) holds what it is long, and one left by
/// an earlier version's pieces starts over.
pub fn on_disk(part: &Path) -> u64 {
    let Ok(length) = std::fs::metadata(part).map(|meta| meta.len()) else { return 0 };
    if manifest_path(part).exists() {
        return progress_of(part, length).values().sum();
    }
    if earlier_manifest_path(part).exists() {
        return 0;
    }
    length
}

/// The mark of the manifest's first entry, which carries the size of the file the manifest was written for.
const WHOLE: u64 = u64::MAX;

fn entry(first: u64, second: u64) -> [u8; 16] {
    let mut entry = [0u8; 16];
    entry[..8].copy_from_slice(&first.to_le_bytes());
    entry[8..].copy_from_slice(&second.to_le_bytes());
    entry
}

/// How far each piece has come, by its first byte: the furthest note of the manifest, which holds only for the file
/// size it was written for.
fn progress_of(part: &Path, total: u64) -> std::collections::HashMap<u64, u64> {
    let mut progress = std::collections::HashMap::new();
    let Ok(bytes) = std::fs::read(manifest_path(part)) else { return progress };
    let mut entries = bytes.chunks_exact(16).map(|entry| {
        let (first, second) = entry.split_at(8);
        (u64::from_le_bytes(first.try_into().unwrap_or_default()), u64::from_le_bytes(second.try_into().unwrap_or_default()))
    });
    if entries.next() != Some((WHOLE, total)) {
        return progress;
    }
    let lengths: std::collections::HashMap<u64, u64> = pieces_of(total).into_iter().map(|(start, end)| (start, end - start + 1)).collect();
    for (start, have) in entries {
        if lengths.get(&start).is_some_and(|length| have <= *length) {
            let noted = progress.entry(start).or_insert(0u64);
            *noted = (*noted).max(have);
        }
    }
    progress
}

/// A piece's progress, noted after its bytes reached the disk: the other order loses a power cut, the manifest
/// promising data that never landed.
fn note(file: &File, manifest: &Mutex<File>, start: u64, have: u64) {
    file.sync_data().ok();
    if let Ok(mut manifest) = manifest.lock() {
        manifest.write_all(&entry(start, have)).ok();
        manifest.sync_data().ok();
    }
}

#[cfg(windows)]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> std::io::Result<usize> {
    use std::os::windows::fs::FileExt;
    file.seek_write(buffer, offset)
}

#[cfg(not(windows))]
fn write_at(file: &File, buffer: &[u8], offset: u64) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.write_at(buffer, offset)
}

pub async fn fetch(
    http: &reqwest::Client,
    url: &str,
    part: &Path,
    plan: Plan,
    written: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    if let Some(parent) = part.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    if !plan.ranged || plan.total == 0 {
        return whole(http, url, part, written, cancel).await;
    }

    // A manifest speaks of the part file it was written beside: left after that file was removed, or beside one of
    // another length, it notes bytes that are not there.
    let reserved = std::fs::metadata(part).is_ok_and(|meta| meta.len() == plan.total);
    let file = Arc::new(
        OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(part)
            .with_context(|| format!("open {}", part.display()))?,
    );
    file.set_len(plan.total).with_context(|| format!("reserve {} bytes", plan.total))?;

    std::fs::remove_file(earlier_manifest_path(part)).ok();
    let progress = if reserved { progress_of(part, plan.total) } else { std::collections::HashMap::new() };
    let mut noted = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(progress.is_empty())
        .append(!progress.is_empty())
        .open(manifest_path(part))
        .with_context(|| format!("open {}", manifest_path(part).display()))?;
    if progress.is_empty() {
        noted.write_all(&entry(WHOLE, plan.total)).with_context(|| format!("write {}", manifest_path(part).display()))?;
    }
    let manifest = Arc::new(Mutex::new(noted));

    let mut pieces = Vec::new();
    for (start, end) in pieces_of(plan.total) {
        let have = progress.get(&start).copied().unwrap_or(0);
        written.fetch_add(have, Ordering::Relaxed);
        if have < end - start + 1 {
            pieces.push((start, end, have));
        }
    }
    if pieces.is_empty() {
        file.sync_all().ok();
        std::fs::remove_file(manifest_path(part)).ok();
        return Ok(());
    }

    let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(pieces)));
    // The caller's cancel flag is the stop flag: a piece that fails stops the
    // rest, and so does the user pressing cancel.
    let stop = cancel;
    let failure: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    // Tasks, not connections: the semaphore above decides how many of them are
    // talking to a server at any moment, so a second download gets a turn.
    let workers = (0..SLOTS.min(queue.lock().map(|queue| queue.len()).unwrap_or(1)).max(1)).map(|_| {
        let (http, url, file, queue, written, stop, failure, manifest) = (
            http.clone(),
            url.to_string(),
            file.clone(),
            queue.clone(),
            written.clone(),
            stop.clone(),
            failure.clone(),
            manifest.clone(),
        );
        async move {
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                let Some((start, end, have)) = ({ let mut queue = queue.lock().unwrap(); queue.pop_front() }) else { return };
                if let Err(error) = piece(&http, &url, &file, start, end, have, &written, &stop, &manifest).await {
                    stop.store(true, Ordering::Relaxed);
                    *failure.lock().unwrap() = Some(error.to_string());
                    return;
                }
            }
        }
    });
    futures_util::future::join_all(workers).await;

    if let Some(error) = failure.lock().unwrap().take() {
        // The part file and its manifest stay: what did arrive is on the disk,
        // and the next attempt fetches only what did not.
        bail!(error);
    }
    file.sync_all().ok();
    std::fs::remove_file(manifest_path(part)).ok();
    Ok(())
}

/// One piece from the byte it has come to, with the retries that make a long download survive a bad line: what an
/// attempt brought stays on the disk and the next one asks for the rest.
async fn piece(
    http: &reqwest::Client,
    url: &str,
    file: &Arc<File>,
    start: u64,
    end: u64,
    mut have: u64,
    written: &Arc<AtomicU64>,
    stop: &Arc<AtomicBool>,
    manifest: &Arc<Mutex<File>>,
) -> Result<()> {
    let wanted = end - start + 1;
    let mut empty = 0u32;
    while have < wanted {
        if stop.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let mut got = 0u64;
        let outcome = once(http, url, file, start, end, have, written, &mut got, stop, manifest).await;
        have += got;
        let last = match outcome {
            Ok(()) if have == wanted => break,
            Ok(()) => format!("incomplete range: {have} of {wanted} bytes"),
            Err(error) => {
                if stop.load(Ordering::Relaxed) {
                    bail!("cancelled");
                }
                error.to_string()
            }
        };
        empty = if got > 0 { 0 } else { empty + 1 };
        if empty >= RETRIES {
            return Err(anyhow!("range {}-{end} failed after {RETRIES} attempts: {last}", start + have));
        }
        // Exponential, as the Hub's own download protocol asks for: half a
        // second, then one, two, four - eight attempts spanning a minute
        // instead of the three seconds a linear back-off gave a network that
        // needs a moment to come back.
        tokio::time::sleep(std::time::Duration::from_millis(500 << empty.min(7))).await;
    }
    note(file, manifest, start, wanted);
    Ok(())
}

async fn once(
    http: &reqwest::Client,
    url: &str,
    file: &Arc<File>,
    start: u64,
    end: u64,
    have: u64,
    written: &Arc<AtomicU64>,
    got: &mut u64,
    stop: &Arc<AtomicBool>,
    manifest: &Arc<Mutex<File>>,
) -> Result<()> {
    let from = start + have;
    let response = send(http, url, Some(format!("bytes={from}-{end}"))).await?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        bail!("range {from}-{end}: answered {} where 206 was expected", response.status());
    }
    let mut stream = response.bytes_stream();
    let mut offset = from;
    let mut unnoted = 0u64;
    while let Some(chunk) = stream.next().await {
        // Checked here and not only between pieces: a piece is a quarter of
        // the file, which is minutes of downloading after the user has pressed
        // stop, and that reads as a button that did nothing.
        if stop.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let chunk = chunk.context("read range")?;
        if offset + chunk.len() as u64 > end + 1 {
            bail!("range {from}-{end}: the server sent more than was asked for");
        }
        let mut placed = 0usize;
        while placed < chunk.len() {
            let count = write_at(file, &chunk[placed..], offset + placed as u64).context("write range")?;
            if count == 0 {
                bail!("short write at {}", offset + placed as u64);
            }
            placed += count;
        }
        offset += chunk.len() as u64;
        *got += chunk.len() as u64;
        written.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        unnoted += chunk.len() as u64;
        if unnoted >= CHECKPOINT {
            note(file, manifest, start, have + *got);
            unnoted = 0;
        }
    }
    Ok(())
}

/// For a server that will not serve ranges: the body, in order, from the start.
async fn whole(http: &reqwest::Client, url: &str, part: &Path, written: Arc<AtomicU64>, cancel: Arc<AtomicBool>) -> Result<()> {
    let response = send(http, url, None).await?;
    if !response.status().is_success() {
        bail!("{url} answered {}", response.status());
    }
    let mut file = File::create(part).with_context(|| format!("create {}", part.display()))?;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled");
        }
        let chunk = chunk.context("read download chunk")?;
        file.write_all(&chunk).context("write download chunk")?;
        written.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    file.flush().context("flush download")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_gives_how_far_each_piece_of_its_file_has_come() {
        let directory = std::env::temp_dir().join(format!("yue2-chunked-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let part = directory.join("model.gguf.part");
        let total = PIECE_MIN * 8;
        let pieces = pieces_of(total);
        assert_eq!(pieces.len(), SLOTS, "a file is one piece a connection, whatever its size");
        assert_eq!(pieces[0], (0, PIECE_MIN * 2 - 1));
        assert_eq!(pieces.last().unwrap().1, total - 1);
        let second = pieces[1].0;
        let notes: Vec<u8> = [entry(WHOLE, total), entry(0, CHECKPOINT), entry(0, CHECKPOINT * 3), entry(second, CHECKPOINT), entry(7, CHECKPOINT), entry(second, PIECE_MIN * 9)].concat();
        std::fs::write(manifest_path(&part), &notes).unwrap();
        let progress = progress_of(&part, total);
        assert_eq!(progress.get(&0), Some(&(CHECKPOINT * 3)), "the furthest note of a piece");
        assert_eq!(progress.get(&second), Some(&CHECKPOINT), "a note longer than its piece is not believed");
        assert_eq!(progress.len(), 2, "a note of a byte no piece starts at is not believed");
        assert!(progress_of(&part, total + 1).is_empty(), "a manifest holds for the file size it was written for");
        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn a_small_file_is_one_piece_and_pieces_cover_the_file_without_gaps() {
        assert_eq!(pieces_of(0), vec![]);
        assert_eq!(pieces_of(10), vec![(0, 9)]);
        assert_eq!(pieces_of(PIECE_MIN + 1).len(), 2);
        for total in [1, PIECE_MIN - 1, PIECE_MIN, PIECE_MIN + 1, PIECE_MIN * 3 + 5, PIECE_MIN * 61 + 7] {
            let pieces = pieces_of(total);
            assert!(pieces.len() <= SLOTS);
            assert_eq!(pieces[0].0, 0);
            assert_eq!(pieces.last().unwrap().1, total - 1);
            assert!(pieces.windows(2).all(|pair| pair[0].1 + 1 == pair[1].0), "{total}: {pieces:?}");
        }
    }

    /// The header as Hugging Face actually sends it, copied from a live
    /// response rather than from the specification: what matters is what comes
    /// down the wire.
    #[test]
    fn the_rate_limit_header_is_read_the_way_the_hub_writes_it() {
        let header = "\"resolvers\";r=2819;t=216";
        let field = |name: &str| {
            header
                .split(';')
                .filter_map(|part| part.trim().split_once('='))
                .find(|(key, _)| key.trim() == name)
                .and_then(|(_, value)| value.trim().parse::<u64>().ok())
        };
        assert_eq!(field("r"), Some(2819), "requests left in this five minute window");
        assert_eq!(field("t"), Some(216), "seconds until the window resets");
    }

    #[test]
    fn a_missing_manifest_means_nothing_is_done_yet() {
        assert!(progress_of(Path::new("nowhere/at/all.part"), 10).is_empty());
    }

    #[test]
    fn a_part_file_holds_what_its_manifest_says_of_its_pieces() {
        let folder = std::env::temp_dir().join(format!("on-disk-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&folder).unwrap();
        let part = folder.join("model.gguf.part");
        let total = PIECE_MIN * 2 + 10;
        std::fs::File::create(&part).unwrap().set_len(total).unwrap();
        assert_eq!(on_disk(&part), total, "no manifest: a download without ranges");
        std::fs::write(earlier_manifest_path(&part), 0u64.to_le_bytes()).unwrap();
        assert_eq!(on_disk(&part), 0, "the pieces of an earlier version start over");
        let last = pieces_of(total).last().unwrap().0;
        std::fs::write(manifest_path(&part), [entry(WHOLE, total), entry(0, CHECKPOINT), entry(last, 10)].concat()).unwrap();
        assert_eq!(on_disk(&part), CHECKPOINT + 10);
        assert_eq!(on_disk(&folder.join("nothing.part")), 0);
        std::fs::remove_dir_all(&folder).ok();
    }

    /// A server of one file by ranges; with `cut`, its first answer stops after that many bytes. Gives its port and
    /// the ranges it was asked for.
    async fn range_server(body: Vec<u8>, cut: Option<usize>) -> (u16, Arc<Mutex<Vec<(u64, u64)>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let asked: Arc<Mutex<Vec<(u64, u64)>>> = Arc::new(Mutex::new(Vec::new()));
        let seen = asked.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let Ok(count) = socket.read(&mut buffer).await else { break };
                    if count == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..count]);
                }
                let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                let range = text.lines().find_map(|line| line.strip_prefix("range: bytes=")).unwrap_or("0-0").trim().to_string();
                let (from, to) = range.split_once('-').unwrap();
                let (from, to): (u64, u64) = (from.parse().unwrap(), to.parse().unwrap());
                let first = { let mut seen = seen.lock().unwrap(); seen.push((from, to)); seen.len() == 1 };
                let head = format!("HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {from}-{to}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len(), to - from + 1);
                socket.write_all(head.as_bytes()).await.ok();
                let slice = &body[from as usize..=to as usize];
                socket.write_all(match cut { Some(cut) if first => &slice[..cut], _ => slice }).await.ok();
                socket.shutdown().await.ok();
            }
        });
        (port, asked)
    }

    /// A server that drops the connection halfway and answers the rest on the next request: the piece is asked for
    /// from the byte it has come to, and the file is whole.
    #[tokio::test]
    async fn a_dropped_connection_asks_only_for_the_rest_of_its_piece() {
        let body: Vec<u8> = (0..200_000u32).map(|index| (index % 251) as u8).collect();
        let (port, asked) = range_server(body.clone(), Some(70_000)).await;
        let folder = std::env::temp_dir().join(format!("resume-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&folder).unwrap();
        let part = folder.join("model.gguf.part");
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let written = Arc::new(AtomicU64::new(0));
        fetch(&http, &format!("http://127.0.0.1:{port}/model.gguf"), &part, Plan { total: body.len() as u64, ranged: true }, written.clone(), Arc::new(AtomicBool::new(false))).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), body);
        assert_eq!(written.load(Ordering::Relaxed), body.len() as u64, "the bar counts every byte once");
        assert_eq!(*asked.lock().unwrap(), vec![(0, 199_999), (70_000, 199_999)]);
        assert!(!manifest_path(&part).exists());
        std::fs::remove_dir_all(&folder).ok();
    }

    #[tokio::test]
    async fn a_manifest_without_its_part_file_is_not_believed() {
        let body: Vec<u8> = (0..100_000u32).map(|index| (index % 241) as u8).collect();
        let total = body.len() as u64;
        let (port, asked) = range_server(body.clone(), None).await;
        let folder = std::env::temp_dir().join(format!("stale-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&folder).unwrap();
        let part = folder.join("model.gguf.part");
        std::fs::write(manifest_path(&part), [entry(WHOLE, total), entry(0, total)].concat()).unwrap();
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let written = Arc::new(AtomicU64::new(0));
        fetch(&http, &format!("http://127.0.0.1:{port}/model.gguf"), &part, Plan { total, ranged: true }, written.clone(), Arc::new(AtomicBool::new(false))).await.unwrap();
        assert_eq!(std::fs::read(&part).unwrap(), body, "the bytes the manifest promised were not on the disk");
        assert_eq!(*asked.lock().unwrap(), vec![(0, total - 1)]);
        assert_eq!(written.load(Ordering::Relaxed), total);
        std::fs::remove_dir_all(&folder).ok();
    }

    /// A host nothing listens on is given up after three attempts, not eight,
    /// and the reason names it beside the address.
    #[tokio::test]
    async fn a_host_that_cannot_be_reached_is_named_and_not_retried_eight_times() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let http = reqwest::Client::builder().no_proxy().build().unwrap();
        let error = probe(&http, &format!("http://127.0.0.1:{port}/model.gguf")).await.unwrap_err().to_string();
        assert!(error.starts_with(&format!("could not connect to 127.0.0.1 (3 attempts) for http://127.0.0.1:{port}/model.gguf: ")), "{error}");
        assert!(!error.contains("HF-Mirror"));
        assert!(start_failure("https://huggingface.co/a/b/resolve/c/d.gguf", 3, true, "x", None).starts_with("could not connect to huggingface.co (3 attempts)"));
        assert!(start_failure("https://huggingface.co/a/b/resolve/c/d.gguf", 3, true, "x", None).ends_with("HF-Mirror instead"));
        assert_eq!(start_failure("https://github.com/a/b.zip", 8, false, "x", None), "could not start https://github.com/a/b.zip from github.com after 8 attempts: x");
    }

    #[test]
    fn a_proxy_that_does_not_connect_is_named_and_the_mirror_is_not_offered() {
        let system = crate::net::ProxyRoute { address: "http://127.0.0.1:10809".into(), system: true };
        let through = start_failure("https://huggingface.co/a/b/resolve/c/d.gguf", 3, true, "tunnel error", Some(&system));
        assert!(through.contains(". The system proxy http://127.0.0.1:10809 did not answer"), "{through}");
        assert!(through.contains("choose No proxy in Settings - Providers"), "{through}");
        assert!(!through.contains("HF-Mirror"), "the mirror sits behind the same proxy: {through}");
        let own = crate::net::ProxyRoute { address: "socks5h://proxy.example:1080".into(), system: false };
        assert!(start_failure("https://github.com/a/b.zip", 3, true, "x", Some(&own)).contains("The proxy set in Settings - Providers socks5h://proxy.example:1080"));
        let answered = start_failure("https://huggingface.co/a/b/resolve/c/d.gguf", 8, false, "x", Some(&system));
        assert!(answered.ends_with("HF-Mirror instead") && !answered.contains("proxy"), "a server that answered through the proxy is not the proxy's fault: {answered}");
    }
}
