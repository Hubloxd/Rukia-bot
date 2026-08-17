use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client,
};
use songbird::input::{
    metadata::YoutubeDlOutput,
    AudioStream, AudioStreamError, Compose, HlsRequest, HttpRequest, Input,
};
use std::{borrow::Cow, io::ErrorKind, process::Stdio, sync::Arc};
use tokio::process::Command;

use super::error::YoutubeError;

const YT_DLP: &str = "yt-dlp";
/// Formaty z bezpośrednim URL (HTTPS), które symphonia/songbird obsługują po włączeniu feature'ów.
const FORMAT: &str =
    "bestaudio[ext=webm][acodec=opus]/bestaudio[ext=webm]/bestaudio[ext=m4a]/bestaudio";
/// `tv`/`ios`/`web` są teraz DRM, PO-token albo SABR. `web_embedded` nie wymaga PO tokenu
/// (tylko filmy z osadzaniem); `tv_downgraded` jako zapas.
const PLAYER_CLIENT: &str = "youtube:player_client=web_embedded,tv_downgraded";

fn apply_ytdl_flags(cmd: &mut Command) -> &mut Command {
    cmd.args([
        "--no-playlist",
        "-f",
        FORMAT,
        "-S",
        "proto:https,ext:webm:opus",
        "--extractor-args",
        PLAYER_CLIENT,
        // Deno jest włączony domyślnie; Node trzeba dodać (m.in. WSL / brew).
        "--js-runtimes",
        "node",
    ])
}

#[derive(Clone, Debug)]
pub struct YtdlSource {
    client: Client,
    query: Cow<'static, str>,
    is_search: bool,
}

impl YtdlSource {
    pub fn url(client: Client, url: String) -> Self {
        Self {
            client,
            query: Cow::Owned(url),
            is_search: false,
        }
    }

    pub fn search(client: Client, query: String) -> Self {
        Self {
            client,
            query: Cow::Owned(query),
            is_search: true,
        }
    }

    fn ytdl_query(&self) -> Cow<'static, str> {
        if self.is_search {
            Cow::Owned(format!("ytsearch1:{}", self.query))
        } else {
            self.query.clone()
        }
    }

    pub async fn fetch_output(&self) -> Result<YoutubeDlOutput, YoutubeError> {
        let outputs = self.run_ytdl_json(1).await?;
        outputs
            .into_iter()
            .next()
            .ok_or_else(|| YoutubeError::ResolveFailed("Brak wyników yt-dlp.".into()))
    }

    async fn run_ytdl_json(&self, n: usize) -> Result<Vec<YoutubeDlOutput>, YoutubeError> {
        let query = self.ytdl_query();
        let mut cmd = Command::new(YT_DLP);
        apply_ytdl_flags(cmd.arg("-j"))
            .arg(query.as_ref())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = cmd.output().await.map_err(ytdl_spawn_error)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(%stderr, "yt-dlp -j nieudany");
            return Err(YoutubeError::ResolveFailed(ytdl_error_message(&stderr)));
        }

        output
            .stdout
            .split(|&b| b == b'\n')
            .filter(|line| !line.is_empty())
            .take(n)
            .map(serde_json::from_slice)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| YoutubeError::ResolveFailed(e.to_string()))
    }

    pub async fn download(&self) -> Result<Arc<[u8]>, YoutubeError> {
        let mut last_err = None;
        for attempt in 0..3u8 {
            match self.download_via_ytdl_stdout().await {
                Ok(bytes) => {
                    tracing::info!(bytes = bytes.len(), "Pobrano audio przez yt-dlp");
                    return Ok(bytes);
                }
                Err(e) if is_retryable_download(&e.to_string()) && attempt < 2 => {
                    let delay_ms = if attempt == 0 { 500 } else { 1500 };
                    tracing::warn!(
                        attempt = attempt + 1,
                        delay_ms,
                        error = %e,
                        "Ponawiam pobieranie audio"
                    );
                    last_err = Some(e);
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.expect("retry loop"))
    }

    async fn download_via_ytdl_stdout(&self) -> Result<Arc<[u8]>, YoutubeError> {
        let query = self.ytdl_query();
        let mut cmd = Command::new(YT_DLP);
        apply_ytdl_flags(&mut cmd)
            .args(["-o", "-"])
            .arg(query.as_ref())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = cmd.output().await.map_err(ytdl_spawn_error)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::warn!(%stderr, "yt-dlp -o - nieudany");
            return Err(YoutubeError::ResolveFailed(ytdl_error_message(&stderr)));
        }
        if output.stdout.is_empty() {
            return Err(YoutubeError::ResolveFailed(
                "yt-dlp nie zwrócił danych audio.".into(),
            ));
        }

        Ok(Arc::from(output.stdout))
    }

    async fn open_stream(
        &self,
        result: &YoutubeDlOutput,
    ) -> Result<AudioStream<Box<dyn symphonia::core::io::MediaSource>>, AudioStreamError> {
        let mut headers = HeaderMap::default();
        if let Some(map) = &result.http_headers {
            headers.extend(map.iter().filter_map(|(k, v)| {
                Some((
                    HeaderName::from_bytes(k.as_bytes()).ok()?,
                    HeaderValue::from_str(v).ok()?,
                ))
            }));
        }

        match result.protocol.as_deref() {
            Some("m3u8_native") | Some("m3u8") => {
                let mut req = HlsRequest::new_with_headers(
                    self.client.clone(),
                    result.url.clone(),
                    headers,
                );
                req.create()
            }
            _ => {
                let mut req = HttpRequest {
                    client: self.client.clone(),
                    request: result.url.clone(),
                    headers,
                    content_length: None,
                };
                req.create_async().await
            }
        }
    }
}

#[async_trait::async_trait]
impl Compose for YtdlSource {
    fn create(&mut self) -> Result<AudioStream<Box<dyn symphonia::core::io::MediaSource>>, AudioStreamError> {
        Err(AudioStreamError::Unsupported)
    }

    async fn create_async(
        &mut self,
    ) -> Result<AudioStream<Box<dyn symphonia::core::io::MediaSource>>, AudioStreamError> {
        let mut last_err = None;
        for attempt in 0..3u8 {
            let output = self
                .fetch_output()
                .await
                .map_err(|e| AudioStreamError::Fail(e.to_string().into()))?;
            match self.open_stream(&output).await {
                Ok(stream) => return Ok(stream),
                Err(e) if is_http_forbidden(&e) && attempt < 2 => {
                    let delay_ms = if attempt == 0 { 500 } else { 1500 };
                    tracing::warn!(
                        attempt = attempt + 1,
                        delay_ms,
                        "YouTube 403 przy otwieraniu streamu, ponawiam yt-dlp"
                    );
                    last_err = Some(e);
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.expect("retry loop"))
    }

    fn should_create_async(&self) -> bool {
        true
    }

    async fn aux_metadata(&mut self) -> Result<songbird::input::AuxMetadata, AudioStreamError> {
        let output = self
            .fetch_output()
            .await
            .map_err(|e| AudioStreamError::Fail(e.to_string().into()))?;
        Ok(output.as_aux_metadata())
    }
}

impl From<YtdlSource> for Input {
    fn from(val: YtdlSource) -> Input {
        Input::Lazy(Box::new(val))
    }
}

fn ytdl_spawn_error(e: std::io::Error) -> YoutubeError {
    if e.kind() == ErrorKind::NotFound {
        YoutubeError::YtdlNotFound
    } else {
        YoutubeError::ResolveFailed(e.to_string())
    }
}

fn ytdl_error_message(stderr: &str) -> String {
    let error_line = stderr.lines().rev().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("ERROR:")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| trimmed.starts_with("ERROR:").then_some(trimmed))
    });

    match error_line {
        Some(msg) if msg.contains("DRM protected") => {
            "YouTube zablokował ten strumień (eksperyment DRM / brak formatu). Spróbuj ponownie za chwilę.".into()
        }
        Some(msg) if msg.contains("Requested format is not available") => {
            "YouTube nie oddał strumienia audio. Spróbuj ponownie.".into()
        }
        Some(msg) if msg.contains("403") => "YouTube zwrócił 403 Forbidden.".into(),
        Some(msg) => {
            let short = msg.rsplit("ERROR:").next().unwrap_or(msg).trim();
            short.chars().take(280).collect()
        }
        None => "yt-dlp zakończył się błędem.".into(),
    }
}

fn is_http_forbidden(err: &AudioStreamError) -> bool {
    is_forbidden_msg(&err.to_string())
}

fn is_forbidden_msg(msg: &str) -> bool {
    msg.contains("403")
}

fn is_retryable_download(msg: &str) -> bool {
    is_forbidden_msg(msg)
        || msg.contains("Requested format is not available")
        || msg.contains("DRM protected")
        || msg.contains("nie oddał strumienia")
}
