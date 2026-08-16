use reqwest::Client;
use songbird::input::Input;

use super::error::YoutubeError;
use super::ytdl::YtdlSource;

#[derive(Debug, Clone)]
pub struct YoutubeQuery {
    text: String,
    is_url: bool,
}

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration: Option<u64>,
}

pub fn is_youtube_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("youtu.be/")
        || lower.contains("youtube.com/")
        || lower.contains("music.youtube.com/")
}

impl YoutubeQuery {
    pub fn parse(input: &str) -> Result<Self, YoutubeError> {
        let text = input.trim();
        if text.is_empty() {
            return Err(YoutubeError::InvalidInput);
        }

        let is_url = text.starts_with("http://") || text.starts_with("https://");

        Ok(Self {
            text: text.to_string(),
            is_url,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_url(&self) -> bool {
        self.is_url
    }

    fn build_source(&self, client: Client) -> YtdlSource {
        if self.is_url {
            YtdlSource::url(client, self.text.clone())
        } else {
            YtdlSource::search(client, self.text.clone())
        }
    }

    pub async fn resolve_metadata(
        &self,
        client: &Client,
    ) -> Result<TrackInfo, YoutubeError> {
        let source = self.build_source(client.clone());
        let output = source.fetch_output().await?;
        let meta = output.as_aux_metadata();

        Ok(TrackInfo {
            title: meta.track.or(meta.title),
            artist: meta.artist,
            duration: meta.duration.map(|d| d.as_secs()),
        })
    }

    pub fn into_input(self, client: Client) -> Input {
        self.build_source(client).into()
    }

    /// Cache the stream in memory so `!seek` / `!loop` don't re-hit YouTube
    /// (googlevideo URLs expire and return 403 on recreate).
    pub async fn into_seekable_input(self, client: Client) -> Result<Input, YoutubeError> {
        let input = self.into_input(client);
        let cached = songbird::input::cached::Memory::new(input)
            .await
            .map_err(|e| YoutubeError::ResolveFailed(e.to_string()))?;
        Ok(cached.into())
    }
}
