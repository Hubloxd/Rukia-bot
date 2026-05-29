use std::time::Duration;

/// Parsuje `sekundy`, `mm:ss` lub `hh:mm:ss`.
pub fn parse_seek_position(input: &str) -> Result<Duration, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err(
            "Podaj czas, np. `45`, `1:30` (min:sek) lub `1:05:20` (godz:min:sek).".to_string(),
        );
    }

    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        1 => {
            let secs: u64 = parts[0]
                .parse()
                .map_err(|_| parse_err())?;
            Ok(Duration::from_secs(secs))
        }
        2 => {
            let mins: u64 = parts[0].parse().map_err(|_| parse_err())?;
            let secs: u64 = parts[1].parse().map_err(|_| parse_err())?;
            if secs >= 60 {
                return Err("Sekundy w formacie mm:ss muszą być mniejsze niż 60.".to_string());
            }
            Ok(Duration::from_secs(mins * 60 + secs))
        }
        3 => {
            let hours: u64 = parts[0].parse().map_err(|_| parse_err())?;
            let mins: u64 = parts[1].parse().map_err(|_| parse_err())?;
            let secs: u64 = parts[2].parse().map_err(|_| parse_err())?;
            if mins >= 60 || secs >= 60 {
                return Err(
                    "Minuty i sekundy w formacie hh:mm:ss muszą być mniejsze niż 60.".to_string(),
                );
            }
            Ok(Duration::from_secs(hours * 3600 + mins * 60 + secs))
        }
        _ => Err(parse_err()),
    }
}

pub fn format_timestamp(d: Duration) -> String {
    let total = d.as_secs();
    let hours = total / 3600;
    let mins = (total % 3600) / 60;
    let secs = total % 60;

    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

fn parse_err() -> String {
    "Nieprawidłowy format czasu. Użyj `45`, `1:30` lub `1:05:20`.".to_string()
}

/// Komunikat dla błędów tracku związanych z seekiem poza koniec strumienia.
pub fn is_seek_past_end(err: &impl std::fmt::Display) -> bool {
    err.to_string().contains("end of stream")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_seconds() {
        assert_eq!(parse_seek_position("90").unwrap(), Duration::from_secs(90));
    }

    #[test]
    fn parses_mm_ss() {
        assert_eq!(
            parse_seek_position("1:30").unwrap(),
            Duration::from_secs(90)
        );
    }

    #[test]
    fn parses_hh_mm_ss() {
        assert_eq!(
            parse_seek_position("1:05:20").unwrap(),
            Duration::from_secs(3920)
        );
    }
}
