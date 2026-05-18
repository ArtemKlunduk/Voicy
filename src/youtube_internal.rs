//! YouTube Internal API (youtubei/v1) клиент.
//!
//! Использует официальные POST-эндпоинты, которые сам YouTube вызывает
//! при загрузке страниц. Не требует API key, возвращает стабильный JSON.
//!
//! Endpoints:
//!   - /youtubei/v1/search    → результаты поиска (videoRenderer)
//!   - /youtubei/v1/next      → related videos (lockupViewModel) + channelId

use anyhow::{anyhow, Context, Result};
use serde_json::json;
use std::time::Duration;

const YT_API_BASE: &str = "https://www.youtube.com/youtubei/v1";
const CLIENT_VERSION: &str = "2.20241114.01.00";
const YT_INTERNAL_KEY: &str = "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8";

/// Один результат: videoId + title.
#[derive(Debug, Clone)]
pub struct YtInternalResult {
    pub video_id: String,
    pub title: String,
}

/// Расширенный результат поиска с метаданными.
#[derive(Debug, Clone)]
pub struct YtSearchResult {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub thumbnail: String,
    pub duration: String,
    /// Тип контента из YouTube Music: "Song", "Video", "Album", "Episode", "Playlist", "Artist" и т.д.
    pub music_type: String,
    /// YouTube Music video type: "MUSIC_VIDEO_TYPE_ATV", "MUSIC_VIDEO_TYPE_UGC", "MUSIC_VIDEO_TYPE_OFFICIAL_SOURCE_MUSIC" и т.д.
    pub music_video_type: String,
}

/// Поиск видео по запросу через youtubei/v1/search.
pub fn search_videos(query: &str, max_results: u8) -> Result<Vec<YtInternalResult>> {
    let body = json!({
        "context": {
            "client": {
                "clientName": "WEB",
                "clientVersion": CLIENT_VERSION,
                "hl": "en",
                "gl": "US"
            }
        },
        "query": query
    });

    let url = format!("{}/search?key={}&prettyPrint=false", YT_API_BASE, YT_INTERNAL_KEY);
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(Duration::from_secs(10))
        .send_json(&body)
        .context("youtubei search")?;

    let body_str = resp.into_string().context("read search response")?;
    let value: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| anyhow!("parse search json: {} — body: {}", e, &body_str[..body_str.len().min(200)]))?;

    let sections = value
        .pointer("/contents/twoColumnSearchResultsRenderer/primaryContents/sectionListRenderer/contents")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow!("no search sections"))?;

    let mut out = Vec::new();
    for sec in sections {
        let Some(items) = sec.pointer("/itemSectionRenderer/contents").and_then(|c| c.as_array()) else { continue };
        for it in items {
            let Some(vr) = it.get("videoRenderer") else { continue };
            let Some(vid) = vr.get("videoId").and_then(|v| v.as_str()) else { continue };
            let title = vr.pointer("/title/runs/0/text")
                .or_else(|| vr.pointer("/title/simpleText"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            out.push(YtInternalResult { video_id: vid.to_string(), title });
            if out.len() >= max_results as usize { break; }
        }
        if out.len() >= max_results as usize { break; }
    }
    Ok(out)
}

/// Расширенный поиск через WEB (обычный YouTube): возвращает videoId + title + channel + thumbnail + duration.
pub fn search_results(query: &str, max_results: u8) -> Result<Vec<YtSearchResult>> {
    let body = json!({
        "context": {
            "client": {
                "clientName": "WEB",
                "clientVersion": CLIENT_VERSION,
                "hl": "en",
                "gl": "US"
            }
        },
        "query": query
    });

    let url = format!("{}/search?key={}&prettyPrint=false", YT_API_BASE, YT_INTERNAL_KEY);
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(Duration::from_secs(10))
        .send_json(&body)
        .context("youtubei search results")?;

    let body_str = resp.into_string().context("read search response")?;
    let value: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| anyhow!("parse search json: {} — body: {}", e, &body_str[..body_str.len().min(200)]))?;

    let sections = value
        .pointer("/contents/twoColumnSearchResultsRenderer/primaryContents/sectionListRenderer/contents")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow!("no search sections"))?;

    let mut out = Vec::new();
    for sec in sections {
        let Some(items) = sec.pointer("/itemSectionRenderer/contents").and_then(|c| c.as_array()) else { continue };
        for it in items {
            let Some(vr) = it.get("videoRenderer") else { continue };
            let Some(vid) = vr.get("videoId").and_then(|v| v.as_str()) else { continue };
            let title = vr.pointer("/title/runs/0/text")
                .or_else(|| vr.pointer("/title/simpleText"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let channel = vr.pointer("/ownerText/runs/0/text")
                .or_else(|| vr.pointer("/longBylineText/runs/0/text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let duration = vr.pointer("/lengthText/simpleText")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            let thumbnail = vr.pointer("/thumbnail/thumbnails/0/url")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            // Если thumbnail не найден — fallback на стандартный URL
            let thumbnail = if thumbnail.is_empty() {
                format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", vid)
            } else {
                thumbnail
            };
            out.push(YtSearchResult {
                video_id: vid.to_string(),
                title,
                channel,
                thumbnail,
                duration,
                music_type: String::new(),
                music_video_type: String::new(),
            });
            if out.len() >= max_results as usize { break; }
        }
        if out.len() >= max_results as usize { break; }
    }
    Ok(out)
}

/// Извлекает категорию (Song/Video/Album/Episode/Playlist/...) и реального артиста
/// из flexColumns[1] musicResponsiveListItemRenderer.
fn extract_music_item_meta(mr: &serde_json::Value) -> (String, String) {
    let runs = mr.pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();

    // Первая непустая run — это категория (Video, Song, Album, Episode, ...)
    let category = runs.iter()
        .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
        .find(|t| !t.trim().is_empty() && *t != " • " && *t != " & ")
        .unwrap_or("")
        .to_string();

    // Артисты — runs с navigationEndpoint.browseEndpoint, предпочитаем MUSIC_PAGE_TYPE_ARTIST
    let mut artists = Vec::new();
    for run in &runs {
        let text = run.get("text").and_then(|t| t.as_str()).unwrap_or("");
        if text.trim().is_empty() || text == " • " || text == " & " {
            continue;
        }
        let page_type = run.pointer("/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType")
            .and_then(|p| p.as_str());
        if page_type == Some("MUSIC_PAGE_TYPE_ARTIST") || page_type == Some("MUSIC_PAGE_TYPE_USER_CHANNEL") {
            artists.push(text.to_string());
        }
    }

    let artist = if artists.is_empty() {
        // fallback: берём всё после категории и разделителей
        runs.iter()
            .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
            .skip_while(|t| *t == category.as_str() || t.trim().is_empty() || *t == " • " || *t == " & ")
            .next()
            .unwrap_or("")
            .to_string()
    } else {
        artists.join(" & ")
    };

    (category, artist)
}

/// Поиск музыки через YouTube Music (WEB_REMIX клиент).
/// Парсит musicResponsiveListItemRenderer — результаты гораздо точнее для песен.
pub fn search_music_remix(query: &str, max_results: u8) -> Result<Vec<YtSearchResult>> {
    let body = json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": "1.20241118.01.00",
                "hl": "en",
                "gl": "US"
            }
        },
        "query": query
    });

    let url = format!("{}/search?key={}&prettyPrint=false", YT_API_BASE, YT_INTERNAL_KEY);
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(Duration::from_secs(10))
        .send_json(&body)
        .context("youtubei music search (WEB_REMIX)")?;

    let body_str = resp.into_string().context("read music search response")?;
    let value: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| anyhow!("parse music search json: {} — body: {}", e, &body_str[..body_str.len().min(200)]))?;

    let sections = value
        .pointer("/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents")
        .and_then(|c| c.as_array())
        .or_else(|| {
            // fallback на старый формат (на всякий случай)
            value.pointer("/contents/sectionListRenderer/contents").and_then(|c| c.as_array())
        })
        .ok_or_else(|| anyhow!("no music search sections"))?;

    let mut out = Vec::new();
    for sec in sections {
        let _shelf_title = sec.pointer("/musicShelfRenderer/title/runs/0/text")
            .and_then(|t| t.as_str()).unwrap_or("");
        let Some(items) = sec.pointer("/musicShelfRenderer/contents").and_then(|c| c.as_array()) else { continue };
        for it in items {
            let Some(mr) = it.get("musicResponsiveListItemRenderer") else { continue };

            // Title — flexColumns[0]
            let title = mr.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();

            // Категория и артист из flexColumns[1]
            let (category, artist) = extract_music_item_meta(mr);

            // Thumbnail
            let thumbnail = mr.pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails/0/url")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();

            // Duration — fixedColumns[0] (часто null в WEB_REMIX)
            let duration = mr.pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();

            // videoId из overlay play button или playlistItemData
            let video_id = mr
                .pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    mr.pointer("/playlistItemData/videoId").and_then(|v| v.as_str())
                })
                .unwrap_or("")
                .to_string();

            // musicVideoType из watchEndpoint внутри title-run (если есть)
            let mv_type = mr.pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if video_id.is_empty() || title.is_empty() {
                continue;
            }

            // Фильтруем явно не-треки: подкасты, плейлисты, профили, артист-карточки
            let skip_types = ["Episode", "Podcast", "Playlist", "Profile", "Artist"];
            if skip_types.contains(&category.as_str()) {
                continue;
            }

            let thumbnail = if thumbnail.is_empty() {
                format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", video_id)
            } else {
                thumbnail
            };

            out.push(YtSearchResult {
                video_id,
                title,
                channel: artist,
                thumbnail,
                duration,
                music_type: category,
                music_video_type: mv_type.to_string(),
            });
            if out.len() >= max_results as usize { break; }
        }
        if out.len() >= max_results as usize { break; }
    }
    Ok(out)
}

/// Получить related videos для данного videoId через youtubei/v1/next.
pub fn get_related_videos(video_id: &str, max_results: u8) -> Result<Vec<YtInternalResult>> {
    let body = json!({
        "context": {
            "client": {
                "clientName": "WEB",
                "clientVersion": CLIENT_VERSION,
                "hl": "en",
                "gl": "US"
            }
        },
        "videoId": video_id
    });

    let url = format!("{}/next?key={}&prettyPrint=false", YT_API_BASE, YT_INTERNAL_KEY);
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(Duration::from_secs(10))
        .send_json(&body)
        .context("youtubei next")?;

    let body_str = resp.into_string().context("read next response")?;
    let value: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| anyhow!("parse next json: {} — body: {}", e, &body_str[..body_str.len().min(200)]))?;

    let results = value
        .pointer("/contents/twoColumnWatchNextResults/secondaryResults/secondaryResults/results")
        .and_then(|r| r.as_array())
        .ok_or_else(|| anyhow!("no related results"))?;

    let mut out = Vec::new();
    for it in results {
        // Новый формат: lockupViewModel
        if let Some(vm) = it.get("lockupViewModel") {
            let ctype = vm.get("contentType").and_then(|c| c.as_str());
            if ctype != Some("LOCKUP_CONTENT_TYPE_VIDEO") { continue; }
            let Some(vid) = vm.get("contentId").and_then(|v| v.as_str()) else { continue };
            let title = vm
                .pointer("/metadata/lockupMetadataViewModel/title/content")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            out.push(YtInternalResult { video_id: vid.to_string(), title });
        }
        // Старый формат: compactVideoRenderer (на всякий случай)
        else if let Some(vr) = it.get("compactVideoRenderer") {
            let Some(vid) = vr.get("videoId").and_then(|v| v.as_str()) else { continue };
            let title = vr.pointer("/title/simpleText")
                .or_else(|| vr.pointer("/title/runs/0/text"))
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            out.push(YtInternalResult { video_id: vid.to_string(), title });
        }
        // Внутри itemSectionRenderer
        else if let Some(contents) = it.pointer("/itemSectionRenderer/contents").and_then(|c| c.as_array()) {
            for child in contents {
                if let Some(vm) = child.get("lockupViewModel") {
                    let ctype = vm.get("contentType").and_then(|c| c.as_str());
                    if ctype != Some("LOCKUP_CONTENT_TYPE_VIDEO") { continue; }
                    let Some(vid) = vm.get("contentId").and_then(|v| v.as_str()) else { continue };
                    let title = vm
                        .pointer("/metadata/lockupMetadataViewModel/title/content")
                        .and_then(|t| t.as_str())
                        .unwrap_or("")
                        .to_string();
                    out.push(YtInternalResult { video_id: vid.to_string(), title });
                }
            }
        }
        if out.len() >= max_results as usize { break; }
    }
    Ok(out)
}

/// Получить channelId (browseId) видео через youtubei/v1/next.
pub fn get_video_channel_id(video_id: &str) -> Result<String> {
    let body = json!({
        "context": {
            "client": {
                "clientName": "WEB",
                "clientVersion": CLIENT_VERSION,
                "hl": "en",
                "gl": "US"
            }
        },
        "videoId": video_id
    });

    let url = format!("{}/next?key={}&prettyPrint=false", YT_API_BASE, YT_INTERNAL_KEY);
    let resp = ureq::post(&url)
        .set("Content-Type", "application/json")
        .set("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(Duration::from_secs(10))
        .send_json(&body)
        .context("youtubei next for channel")?;

    let body_str = resp.into_string().context("read next response")?;
    let value: serde_json::Value = serde_json::from_str(&body_str)
        .map_err(|e| anyhow!("parse next json: {} — body: {}", e, &body_str[..body_str.len().min(200)]))?;

    let contents = value
        .pointer("/contents/twoColumnWatchNextResults/results/results/contents")
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow!("no contents in next response"))?;

    for item in contents {
        if let Some(vsir) = item.get("videoSecondaryInfoRenderer") {
            if let Some(owner) = vsir.pointer("/owner/videoOwnerRenderer/navigationEndpoint/browseEndpoint/browseId")
                .and_then(|v| v.as_str())
            {
                return Ok(owner.to_string());
            }
        }
    }

    Err(anyhow!("channelId not found in youtubei response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_videos_smoke() {
        let res = search_videos("cats", 5);
        assert!(res.is_ok(), "search failed: {:?}", res.err());
        let videos = res.unwrap();
        assert!(!videos.is_empty(), "no videos found");
        assert!(videos[0].video_id.len() == 11, "invalid videoId: {}", videos[0].video_id);
        assert!(!videos[0].title.is_empty(), "empty title");
    }

    #[test]
    fn related_videos_smoke() {
        let res = get_related_videos("dQw4w9WgXcQ", 5);
        assert!(res.is_ok(), "related failed: {:?}", res.err());
        let videos = res.unwrap();
        assert!(!videos.is_empty(), "no related videos found");
    }

    #[test]
    fn channel_id_smoke() {
        let res = get_video_channel_id("dQw4w9WgXcQ");
        assert!(res.is_ok(), "channel_id failed: {:?}", res.err());
        let id = res.unwrap();
        assert!(id.starts_with("UC"), "invalid channel id: {}", id);
    }

    #[test]
    #[ignore]
    fn debug_web_search_kizaru() {
        let res = search_results("кизару дежавю", 8).unwrap();
        for (i, r) in res.iter().enumerate() {
            println!("#{}: id={} title='{}' artist='{}' duration='{}'", i, r.video_id, r.title, r.channel, r.duration);
        }
    }
}
