use songbird::input::AudioStreamError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum YoutubeError {
    #[error("Podaj URL lub frazę wyszukiwania.")]
    InvalidInput,

    #[error("Nie znaleziono programu yt-dlp w PATH. Zainstaluj: brew install yt-dlp")]
    YtdlNotFound,

    #[error("Nie udało się pobrać utworu: {0}")]
    ResolveFailed(String),
}

impl From<AudioStreamError> for YoutubeError {
    fn from(err: AudioStreamError) -> Self {
        let msg = err.to_string();
        if msg.contains("could not find executable") {
            return YoutubeError::YtdlNotFound;
        }
        YoutubeError::ResolveFailed(msg)
    }
}
