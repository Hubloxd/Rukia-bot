use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client,
};
use songbird::input::{
    metadata::YoutubeDlOutput,
    AudioStream, AudioStreamError, Compose, HlsRequest, HttpRequest, Input,
};
use std::{borrow::Cow, io::ErrorKind, process::Stdio};
use tokio::process::Command;

use super::error::YoutubeError;

const YT_DLP: &str = "yt-dlp";
/// Formaty z bezpośrednim URL (HTTPS), które symphonia/songbird obsługują po włączeniu feature'ów.
const FORMAT: &str =
    "bestaudio[ext=webm][acodec=opus]/bestaudio[ext=webm]/bestaudio[ext=m4a]/bestaudio";

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
        let output = Command::new(YT_DLP)
            .args([
                "-j",
                "--no-playlist",
                "-f",
                FORMAT,
                "-S",
                "proto:https,ext:webm:opus",
            ])
            .arg(query.as_ref())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                if e.kind() == ErrorKind::NotFound {
                    YoutubeError::YtdlNotFound
                } else {
                    YoutubeError::ResolveFailed(e.to_string())
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(YoutubeError::ResolveFailed(stderr.trim().to_string()));
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
                    content_length: result.filesize,
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
        let output = self
            .fetch_output()
            .await
            .map_err(|e| AudioStreamError::Fail(e.to_string().into()))?;
        self.open_stream(&output).await
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
