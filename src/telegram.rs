//! Telegram MTProto через grammers-client.
//! Управляет sign-in flow + кэширует Client между вызовами.

use crate::config::Config;
use anyhow::{anyhow, Context, Result};
use grammers_client::types::Media;
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

/// Низкоуровневый коннект: загрузить сессию с диска и поднять Client. Лечение
/// self-user тут НЕ делается (это делает `connect`).
async fn build_client(cfg: &Config) -> Result<Client> {
    let sp = session_path(cfg);
    let session = Session::load_file_or_create(&sp).context("Session::load")?;
    log_session_state("connect(load)", &session, &sp);
    info!("[tg-connect] api_id={} api_hash_len={}", cfg.telegram.api_id, cfg.telegram.api_hash.len());
    let params = InitParams {
        reconnection_policy: &RECONN,
        ..Default::default()
    };
    match Client::connect(TgConfig {
        session,
        api_id: cfg.telegram.api_id,
        api_hash: cfg.telegram.api_hash.clone(),
        params,
    }).await {
        Ok(c) => Ok(c),
        Err(e) => {
            warn!("[tg-connect] Client::connect FAILED: {:?}", e);
            Err(anyhow!("Client::connect: {:?}", e))
        }
    }
}

/// Домашний DC: тот, для которого в сессии есть auth-key. Нужен только чтобы
/// записать self-user в сессию (см. `connect`); ChatHashCache сам dc игнорирует.
fn home_dc(client: &Client) -> i32 {
    let sess = client.session();
    for dc in sess.get_dcs() {
        if sess.dc_auth_key(dc.id).is_some() {
            return dc.id;
        }
    }
    2 // дефолт: DC2 (самый частый home), если auth-key не нашёлся
}

/// Подключиться к Telegram. Если сессия валидна, возвращаем готового Client,
/// иначе Err (вызывай `interactive_login` отдельно).
///
/// ЛЕЧЕНИЕ self-user: старые/кривые сессии бывают авторизованы по сети
/// (`is_authorized()` = true), но без сохранённого self-user
/// (`session.get_user()` = None). Тогда grammers строит ChatHashCache с
/// self_id = None и ПАДАЕТ паникой `tried to query self_id before it's known`
/// при первом же `updateShortMessage` (например мгновенный ответ бота в режиме
/// «скачай»). Лечим до любых операций: тянем get_me, пишем self-user в сессию,
/// сохраняем и реконнектимся, чтобы свежий ChatHashCache получил self_id.
pub async fn connect(cfg: &Config) -> Result<Client> {
    let client = build_client(cfg).await?;
    if client.session().get_user().is_none() {
        // get_me падает с 401 если реально не залогинены: тогда лечить нечего.
        if let Ok(me) = client.get_me().await {
            let dc = home_dc(&client);
            client.session().set_user(me.id(), dc, me.is_bot());
            match save_session(&client, cfg).await {
                Ok(()) => {
                    info!("[tg] self-user восстановлен id={} dc={}, реконнект", me.id(), dc);
                    return build_client(cfg).await;
                }
                Err(e) => warn!("[tg] heal save_session: {} (продолжаем без реконнекта)", e),
            }
        }
    }
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

// ── Скачивание музыки через бота (@cloudpullbot) ────────────────────────────
// Voicy остаётся тонким: резолвит бота, шлёт ему «/<формат> <url>», поллингом
// ждёт ВХОДЯЩИЙ файл-документ и пересылает его в музыкальный чат. Всё тяжёлое
// (само скачивание) на стороне бота. Без персистентного update-цикла: читаем
// историю чата опросом, это проще и не конфликтует с остальным приложением.

/// Резолв @username в PackedChat (+ кэш). Принимает «name» или «@name».
async fn resolve_username_packed(client: &Client, username: &str) -> Result<PackedChat> {
    let uname = username.trim().trim_start_matches('@');
    let chat = client
        .resolve_username(uname)
        .await
        .map_err(|e| anyhow!("resolve_username(@{}): {}", uname, e))?
        .ok_or_else(|| anyhow!("@{} не найден в Telegram", uname))?;
    let packed = chat.pack();
    cache().lock().insert(chat.id(), packed);
    Ok(packed)
}

/// PackedChat по числовому uid: из кэша, иначе листаем диалоги (как send_message).
async fn resolve_uid_packed(client: &Client, uid: i64) -> Result<PackedChat> {
    if let Some(p) = cache().lock().get(&uid).copied() {
        return Ok(p);
    }
    let mut dialogs = client.iter_dialogs();
    while let Some(d) = dialogs
        .next()
        .await
        .map_err(|e| anyhow!("iter_dialogs: {}", e))?
    {
        let chat = d.chat();
        let packed = chat.pack();
        cache().lock().insert(chat.id(), packed);
        if chat.id() == uid {
            return Ok(packed);
        }
    }
    Err(anyhow!("uid {} не найден в диалогах", uid))
}

/// Найти чат по НАЗВАНИЮ среди диалогов (case-insensitive). Нужно потому, что
/// пользователь вписывает в «Куда слать» отображаемое имя чата (например
/// «музыка» кириллицей), а username у такого чата нет.
async fn resolve_by_title(client: &Client, title: &str) -> Result<PackedChat> {
    let want = title.trim().to_lowercase();
    let mut dialogs = client.iter_dialogs();
    while let Some(d) = dialogs
        .next()
        .await
        .map_err(|e| anyhow!("iter_dialogs: {}", e))?
    {
        let chat = d.chat();
        if chat.name().trim().to_lowercase() == want {
            let packed = chat.pack();
            cache().lock().insert(chat.id(), packed);
            return Ok(packed);
        }
    }
    Err(anyhow!("чат «{}» не найден среди диалогов", title))
}

/// Куда пересылать результат: пусто = Saved Messages (чат с собой); число = uid;
/// «@name» = username; иначе трактуем как НАЗВАНИЕ чата (ищем по диалогам, в т.ч.
/// кириллица), а если не нашли по названию, пробуем как username.
async fn resolve_music_dest(client: &Client, dest: &str) -> Result<PackedChat> {
    let d = dest.trim();
    if d.is_empty() {
        let me = client.get_me().await.map_err(|e| anyhow!("get_me: {}", e))?;
        return resolve_uid_packed(client, me.id()).await;
    }
    let digits = d.trim_start_matches('@');
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '-') {
        if let Ok(uid) = digits.parse::<i64>() {
            return resolve_uid_packed(client, uid).await;
        }
    }
    if let Some(uname) = d.strip_prefix('@') {
        return resolve_username_packed(client, uname).await;
    }
    // Без @: сначала по названию чата, потом как username (на случай ascii-ника).
    match resolve_by_title(client, d).await {
        Ok(p) => Ok(p),
        Err(title_err) => resolve_username_packed(client, d)
            .await
            .map_err(|_| title_err),
    }
}

/// id последнего сообщения в чате (0 если пусто). База для «дождаться нового».
async fn latest_msg_id(client: &Client, chat: PackedChat) -> Result<i32> {
    let mut it = client.iter_messages(chat);
    match it.next().await.map_err(|e| anyhow!("iter_messages: {}", e))? {
        Some(m) => Ok(m.id()),
        None => Ok(0),
    }
}

/// Опросом дождаться первого ВХОДЯЩЕГО сообщения с документом и id > after_id.
/// Бот отвечает аудиофайлом (Document). Таймаут защищает от зависания, если бот
/// молчит. Опрашиваем верхушку истории раз в 2 секунды.
async fn wait_for_file(
    client: &Client,
    chat: PackedChat,
    after_id: i32,
    timeout: Duration,
) -> Result<i32> {
    let start = std::time::Instant::now();
    loop {
        let mut it = client.iter_messages(chat);
        // Верхушка истории (новейшие первыми); глубже after_id смотреть незачем.
        for _ in 0..10 {
            match it.next().await.map_err(|e| anyhow!("iter_messages: {}", e))? {
                Some(m) => {
                    if m.id() <= after_id {
                        break;
                    }
                    if !m.outgoing() && matches!(m.media(), Some(Media::Document(_))) {
                        return Ok(m.id());
                    }
                }
                None => break,
            }
        }
        if start.elapsed() >= timeout {
            return Err(anyhow!("бот не прислал файл за {} c", timeout.as_secs()));
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Переслать сообщение `msg_id` из `source` в `dest`.
async fn forward_msg(
    client: &Client,
    dest: PackedChat,
    msg_id: i32,
    source: PackedChat,
) -> Result<()> {
    let res = client
        .forward_messages(dest, &[msg_id], source)
        .await
        .map_err(|e| anyhow!("forward_messages: {}", e))?;
    if res.iter().all(|m| m.is_none()) {
        return Err(anyhow!("пересылка не удалась (пустой ответ сервера)"));
    }
    Ok(())
}

/// Полный цикл «скачать музыку через бота»:
///   1. резолв бота,
///   2. baseline = id последнего сообщения чата с ботом,
///   3. отправка боту команды `/<format> <url>` (как есть, без грам-форматтера),
///   4. ожидание входящего файла-документа,
///   5. пересылка файла в `music_dest`.
/// Сериализовано по своей природе (одно скачивание за вызов): корреляция ответа
/// идёт по «первый новый входящий документ после baseline».
pub async fn download_via_bot(
    client: &Client,
    url: &str,
    format: &str,
    bot_username: &str,
    music_dest: &str,
    dest_override: Option<i64>,
) -> Result<()> {
    let bot = resolve_username_packed(client, bot_username).await?;
    let baseline = latest_msg_id(client, bot).await.unwrap_or(0);
    // Команда боту: слеш-формат строго как есть, грамматический форматтер
    // (заглавная/точка) сюда НЕ применяем, иначе бот не распознает команду.
    let command = format!("/{} {}", format, url);
    client
        .send_message(bot, command.as_str())
        .await
        .map_err(|e| anyhow!("отправка боту: {}", e))?;
    info!("[tg] → бот: {}", command);
    let file_id = wait_for_file(client, bot, baseline, Duration::from_secs(120)).await?;
    // Получатель: явный (назван голосом «...и скинь Маше», SELF → себе) перебивает
    // music_dest из конфига.
    let dest = match dest_override {
        // «скинь мне/себе» при скачивании = дефолтный чат (music_dest), а НЕ
        // Saved Messages: пользователь считает дефолтный чат «своим».
        Some(uid) if uid == crate::contacts::SELF_SENTINEL_UID => {
            resolve_music_dest(client, music_dest).await?
        }
        Some(uid) => resolve_uid_packed(client, uid).await?,
        None => resolve_music_dest(client, music_dest).await?,
    };
    forward_msg(client, dest, file_id, bot).await?;
    info!("[tg] ✅ файл от бота переслан получателю");
    Ok(())
}

// ── «Включи <песня>»: вектор-индекс музыкального канала ─────────────────────
// Из канала-источника собираем (msg_id, название) аудио, кэшируем на диск,
// строим локальный вектор-индекс (music_index) и пересылаем самый похожий трек.

use crate::music_index::{MusicIndex, Track};

#[derive(serde::Serialize, serde::Deserialize)]
struct MusicCache {
    source: String,
    tracks: Vec<Track>,
}

fn music_cache_path() -> PathBuf {
    app_data_dir().join("music_index.json")
}

fn load_music_cache(source: &str) -> Option<Vec<Track>> {
    let txt = std::fs::read_to_string(music_cache_path()).ok()?;
    let cache: MusicCache = serde_json::from_str(&txt).ok()?;
    if cache.source == source && !cache.tracks.is_empty() {
        Some(cache.tracks)
    } else {
        None
    }
}

fn save_music_cache(source: &str, tracks: &[Track]) {
    let cache = MusicCache {
        source: source.to_string(),
        tracks: tracks.to_vec(),
    };
    match serde_json::to_string(&cache) {
        Ok(txt) => {
            if let Err(e) = std::fs::write(music_cache_path(), txt) {
                warn!("[tg] save music cache: {}", e);
            }
        }
        Err(e) => warn!("[tg] serialize music cache: {}", e),
    }
}

/// Документ похож на музыкальный трек (а не голосовуху/прочее)?
fn is_music_doc(doc: &grammers_client::types::media::Document) -> bool {
    let has_meta = doc.performer().is_some() || doc.audio_title().is_some();
    let name = doc.name().to_lowercase();
    let music_ext = [".mp3", ".flac", ".wav", ".m4a", ".ogg", ".opus", ".aac"]
        .iter()
        .any(|e| name.ends_with(e));
    has_meta || music_ext
}

/// Человекочитаемое название трека: «исполнитель - название», иначе имя файла.
fn doc_track_title(doc: &grammers_client::types::media::Document) -> Option<String> {
    match (doc.performer(), doc.audio_title()) {
        (Some(p), Some(t)) => Some(format!("{} - {}", p.trim(), t.trim())),
        (Some(p), None) => Some(p.trim().to_string()),
        (None, Some(t)) => Some(t.trim().to_string()),
        (None, None) => {
            let n = doc.name().trim();
            let base = n.rsplit_once('.').map(|(b, _)| b).unwrap_or(n).trim();
            if base.is_empty() {
                None
            } else {
                Some(base.to_string())
            }
        }
    }
}

/// Пройти историю канала и собрать аудио-треки (новейшие первыми). Ограничено
/// сверху, чтобы не зависнуть на огромном канале.
async fn fetch_music_tracks(client: &Client, chat: PackedChat) -> Result<Vec<Track>> {
    const MAX_TRACKS: usize = 2000;
    let mut it = client.iter_messages(chat);
    let mut tracks = Vec::new();
    while let Some(m) = it
        .next()
        .await
        .map_err(|e| anyhow!("iter_messages: {}", e))?
    {
        if tracks.len() >= MAX_TRACKS {
            break;
        }
        if let Some(grammers_client::types::Media::Document(doc)) = m.media() {
            if is_music_doc(&doc) {
                if let Some(title) = doc_track_title(&doc) {
                    tracks.push(Track { msg_id: m.id(), title });
                }
            }
        }
    }
    info!("[tg] музыкальный индекс: собрано {} треков", tracks.len());
    Ok(tracks)
}

/// Полный цикл «включи <запрос>»: загрузить треки (кэш или дотянуть из канала),
/// построить индекс, найти лучший матч и переслать его в music_dest. Возвращает
/// название включённого трека. `force_reindex` игнорирует кэш и перечитывает канал.
pub async fn play_track(
    client: &Client,
    source: &str,
    query: &str,
    music_dest: &str,
    force_reindex: bool,
) -> Result<String> {
    if source.trim().is_empty() {
        return Err(anyhow!(
            "музыкальный канал не задан (Настройки → Музыка → Источник)"
        ));
    }
    let src_chat = resolve_music_dest(client, source).await?;
    let tracks = match (force_reindex, load_music_cache(source)) {
        (false, Some(cached)) => cached,
        _ => {
            let t = fetch_music_tracks(client, src_chat).await?;
            save_music_cache(source, &t);
            t
        }
    };
    if tracks.is_empty() {
        return Err(anyhow!("в канале не нашлось аудио для индексации"));
    }
    let idx = MusicIndex::build(tracks);
    let (msg_id, title, score) = idx
        .best_match(query)
        .ok_or_else(|| anyhow!("не нашёл трек по запросу «{}»", query))?;
    info!(
        "[tg] play «{}» → «{}» (score {:.2}, из {} треков)",
        query,
        title,
        score,
        idx.len()
    );
    let dest = resolve_music_dest(client, music_dest).await?;
    forward_msg(client, dest, msg_id, src_chat).await?;
    Ok(title)
}

/// Перечитать канал-источник и пересобрать кэш индекса. Возвращает число треков.
pub async fn reindex_music(client: &Client, source: &str) -> Result<usize> {
    if source.trim().is_empty() {
        return Err(anyhow!("музыкальный канал не задан"));
    }
    let src_chat = resolve_music_dest(client, source).await?;
    let tracks = fetch_music_tracks(client, src_chat).await?;
    let n = tracks.len();
    save_music_cache(source, &tracks);
    Ok(n)
}
