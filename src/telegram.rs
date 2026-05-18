//! Telegram MTProto через grammers-client.
//! Управляет sign-in flow + кэширует Client между вызовами.

use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use grammers_client::{Client, Config as TgConfig, FixedReconnect, InitParams, SignInError};
use grammers_session::{PackedChat, Session};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{info, warn};

/// Кэш «UID → PackedChat» — наполняется через `warm_dialog_cache` и при `list_dialogs`.
/// Без этого `send_message(uid)` каждый раз листает все диалоги (медленно + сетевой шторм).
static DIALOG_CACHE: OnceLock<Mutex<HashMap<i64, PackedChat>>> = OnceLock::new();

fn cache() -> &'static Mutex<HashMap<i64, PackedChat>> {
    DIALOG_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn cached_size() -> usize {
    cache().lock().len()
}

/// Кэш «кто я» — обновляется после connect/login, читается синхронно из `cmd_info`.
/// Без него каждый refresh раз в 5 сек делает сетевой `is_authorized()` + `get_me()`,
/// и UI висит пока MTProto-roundtrip не закончится.
#[derive(Debug, Clone, Default)]
pub struct AuthSnapshot {
    pub signed_in: bool,
    pub user_id: Option<i64>,
    pub username: Option<String>,
}

static AUTH_SNAPSHOT: OnceLock<Mutex<AuthSnapshot>> = OnceLock::new();

fn auth_state() -> &'static Mutex<AuthSnapshot> {
    AUTH_SNAPSHOT.get_or_init(|| Mutex::new(AuthSnapshot::default()))
}

pub fn get_auth_snapshot() -> AuthSnapshot {
    auth_state().lock().clone()
}

pub fn set_auth_snapshot(snap: AuthSnapshot) {
    *auth_state().lock() = snap;
}

/// Сетевой refresh — вызывать на стартовом connect и после login/logout.
/// `cmd_info` сам этого делать НЕ должен, иначе вернётся блокировка.
pub async fn refresh_auth_snapshot(client: &Client) -> AuthSnapshot {
    let signed_in = client.is_authorized().await.unwrap_or(false);
    let (uid, username) = if signed_in {
        match client.get_me().await {
            Ok(me) => (Some(me.id()), me.username().map(|s| s.to_string())),
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };
    let snap = AuthSnapshot {
        signed_in,
        user_id: uid,
        username,
    };
    set_auth_snapshot(snap.clone());
    info!(
        "[tg] auth snapshot: signed_in={} uid={:?} @{:?}",
        snap.signed_in, snap.user_id, snap.username
    );
    snap
}

/// Путь к файлу кэша диалогов — `<app_data_dir>/voicy_dialogs.cache`.
pub fn dialog_cache_path() -> PathBuf {
    let base = app_data_dir();
    std::fs::create_dir_all(&base).ok();
    base.join("voicy_dialogs.cache")
}

/// Загрузить кэш с диска (вызывать на старте). Возвращает количество записей.
/// Формат: текст, каждая строка `uid=hexpacked`.
pub fn load_dialog_cache(path: &Path) -> usize {
    let Ok(txt) = std::fs::read_to_string(path) else {
        return 0;
    };
    let mut map = cache().lock();
    let mut n = 0;
    for line in txt.lines() {
        let Some((uid_s, hex)) = line.split_once('=') else { continue };
        let Ok(uid) = uid_s.trim().parse::<i64>() else { continue };
        let Ok(pc) = PackedChat::from_hex(hex.trim()) else { continue };
        map.insert(uid, pc);
        n += 1;
    }
    info!("[tg] dialog cache loaded from disk: {} entries", n);
    n
}

/// Сохранить кэш на диск. Вызывать после успешного `warm_dialog_cache`.
pub fn save_dialog_cache(path: &Path) -> std::io::Result<usize> {
    let map = cache().lock();
    let mut content = String::with_capacity(map.len() * 40);
    for (uid, pc) in map.iter() {
        content.push_str(&format!("{}={}\n", uid, pc.to_hex()));
    }
    let n = map.len();
    std::fs::write(path, content)?;
    info!("[tg] dialog cache saved to disk: {} entries", n);
    Ok(n)
}

/// Политика переподключения: пытаемся ~неограниченно с 3-сек задержкой.
/// Telegram периодически рвёт idle TCP — без этого клиент остаётся мёртвым
/// после первой же дисконнектии (os error 10054 на Windows).
static RECONN: FixedReconnect = FixedReconnect {
    attempts: usize::MAX,
    delay: Duration::from_secs(3),
};

/// Стабильная директория для данных приложения (%APPDATA%/voicy на Windows).
fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("voicy"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Путь к session-файлу: <app_data_dir>/<session>.session
pub fn session_path(cfg: &Config) -> PathBuf {
    let base = app_data_dir();
    std::fs::create_dir_all(&base).ok();
    base.join(format!("{}.session", cfg.telegram.session))
}

/// Проверить размер session-файла и залоггировать состояние.
fn log_session_state(label: &str, session: &Session, path: &std::path::Path) {
    let signed_in = session.signed_in();
    let dcs = session.get_dcs();
    let data = session.save();
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    info!(
        "[tg-session] {} path={} signed_in={} dcs={} ram_bytes={} file_bytes={}",
        label,
        path.display(),
        signed_in,
        dcs.len(),
        data.len(),
        file_size
    );
}

/// Подключиться к Telegram. Если сессия валидна — возвращаем готового Client.
/// Если нет — Err (вызывай `interactive_login` отдельно).
pub async fn connect(cfg: &Config) -> Result<Client> {
    let sp = session_path(cfg);
    let session = Session::load_file_or_create(&sp).context("Session::load")?;
    log_session_state("connect(load)", &session, &sp);
    info!("[tg-connect] api_id={} api_hash_len={}", cfg.telegram.api_id, cfg.telegram.api_hash.len());
    let params = InitParams {
        reconnection_policy: &RECONN,
        ..Default::default()
    };
    let client = match Client::connect(TgConfig {
        session,
        api_id: cfg.telegram.api_id,
        api_hash: cfg.telegram.api_hash.clone(),
        params,
    }).await {
        Ok(c) => c,
        Err(e) => {
            warn!("[tg-connect] Client::connect FAILED: {:?}", e);
            return Err(anyhow!("Client::connect: {:?}", e));
        }
    };
    Ok(client)
}

/// Проверить, авторизован ли клиент (есть валидный user в сессии).
pub async fn is_signed_in(client: &Client) -> Result<bool> {
    Ok(client.is_authorized().await?)
}

/// Сохранить сессию на диск.
pub async fn save_session(client: &Client, cfg: &Config) -> Result<()> {
    let sp = session_path(cfg);
    log_session_state("save(before)", client.session(), &sp);
    client.session().save_to_file(&sp).context("save session")?;
    log_session_state("save(after)", client.session(), &sp);
    Ok(())
}

/// Интерактивный логин через phone + code (+ optional 2FA).
/// Читает phone/code из stdin. Сохраняет сессию.
pub async fn interactive_login(cfg: &Config) -> Result<Client> {
    let client = connect(cfg).await?;
    if is_signed_in(&client).await? {
        println!("Уже залогинен.");
        return Ok(client);
    }

    print!("Введи номер телефона (+12345678901): ");
    io::stdout().flush()?;
    let stdin = io::stdin();
    let phone = stdin.lock().lines().next().context("read phone")??.trim().to_string();

    let token = client
        .request_login_code(&phone)
        .await
        .context("request_login_code")?;

    print!("Введи код из Telegram: ");
    io::stdout().flush()?;
    let code = stdin.lock().lines().next().context("read code")??.trim().to_string();

    let signed = client.sign_in(&token, &code).await;
    match signed {
        Ok(_) => {}
        Err(SignInError::PasswordRequired(password_token)) => {
            print!("2FA — введи cloud-пароль: ");
            io::stdout().flush()?;
            // На Windows эхо есть; в идеале — rpassword, но добавим позже.
            let pwd = stdin.lock().lines().next().context("read pwd")??.trim().to_string();
            client
                .check_password(password_token, &pwd)
                .await
                .context("check_password")?;
        }
        Err(e) => return Err(anyhow!("sign_in: {}", e)),
    }

    // grammers закоммитит session state только после get_me() / is_authorized().
    let _ = client.get_me().await;
    save_session(&client, cfg).await?;
    info!("[tg] логин успешен, сессия сохранена → {}", session_path(cfg).display());
    Ok(client)
}

/// Краткое представление диалога для UI.
#[derive(Debug, serde::Serialize)]
pub struct DialogInfo {
    pub uid: i64,
    pub name: String,
    pub username: Option<String>,
    pub kind: &'static str, // "user" | "group" | "channel"
}

/// Каталог где кэшируем avatar JPEG'и: `%APPDATA%/voicy/avatars/<uid>.jpg`.
pub fn avatar_cache_dir() -> PathBuf {
    let base = dirs::data_dir()
        .map(|d| d.join("voicy"))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("avatars");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn avatar_cache_path(uid: i64) -> PathBuf {
    avatar_cache_dir().join(format!("{}.jpg", uid))
}

/// Скачать аватар пользователя в кэш. Возвращает Path к файлу или None если
/// у пользователя нет аватара / не получилось скачать. Идемпотентно — если
/// файл уже есть и не пустой, ничего не делаем.
///
/// Использует PackedChat из DIALOG_CACHE — то есть юзер должен быть в кэше
/// диалогов (warm_dialog_cache вызывается на старте).
pub async fn fetch_avatar(client: &Client, uid: i64) -> Option<PathBuf> {
    let path = avatar_cache_path(uid);
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 0 {
            return Some(path);
        }
    }
    // Сначала ищем PackedChat в DIALOG_CACHE. Если не нашли — может это сам юзер,
    // который не появляется в iter_dialogs. Тогда fallback на client.get_me().
    let packed = cache().lock().get(&uid).copied();
    let chat = match packed {
        Some(p) => match client.unpack_chat(p).await {
            Ok(c) => c,
            Err(e) => {
                warn!("[avatar] unpack_chat({}): {}", uid, e);
                return None;
            }
        },
        None => {
            // Пробуем get_me — может это собственный uid
            match client.get_me().await {
                Ok(me) if me.id() == uid => {
                    cache().lock().insert(uid, me.pack());
                    grammers_client::types::Chat::User(me)
                }
                Ok(_) => {
                    warn!("[avatar] uid {} not in dialog cache and not self", uid);
                    return None;
                }
                Err(e) => {
                    warn!("[avatar] get_me fallback for {}: {}", uid, e);
                    return None;
                }
            }
        }
    };
    let dl = chat.photo_downloadable(false)?; // small (~51x51)
    match client.download_media(&dl, &path).await {
        Ok(_) => {
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() > 0 {
                    return Some(path);
                }
            }
            None
        }
        Err(e) => {
            warn!("[avatar] download {}: {}", uid, e);
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

/// Перечислить недавние диалоги — только люди (для импорта в контакты).
/// Возвращает до `limit` юзеров. Параллельно прогревает `DIALOG_CACHE`.
pub async fn list_dialogs(client: &Client, limit: usize) -> Result<Vec<DialogInfo>> {
    let mut out = Vec::new();
    let mut it = client.iter_dialogs();
    while let Some(d) = it.next().await? {
        let chat = d.chat();
        cache().lock().insert(chat.id(), chat.pack());
        if out.len() >= limit {
            continue; // продолжаем прогревать кэш, не возвращаем больше
        }
        let kind: &'static str = match chat {
            grammers_client::types::Chat::User(_) => "user",
            grammers_client::types::Chat::Group(_) => "group",
            grammers_client::types::Chat::Channel(_) => "channel",
        };
        if kind != "user" {
            continue;
        }
        let username = match chat {
            grammers_client::types::Chat::User(u) => u.username().map(|s| s.to_string()),
            _ => None,
        };
        out.push(DialogInfo {
            uid: chat.id(),
            name: chat.name().to_string(),
            username,
            kind,
        });
    }
    Ok(out)
}

/// Прогреть кэш диалогов: листает все диалоги и записывает PackedChat в DIALOG_CACHE.
/// Чтобы send_message не делал iter_dialogs на каждое сообщение.
/// После прогрева сохраняет кэш на диск — следующий запуск стартует мгновенно.
pub async fn warm_dialog_cache(client: &Client) -> Result<usize> {
    let mut it = client.iter_dialogs();
    let mut n = 0;
    while let Some(d) = it.next().await? {
        let chat = d.chat();
        cache().lock().insert(chat.id(), chat.pack());
        n += 1;
    }
    info!("[tg] dialog cache warmed: {} entries", n);
    if let Err(e) = save_dialog_cache(&dialog_cache_path()) {
        warn!("[tg] failed to persist dialog cache: {}", e);
    }
    Ok(n)
}

/// Послать сообщение пользователю по числовому user_id.
/// Сначала пробуем PackedChat из кэша (мгновенно), потом — fallback на iter_dialogs.
pub async fn send_message(client: &Client, uid: i64, text: &str) -> Result<()> {
    // Hot path: PackedChat уже в кэше → шлём напрямую.
    let packed = cache().lock().get(&uid).copied();
    if let Some(packed) = packed {
        match client.send_message(packed, text).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                let s = e.to_string();
                if is_auth_expired(&s) {
                    set_auth_snapshot(AuthSnapshot::default());
                    return Err(anyhow!(
                        "Сессия Telegram истекла (AUTH_KEY_UNREGISTERED). Залогинься заново — QR или phone+code."
                    ));
                }
                warn!("[tg] cached send failed ({}): refreshing dialogs…", e);
            }
        }
    }

    // Fallback: листаем диалоги, по пути обновляя кэш.
    let mut dialogs = client.iter_dialogs();
    loop {
        match dialogs.next().await {
            Ok(Some(d)) => {
                let chat = d.chat();
                cache().lock().insert(chat.id(), chat.pack());
                if chat.id() == uid {
                    client.send_message(chat.pack(), text).await?;
                    return Ok(());
                }
            }
            Ok(None) => break,
            Err(e) => {
                let s = e.to_string();
                if is_auth_expired(&s) {
                    set_auth_snapshot(AuthSnapshot::default());
                    return Err(anyhow!(
                        "Сессия Telegram истекла (AUTH_KEY_UNREGISTERED). Залогинься заново."
                    ));
                }
                return Err(anyhow!("iter_dialogs: {}", e));
            }
        }
    }
    Err(anyhow!(
        "user id {} не найден в диалогах (проверь что ты с ним переписывался)",
        uid
    ))
}

fn is_auth_expired(err: &str) -> bool {
    err.contains("AUTH_KEY_UNREGISTERED")
        || err.contains("SESSION_REVOKED")
        || err.contains("AUTH_KEY_INVALID")
        || err.contains("USER_DEACTIVATED")
}
