//! Durable screenshot history backed by SQLite and PNG files in app data.

use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use image::{RgbaImage, imageops::thumbnail};
use rusqlite::{Connection, params};
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime};

const HISTORY_DIRECTORY: &str = "history";
const IMAGES_DIRECTORY: &str = "images";
const THUMBNAILS_DIRECTORY: &str = "thumbnails";
const DATABASE_NAME: &str = "snaprust-history.sqlite3";
const MAX_QUERY_LENGTH: usize = 256;
const MAX_OCR_TEXT_LENGTH: usize = 1_000_000;
const MAX_TAG_COUNT: usize = 12;
const MAX_TAG_LENGTH: usize = 48;
pub const MAX_HISTORY_ITEMS: usize = 500;
pub const MAX_HISTORY_IMAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_BATCH_ITEMS: usize = 200;
const EXPORT_DIRECTORY: &str = "SnapRust Exports";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItemPayload {
    id: i64,
    width: u32,
    height: u32,
    created_at_ms: i64,
    favorite: bool,
    ocr_text: Option<String>,
    tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryUsagePayload {
    item_count: usize,
    image_bytes: u64,
    max_items: usize,
    max_image_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryExportPayload {
    directory: String,
    exported_count: usize,
}

#[derive(Debug, Clone)]
struct StoredHistoryEntry {
    id: i64,
    filename: String,
    width: u32,
    height: u32,
    created_at_ms: i64,
    favorite: bool,
    ocr_text: Option<String>,
    tags: Vec<String>,
}

pub struct HistoryStore {
    connection: Mutex<Connection>,
    images_directory: PathBuf,
    thumbnails_directory: PathBuf,
    next_filename: AtomicU64,
}

impl HistoryStore {
    pub fn open<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("failed to resolve the SnapRust data directory: {error}"))?;
        Self::open_in(app_data.join(HISTORY_DIRECTORY))
    }

    fn open_in(directory: PathBuf) -> Result<Self, String> {
        let images_directory = directory.join(IMAGES_DIRECTORY);
        let thumbnails_directory = directory.join(THUMBNAILS_DIRECTORY);
        fs::create_dir_all(&images_directory).map_err(|error| {
            format!("failed to create the screenshot history directory: {error}")
        })?;
        fs::create_dir_all(&thumbnails_directory).map_err(|error| {
            format!("failed to create the screenshot history thumbnails directory: {error}")
        })?;
        let connection = Connection::open(directory.join(DATABASE_NAME))
            .map_err(|error| format!("failed to open the screenshot history database: {error}"))?;
        connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;
                CREATE TABLE IF NOT EXISTS screenshots (
                    id INTEGER PRIMARY KEY,
                    filename TEXT NOT NULL UNIQUE,
                    ocr_text TEXT,
                    width INTEGER NOT NULL CHECK (width > 0),
                    height INTEGER NOT NULL CHECK (height > 0),
                    created_at_ms INTEGER NOT NULL,
                    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
                    tags TEXT NOT NULL DEFAULT ''
                );
                CREATE INDEX IF NOT EXISTS screenshots_created_at_idx
                    ON screenshots(created_at_ms DESC);
                CREATE INDEX IF NOT EXISTS screenshots_favorite_created_at_idx
                    ON screenshots(favorite, created_at_ms DESC);
                ",
            )
            .map_err(|error| {
                format!("failed to initialize the screenshot history database: {error}")
            })?;
        ensure_tags_column(&connection)?;
        reconcile_filesystem(&connection, &images_directory, &thumbnails_directory)?;

        Ok(Self {
            connection: Mutex::new(connection),
            images_directory,
            thumbnails_directory,
            next_filename: AtomicU64::new(0),
        })
    }

    pub fn save(&self, image: &RgbaImage, ocr_text: Option<&str>) -> Result<i64, String> {
        let width = image.width();
        let height = image.height();
        if width == 0 || height == 0 {
            return Err("cannot save an empty screenshot to history".to_owned());
        }
        let stored_ocr_text = normalize_ocr_text(ocr_text)?;
        let created_at_ms = current_unix_millis()?;
        let filename = self.next_filename(created_at_ms);
        let image_path = self.image_path_from_filename(&filename)?;
        let thumbnail_path = self.thumbnail_path_from_filename(&filename)?;
        let png = crate::screenshot::encode_png(image)?;
        let thumbnail_png = crate::screenshot::encode_png(&thumbnail(image, 320, 210))?;
        if let Err(error) = write_file_atomically(&image_path, &png) {
            return Err(format!(
                "failed to write the screenshot history image: {error}"
            ));
        }
        if let Err(error) = write_file_atomically(&thumbnail_path, &thumbnail_png) {
            let _ = fs::remove_file(&image_path);
            return Err(format!(
                "failed to write the screenshot history thumbnail: {error}"
            ));
        }

        let database_result: Result<i64, String> = (|| {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "screenshot history database lock is poisoned".to_owned())?;
            connection
                .execute(
                    "INSERT INTO screenshots (filename, ocr_text, width, height, created_at_ms, tags)
                     VALUES (?1, ?2, ?3, ?4, ?5, '')",
                    params![
                        filename,
                        stored_ocr_text,
                        i64::from(width),
                        i64::from(height),
                        created_at_ms
                    ],
                )
                .map_err(|error| format!("failed to save screenshot history metadata: {error}"))?;
            Ok(connection.last_insert_rowid())
        })();
        match database_result {
            Ok(id) => {
                if let Err(error) =
                    self.cleanup_retention(MAX_HISTORY_ITEMS, MAX_HISTORY_IMAGE_BYTES)
                {
                    eprintln!("failed to automatically clean screenshot history: {error}");
                }
                Ok(id)
            }
            Err(error) => {
                let _ = fs::remove_file(&image_path);
                let _ = fs::remove_file(&thumbnail_path);
                Err(error)
            }
        }
    }

    pub fn list(
        &self,
        query: Option<&str>,
        favorites_only: bool,
    ) -> Result<Vec<HistoryItemPayload>, String> {
        let query = normalized_query(query)?;
        let pattern = query.as_deref().map(|query| format!("%{query}%"));
        let connection = self
            .connection
            .lock()
            .map_err(|_| "screenshot history database lock is poisoned".to_owned())?;
        let sql = match (favorites_only, pattern.is_some()) {
            (false, false) => {
                "SELECT id, width, height, created_at_ms, favorite, ocr_text, tags
                 FROM screenshots ORDER BY created_at_ms DESC, id DESC LIMIT 200"
            }
            (true, false) => {
                "SELECT id, width, height, created_at_ms, favorite, ocr_text, tags
                 FROM screenshots WHERE favorite = 1 ORDER BY created_at_ms DESC, id DESC LIMIT 200"
            }
            (false, true) => {
                "SELECT id, width, height, created_at_ms, favorite, ocr_text, tags
                 FROM screenshots
                 WHERE COALESCE(ocr_text, '') LIKE ?1 OR tags LIKE ?1
                 ORDER BY created_at_ms DESC, id DESC LIMIT 200"
            }
            (true, true) => {
                "SELECT id, width, height, created_at_ms, favorite, ocr_text, tags
                 FROM screenshots
                 WHERE favorite = 1 AND (COALESCE(ocr_text, '') LIKE ?1 OR tags LIKE ?1)
                 ORDER BY created_at_ms DESC, id DESC LIMIT 200"
            }
        };
        let mut statement = connection
            .prepare(sql)
            .map_err(|error| format!("failed to prepare screenshot history query: {error}"))?;
        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<HistoryItemPayload> {
            let width: i64 = row.get(1)?;
            let height: i64 = row.get(2)?;
            Ok(HistoryItemPayload {
                id: row.get(0)?,
                width: u32::try_from(width)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(1, width))?,
                height: u32::try_from(height)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, height))?,
                created_at_ms: row.get(3)?,
                favorite: row.get::<_, i64>(4)? != 0,
                ocr_text: row.get(5)?,
                tags: decode_tags(&row.get::<_, String>(6)?),
            })
        };
        let rows = if let Some(pattern) = pattern {
            statement
                .query_map([pattern], map_row)
                .map_err(|error| format!("failed to read screenshot history: {error}"))?
        } else {
            statement
                .query_map([], map_row)
                .map_err(|error| format!("failed to read screenshot history: {error}"))?
        };
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode screenshot history: {error}"))
    }

    pub fn thumbnail_png(&self, id: i64) -> Result<Vec<u8>, String> {
        let filename = self.filename_for_id(id)?;
        let thumbnail_path = self.thumbnail_path_from_filename(&filename)?;
        match fs::read(&thumbnail_path) {
            Ok(png) => return Ok(png),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "failed to read screenshot history thumbnail: {error}"
                ));
            }
        }

        let image = self.load_image(id)?;
        let png = crate::screenshot::encode_png(&thumbnail(&image, 320, 210))?;
        if let Err(error) = write_file_atomically(&thumbnail_path, &png) {
            eprintln!("failed to cache screenshot history thumbnail: {error}");
        }
        Ok(png)
    }

    pub fn copy(&self, id: i64) -> Result<(), String> {
        crate::clipboard::write_image(&self.load_image(id)?)
    }

    pub fn image(&self, id: i64) -> Result<RgbaImage, String> {
        self.load_image(id)
    }

    pub fn set_favorite(&self, id: i64, favorite: bool) -> Result<(), String> {
        let changed = self
            .connection
            .lock()
            .map_err(|_| "screenshot history database lock is poisoned".to_owned())?
            .execute(
                "UPDATE screenshots SET favorite = ?1 WHERE id = ?2",
                params![i64::from(favorite), id],
            )
            .map_err(|error| format!("failed to update screenshot favorite state: {error}"))?;
        if changed == 0 {
            return Err(format!("screenshot history entry does not exist: {id}"));
        }
        Ok(())
    }

    pub fn set_tags(&self, id: i64, tags: Vec<String>) -> Result<(), String> {
        let tags = normalize_tags(&tags)?;
        let changed = self
            .connection
            .lock()
            .map_err(|_| "screenshot history database lock is poisoned".to_owned())?
            .execute(
                "UPDATE screenshots SET tags = ?1 WHERE id = ?2",
                params![tags.join(","), id],
            )
            .map_err(|error| format!("failed to update screenshot tags: {error}"))?;
        if changed == 0 {
            return Err(format!("screenshot history entry does not exist: {id}"));
        }
        Ok(())
    }

    pub fn set_favorite_batch(&self, ids: Vec<i64>, favorite: bool) -> Result<(), String> {
        for id in normalize_batch_ids(ids)? {
            self.set_favorite(id, favorite)?;
        }
        Ok(())
    }

    pub fn delete(&self, id: i64) -> Result<(), String> {
        let filename = self.filename_for_id(id)?;
        let image_path = self.image_path_from_filename(&filename)?;
        let thumbnail_path = self.thumbnail_path_from_filename(&filename)?;
        let tombstone_path = image_path.with_extension("deleting");
        if tombstone_path.exists() {
            return Err(format!(
                "screenshot history deletion is already in progress: {filename}"
            ));
        }
        let moved_to_tombstone = match fs::rename(&image_path, &tombstone_path) {
            Ok(()) => true,
            Err(error) if error.kind() == ErrorKind::NotFound => false,
            Err(error) => {
                return Err(format!(
                    "failed to stage the screenshot history image for deletion: {error}"
                ));
            }
        };

        let delete_result = match self.connection.lock() {
            Ok(connection) => connection
                .execute("DELETE FROM screenshots WHERE id = ?1", [id])
                .map_err(|error| format!("failed to remove screenshot history metadata: {error}")),
            Err(_) => Err("screenshot history database lock is poisoned".to_owned()),
        };

        let changed = match delete_result {
            Ok(changed) => changed,
            Err(error) => {
                if moved_to_tombstone {
                    restore_tombstone(&tombstone_path, &image_path)?;
                }
                return Err(error);
            }
        };
        if changed == 0 {
            if moved_to_tombstone {
                restore_tombstone(&tombstone_path, &image_path)?;
            }
            return Err(format!("screenshot history entry does not exist: {id}"));
        }

        if moved_to_tombstone {
            fs::remove_file(&tombstone_path).map_err(|error| {
                format!(
                    "screenshot history metadata was removed, but its image cleanup failed: {error}"
                )
            })?;
        }
        if let Err(error) = fs::remove_file(&thumbnail_path)
            && error.kind() != ErrorKind::NotFound
        {
            eprintln!("failed to remove screenshot history thumbnail: {error}");
        }
        Ok(())
    }

    pub fn delete_batch(&self, ids: Vec<i64>) -> Result<(), String> {
        for id in normalize_batch_ids(ids)? {
            self.delete(id)?;
        }
        Ok(())
    }

    pub fn usage(&self) -> Result<HistoryUsagePayload, String> {
        let entries = self.stored_entries()?;
        let image_bytes = entries.iter().try_fold(0_u64, |total, entry| {
            Ok::<_, String>(total.saturating_add(self.image_file_size(&entry.filename)?))
        })?;
        Ok(HistoryUsagePayload {
            item_count: entries.len(),
            image_bytes,
            max_items: MAX_HISTORY_ITEMS,
            max_image_bytes: MAX_HISTORY_IMAGE_BYTES,
        })
    }

    pub fn export<R: Runtime>(
        &self,
        app: &AppHandle<R>,
        ids: Vec<i64>,
    ) -> Result<HistoryExportPayload, String> {
        let downloads = app.path().download_dir().map_err(|error| {
            format!("failed to resolve the Windows Downloads directory for history export: {error}")
        })?;
        self.export_to_directory(ids, downloads.join(EXPORT_DIRECTORY))
    }

    fn export_to_directory(
        &self,
        ids: Vec<i64>,
        export_root: PathBuf,
    ) -> Result<HistoryExportPayload, String> {
        let ids = normalize_batch_ids(ids)?;
        let entries = ids
            .iter()
            .map(|id| self.stored_entry(*id))
            .collect::<Result<Vec<_>, _>>()?;
        let directory = create_export_directory(&export_root)?;
        let mut metadata = String::from(
            "\u{feff}id,filename,width,height,created_at_ms,favorite,tags,ocr_text\r\n",
        );

        for (index, entry) in entries.iter().enumerate() {
            let exported_filename = format!("{:03}-{}.png", index + 1, entry.id);
            let source = self.image_path_from_filename(&entry.filename)?;
            fs::copy(&source, directory.join(&exported_filename)).map_err(|error| {
                format!(
                    "failed to export screenshot history image {}: {error}",
                    entry.id
                )
            })?;
            metadata.push_str(&format!(
                "{},{},{},{},{},{},{},{}\r\n",
                entry.id,
                csv_field(&exported_filename),
                entry.width,
                entry.height,
                entry.created_at_ms,
                i64::from(entry.favorite),
                csv_field(&entry.tags.join(",")),
                csv_field(entry.ocr_text.as_deref().unwrap_or("")),
            ));
        }
        fs::write(directory.join("metadata.csv"), metadata.as_bytes()).map_err(|error| {
            format!("failed to write screenshot history export metadata: {error}")
        })?;

        Ok(HistoryExportPayload {
            directory: directory.display().to_string(),
            exported_count: entries.len(),
        })
    }

    fn cleanup_retention(
        &self,
        maximum_items: usize,
        maximum_image_bytes: u64,
    ) -> Result<usize, String> {
        let entries = self.stored_entries()?;
        let mut remaining_items = entries.len();
        let mut remaining_image_bytes = entries.iter().try_fold(0_u64, |total, entry| {
            Ok::<_, String>(total.saturating_add(self.image_file_size(&entry.filename)?))
        })?;
        let mut deleted = 0;
        for entry in entries.iter().filter(|entry| !entry.favorite) {
            if remaining_items <= maximum_items && remaining_image_bytes <= maximum_image_bytes {
                break;
            }
            let image_bytes = self.image_file_size(&entry.filename)?;
            self.delete(entry.id)?;
            remaining_items = remaining_items.saturating_sub(1);
            remaining_image_bytes = remaining_image_bytes.saturating_sub(image_bytes);
            deleted += 1;
        }
        Ok(deleted)
    }

    fn stored_entries(&self) -> Result<Vec<StoredHistoryEntry>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "screenshot history database lock is poisoned".to_owned())?;
        let mut statement = connection
            .prepare(
                "SELECT id, filename, width, height, created_at_ms, favorite, ocr_text, tags
                 FROM screenshots ORDER BY created_at_ms ASC, id ASC",
            )
            .map_err(|error| {
                format!("failed to prepare screenshot history storage query: {error}")
            })?;
        let rows = statement
            .query_map([], stored_history_entry_from_row)
            .map_err(|error| format!("failed to read screenshot history storage data: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode screenshot history storage data: {error}"))
    }

    fn stored_entry(&self, id: i64) -> Result<StoredHistoryEntry, String> {
        self.connection
            .lock()
            .map_err(|_| "screenshot history database lock is poisoned".to_owned())?
            .query_row(
                "SELECT id, filename, width, height, created_at_ms, favorite, ocr_text, tags
                 FROM screenshots WHERE id = ?1",
                [id],
                stored_history_entry_from_row,
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    format!("screenshot history entry does not exist: {id}")
                }
                error => format!("failed to read screenshot history metadata: {error}"),
            })
    }

    fn image_file_size(&self, filename: &str) -> Result<u64, String> {
        let image_path = self.image_path_from_filename(filename)?;
        match fs::metadata(image_path) {
            Ok(metadata) => Ok(metadata.len()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(0),
            Err(error) => Err(format!(
                "failed to inspect screenshot history image: {error}"
            )),
        }
    }

    fn load_image(&self, id: i64) -> Result<RgbaImage, String> {
        let filename = self.filename_for_id(id)?;
        let image_path = self.image_path_from_filename(&filename)?;
        let bytes = fs::read(&image_path)
            .map_err(|error| format!("failed to read screenshot history image: {error}"))?;
        image::load_from_memory(&bytes)
            .map(|image| image.to_rgba8())
            .map_err(|error| format!("failed to decode screenshot history image: {error}"))
    }

    fn filename_for_id(&self, id: i64) -> Result<String, String> {
        self.connection
            .lock()
            .map_err(|_| "screenshot history database lock is poisoned".to_owned())?
            .query_row(
                "SELECT filename FROM screenshots WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    format!("screenshot history entry does not exist: {id}")
                }
                error => format!("failed to read screenshot history metadata: {error}"),
            })
    }

    fn image_path_from_filename(&self, filename: &str) -> Result<PathBuf, String> {
        if !is_safe_image_filename(filename) {
            return Err("screenshot history contains an unsafe image filename".to_owned());
        }
        Ok(self.images_directory.join(filename))
    }

    fn thumbnail_path_from_filename(&self, filename: &str) -> Result<PathBuf, String> {
        if !is_safe_image_filename(filename) {
            return Err("screenshot history contains an unsafe image filename".to_owned());
        }
        Ok(self.thumbnails_directory.join(filename))
    }

    fn next_filename(&self, created_at_ms: i64) -> String {
        let sequence = self.next_filename.fetch_add(1, Ordering::Relaxed);
        format!("{created_at_ms}-{sequence}.png")
    }
}

fn stored_history_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredHistoryEntry> {
    let width: i64 = row.get(2)?;
    let height: i64 = row.get(3)?;
    Ok(StoredHistoryEntry {
        id: row.get(0)?,
        filename: row.get(1)?,
        width: u32::try_from(width)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, width))?,
        height: u32::try_from(height)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(3, height))?,
        created_at_ms: row.get(4)?,
        favorite: row.get::<_, i64>(5)? != 0,
        ocr_text: row.get(6)?,
        tags: decode_tags(&row.get::<_, String>(7)?),
    })
}

fn create_export_directory(export_root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(export_root)
        .map_err(|error| format!("failed to create screenshot history export root: {error}"))?;
    let timestamp = current_unix_millis()?;
    for sequence in 0..1_000 {
        let suffix = if sequence == 0 {
            String::new()
        } else {
            format!("-{sequence}")
        };
        let directory = export_root.join(format!("SnapRust-{timestamp}{suffix}"));
        match fs::create_dir(&directory) {
            Ok(()) => return Ok(directory),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "failed to create screenshot history export directory: {error}"
                ));
            }
        }
    }
    Err("could not allocate a unique screenshot history export directory".to_owned())
}

fn csv_field(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn current_unix_millis() -> Result<i64, String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before Unix epoch: {error}"))?;
    i64::try_from(duration.as_millis()).map_err(|_| "system clock value overflowed".to_owned())
}

fn normalize_ocr_text(ocr_text: Option<&str>) -> Result<Option<String>, String> {
    let Some(text) = ocr_text.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    if text.len() > MAX_OCR_TEXT_LENGTH {
        return Err("OCR text is too large to save in screenshot history".to_owned());
    }
    Ok(Some(text.to_owned()))
}

fn normalized_query(query: Option<&str>) -> Result<Option<String>, String> {
    let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) else {
        return Ok(None);
    };
    if query.len() > MAX_QUERY_LENGTH {
        return Err("history search query is too long".to_owned());
    }
    Ok(Some(query.to_owned()))
}

fn ensure_tags_column(connection: &Connection) -> Result<(), String> {
    let mut statement = connection
        .prepare("PRAGMA table_info(screenshots)")
        .map_err(|error| format!("failed to inspect screenshot history schema: {error}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read screenshot history schema: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to decode screenshot history schema: {error}"))?;
    if !columns.iter().any(|column| column == "tags") {
        connection
            .execute_batch("ALTER TABLE screenshots ADD COLUMN tags TEXT NOT NULL DEFAULT ''")
            .map_err(|error| format!("failed to migrate screenshot history tags: {error}"))?;
    }
    Ok(())
}

fn reconcile_filesystem(
    connection: &Connection,
    images_directory: &Path,
    thumbnails_directory: &Path,
) -> Result<(), String> {
    let stored_files = {
        let mut statement = connection
            .prepare("SELECT id, filename FROM screenshots")
            .map_err(|error| format!("failed to inspect screenshot history files: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("failed to read screenshot history files: {error}"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("failed to decode screenshot history files: {error}"))?
    };

    let mut referenced_files = HashSet::with_capacity(stored_files.len());
    let mut missing_ids = Vec::new();
    let mut missing_files = Vec::new();
    for (id, filename) in &stored_files {
        if !is_safe_image_filename(filename) {
            return Err("screenshot history contains an unsafe image filename".to_owned());
        }
        referenced_files.insert(filename.clone());
        if !images_directory.join(filename).is_file() {
            missing_ids.push(*id);
            missing_files.push(filename.clone());
        }
    }

    for id in missing_ids {
        connection
            .execute("DELETE FROM screenshots WHERE id = ?1", [id])
            .map_err(|error| {
                format!("failed to remove screenshot history metadata for a missing image: {error}")
            })?;
    }

    for entry in fs::read_dir(images_directory)
        .map_err(|error| format!("failed to scan screenshot history images: {error}"))?
    {
        let entry = entry
            .map_err(|error| format!("failed to read screenshot history image entry: {error}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        let extension = path.extension().and_then(|extension| extension.to_str());
        let is_temporary = matches!(extension, Some("tmp" | "deleting"));
        let is_orphan_png = extension == Some("png")
            && is_safe_image_filename(name)
            && !referenced_files.contains(name);
        if is_temporary || is_orphan_png {
            fs::remove_file(&path).map_err(|error| {
                format!("failed to remove an incomplete screenshot history file: {error}")
            })?;
        }
    }

    for filename in missing_files {
        referenced_files.remove(&filename);
    }

    for entry in fs::read_dir(thumbnails_directory)
        .map_err(|error| format!("failed to scan screenshot history thumbnails: {error}"))?
    {
        let entry = entry.map_err(|error| {
            format!("failed to read screenshot history thumbnail entry: {error}")
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        let extension = path.extension().and_then(|extension| extension.to_str());
        let is_temporary = matches!(extension, Some("tmp" | "deleting"));
        let is_orphan_png = extension == Some("png")
            && is_safe_image_filename(name)
            && !referenced_files.contains(name);
        if is_temporary || is_orphan_png {
            fs::remove_file(&path).map_err(|error| {
                format!("failed to remove an incomplete screenshot history thumbnail: {error}")
            })?;
        }
    }

    Ok(())
}

fn write_file_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary_path = path.with_extension("png.tmp");
    if let Err(error) = fs::write(&temporary_path, bytes) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error.to_string());
    }
    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);
        return Err(error.to_string());
    }
    Ok(())
}

fn restore_tombstone(tombstone_path: &Path, image_path: &Path) -> Result<(), String> {
    fs::rename(tombstone_path, image_path).map_err(|error| {
        format!("history deletion failed and the original image could not be restored: {error}")
    })
}

fn is_safe_image_filename(filename: &str) -> bool {
    filename.strip_suffix(".png").is_some_and(|stem| {
        !stem.is_empty()
            && stem
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'-')
    })
}

fn normalize_tags(tags: &[String]) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        if tag.len() > MAX_TAG_LENGTH
            || tag.contains(',')
            || tag.contains('，')
            || tag.chars().any(char::is_control)
        {
            return Err("each history tag must be at most 48 characters and cannot contain commas or control characters".to_owned());
        }
        if !normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(tag))
        {
            normalized.push(tag.to_owned());
        }
    }
    if normalized.len() > MAX_TAG_COUNT {
        return Err("a screenshot can have at most 12 tags".to_owned());
    }
    Ok(normalized)
}

fn decode_tags(tags: &str) -> Vec<String> {
    tags.split(',')
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect()
}

fn normalize_batch_ids(ids: Vec<i64>) -> Result<Vec<i64>, String> {
    let mut normalized = Vec::new();
    for id in ids {
        if id <= 0 {
            return Err("history entry id must be positive".to_owned());
        }
        if !normalized.contains(&id) {
            normalized.push(id);
        }
    }
    if normalized.is_empty() {
        return Err("select at least one history entry".to_owned());
    }
    if normalized.len() > MAX_BATCH_ITEMS {
        return Err("batch history actions are limited to 200 entries".to_owned());
    }
    Ok(normalized)
}

pub fn show_history_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if app
        .state::<crate::screenshot::CaptureSession>()
        .is_active()?
    {
        return Err("finish or cancel the current screenshot before opening history".to_owned());
    }
    crate::window::prepare_history_window(app)
}

pub fn hide_history_window<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    crate::window::hide_capture_overlay(app)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        thread,
        time::SystemTime,
    };

    use image::{Rgba, RgbaImage};

    use super::{DATABASE_NAME, HistoryStore, MAX_OCR_TEXT_LENGTH};

    static TEST_DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_directory() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "snaprust-history-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            TEST_DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn saves_searches_favorites_and_deletes_screenshots() {
        let directory = test_directory();
        let store = HistoryStore::open_in(directory.clone()).unwrap();
        let image = RgbaImage::from_pixel(48, 24, Rgba([12, 34, 56, 255]));
        let id = store.save(&image, Some("Rust OCR history")).unwrap();
        let filename = store.filename_for_id(id).unwrap();
        assert!(directory.join("thumbnails").join(&filename).is_file());

        let items = store.list(Some("OCR"), false).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].width, items[0].height), (48, 24));
        assert_eq!(items[0].ocr_text.as_deref(), Some("Rust OCR history"));
        assert!(!items[0].favorite);
        assert!(items[0].tags.is_empty());
        assert!(
            store
                .thumbnail_png(id)
                .unwrap()
                .starts_with(&[137, 80, 78, 71])
        );

        store.set_favorite(id, true).unwrap();
        assert_eq!(store.list(None, true).unwrap().len(), 1);
        store
            .set_tags(
                id,
                vec!["代码".to_owned(), "OCR".to_owned(), "代码".to_owned()],
            )
            .unwrap();
        let tagged = store.list(Some("代码"), false).unwrap();
        assert_eq!(tagged[0].tags, ["代码", "OCR"]);
        store.delete(id).unwrap();
        assert!(store.list(None, false).unwrap().is_empty());
        assert!(store.image(id).is_err());
        assert!(!directory.join("thumbnails").join(filename).exists());

        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn repairs_missing_and_orphaned_history_files_on_open() {
        let directory = test_directory();
        let store = HistoryStore::open_in(directory.clone()).unwrap();
        let id = store.save(&RgbaImage::new(12, 8), None).unwrap();
        let filename = store.filename_for_id(id).unwrap();
        let images_directory = directory.join("images");
        fs::remove_file(images_directory.join(&filename)).unwrap();
        fs::write(images_directory.join("999999999999-0.png"), b"orphan").unwrap();
        fs::write(
            images_directory.join("999999999999-1.png.tmp"),
            b"incomplete",
        )
        .unwrap();
        drop(store);

        let reopened = HistoryStore::open_in(directory.clone()).unwrap();
        assert!(reopened.list(None, false).unwrap().is_empty());
        assert!(!images_directory.join("999999999999-0.png").exists());
        assert!(!images_directory.join("999999999999-1.png.tmp").exists());

        drop(reopened);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_oversized_ocr_without_writing_a_history_image() {
        let directory = test_directory();
        let store = HistoryStore::open_in(directory.clone()).unwrap();
        let oversized_text = "x".repeat(MAX_OCR_TEXT_LENGTH + 1);
        assert!(
            store
                .save(&RgbaImage::new(12, 8), Some(&oversized_text))
                .is_err()
        );
        assert!(
            fs::read_dir(directory.join("images"))
                .unwrap()
                .next()
                .is_none()
        );

        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn concurrent_saves_return_distinct_database_ids() {
        let directory = test_directory();
        let store = Arc::new(HistoryStore::open_in(directory.clone()).unwrap());
        let handles = (0..16)
            .map(|index| {
                let store = Arc::clone(&store);
                thread::spawn(move || {
                    let text = format!("concurrent-{index}");
                    store.save(&RgbaImage::new(8, 8), Some(&text)).unwrap()
                })
            })
            .collect::<Vec<_>>();
        let ids = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        let unique_ids = ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(unique_ids.len(), ids.len());
        assert_eq!(store.list(None, false).unwrap().len(), ids.len());

        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn migrates_existing_history_databases_to_add_tags() {
        let directory = test_directory();
        fs::create_dir_all(&directory).unwrap();
        let legacy = rusqlite::Connection::open(directory.join(DATABASE_NAME)).unwrap();
        legacy
            .execute_batch(
                "
                CREATE TABLE screenshots (
                    id INTEGER PRIMARY KEY,
                    filename TEXT NOT NULL UNIQUE,
                    ocr_text TEXT,
                    width INTEGER NOT NULL,
                    height INTEGER NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    favorite INTEGER NOT NULL DEFAULT 0
                );
                ",
            )
            .unwrap();
        drop(legacy);

        let store = HistoryStore::open_in(directory.clone()).unwrap();
        let id = store
            .save(&RgbaImage::new(16, 9), Some("migrated database"))
            .unwrap();
        store.set_tags(id, vec!["升级".to_owned()]).unwrap();
        assert_eq!(store.list(Some("升级"), false).unwrap()[0].tags, ["升级"]);

        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn cleans_oldest_unfavorited_entries_for_count_and_disk_limits() {
        let directory = test_directory();
        let store = HistoryStore::open_in(directory.clone()).unwrap();
        let image = RgbaImage::new(8, 8);
        let first = store.save(&image, Some("first")).unwrap();
        let favorite = store.save(&image, Some("favorite")).unwrap();
        let newest = store.save(&image, Some("newest")).unwrap();
        store.set_favorite(favorite, true).unwrap();

        assert_eq!(store.cleanup_retention(2, u64::MAX).unwrap(), 1);
        let entries = store.list(None, false).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(!entries.iter().any(|entry| entry.id == first));
        assert!(entries.iter().any(|entry| entry.id == favorite));
        assert!(entries.iter().any(|entry| entry.id == newest));

        let favorite_bytes = store
            .image_file_size(&store.filename_for_id(favorite).unwrap())
            .unwrap();
        assert_eq!(store.cleanup_retention(10, favorite_bytes).unwrap(), 1);
        let remaining = store.list(None, false).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, favorite);

        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn exports_selected_pngs_and_csv_metadata() {
        let directory = test_directory();
        let export_root = directory.join("exports");
        let store = HistoryStore::open_in(directory.clone()).unwrap();
        let first = store
            .save(
                &RgbaImage::from_pixel(9, 7, Rgba([20, 40, 60, 255])),
                Some("OCR, export"),
            )
            .unwrap();
        let second = store
            .save(&RgbaImage::from_pixel(5, 3, Rgba([70, 80, 90, 255])), None)
            .unwrap();
        store
            .set_tags(first, vec!["代码".to_owned(), "分享".to_owned()])
            .unwrap();
        store.set_favorite(second, true).unwrap();

        let export = store
            .export_to_directory(vec![second, first], export_root)
            .unwrap();
        let export_directory = std::path::PathBuf::from(export.directory);
        assert_eq!(export.exported_count, 2);
        assert!(export_directory.join(format!("001-{second}.png")).is_file());
        assert!(export_directory.join(format!("002-{first}.png")).is_file());
        let metadata = fs::read_to_string(export_directory.join("metadata.csv")).unwrap();
        assert!(metadata.starts_with('\u{feff}'));
        assert!(metadata.contains("\"OCR, export\""));
        assert!(metadata.contains("\"代码,分享\""));

        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn updates_and_deletes_multiple_history_entries() {
        let directory = test_directory();
        let store = HistoryStore::open_in(directory.clone()).unwrap();
        let image = RgbaImage::new(8, 8);
        let first = store.save(&image, None).unwrap();
        let second = store.save(&image, None).unwrap();

        store
            .set_favorite_batch(vec![first, second, first], true)
            .unwrap();
        assert_eq!(store.list(None, true).unwrap().len(), 2);
        store.delete_batch(vec![second, first, second]).unwrap();
        assert!(store.list(None, false).unwrap().is_empty());

        drop(store);
        fs::remove_dir_all(directory).unwrap();
    }
}
